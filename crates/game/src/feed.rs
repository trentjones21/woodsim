//! The table saw station's state: the saw, the board on it, and how far it has been fed.
//!
//! `woodsim` decides what the blade does to the board; this module keeps track of where the
//! board is and where to draw it. Positions are in meters, in table space: x = 0 is the blade's
//! fence-side face with +x toward the fence, y = 0 is the table top, and z = 0 is under the
//! blade's center, with the board feeding toward -z, away from the operator.
//!
//! The board rides the fence on its `w = width` edge and leads with its `l = 0` end, so its own
//! axes line up with table space: `w` along x, `t` along y, and `l` along z.

use bevy::prelude::*;
use woodsim::{Blade, Board, CutError, Length, Pass, Piece, TableSaw};

use crate::shop::{Part, Placed, SawBoards, describe_board, describe_cut, describe_kerf};

pub const FENCE_MIN: Length = Length::from_inch_fraction(1, 16);
pub const FENCE_MAX: Length = Length::from_inches(24);
/// As high as a 10" blade goes.
pub const HEIGHT_MAX: Length = Length::from_inch_fraction(25, 8);
/// A blade's teeth reach at most its radius in front of its center, so a board this far in
/// front of the center (or further) is clear of it.
const CLEAR: Length = Length::from_inches(5);
/// Where a board waits: its leading end 1" short of where any blade could reach it.
const START: Length = Length::from_inches(-6);

#[derive(Resource)]
pub struct TableSawStation {
    pub saw: TableSaw,
    /// The board on the saw, with the slots earlier passes cut. During a pass, this is how it
    /// was before the pass.
    board: Board,
    /// Where the blade's center is along the board, from its leading end: negative while the
    /// board is in front of the blade.
    advance: Length,
    /// The pass under way: from when the board starts moving until it's pulled back clear of
    /// the blade, or replaced. A board pushed all the way past keeps its pass, so its pieces
    /// stay where they were cut.
    pass: Option<Feeding>,
    /// Parts carried off the saw.
    taken: Vec<Part>,
}

struct Feeding {
    /// The saw as it was set when the pass began.
    saw: TableSaw,
    /// The furthest back and forward the blade's center has been along the board.
    reached: (Length, Length),
    result: Result<Pass, CutError>,
}

impl Default for TableSawStation {
    fn default() -> TableSawStation {
        TableSawStation {
            saw: TableSaw {
                blade: Blade {
                    diameter: Length::from_inches(10),
                    kerf: Blade::FULL_KERF,
                },
                fence: Length::from_inches(6),
                height: Length::from_inches(1),
            },
            board: new_stock(),
            advance: START,
            pass: None,
            taken: Vec::new(),
        }
    }
}

/// A fresh 1x8, 24" long.
fn new_stock() -> Board {
    Board::new(
        Length::from_inches(24),
        Length::from_inch_fraction(29, 4),
        Length::from_inch_fraction(3, 4),
    )
}

impl TableSawStation {
    /// Moves the fence, within its range. It stays put while a board is at the blade.
    pub fn set_fence(&mut self, fence: Length) {
        if !self.at_blade() {
            self.saw.fence = fence.clamp(FENCE_MIN, FENCE_MAX);
        }
    }

    /// Raises or lowers the blade, from level with the table to as high as it goes. It stays
    /// put while a board is at the blade.
    pub fn set_height(&mut self, height: Length) {
        if !self.at_blade() {
            self.saw.height = height.clamp(Length::ZERO, HEIGHT_MAX);
        }
    }

    /// Pushes the board `distance` meters along the fence, or pulls it back if `distance` is
    /// negative. It moves in 64ths of an inch.
    pub fn push(&mut self, distance: f32) {
        let step = crate::shop::snap(distance, 64);
        self.move_to(self.advance + step);
    }

    /// Slides the board along the fence until the blade's center is `advance` along it. A board
    /// that's been cut apart, or pushed all the way past the blade, only goes forward; you take
    /// the pieces off the outfeed side.
    pub fn move_to(&mut self, advance: Length) {
        if self.on_saw().is_none() {
            return;
        }
        let back_limit = if self.cut_apart() || self.past_blade() {
            self.advance
        } else {
            START
        };
        let advance = advance.clamp(
            back_limit,
            self.board.length() + CLEAR + Length::from_inches(1),
        );
        if advance == self.advance {
            return;
        }
        let pass = self.pass.get_or_insert(Feeding {
            saw: self.saw,
            reached: (self.advance, self.advance),
            result: Ok(Pass::Missed),
        });
        pass.reached = (pass.reached.0.min(advance), pass.reached.1.max(advance));
        pass.result = pass.saw.feed(&self.board, pass.reached.0, pass.reached.1);
        self.advance = advance;
        // Pulled back clear of the blade, the board keeps whatever slot it got, and the fence
        // and blade are free to move for the next pass.
        if self.advance <= -CLEAR {
            self.finish_pass();
        }
    }

    /// Slides a board that's in one piece back to where boards wait, ready for another pass:
    /// around the saw, if it's on the outfeed side. Returns false if the board is at the blade,
    /// cut apart, or gone.
    pub fn bring_back(&mut self) -> bool {
        if self.on_saw() != Some(Part::Whole) || self.at_blade() {
            return false;
        }
        self.finish_pass();
        self.advance = START;
        true
    }

    /// Lifts a board off the saw to carry it elsewhere: the whole board, once it's clear of the
    /// blade, or a piece of one the blade cut apart, once it's past.
    pub fn take(&mut self, part: Part) -> Option<Board> {
        if !self.can_take(part) {
            return None;
        }
        let board = match (part, self.result()) {
            (Part::Whole, _) => self.current_board(),
            (Part::Piece, Some(Ok(Pass::Cut(cut)))) => cut.piece.board.clone(),
            (Part::Offcut, Some(Ok(Pass::Cut(cut)))) => cut.offcut.as_ref()?.board.clone(),
            _ => return None,
        };
        self.taken.push(part);
        Some(board)
    }

    /// Whether `part` is on the saw and clear of the blade, so it can be carried off.
    pub fn can_take(&self, part: Part) -> bool {
        if self.taken.contains(&part) {
            return false;
        }
        match (part, self.result()) {
            (Part::Whole, Some(Ok(Pass::Cut(_)))) => false,
            (Part::Whole, _) => !self.at_blade(),
            (Part::Piece, Some(Ok(Pass::Cut(_)))) => self.past_blade(),
            (Part::Offcut, Some(Ok(Pass::Cut(cut)))) => cut.offcut.is_some() && self.past_blade(),
            _ => false,
        }
    }

    /// Puts a fresh 1x8 against the fence. Whatever was on the saw goes in the scrap bin.
    pub fn new_board(&mut self) {
        self.load(new_stock());
    }

    /// Puts `board` against the fence, ready to rip, and hands back whatever was on the saw that
    /// wasn't carried off: the board, slots and all, or the pieces it was cut into.
    pub fn load(&mut self, board: Board) -> Vec<Board> {
        let leftovers = match self.result() {
            Some(Ok(Pass::Cut(cut))) => [
                (Part::Piece, Some(&cut.piece)),
                (Part::Offcut, cut.offcut.as_ref()),
            ]
            .into_iter()
            .filter(|(part, _)| !self.taken.contains(part))
            .filter_map(|(_, piece)| piece.map(|piece| piece.board.clone()))
            .chain(cut.loose.iter().map(|piece| piece.board.clone()))
            .collect(),
            _ if self.taken.contains(&Part::Whole) => Vec::new(),
            _ => vec![self.current_board()],
        };
        self.board = board;
        self.advance = START;
        self.pass = None;
        self.taken.clear();
        leftovers
    }

    /// Whether any of the board is over the blade, where the fence and blade can't move.
    pub fn at_blade(&self) -> bool {
        self.on_saw().is_some()
            && self.advance > -CLEAR
            && self.advance < self.board.length() + CLEAR
    }

    /// Whether the whole board is past the blade, on the outfeed side.
    pub fn past_blade(&self) -> bool {
        self.advance >= self.board.length() + CLEAR
    }

    fn cut_apart(&self) -> bool {
        matches!(self.result(), Some(Ok(Pass::Cut(_))))
    }

    fn result(&self) -> Option<&Result<Pass, CutError>> {
        self.pass.as_ref().map(|pass| &pass.result)
    }

    /// The board in one piece as it is now, with any slot the pass under way has cut.
    fn current_board(&self) -> Board {
        match self.result() {
            Some(Ok(Pass::Kerfed(board))) => board.clone(),
            _ => self.board.clone(),
        }
    }

    /// Ends the pass under way, keeping the slot it cut.
    fn finish_pass(&mut self) {
        self.board = self.current_board();
        self.pass = None;
    }

    /// What's left on the saw: the whole board, or a piece of it, or nothing.
    fn on_saw(&self) -> Option<Part> {
        [Part::Whole, Part::Piece, Part::Offcut]
            .into_iter()
            .find(|&part| match (part, self.result()) {
                (Part::Whole, Some(Ok(Pass::Cut(_)))) => false,
                (Part::Whole, _) => !self.taken.contains(&part),
                (Part::Piece, Some(Ok(Pass::Cut(_)))) => !self.taken.contains(&part),
                (Part::Offcut, Some(Ok(Pass::Cut(cut)))) => {
                    cut.offcut.is_some() && !self.taken.contains(&part)
                }
                _ => false,
            })
    }

    pub fn boards(&self) -> SawBoards {
        let fence = self
            .pass
            .as_ref()
            .map_or(self.saw.fence, |pass| pass.saw.fence);
        let width = self.board.width();
        let corner = Vec3::new(
            (fence - width).as_meters_f32(),
            0.0,
            (-self.advance).as_meters_f32(),
        );
        let unless_taken =
            |part: Part, placed: Placed| (!self.taken.contains(&part)).then_some(placed);
        match self.result() {
            Some(Ok(Pass::Cut(cut))) => {
                // Each piece sits where it was on the board: its corner `across` and `along` it,
                // which are x and z in table space.
                let place = |piece: &Piece| {
                    let offset = Vec3::new(
                        piece.across.as_meters_f32(),
                        0.0,
                        piece.along.as_meters_f32(),
                    );
                    Placed {
                        board: piece.board.clone(),
                        corner: corner + offset,
                        grain: offset,
                    }
                };
                // A table saw pass never sets pieces loose: every slot so far runs in from the
                // board's leading end, so the wood beside the new slot holds together. Any would
                // go back with the leftovers when the next board loads.
                SawBoards {
                    whole: None,
                    piece: unless_taken(Part::Piece, place(&cut.piece)),
                    offcut: cut
                        .offcut
                        .as_ref()
                        .and_then(|offcut| unless_taken(Part::Offcut, place(offcut))),
                }
            }
            _ => SawBoards {
                whole: unless_taken(
                    Part::Whole,
                    Placed {
                        board: self.current_board(),
                        corner,
                        grain: Vec3::ZERO,
                    },
                ),
                ..default()
            },
        }
    }

    /// One line describing what's on the saw.
    pub fn status(&self) -> String {
        if self.on_saw().is_none() {
            return "Nothing on the saw. Press R for a new board.".to_string();
        }
        match self.result() {
            Some(Err(error)) => format!("No cut: {error}."),
            Some(Ok(Pass::Cut(cut))) => describe_cut("Ripped", cut),
            Some(Ok(Pass::Kerfed(board))) => {
                let kerf = describe_kerf(board, board.kerfs().last().expect("a pass's slot"));
                if self.at_blade() {
                    format!("Cutting. So far, {kerf}.")
                } else {
                    format!("Cut {kerf}. Press B to bring the board back around for another pass.")
                }
            }
            Some(Ok(Pass::Missed)) if self.past_blade() => {
                "The blade is below the table, so the board went over it uncut. \
                 Press B to bring it back around."
                    .to_string()
            }
            _ => format!(
                "Board on the saw: {}. Drag it, or hold Space, to push it through.",
                describe_board(&self.board)
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shop::inches;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "{actual} m is not {expected} m"
        );
    }

    /// Pushes the board until the blade's center is `inches` along it.
    fn push_to(station: &mut TableSawStation, inches: i64) {
        station.move_to(Length::from_inches(inches));
    }

    /// Pushes the board all the way past the blade.
    fn push_through(station: &mut TableSawStation) {
        station.push(10.0);
    }

    #[test]
    fn board_waits_against_the_fence_in_front_of_the_blade() {
        let station = TableSawStation::default();
        let boards = station.boards();

        let whole = boards.whole.unwrap();
        assert_close(whole.corner.x, inches(6.0 - 7.25));
        assert_close(whole.corner.z, inches(6.0));
        assert_eq!((boards.piece, boards.offcut), (None, None));
        assert!(!station.at_blade());
    }

    #[test]
    fn stopping_partway_leaves_a_stopped_cut() {
        let mut station = TableSawStation::default();
        push_to(&mut station, 10);
        let boards = station.boards();

        let whole = boards.whole.unwrap();
        assert_eq!(whole.board.kerfs().len(), 1);
        assert_close(whole.corner.z, inches(-10.0));
        assert!(station.at_blade());
        assert_eq!(
            station.status(),
            "Cutting. So far, a stopped cut 11-9/16\" long on top and 13\" on the bottom."
        );
    }

    #[test]
    fn pulling_a_stopped_cut_back_frees_the_fence_and_keeps_the_slot() {
        let mut station = TableSawStation::default();
        push_to(&mut station, 10);
        station.set_fence(Length::from_inches(3));
        assert_eq!(station.saw.fence, Length::from_inches(6));

        station.move_to(START);
        station.set_fence(Length::from_inches(3));
        assert_eq!(station.saw.fence, Length::from_inches(3));
        assert_eq!(
            station.status(),
            "Board on the saw: 3/4\" x 7-1/4\" x 24\", with a stopped cut 11-9/16\" long on top \
             and 13\" on the bottom. Drag it, or hold Space, to push it through."
        );
    }

    #[test]
    fn pushed_all_the_way_it_comes_apart_clear_of_the_blade() {
        let mut station = TableSawStation::default();
        push_through(&mut station);
        let boards = station.boards();

        assert!(station.past_blade() && !station.at_blade());
        assert_eq!(boards.whole, None);
        let piece = boards.piece.unwrap();
        assert_eq!(piece.board.to_string(), "3/4\" x 6\" x 24\"");
        assert_close(piece.corner.x, 0.0);
        let offcut = boards.offcut.unwrap();
        assert_close(offcut.corner.x, inches(-1.25));
        assert!(offcut.corner.z + inches(24.0) < -inches(5.0));
        assert_eq!(
            station.status(),
            "Ripped a 3/4\" x 6\" x 24\" piece, plus a 3/4\" x 1-1/8\" x 24\" offcut. \
             The blade turned 1/8\" to sawdust."
        );
    }

    #[test]
    fn pieces_cut_apart_only_go_forward() {
        let mut station = TableSawStation::default();
        push_through(&mut station);
        let advance = station.advance;
        station.move_to(START);
        assert_eq!(station.advance, advance);
    }

    #[test]
    fn the_fence_moves_again_once_the_pieces_are_past_and_they_stay_put() {
        let mut station = TableSawStation::default();
        push_through(&mut station);
        station.set_fence(Length::from_inches(3));

        assert_eq!(station.saw.fence, Length::from_inches(3));
        assert_close(station.boards().piece.unwrap().corner.x, 0.0);
    }

    #[test]
    fn the_blade_stays_within_its_range_and_put_while_cutting() {
        let mut station = TableSawStation::default();
        station.set_height(Length::from_inches(9));
        assert_eq!(station.saw.height, HEIGHT_MAX);
        station.set_height(-Length::from_inches(1));
        assert_eq!(station.saw.height, Length::ZERO);

        station.set_height(Length::from_inches(1));
        push_to(&mut station, 2);
        station.set_height(Length::from_inches(2));
        assert_eq!(station.saw.height, Length::from_inches(1));
    }

    #[test]
    fn a_low_blade_cuts_a_groove_and_the_board_comes_back_for_another() {
        let mut station = TableSawStation::default();
        station.set_height(Length::from_inch_fraction(1, 4));
        push_through(&mut station);
        assert_eq!(
            station.status(),
            "Cut a 1/4\"-deep groove the full length. \
             Press B to bring the board back around for another pass."
        );

        // Move the fence over and cut a second groove beside the first.
        assert!(station.bring_back());
        station.set_fence(Length::from_inches(5));
        push_through(&mut station);
        let whole = station.boards().whole.unwrap();
        assert_eq!(whole.board.kerfs().len(), 2);
        assert!(station.can_take(Part::Whole));
    }

    #[test]
    fn a_blade_below_the_table_lets_the_board_pass_over() {
        let mut station = TableSawStation::default();
        station.set_height(Length::ZERO);
        push_through(&mut station);

        assert!(station.boards().whole.unwrap().board.kerfs().is_empty());
        assert!(station.status().starts_with("The blade is below the table"));
    }

    #[test]
    fn fence_past_the_board_pushes_it_by_uncut() {
        let mut station = TableSawStation::default();
        station.set_fence(Length::from_inches(8));
        push_through(&mut station);

        assert!(station.boards().whole.is_some());
        assert_eq!(
            station.status(),
            "No cut: the blade misses the board: set to 8\", but the board is only 7-1/4\"."
        );
        assert_eq!(station.take(Part::Piece), None);
    }

    #[test]
    fn pieces_can_be_taken_once_past_the_blade() {
        let mut station = TableSawStation::default();
        // Cut apart, but the pieces still reach back over the blade.
        push_to(&mut station, 26);
        assert!(!station.can_take(Part::Piece));

        push_through(&mut station);
        let piece = station.take(Part::Piece).unwrap();
        assert_eq!(piece.to_string(), "3/4\" x 6\" x 24\"");
        assert_eq!(station.take(Part::Piece), None);
        let boards = station.boards();
        assert_eq!(boards.piece, None);
        assert!(boards.offcut.is_some());
    }

    #[test]
    fn a_whole_board_can_be_taken_only_clear_of_the_blade() {
        let mut station = TableSawStation::default();
        push_to(&mut station, 10);
        assert_eq!(station.take(Part::Whole), None);

        station.move_to(START);
        let board = station.take(Part::Whole).unwrap();
        assert_eq!(board.kerfs().len(), 1);
        assert_eq!(station.boards(), SawBoards::default());
        assert_eq!(
            station.status(),
            "Nothing on the saw. Press R for a new board."
        );
    }

    #[test]
    fn new_board_clears_the_saw() {
        let mut station = TableSawStation::default();
        push_through(&mut station);
        station.take(Part::Piece);
        station.new_board();

        assert!(station.pass.is_none());
        assert!(station.boards().whole.is_some());
        assert!(station.taken.is_empty());
    }

    /// A 3/4" x 6" x 12" piece, as if carried over from the miter saw.
    fn crosscut_piece() -> Board {
        Board::new(
            Length::from_inches(12),
            Length::from_inches(6),
            Length::from_inch_fraction(3, 4),
        )
    }

    #[test]
    fn loading_a_board_hands_back_the_board_that_was_waiting() {
        let mut station = TableSawStation::default();
        let leftovers = station.load(crosscut_piece());

        assert_eq!(leftovers, [new_stock()]);
        assert_eq!(station.board, crosscut_piece());
        assert_eq!(
            station.status(),
            "Board on the saw: 3/4\" x 6\" x 12\". Drag it, or hold Space, to push it through."
        );
    }

    #[test]
    fn loading_a_board_hands_back_a_board_left_in_the_blade_with_its_slot() {
        let mut station = TableSawStation::default();
        push_to(&mut station, 10);
        let leftovers = station.load(crosscut_piece());

        assert_eq!(leftovers.len(), 1);
        assert_eq!(leftovers[0].kerfs().len(), 1);
    }

    #[test]
    fn loading_a_board_hands_back_the_pieces_not_carried_off() {
        let mut station = TableSawStation::default();
        push_through(&mut station);
        station.take(Part::Piece);
        let leftovers = station.load(crosscut_piece());

        let widths: Vec<Length> = leftovers.iter().map(Board::width).collect();
        assert_eq!(widths, [Length::from_inch_fraction(9, 8)]);
        assert!(station.pass.is_none());
        assert!(station.taken.is_empty());
    }
}

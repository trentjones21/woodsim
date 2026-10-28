//! The table saw station's state: the saw, the board on it, and how far it has been fed.
//!
//! `woodsim` decides what the blade does to the board; this module keeps track of where the
//! board is and where to draw it. Positions are in meters, in table space: x = 0 is the blade's
//! fence-side face with +x toward the fence, y = 0 is the table top, and z = 0 is under the
//! blade's center, with the board feeding toward -z, away from the operator.
//!
//! Table space is the saw's own frame: across the table (x), up (y), and along it (z). The board
//! lies as its `Orientation` says: unturned, its length runs along the table, ripping; turned a
//! quarter turn, across it, crosscutting.

use bevy::prelude::*;
use woodsim::{Axis, Blade, Board, CutError, Length, Orientation, Pass, TableSaw};

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
    /// How the board lies on the table.
    lie: Orientation,
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
    /// Whether pieces the cut set loose have been handed off to set aside.
    loose_set_aside: bool,
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
            lie: Orientation::default(),
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

    /// Turns the board a quarter turn to the left, seen from above. Returns false unless it's
    /// in one piece, waiting in front of the blade.
    pub fn turn_left(&mut self) -> bool {
        self.relay(self.lie.turned_left())
    }

    /// Turns the board a quarter turn to the right, seen from above.
    pub fn turn_right(&mut self) -> bool {
        self.relay(self.lie.turned_right())
    }

    /// Flips the board over.
    pub fn flip(&mut self) -> bool {
        self.relay(self.lie.flipped())
    }

    /// Lays the board down another way, if it's in one piece and waiting in front of the blade.
    fn relay(&mut self, lie: Orientation) -> bool {
        if self.on_saw() != Some(Part::Whole) || self.pass.is_some() || self.advance > -CLEAR {
            return false;
        }
        self.lie = lie;
        true
    }

    /// Where the blade's center is along the board, from its leading end: negative while the
    /// board is in front of the blade.
    pub fn advance(&self) -> Length {
        self.advance
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
        let advance = advance.clamp(back_limit, self.reach() + CLEAR + Length::from_inches(1));
        if advance == self.advance {
            return;
        }
        let pass = self.pass.get_or_insert(Feeding {
            saw: self.saw,
            reached: (self.advance, self.advance),
            result: Ok(Pass::Missed),
            loose_set_aside: false,
        });
        pass.reached = (pass.reached.0.min(advance), pass.reached.1.max(advance));
        pass.result = pass
            .saw
            .feed(&self.board, self.lie, pass.reached.0, pass.reached.1);
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

    /// Whether the last cut set pieces loose that haven't been handed off yet.
    pub fn has_loose(&self) -> bool {
        matches!(&self.pass, Some(pass) if !pass.loose_set_aside
            && matches!(&pass.result, Ok(Pass::Cut(cut)) if !cut.loose.is_empty()))
    }

    /// Hands off the pieces the last cut set loose, once, to be set aside.
    pub fn take_loose(&mut self) -> Vec<Board> {
        if !self.has_loose() {
            return Vec::new();
        }
        let pass = self.pass.as_mut().expect("loose pieces come from a pass");
        pass.loose_set_aside = true;
        match &pass.result {
            Ok(Pass::Cut(cut)) => cut.loose.iter().map(|piece| piece.board.clone()).collect(),
            _ => Vec::new(),
        }
    }

    /// Puts a fresh 1x8 against the fence. Whatever was on the saw goes in the scrap bin.
    pub fn new_board(&mut self) {
        self.load(new_stock());
    }

    /// Puts `board` against the fence, unturned and ready to rip, and hands back whatever was on
    /// the saw that wasn't carried off: the board, slots and all, or the pieces it was cut into.
    pub fn load(&mut self, board: Board) -> Vec<Board> {
        let mut leftovers = self.take_loose();
        leftovers.extend(match self.result() {
            Some(Ok(Pass::Cut(cut))) => [
                (Part::Piece, Some(&cut.piece)),
                (Part::Offcut, cut.offcut.as_ref()),
            ]
            .into_iter()
            .filter(|(part, _)| !self.taken.contains(part))
            .filter_map(|(_, piece)| piece.map(|piece| piece.board.clone()))
            .collect(),
            _ if self.taken.contains(&Part::Whole) => Vec::new(),
            _ => vec![self.current_board()],
        });
        self.board = board;
        self.lie = Orientation::default();
        self.advance = START;
        self.pass = None;
        self.taken.clear();
        leftovers
    }

    /// Whether any of the board is over the blade, where the fence and blade can't move.
    pub fn at_blade(&self) -> bool {
        self.on_saw().is_some() && self.advance > -CLEAR && self.advance < self.reach() + CLEAR
    }

    /// Whether the whole board is past the blade, on the outfeed side.
    pub fn past_blade(&self) -> bool {
        self.advance >= self.reach() + CLEAR
    }

    /// How far the board reaches along the table, from its leading end.
    fn reach(&self) -> Length {
        self.lie.along().0.of(&self.board)
    }

    /// How far the board reaches across the table, from the fence.
    fn span(&self) -> Length {
        self.lie.across().0.of(&self.board)
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
        // The board reaches from the fence back across the table, and from its leading end
        // back toward the operator.
        let near = Vec3::new(
            (fence - self.span()).as_meters_f32(),
            0.0,
            (-self.advance).as_meters_f32(),
        );
        let unless_taken =
            |part: Part, placed: Placed| (!self.taken.contains(&part)).then_some(placed);
        match self.result() {
            Some(Ok(Pass::Cut(cut))) => {
                // Each piece sits where it was on the board. Loose pieces are set aside.
                let board = Placed::lying(&self.board, self.lie, near);
                SawBoards {
                    whole: None,
                    piece: unless_taken(Part::Piece, board.piece(&cut.piece)),
                    offcut: cut
                        .offcut
                        .as_ref()
                        .and_then(|offcut| unless_taken(Part::Offcut, board.piece(offcut))),
                }
            }
            _ => SawBoards {
                whole: unless_taken(
                    Part::Whole,
                    Placed::lying(&self.current_board(), self.lie, near),
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
        let verb = match self.lie.across().0 {
            Axis::Width => "Ripped",
            Axis::Length => "Crosscut",
        };
        match self.result() {
            Some(Err(error)) => format!("No cut: {error}."),
            Some(Ok(Pass::Cut(cut))) if cut.loose.is_empty() => describe_cut(verb, cut),
            Some(Ok(Pass::Cut(cut))) => format!(
                "{} Set aside on the pile by the miter saw.",
                describe_cut(verb, cut)
            ),
            Some(Ok(Pass::Kerfed(board))) => {
                let kerf = describe_kerf(board, board.kerfs().last().expect("a pass's slot"));
                if self.at_blade() {
                    format!("Cutting. So far, {kerf}.{}", self.warning())
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
                "Board on the saw: {}. Drag it, or hold Space, to push it through.{}",
                describe_board(&self.board),
                self.warning()
            ),
        }
    }

    /// A warning, if the board would ride the fence on its short side: across the table it's
    /// wider than it is long along the fence, so the fence can't keep it from twisting into
    /// the blade and kicking back.
    fn warning(&self) -> &'static str {
        if self.reach() < self.span() && self.saw.fence < self.span() {
            " Careful: it rides the fence on its short side, so it can twist into the blade and \
             kick back. A real shop would use a miter gauge or a crosscut sled."
        } else {
            ""
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
    fn turned_a_quarter_turn_the_board_lies_across_the_table_and_crosscuts() {
        let mut station = TableSawStation::default();
        assert!(station.turn_left());
        let (near, far) = station.boards().whole.unwrap().bounds();
        // 24" across the table, back from the fence at 6"; 7-1/4" along it.
        assert_close(far.x, inches(6.0));
        assert_close(far.x - near.x, inches(24.0));
        assert_close(far.z - near.z, inches(7.25));
        assert!(
            station
                .status()
                .contains("Careful: it rides the fence on its short side")
        );

        station.set_fence(Length::from_inches(12));
        push_through(&mut station);
        assert_eq!(
            station.status(),
            "Crosscut a 3/4\" x 7-1/4\" x 12\" piece, plus a 3/4\" x 7-1/4\" x 11-7/8\" \
             offcut. The blade turned 1/8\" to sawdust."
        );
        // The piece lies between the blade and the fence.
        let (near, far) = station.boards().piece.unwrap().bounds();
        assert_close(near.x, 0.0);
        assert_close(far.x, inches(12.0));
    }

    #[test]
    fn the_board_only_turns_while_it_waits_in_front_of_the_blade() {
        let mut station = TableSawStation::default();
        push_to(&mut station, 2);
        assert!(!station.turn_left() && !station.flip());

        station.move_to(START);
        assert!(station.turn_left());
    }

    #[test]
    fn flipped_over_the_groove_goes_into_the_top_face() {
        let mut station = TableSawStation::default();
        assert!(station.flip());
        station.set_height(Length::from_inch_fraction(1, 4));
        push_through(&mut station);

        let board = station.boards().whole.unwrap().board;
        assert!(!board.kerfs()[0].from_below());
    }

    #[test]
    fn a_crosscut_through_a_rip_slot_sets_a_strip_loose() {
        let mut station = TableSawStation::default();
        // A stopped rip, through the board for its first 9-9/16" or so.
        push_to(&mut station, 8);
        station.move_to(START);
        // Turned, the fence is at the board's far end: crosscut 19" from it, 5" from the
        // slotted end.
        assert!(station.turn_left());
        station.set_fence(Length::from_inches(19));
        push_through(&mut station);

        assert!(station.has_loose());
        let loose = station.take_loose();
        assert_eq!(loose.len(), 1);
        assert_eq!(loose[0].to_string(), "3/4\" x 1-1/8\" x 4-7/8\"");
        assert!(!station.has_loose());
        assert!(
            station
                .status()
                .ends_with("Set aside on the pile by the miter saw.")
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

//! The miter saw station's state: the saw, the stop block, the board against it, how far the
//! blade is down, and the parts pile behind the fence.
//!
//! Positions are in meters, in the saw's own frame: across its table toward the fence (x), up
//! from the table (y), and along the fence toward the stop (z). The origin is where the blade's
//! stop-side face meets the fence face, at table height; the station is that frame turned a
//! quarter turn, by `board_turn`, so the fence runs left to right in front of the operator.
//!
//! A board lies as its `Orientation` says: unturned, its length runs along the fence with its
//! `l = length` end against the stop and its `w = width` edge against the fence.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;
use woodsim::{Blade, Board, Cut, CutError, Length, MiterSaw, Orientation};

use crate::shop::{Part, Placed, SawBoards, describe_board, describe_cut, inches};

pub const STOP_MIN: Length = Length::from_inch_fraction(1, 16);
pub const STOP_MAX: Length = Length::from_inches(36);

/// The parts pile is stacked flat behind the fence, starting near the bench's left end.
const PILE_LEFT: f32 = inches(-38.0);
const PILE_FRONT: f32 = inches(-1.25);
/// A sliver of space between stacked parts so each one reads as its own board.
const PILE_GAP: f32 = inches(1.0 / 16.0);

/// Turns the saw's own frame into the station's: along the fence is +x, toward the stop, and
/// toward the fence is -z, away from the operator.
pub fn board_turn() -> Quat {
    Quat::from_rotation_y(FRAC_PI_2)
}

#[derive(Resource)]
pub struct MiterSawStation {
    pub saw: MiterSaw,
    /// Distance from the blade to the stop block: the length the next cut leaves.
    pub stop: Length,
    pub work: Work,
    /// How far down the blade is, from 0 (all the way up) to 1 (through the table).
    pub lowered: f32,
    /// Finished parts set aside behind the fence, in the order they were set there.
    pub parts: Vec<Board>,
    /// How the board on the saw lies.
    lie: Orientation,
}

pub enum Work {
    /// Nothing on the saw.
    Empty,
    /// A board against the stop block, waiting for the blade. `missed` says why the last
    /// stroke didn't cut it, until the stop moves.
    Ready {
        board: Board,
        missed: Option<CutError>,
    },
    /// A board the blade has cut, with the stop where it was at the time.
    Cut {
        board: Board,
        stop: Length,
        cut: Cut,
    },
}

impl Default for MiterSawStation {
    fn default() -> MiterSawStation {
        MiterSawStation {
            saw: MiterSaw {
                blade: Blade {
                    diameter: Length::from_inches(12),
                    kerf: Blade::FULL_KERF,
                },
                capacity: Length::from_inch_fraction(15, 2),
            },
            stop: Length::from_inches(12),
            work: Work::Empty,
            lowered: 0.0,
            parts: Vec::new(),
            lie: Orientation::default(),
        }
    }
}

impl MiterSawStation {
    /// Puts `board` against the stop block, unturned, setting aside whatever was on the saw.
    pub fn load(&mut self, board: Board) {
        self.clear_saw();
        self.lie = Orientation::default();
        self.work = Work::Ready {
            board,
            missed: None,
        };
    }

    /// Turns the board against the stop a quarter turn to the left, seen from above. Returns
    /// false unless there's a board waiting against the stop with the blade up.
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

    fn relay(&mut self, lie: Orientation) -> bool {
        if !self.blade_up() {
            return false;
        }
        let Work::Ready { missed, .. } = &mut self.work else {
            return false;
        };
        *missed = None;
        self.lie = lie;
        true
    }

    /// Moves the stop block, within its range. It stays put while the blade is down.
    pub fn set_stop(&mut self, stop: Length) {
        if !self.blade_up() {
            return;
        }
        self.stop = stop.clamp(STOP_MIN, STOP_MAX);
        if let Work::Ready { missed, .. } = &mut self.work {
            *missed = None;
        }
    }

    /// Lowers the blade by `amount` of its travel. Reaching the bottom cuts the board. Strips a
    /// slot sets loose fall onto the parts pile.
    pub fn lower(&mut self, amount: f32) {
        self.lowered = (self.lowered + amount).min(1.0);
        if self.lowered < 1.0 {
            return;
        }
        if let Work::Ready { board, missed } = &mut self.work {
            match self.saw.crosscut(board, self.lie, self.stop) {
                Ok(cut) => {
                    self.parts
                        .extend(cut.loose.iter().map(|piece| piece.board.clone()));
                    self.work = Work::Cut {
                        board: board.clone(),
                        stop: self.stop,
                        cut,
                    }
                }
                Err(error) => *missed = Some(error),
            }
        }
    }

    /// Raises the blade by `amount` of its travel.
    pub fn raise(&mut self, amount: f32) {
        self.lowered = (self.lowered - amount).max(0.0);
    }

    /// Whether `part` is on the saw and can be handled: only with the blade up.
    pub fn can_take(&self, part: Part) -> bool {
        self.blade_up()
            && match (&self.work, part) {
                (Work::Ready { .. }, Part::Whole) | (Work::Cut { .. }, Part::Piece) => true,
                (Work::Cut { cut, .. }, Part::Offcut) => cut.offcut.is_some(),
                _ => false,
            }
    }

    /// Lifts a board off the saw to carry it elsewhere: the board against the stop, or one side
    /// of the last cut, with the other side set aside on the pile.
    pub fn take(&mut self, part: Part) -> Option<Board> {
        if !self.can_take(part) {
            return None;
        }
        match std::mem::replace(&mut self.work, Work::Empty) {
            Work::Empty => None,
            Work::Ready { board, .. } => Some(board),
            Work::Cut { cut, .. } => {
                let (taken, other) = match (part, cut.offcut) {
                    (Part::Offcut, Some(offcut)) => (offcut, Some(cut.piece)),
                    (_, offcut) => (cut.piece, offcut),
                };
                self.parts.extend(other.map(|piece| piece.board));
                Some(taken.board)
            }
        }
    }

    /// Slides one side of the last cut back against the stop for another cut, and sets the other
    /// side aside. Returns false if there's no such piece or the blade is down.
    pub fn pick_up(&mut self, part: Part) -> bool {
        if !self.blade_up() {
            return false;
        }
        let Work::Cut { cut, .. } = &self.work else {
            return false;
        };
        let (picked, other) = match (part, &cut.offcut) {
            (Part::Piece, offcut) => (&cut.piece, offcut.as_ref()),
            (Part::Offcut, Some(offcut)) => (offcut, Some(&cut.piece)),
            _ => return false,
        };
        let picked = picked.board.clone();
        self.parts.extend(other.map(|piece| piece.board.clone()));
        self.work = Work::Ready {
            board: picked,
            missed: None,
        };
        true
    }

    /// Sets everything on the saw aside on the parts pile. Returns false if the blade is down.
    pub fn set_aside(&mut self) -> bool {
        if !self.blade_up() {
            return false;
        }
        self.clear_saw();
        true
    }

    /// Whether parts can come off the pile: only with the blade up.
    pub fn can_take_from_pile(&self) -> bool {
        self.blade_up()
    }

    /// Puts the part at `index` on the pile back against the stop, setting aside whatever was on
    /// the saw. Returns false if there's no such part or the blade is down.
    pub fn take_from_pile(&mut self, index: usize) -> bool {
        if !self.can_take_from_pile() || index >= self.parts.len() {
            return false;
        }
        let board = self.parts.remove(index);
        self.load(board);
        true
    }

    /// Lifts the part at `index` off the pile to carry it elsewhere.
    pub fn remove_from_pile(&mut self, index: usize) -> Option<Board> {
        (index < self.parts.len()).then(|| self.parts.remove(index))
    }

    fn clear_saw(&mut self) {
        match std::mem::replace(&mut self.work, Work::Empty) {
            Work::Empty => {}
            Work::Ready { board, .. } => self.parts.push(board),
            Work::Cut { cut, .. } => self.parts.extend(
                [Some(cut.piece), cut.offcut]
                    .into_iter()
                    .flatten()
                    .map(|piece| piece.board),
            ),
        }
    }

    fn blade_up(&self) -> bool {
        self.lowered <= 0.0
    }

    pub fn boards(&self) -> SawBoards {
        let (board, stop, cut) = match &self.work {
            Work::Empty => return SawBoards::default(),
            Work::Ready { board, .. } => (board, self.stop, None),
            Work::Cut { board, stop, cut } => (board, *stop, Some(cut)),
        };
        // The board's far end along the fence rests against the stop block; the rest reaches
        // left past the blade. It reaches from the fence out toward the operator.
        let (across, _) = self.lie.across();
        let (along, _) = self.lie.along();
        let near = Vec3::new(
            -across.of(board).as_meters_f32(),
            0.0,
            stop.as_meters_f32() - along.of(board).as_meters_f32(),
        );
        let placed = Placed::lying(board, self.lie, near);
        match cut {
            None => SawBoards {
                whole: Some(placed),
                ..default()
            },
            // Each piece sits where it was on the board. Loose ones are on the pile.
            Some(cut) => SawBoards {
                whole: None,
                piece: Some(placed.piece(&cut.piece)),
                offcut: cut.offcut.as_ref().map(|offcut| placed.piece(offcut)),
            },
        }
    }

    /// Where each part on the pile sits: stacked flat behind the fence, oldest at the bottom,
    /// with their length along the fence. The grain shifts from part to part, so they don't
    /// all look like copies of each other.
    pub fn pile(&self) -> Vec<Placed> {
        let mut bottom = 0.0;
        self.parts
            .iter()
            .enumerate()
            .map(|(index, part)| {
                let mut placed = Placed::lying(
                    part,
                    Orientation::default(),
                    Vec3::new(-PILE_FRONT, bottom, PILE_LEFT),
                );
                placed.grain = Vec3::new(0.11, 0.0, 0.37) * index as f32;
                bottom += part.thickness().as_meters_f32() + PILE_GAP;
                placed
            })
            .collect()
    }

    /// The parts pile as a cut list, with identical parts counted together.
    pub fn cut_list(&self) -> String {
        let mut counts: Vec<(&Board, usize)> = Vec::new();
        for part in &self.parts {
            match counts.iter_mut().find(|(board, _)| *board == part) {
                Some((_, count)) => *count += 1,
                None => counts.push((part, 1)),
            }
        }
        let mut list = match self.parts.len() {
            0 => "Cut list: no parts set aside yet".to_string(),
            1 => "Cut list: 1 part set aside".to_string(),
            n => format!("Cut list: {n} parts set aside"),
        };
        for (board, count) in counts {
            let pieces = if count == 1 { "pc" } else { "pcs" };
            list.push_str(&format!("\n{count} {pieces}: {}", describe_board(board)));
        }
        list
    }

    /// One line describing what's on the saw.
    pub fn status(&self) -> String {
        match &self.work {
            Work::Empty if self.parts.is_empty() => {
                "Nothing on the saw yet. Rip a board at the table saw and carry a piece over."
                    .to_string()
            }
            Work::Empty => {
                "Nothing on the saw. Carry another piece over from the table saw.".to_string()
            }
            Work::Ready {
                missed: Some(error),
                ..
            } => format!("No cut: {error}."),
            Work::Ready { board, .. } => format!(
                "Board on the saw: {}. Hold Space to lower the blade.",
                describe_board(board)
            ),
            Work::Cut { cut, .. } if cut.loose.is_empty() => describe_cut("Cut", cut),
            Work::Cut { cut, .. } => format!("{} Set aside on the pile.", describe_cut("Cut", cut)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "{actual} m is not {expected} m"
        );
    }

    /// The piece a 6" rip leaves from a 1x8: 3/4" x 6" x 24".
    fn ripped_piece() -> Board {
        Board::new(
            Length::from_inches(24),
            Length::from_inches(6),
            Length::from_inch_fraction(3, 4),
        )
    }

    fn loaded() -> MiterSawStation {
        let mut station = MiterSawStation::default();
        station.load(ripped_piece());
        station
    }

    /// Brings the blade all the way down and back up.
    fn chop(station: &mut MiterSawStation) {
        station.lower(1.0);
        station.raise(1.0);
    }

    #[test]
    fn empty_saw_shows_no_board() {
        assert_eq!(MiterSawStation::default().boards(), SawBoards::default());
    }

    /// Where a board on the saw starts and ends along the fence.
    fn x_span(placed: &Placed) -> (f32, f32) {
        let (near, far) = placed.bounds();
        (near.z, far.z)
    }

    #[test]
    fn loaded_board_rests_against_the_stop() {
        let whole = loaded().boards().whole.unwrap();

        assert_close(x_span(&whole).0, inches(-12.0));
        assert_close(x_span(&whole).1, inches(12.0));
        // It reaches from the fence out a board's width toward the operator.
        let (near, far) = whole.bounds();
        assert_close(near.x, inches(-6.0));
        assert_close(far.x, 0.0);
    }

    #[test]
    fn blade_cuts_only_when_it_reaches_the_bottom() {
        let mut station = loaded();
        station.lower(0.5);
        assert!(matches!(station.work, Work::Ready { .. }));

        station.lower(0.5);
        let boards = station.boards();
        assert_eq!(boards.whole, None);
        let piece = boards.piece.unwrap();
        assert_close(x_span(&piece).0, 0.0);
        assert_close(x_span(&piece).1, inches(12.0));
        let offcut = boards.offcut.unwrap();
        assert_close(x_span(&offcut).0, inches(-12.0));
        assert_close(x_span(&offcut).1, inches(-0.125));
        assert_eq!(
            station.status(),
            "Cut a 3/4\" x 6\" x 12\" piece, plus a 3/4\" x 6\" x 11-7/8\" offcut. \
             The blade turned 1/8\" to sawdust."
        );
    }

    #[test]
    fn stop_stays_put_while_the_blade_is_down() {
        let mut station = loaded();
        station.lower(1.0);
        station.set_stop(Length::from_inches(20));
        assert_eq!(station.stop, Length::from_inches(12));

        station.raise(1.0);
        station.set_stop(Length::from_inches(20));
        assert_eq!(station.stop, Length::from_inches(20));
        // The pieces already cut stay where they were cut.
        assert_close(x_span(&station.boards().piece.unwrap()).1, inches(12.0));
    }

    #[test]
    fn stop_stays_within_its_range() {
        let mut station = MiterSawStation::default();
        station.set_stop(Length::ZERO);
        assert_eq!(station.stop, STOP_MIN);
        station.set_stop(Length::from_inches(100));
        assert_eq!(station.stop, STOP_MAX);
    }

    #[test]
    fn after_a_miss_the_board_follows_the_stop_and_can_be_cut() {
        let mut station = loaded();
        station.set_stop(Length::from_inches(30));
        chop(&mut station);
        assert!(station.boards().whole.is_some());
        assert_eq!(
            station.status(),
            "No cut: the blade misses the board: set to 30\", but the board is only 24\"."
        );

        station.set_stop(Length::from_inches(12));
        assert_close(x_span(&station.boards().whole.unwrap()).1, inches(12.0));
        assert!(station.status().starts_with("Board on the saw"));
        chop(&mut station);
        assert!(matches!(station.work, Work::Cut { .. }));
    }

    #[test]
    fn picking_up_one_side_puts_it_against_the_stop_and_sets_the_other_aside() {
        let mut station = loaded();
        chop(&mut station);
        assert!(station.pick_up(Part::Offcut));

        let (start, end) = x_span(&station.boards().whole.unwrap());
        assert_close(end, inches(12.0));
        assert_close(end - start, inches(11.875));
        assert_eq!(station.parts.len(), 1);
        assert_eq!(station.parts[0].length(), Length::from_inches(12));
    }

    #[test]
    fn nothing_can_be_picked_up_with_the_blade_down() {
        let mut station = loaded();
        station.lower(1.0);
        assert!(!station.can_take(Part::Piece));
        assert!(!station.pick_up(Part::Piece));
        assert_eq!(station.take(Part::Piece), None);
        assert!(!station.set_aside());
        assert!(station.parts.is_empty());
        assert!(matches!(station.work, Work::Cut { .. }));
    }

    #[test]
    fn taking_the_board_against_the_stop_empties_the_saw() {
        let mut station = loaded();

        assert_eq!(station.take(Part::Piece), None);
        assert_eq!(station.take(Part::Whole), Some(ripped_piece()));
        assert!(matches!(station.work, Work::Empty));
        assert!(station.parts.is_empty());
    }

    #[test]
    fn taking_one_side_of_a_cut_sets_the_other_aside() {
        let mut station = loaded();
        chop(&mut station);
        let taken = station.take(Part::Piece).unwrap();

        assert_eq!(taken.length(), Length::from_inches(12));
        assert!(matches!(station.work, Work::Empty));
        assert_eq!(station.parts.len(), 1);
        assert_eq!(station.parts[0].length(), Length::from_inch_fraction(95, 8));
    }

    #[test]
    fn parts_can_be_lifted_off_the_pile() {
        let mut station = loaded();
        station.set_aside();

        assert_eq!(station.remove_from_pile(1), None);
        assert_eq!(station.remove_from_pile(0), Some(ripped_piece()));
        assert!(station.parts.is_empty());
    }

    #[test]
    fn repeat_cuts_turn_one_board_into_parts() {
        let mut station = loaded();
        station.set_stop(Length::from_inches(6));
        for _ in 0..2 {
            chop(&mut station);
            assert!(station.pick_up(Part::Offcut));
        }
        chop(&mut station);
        assert!(station.set_aside());

        assert!(matches!(station.work, Work::Empty));
        // Three 6" parts, what's left, and three kerfs add back up to the 24" board.
        let total: Length = station.parts.iter().map(Board::length).sum();
        assert_eq!(
            total + Length::from_inch_fraction(3, 8),
            Length::from_inches(24)
        );
        assert_eq!(
            station.cut_list(),
            "Cut list: 4 parts set aside\n\
             3 pcs: 3/4\" x 6\" x 6\"\n\
             1 pc: 3/4\" x 6\" x 5-5/8\""
        );
    }

    #[test]
    fn loading_a_board_sets_aside_whatever_was_on_the_saw() {
        let mut station = loaded();
        chop(&mut station);
        station.load(ripped_piece());

        assert!(matches!(station.work, Work::Ready { .. }));
        assert_eq!(station.parts.len(), 2);
    }

    #[test]
    fn pile_stacks_parts_behind_the_fence() {
        let mut station = loaded();
        chop(&mut station);
        station.set_aside();
        let pile = station.pile();

        assert_eq!(pile.len(), 2);
        for part in &pile {
            let (near, _) = part.bounds();
            assert_close(near.z, PILE_LEFT);
            // Behind the fence, which is at x = 0.
            assert_close(near.x, -PILE_FRONT);
        }
        assert_close(pile[0].bounds().0.y, 0.0);
        assert_close(pile[1].bounds().0.y, inches(0.75) + PILE_GAP);
    }

    #[test]
    fn taking_a_part_off_the_pile_swaps_it_with_whatever_is_on_the_saw() {
        let mut station = loaded();
        chop(&mut station);
        station.set_aside();
        station.load(ripped_piece());
        // The pile is now the 12" piece, then the 11-7/8" offcut; the 24" board is on the saw.

        assert!(station.take_from_pile(1));
        let Work::Ready { board, .. } = &station.work else {
            panic!("the offcut should be against the stop");
        };
        assert_eq!(board.length(), Length::from_inch_fraction(95, 8));
        let lengths: Vec<Length> = station.parts.iter().map(Board::length).collect();
        assert_eq!(lengths, [Length::from_inches(12), Length::from_inches(24)]);
    }

    #[test]
    fn nothing_comes_off_the_pile_with_the_blade_down_or_past_its_end() {
        let mut station = loaded();
        station.set_aside();
        assert!(!station.take_from_pile(1));

        station.lower(0.5);
        assert!(!station.take_from_pile(0));
        assert_eq!(station.parts.len(), 1);
    }

    /// A 1x8 off the table saw with a stopped cut 6" from its fence edge, slotted through for
    /// about its first 11-9/16".
    fn slotted_board() -> Board {
        let saw = woodsim::TableSaw {
            blade: Blade {
                diameter: Length::from_inches(10),
                kerf: Blade::FULL_KERF,
            },
            fence: Length::from_inches(6),
            height: Length::from_inches(1),
        };
        let board = Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(29, 4),
            Length::from_inch_fraction(3, 4),
        );
        match saw.feed(
            &board,
            Orientation::default(),
            -Length::from_inches(6),
            Length::from_inches(10),
        ) {
            Ok(woodsim::Pass::Kerfed(board)) => board,
            other => panic!("expected a stopped cut, got {other:?}"),
        }
    }

    #[test]
    fn a_crosscut_through_a_slot_sets_the_loose_strip_aside() {
        let mut station = MiterSawStation::default();
        station.load(slotted_board());
        station.set_stop(Length::from_inches(14));
        chop(&mut station);

        // The other 9-7/8" fell apart along the slot. The narrow strip is on the pile.
        assert_eq!(station.parts.len(), 1);
        assert_eq!(station.parts[0].to_string(), "3/4\" x 1-1/8\" x 9-7/8\"");
        assert!(station.status().ends_with(
            "The slot already in the board split off a 3/4\" x 1-1/8\" x 9-7/8\". \
             Set aside on the pile."
        ));
        // The offcut is the 6" strip, still against the fence where it was on the board.
        let offcut = station.boards().offcut.unwrap();
        assert_eq!(offcut.board.width(), Length::from_inches(6));
        let (near, far) = offcut.bounds();
        assert_close(far.x, 0.0);
        assert_close(near.x, inches(-6.0));
    }

    #[test]
    fn a_board_turned_across_the_saw_is_too_wide_to_cut() {
        let mut station = loaded();
        assert!(station.turn_left());
        chop(&mut station);

        assert!(matches!(station.work, Work::Ready { .. }));
        assert_eq!(
            station.status(),
            "No cut: the blade can't reach across 24\" of board; it cuts boards up to 7-1/2\" \
             wide."
        );
    }

    #[test]
    fn turned_end_for_end_the_slotted_end_goes_against_the_stop() {
        let mut station = MiterSawStation::default();
        station.load(slotted_board());
        assert!(station.turn_left() && station.turn_left());
        station.set_stop(Length::from_inches(10));
        chop(&mut station);

        // The 10" against the stop falls apart along the slot; its narrow strip goes on the pile.
        assert_eq!(station.parts.len(), 1);
        assert_eq!(station.parts[0].to_string(), "3/4\" x 1-1/8\" x 10\"");
        let piece = station.boards().piece.unwrap();
        assert_eq!(piece.board.to_string(), "3/4\" x 6\" x 10\"");
        assert_close(x_span(&piece).1, inches(10.0));
    }

    #[test]
    fn empty_pile_says_so() {
        assert_eq!(
            MiterSawStation::default().cut_list(),
            "Cut list: no parts set aside yet"
        );
    }
}

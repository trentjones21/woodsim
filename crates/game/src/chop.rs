//! The miter saw station's state: the saw, the stop block, the board against it, how far the
//! blade is down, and the parts pile behind the fence.
//!
//! Positions are in meters, in the station's own coordinates: x = 0 is the blade's stop-side
//! face with +x toward the stop block, y = 0 is the saw table, and z = 0 is the fence face with
//! +z toward the operator.

use bevy::prelude::*;
use woodsim::{Blade, Board, Cut, CutError, Length, MiterSaw};

use crate::shop::{Block, BoardBlocks, Part, describe_cut, inches};

pub const STOP_MIN: Length = Length::from_inch_fraction(1, 16);
pub const STOP_MAX: Length = Length::from_inches(36);

/// The parts pile is stacked flat behind the fence, starting near the bench's left end.
const PILE_LEFT: f32 = inches(-38.0);
const PILE_FRONT: f32 = inches(-1.25);
/// A sliver of space between stacked parts so each one reads as its own board.
const PILE_GAP: f32 = inches(1.0 / 16.0);

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
                blade: Blade::FULL_KERF,
            },
            stop: Length::from_inches(12),
            work: Work::Empty,
            lowered: 0.0,
            parts: Vec::new(),
        }
    }
}

impl MiterSawStation {
    /// Puts `board` against the stop block, setting aside whatever was on the saw.
    pub fn load(&mut self, board: Board) {
        self.clear_saw();
        self.work = Work::Ready {
            board,
            missed: None,
        };
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

    /// Lowers the blade by `amount` of its travel. Reaching the bottom cuts the board.
    pub fn lower(&mut self, amount: f32) {
        self.lowered = (self.lowered + amount).min(1.0);
        if self.lowered < 1.0 {
            return;
        }
        if let Work::Ready { board, missed } = &mut self.work {
            match self.saw.crosscut(board, self.stop) {
                Ok(cut) => {
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
                (Work::Ready { .. }, Part::Uncut) | (Work::Cut { .. }, Part::Piece) => true,
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
                self.parts.extend(other);
                Some(taken)
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
            (Part::Piece, offcut) => (cut.piece.clone(), offcut.clone()),
            (Part::Offcut, Some(offcut)) => (offcut.clone(), Some(cut.piece.clone())),
            _ => return false,
        };
        self.parts.extend(other);
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
            Work::Cut { cut, .. } => self
                .parts
                .extend([Some(cut.piece), cut.offcut].into_iter().flatten()),
        }
    }

    fn blade_up(&self) -> bool {
        self.lowered <= 0.0
    }

    pub fn board_blocks(&self) -> BoardBlocks {
        let (board, stop, cut) = match &self.work {
            Work::Empty => return BoardBlocks::default(),
            Work::Ready { board, .. } => (board, self.stop, None),
            Work::Cut { board, stop, cut } => (board, *stop, Some(cut)),
        };
        // The board's right end rests against the stop block; the rest reaches left past the blade.
        let stop = stop.as_meters_f32();
        let left_end = stop - board.length().as_meters_f32();
        let width = board.width().as_meters_f32();
        let thickness = board.thickness().as_meters_f32();
        let block = |left: f32, right: f32| Block {
            min: Vec3::new(left, 0.0, 0.0),
            max: Vec3::new(right, thickness, width),
        };
        let grain_origin = Vec3::new(left_end, 0.0, 0.0);
        match cut {
            None => BoardBlocks {
                uncut: Some(block(left_end, stop)),
                grain_origin,
                ..default()
            },
            Some(cut) => BoardBlocks {
                uncut: None,
                piece: Some(block(stop - cut.piece.length().as_meters_f32(), stop)),
                offcut: cut
                    .offcut
                    .as_ref()
                    .map(|offcut| block(left_end, left_end + offcut.length().as_meters_f32())),
                grain_origin,
            },
        }
    }

    /// Where each part on the pile sits: stacked flat behind the fence, oldest at the bottom.
    pub fn pile_blocks(&self) -> Vec<Block> {
        let mut bottom = 0.0;
        self.parts
            .iter()
            .map(|part| {
                let top = bottom + part.thickness().as_meters_f32();
                let block = Block {
                    min: Vec3::new(PILE_LEFT, bottom, PILE_FRONT - part.width().as_meters_f32()),
                    max: Vec3::new(PILE_LEFT + part.length().as_meters_f32(), top, PILE_FRONT),
                };
                bottom = top + PILE_GAP;
                block
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
            list.push_str(&format!("\n{count} {pieces}: {board}"));
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
            Work::Ready { board, .. } => {
                format!("Board on the saw: {board}. Hold Space to lower the blade.")
            }
            Work::Cut { cut, .. } => describe_cut("Cut", cut),
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
        assert_eq!(
            MiterSawStation::default().board_blocks(),
            BoardBlocks::default()
        );
    }

    #[test]
    fn loaded_board_rests_against_the_stop() {
        let uncut = loaded().board_blocks().uncut.unwrap();

        assert_close(uncut.min.x, inches(-12.0));
        assert_close(uncut.max.x, inches(12.0));
        assert_close(uncut.max.z, inches(6.0));
    }

    #[test]
    fn blade_cuts_only_when_it_reaches_the_bottom() {
        let mut station = loaded();
        station.lower(0.5);
        assert!(matches!(station.work, Work::Ready { .. }));

        station.lower(0.5);
        let blocks = station.board_blocks();
        assert_eq!(blocks.uncut, None);
        let piece = blocks.piece.unwrap();
        assert_close(piece.min.x, 0.0);
        assert_close(piece.max.x, inches(12.0));
        let offcut = blocks.offcut.unwrap();
        assert_close(offcut.min.x, inches(-12.0));
        assert_close(offcut.max.x, inches(-0.125));
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
        assert_close(station.board_blocks().piece.unwrap().max.x, inches(12.0));
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
        assert!(station.board_blocks().uncut.is_some());
        assert_eq!(
            station.status(),
            "No cut: the blade misses the board: set to 30\", but the board is only 24\"."
        );

        station.set_stop(Length::from_inches(12));
        assert_close(station.board_blocks().uncut.unwrap().max.x, inches(12.0));
        assert!(station.status().starts_with("Board on the saw"));
        chop(&mut station);
        assert!(matches!(station.work, Work::Cut { .. }));
    }

    #[test]
    fn picking_up_one_side_puts_it_against_the_stop_and_sets_the_other_aside() {
        let mut station = loaded();
        chop(&mut station);
        assert!(station.pick_up(Part::Offcut));

        let uncut = station.board_blocks().uncut.unwrap();
        assert_close(uncut.max.x, inches(12.0));
        assert_close(uncut.max.x - uncut.min.x, inches(11.875));
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
        assert_eq!(station.take(Part::Uncut), Some(ripped_piece()));
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
        let blocks = station.pile_blocks();

        assert_eq!(blocks.len(), 2);
        for block in &blocks {
            assert_close(block.min.x, PILE_LEFT);
            assert_close(block.max.z, PILE_FRONT);
        }
        assert_close(blocks[0].min.y, 0.0);
        assert_close(blocks[1].min.y, blocks[0].max.y + PILE_GAP);
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

    #[test]
    fn empty_pile_says_so() {
        assert_eq!(
            MiterSawStation::default().cut_list(),
            "Cut list: no parts set aside yet"
        );
    }
}

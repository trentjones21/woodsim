//! The miter saw station's state: the saw, the stop block, the board against it, and how far
//! the blade is down.
//!
//! Positions are in meters, in the station's own coordinates: x = 0 is the blade's stop-side
//! face with +x toward the stop block, y = 0 is the saw table, and z = 0 is the fence face with
//! +z toward the operator.

use bevy::prelude::*;
use woodsim::{Blade, Board, Cut, CutError, Length, MiterSaw};

use crate::shop::{Block, BoardBlocks, describe_cut};

pub const STOP_MIN: Length = Length::from_inch_fraction(1, 16);
pub const STOP_MAX: Length = Length::from_inches(36);

#[derive(Resource)]
pub struct MiterSawStation {
    pub saw: MiterSaw,
    /// Distance from the blade to the stop block: the length the next cut leaves.
    pub stop: Length,
    pub work: Work,
    /// How far down the blade is, from 0 (all the way up) to 1 (through the table).
    pub lowered: f32,
}

pub enum Work {
    /// Nothing on the saw.
    Empty,
    /// A board against the stop block, waiting for the blade.
    Ready(Board),
    /// A board the blade has come down on, with the stop where it was at the time.
    Cut {
        board: Board,
        stop: Length,
        result: Result<Cut, CutError>,
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
        }
    }
}

impl MiterSawStation {
    /// Puts `board` against the stop block, replacing whatever was on the saw.
    pub fn load(&mut self, board: Board) {
        self.work = Work::Ready(board);
    }

    /// Moves the stop block, within its range. It stays put while the blade is down.
    pub fn set_stop(&mut self, stop: Length) {
        if self.lowered <= 0.0 {
            self.stop = stop.clamp(STOP_MIN, STOP_MAX);
        }
    }

    /// Lowers the blade by `amount` of its travel. Reaching the bottom cuts the board.
    pub fn lower(&mut self, amount: f32) {
        self.lowered = (self.lowered + amount).min(1.0);
        if self.lowered >= 1.0
            && let Work::Ready(board) = &self.work
        {
            self.work = Work::Cut {
                result: self.saw.crosscut(board, self.stop),
                board: board.clone(),
                stop: self.stop,
            };
        }
    }

    /// Raises the blade by `amount` of its travel.
    pub fn raise(&mut self, amount: f32) {
        self.lowered = (self.lowered - amount).max(0.0);
    }

    pub fn board_blocks(&self) -> BoardBlocks {
        let (board, stop, cut) = match &self.work {
            Work::Empty => return BoardBlocks::default(),
            Work::Ready(board) => (board, self.stop, None),
            Work::Cut {
                board,
                stop,
                result,
            } => (board, *stop, result.as_ref().ok()),
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
        match cut {
            None => BoardBlocks {
                uncut: Some(block(left_end, stop)),
                ..default()
            },
            Some(cut) => BoardBlocks {
                uncut: None,
                piece: Some(block(stop - cut.piece.length().as_meters_f32(), stop)),
                offcut: cut
                    .offcut
                    .as_ref()
                    .map(|offcut| block(left_end, left_end + offcut.length().as_meters_f32())),
            },
        }
    }

    /// One line describing what's on the saw.
    pub fn status(&self) -> String {
        match &self.work {
            Work::Empty => {
                "Nothing on the saw yet. Rip a board at the table saw and carry a piece over."
                    .to_string()
            }
            Work::Ready(board) => {
                format!("Board on the saw: {board}. Hold Space to lower the blade.")
            }
            Work::Cut {
                result: Err(error), ..
            } => format!("No cut: {error}."),
            Work::Cut {
                result: Ok(cut), ..
            } => describe_cut("Cut", cut),
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
        assert!(matches!(station.work, Work::Ready(_)));

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
    fn stop_past_the_board_end_misses_the_board() {
        let mut station = loaded();
        station.set_stop(Length::from_inches(30));
        station.lower(1.0);

        assert!(station.board_blocks().uncut.is_some());
        assert_eq!(
            station.status(),
            "No cut: the blade misses the board: set to 30\", but the board is only 24\"."
        );
    }

    #[test]
    fn loading_a_board_replaces_the_last_cut() {
        let mut station = loaded();
        station.lower(1.0);
        station.raise(1.0);
        station.load(ripped_piece());

        assert!(matches!(station.work, Work::Ready(_)));
        assert!(station.board_blocks().uncut.is_some());
    }
}

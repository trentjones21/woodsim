//! The table saw station's state: the saw, the board on it, and how far it has been pushed.
//!
//! `woodsim` decides what the pieces are. This module decides where to draw them while the
//! board moves past the blade. Positions are in meters, in table space: x = 0 is the blade's
//! fence-side face with +x toward the fence, y = 0 is the table top, and the board feeds
//! toward -z, away from the operator.

use bevy::prelude::*;
use woodsim::{Blade, Board, Cut, CutError, Length, TableSaw};

use crate::shop::{Block, BoardBlocks, Part, describe_cut, inches};

/// A 10" blade.
pub const BLADE_RADIUS: f32 = inches(5.0);
/// How far the blade's center sits below the table top, leaving 1-1/4" of blade above it.
pub const BLADE_DROP: f32 = inches(3.75);
/// Where the blade's teeth come up through the table on the infeed side: sqrt(5² − 3.75²).
const BLADE_FRONT: f32 = inches(3.307);
/// Where the board's leading edge waits before it's pushed: just short of the blade.
const START_Z: f32 = BLADE_FRONT + inches(1.0);

pub const FENCE_MIN: Length = Length::from_inch_fraction(1, 16);
pub const FENCE_MAX: Length = Length::from_inches(24);

#[derive(Resource)]
pub struct TableSawStation {
    pub saw: TableSaw,
    /// The board on the saw. It's replaced only when the player asks for a new one.
    pub stock: Board,
    pub feed: Feed,
    /// Pieces already carried off the outfeed side.
    taken: Vec<Part>,
}

pub enum Feed {
    /// The board waits on the infeed side, against the fence.
    Ready,
    /// The board is being pushed. The cut was decided when it started moving, with the fence
    /// where it was then. `fed` is how far the board has moved, in meters.
    Pushing {
        fence: Length,
        result: Result<Cut, CutError>,
        fed: f32,
    },
}

impl Default for TableSawStation {
    fn default() -> TableSawStation {
        TableSawStation {
            saw: TableSaw {
                blade: Blade::FULL_KERF,
                fence: Length::from_inches(6),
            },
            stock: new_stock(),
            feed: Feed::Ready,
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
    /// Moves the fence, within its range. The fence stays put while a board is mid-cut.
    pub fn set_fence(&mut self, fence: Length) {
        if !self.mid_cut() {
            self.saw.fence = fence.clamp(FENCE_MIN, FENCE_MAX);
        }
    }

    /// Pushes the board `distance` meters. The first push decides the cut.
    pub fn push(&mut self, distance: f32) {
        if let Feed::Ready = self.feed {
            self.feed = Feed::Pushing {
                fence: self.saw.fence,
                result: self.saw.rip(&self.stock),
                fed: 0.0,
            };
        }
        let travel = self.travel();
        if let Feed::Pushing { fed, .. } = &mut self.feed {
            *fed = (*fed + distance).min(travel);
        }
    }

    /// Lifts a cut piece off the outfeed side, once the board is through, to carry it elsewhere.
    pub fn take(&mut self, part: Part) -> Option<Board> {
        let Feed::Pushing {
            result: Ok(cut), ..
        } = &self.feed
        else {
            return None;
        };
        if !self.is_through() || self.taken.contains(&part) {
            return None;
        }
        let board = match part {
            Part::Piece => Some(cut.piece.clone()),
            Part::Offcut => cut.offcut.clone(),
            Part::Uncut => None,
        }?;
        self.taken.push(part);
        Some(board)
    }

    /// Clears the saw and puts a fresh board against the fence.
    pub fn new_board(&mut self) {
        self.stock = new_stock();
        self.feed = Feed::Ready;
        self.taken.clear();
    }

    /// Whether a board is partway past the blade.
    pub fn mid_cut(&self) -> bool {
        matches!(self.feed, Feed::Pushing { fed, .. } if fed < self.travel())
    }

    /// Whether the board has been pushed all the way past the blade.
    pub fn is_through(&self) -> bool {
        matches!(self.feed, Feed::Pushing { fed, .. } if fed >= self.travel())
    }

    /// How far the board moves from where it waits until it's clear of the blade.
    fn travel(&self) -> f32 {
        self.stock.length().as_meters_f32() + 2.0 * START_Z
    }

    pub fn board_blocks(&self) -> BoardBlocks {
        let width = self.stock.width().as_meters_f32();
        let length = self.stock.length().as_meters_f32();
        let thickness = self.stock.thickness().as_meters_f32();
        // `lead` is the z of the board's leading edge; the board runs from there back toward +z.
        let (fence, lead, cut) = match &self.feed {
            Feed::Ready => (self.saw.fence, START_Z, None),
            Feed::Pushing { fence, result, fed } => (*fence, START_Z - fed, result.as_ref().ok()),
        };
        let fence = fence.as_meters_f32();
        // The fence is at +x, so the board's edge away from the fence is its left edge.
        let left_edge = fence - width;
        let block = |left: f32, right: f32, near_z: f32, far_z: f32| Block {
            min: Vec3::new(left, 0.0, far_z),
            max: Vec3::new(right, thickness, near_z),
        };

        // Whatever has passed the blade's teeth is cut through; the rest is still one board.
        let cut_length = match cut {
            Some(_) => (BLADE_FRONT - lead).clamp(0.0, length),
            None => 0.0,
        };
        let cut = cut.filter(|_| cut_length > 0.0);
        let piece = cut.map(|cut| {
            let left = fence - cut.piece.width().as_meters_f32();
            block(left, fence, lead + cut_length, lead)
        });
        let offcut = cut.and_then(|cut| cut.offcut.as_ref()).map(|offcut| {
            let right = left_edge + offcut.width().as_meters_f32();
            block(left_edge, right, lead + cut_length, lead)
        });
        let unless_taken =
            |part: Part, block: Option<Block>| block.filter(|_| !self.taken.contains(&part));
        BoardBlocks {
            uncut: (cut_length < length)
                .then(|| block(left_edge, fence, lead + length, lead + cut_length)),
            piece: unless_taken(Part::Piece, piece),
            offcut: unless_taken(Part::Offcut, offcut),
        }
    }

    /// One line describing what's on the saw.
    pub fn status(&self) -> String {
        match &self.feed {
            Feed::Ready => format!(
                "Board on the saw: {}. Hold Space to push it through.",
                self.stock
            ),
            Feed::Pushing {
                result: Err(error), ..
            } => format!("No cut: {error}."),
            Feed::Pushing { .. } if self.mid_cut() => "Cutting...".to_string(),
            Feed::Pushing {
                result: Ok(cut), ..
            } => describe_cut("Ripped", cut),
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

    #[test]
    fn board_waits_against_the_fence_short_of_the_blade() {
        let blocks = TableSawStation::default().board_blocks();

        let uncut = blocks.uncut.unwrap();
        assert_close(uncut.min.x, inches(6.0 - 7.25));
        assert_close(uncut.max.x, inches(6.0));
        assert_close(uncut.min.z, START_Z);
        assert_eq!((blocks.piece, blocks.offcut), (None, None));
    }

    #[test]
    fn partway_through_the_part_past_the_blade_is_split() {
        let mut station = TableSawStation::default();
        station.push(START_Z - BLADE_FRONT + inches(10.0));
        let blocks = station.board_blocks();

        let piece = blocks.piece.unwrap();
        assert_close(piece.max.z - piece.min.z, inches(10.0));
        assert_close(piece.min.x, 0.0);
        assert_close(piece.max.x, inches(6.0));
        let offcut = blocks.offcut.unwrap();
        assert_close(offcut.min.x, inches(-1.25));
        assert_close(offcut.max.x, inches(-0.125));
        let uncut = blocks.uncut.unwrap();
        assert_close(uncut.min.z, piece.max.z);
        assert_close(uncut.max.z - uncut.min.z, inches(14.0));
        assert!(station.mid_cut());
        assert_eq!(station.status(), "Cutting...");
    }

    #[test]
    fn pushed_all_the_way_leaves_two_whole_pieces_clear_of_the_blade() {
        let mut station = TableSawStation::default();
        station.push(10.0);
        let blocks = station.board_blocks();

        assert!(station.is_through() && !station.mid_cut());
        assert_eq!(blocks.uncut, None);
        for block in [blocks.piece.unwrap(), blocks.offcut.unwrap()] {
            assert_close(block.max.z - block.min.z, inches(24.0));
            assert!(block.max.z < -BLADE_FRONT);
        }
        assert_eq!(
            station.status(),
            "Ripped a 3/4\" x 6\" x 24\" piece, plus a 3/4\" x 1-1/8\" x 24\" offcut. \
             The blade turned 1/8\" to sawdust."
        );
    }

    #[test]
    fn fence_stays_put_mid_cut_and_moves_again_once_through() {
        let mut station = TableSawStation::default();
        station.push(inches(10.0));
        station.set_fence(Length::from_inches(3));
        assert_eq!(station.saw.fence, Length::from_inches(6));

        station.push(10.0);
        station.set_fence(Length::from_inches(3));
        assert_eq!(station.saw.fence, Length::from_inches(3));
        // The pieces already cut stay where they were cut.
        assert_close(station.board_blocks().piece.unwrap().max.x, inches(6.0));
    }

    #[test]
    fn fence_stays_within_its_range() {
        let mut station = TableSawStation::default();
        station.set_fence(Length::ZERO);
        assert_eq!(station.saw.fence, FENCE_MIN);
        station.set_fence(Length::from_inches(100));
        assert_eq!(station.saw.fence, FENCE_MAX);
    }

    #[test]
    fn fence_past_the_board_pushes_it_by_uncut() {
        let mut station = TableSawStation::default();
        station.set_fence(Length::from_inches(8));
        station.push(10.0);
        let blocks = station.board_blocks();

        assert!(blocks.uncut.is_some());
        assert_eq!((blocks.piece, blocks.offcut), (None, None));
        assert_eq!(
            station.status(),
            "No cut: the blade misses the board: set to 8\", but the board is only 7-1/4\"."
        );
        assert_eq!(station.take(Part::Uncut), None);
    }

    #[test]
    fn pieces_can_be_taken_once_the_board_is_through() {
        let mut station = TableSawStation::default();
        station.push(inches(10.0));
        assert_eq!(station.take(Part::Piece), None);

        station.push(10.0);
        let piece = station.take(Part::Piece).unwrap();
        assert_eq!(piece.to_string(), "3/4\" x 6\" x 24\"");
        assert_eq!(station.take(Part::Piece), None);
        let blocks = station.board_blocks();
        assert_eq!(blocks.piece, None);
        assert!(blocks.offcut.is_some());
    }

    #[test]
    fn new_board_clears_the_saw() {
        let mut station = TableSawStation::default();
        station.push(10.0);
        station.take(Part::Piece);
        station.new_board();

        assert!(matches!(station.feed, Feed::Ready));
        assert!(station.board_blocks().uncut.is_some());
        assert!(station.taken.is_empty());
    }
}

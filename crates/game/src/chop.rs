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
use woodsim::{Angle, Board, Cut, CutError, Length, MiterSaw, Orientation, Pass};

use crate::shop::{
    Block, Part, Placed, SawBoards, degrees, describe_board, describe_cut, describe_kerf, inches,
};

pub const STOP_MIN: Length = Length::from_inch_fraction(1, 16);
pub const STOP_MAX: Length = Length::from_inches(36);
/// The highest the depth stop can hold the blade above the table.
const DEPTH_STOP_MAX: Length = Length::from_inches(4);
/// Where the saw's table clicks into place as it turns, either way from square.
pub const MITER_STOPS: [Angle; 5] = [
    Angle::ZERO,
    Angle::from_degrees(15),
    Angle::from_hundredths(2250),
    Angle::from_degrees(30),
    Angle::from_degrees(45),
];

/// The parts pile is stacked flat behind the fence, starting near the bench's left end, on a
/// tray (in the station's coordinates).
const PILE_LEFT: f32 = inches(-38.0);
const PILE_FRONT: f32 = inches(-1.25);
pub const PILE_TRAY: Block = Block {
    min: Vec3::new(inches(-39.0), inches(-0.05), inches(-10.0)),
    max: Vec3::new(inches(-12.0), inches(0.25), inches(-1.1)),
};
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
    /// How far down the blade has been on the stroke under way, the same way.
    deepest: f32,
    /// How high above the table the depth stop holds the blade's lowest point, if it's on.
    pub depth_stop: Option<Length>,
    /// Finished parts set aside behind the fence, in the order they were set there.
    pub parts: Vec<Board>,
    /// How the board on the saw lies.
    lie: Orientation,
}

pub enum Work {
    /// Nothing on the saw.
    Empty,
    /// A board against the stop block, waiting for the blade. `missed` says why the last
    /// stroke didn't cut it, until the stop moves. `scarred` is the board as the blade has
    /// scarred it on the stroke under way; the scar stays when the blade comes back up.
    Ready {
        board: Board,
        missed: Option<CutError>,
        scarred: Option<Board>,
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
            saw: MiterSaw::twelve_inch(),
            stop: Length::from_inches(12),
            work: Work::Empty,
            lowered: 0.0,
            deepest: 0.0,
            depth_stop: None,
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
            scarred: None,
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

    /// Stands the board up on an edge, with its bottom face toward the fence, or lays it back
    /// down.
    pub fn tip(&mut self) -> bool {
        self.relay(self.lie.tipped())
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

    /// Turns the saw's table to `miter`, as far as it goes either way: positive swings the
    /// blade's front toward the stop. Only with the blade up.
    pub fn set_miter(&mut self, miter: Angle) {
        if !self.blade_up() {
            return;
        }
        let most = self.saw.max_miter;
        self.saw.miter = miter.clamp(-most, most);
        if let Work::Ready { missed, .. } = &mut self.work {
            *missed = None;
        }
    }

    /// Turns the table to the next place it clicks into, toward the stop or away from it, or
    /// as far as it goes past the last one.
    pub fn swing_to_next_stop(&mut self, toward_stop: bool) {
        let now = self.saw.miter;
        let stops = MITER_STOPS.iter().flat_map(|&stop| [stop, -stop]);
        let most = self.saw.max_miter;
        let next = if toward_stop {
            stops.filter(|&stop| stop > now).min().unwrap_or(most)
        } else {
            stops.filter(|&stop| stop < now).max().unwrap_or(-most)
        };
        self.set_miter(next);
    }

    /// Lowers the blade by `amount` of its travel, as far as the depth stop lets it go. Into
    /// the board, it scars it; all the way through, it cuts it. Strips a slot sets loose fall
    /// onto the parts pile.
    pub fn lower(&mut self, amount: f32) {
        self.lowered = (self.lowered + amount).min(self.lowest());
        if self.lowered <= self.deepest {
            return;
        }
        self.deepest = self.lowered;
        let bottom = self.bottom();
        if let Work::Ready {
            board,
            missed,
            scarred,
        } = &mut self.work
        {
            match self.saw.chop(board, self.lie, self.stop, bottom) {
                Ok(Pass::Cut(cut)) => {
                    self.parts
                        .extend(cut.loose.iter().map(|piece| piece.board.clone()));
                    self.work = Work::Cut {
                        board: board.clone(),
                        stop: self.stop,
                        cut,
                    }
                }
                Ok(Pass::Kerfed(scar)) => *scarred = Some(scar),
                Ok(Pass::Missed) => {}
                Err(error) => *missed = Some(error),
            }
        }
    }

    /// Raises the blade by `amount` of its travel. Back at the top, the stroke is over, and any
    /// scar it left stays in the board.
    pub fn raise(&mut self, amount: f32) {
        self.lowered = (self.lowered - amount).max(0.0);
        if self.lowered > 0.0 {
            return;
        }
        self.deepest = 0.0;
        if let Work::Ready { board, scarred, .. } = &mut self.work
            && let Some(scar) = scarred.take()
        {
            *board = scar;
        }
    }

    /// How far down the blade can go, as a fraction of its travel: to the depth stop, or all
    /// the way.
    pub fn lowest(&self) -> f32 {
        let Some(stop) = self.depth_stop else {
            return 1.0;
        };
        let (up, down) = self.saw.stroke;
        ((up - stop).as_nm() as f64 / (up - down).as_nm() as f64).clamp(0.0, 1.0) as f32
    }

    /// How high above the table the blade's lowest point is now. Against the depth stop, it's
    /// exactly the stop's setting.
    pub fn bottom(&self) -> Length {
        if let Some(stop) = self.depth_stop
            && self.lowered >= self.lowest()
        {
            return stop;
        }
        let (up, down) = self.saw.stroke;
        let travel = (down - up).as_nm() as f64 * self.lowered as f64;
        up + Length::from_nm(travel.round() as i64)
    }

    /// Sets the depth stop: how high above the table the blade's lowest point stops, from the
    /// table up to 4", or `None` to let it go all the way down. Only with the blade up.
    pub fn set_depth_stop(&mut self, stop: Option<Length>) {
        if self.blade_up() {
            self.depth_stop = stop.map(|stop| stop.clamp(Length::ZERO, DEPTH_STOP_MAX));
        }
    }

    /// Whether a board can be set down on the saw: only with the blade up.
    pub fn can_load(&self) -> bool {
        self.blade_up()
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
            Work::Ready { board, scarred, .. } => {
                (scarred.as_ref().unwrap_or(board), self.stop, None)
            }
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
        let mut bottom = PILE_TRAY.max.y;
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

    /// A warning if the blade won't reach all the way across `board` as it lies, at the miter
    /// angle: it cuts as far as it reaches and leaves the board in one piece.
    fn reach_note(&self, board: &Board) -> String {
        let span = self.lie.across().0.of(board);
        let widest = self.saw.widest();
        if span <= widest {
            return String::new();
        }
        let angle = match self.saw.miter.abs() {
            square if square == Angle::ZERO => String::new(),
            miter => format!(" at {}", degrees(miter)),
        };
        format!(
            " The blade reaches only {widest} across{angle}, so it can't cut all the way \
             through this {span} board."
        )
    }

    /// What the depth stop will do to `board`, if it's on.
    fn depth_note(&self, board: &Board) -> String {
        let Some(stop) = self.depth_stop else {
            return String::new();
        };
        let thickness = board.thickness();
        if stop >= thickness {
            " The depth stop keeps the blade above it.".to_string()
        } else {
            format!(" The depth stop will cut {} into it.", thickness - stop)
        }
    }

    /// One line describing what's on the saw.
    pub fn status(&self) -> String {
        match &self.work {
            Work::Empty => {
                "Nothing on the saw. Pick up a board and click the saw to set it against the stop."
                    .to_string()
            }
            Work::Ready {
                missed: Some(error),
                ..
            } => format!("No cut: {error}."),
            Work::Ready {
                scarred: Some(scarred),
                ..
            } => {
                let scar = scarred.kerfs().last().expect("a scar");
                format!("Cutting. So far, {}.", describe_kerf(scarred, scar))
            }
            Work::Ready { board, .. } => format!(
                "Board on the saw: {}. Hold Space to lower the blade.{}{}",
                describe_board(board),
                self.reach_note(board),
                self.depth_note(board)
            ),
            Work::Cut { cut, .. } if cut.loose.is_empty() => describe_cut("Cut", cut),
            Work::Cut { cut, .. } => format!("{} Set aside on the pile.", describe_cut("Cut", cut)),
        }
    }
}

#[cfg(test)]
mod tests {
    use woodsim::Blade;

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
    fn one_side_taken_and_set_back_down_goes_against_the_stop() {
        let mut station = loaded();
        chop(&mut station);
        let offcut = station.take(Part::Offcut).unwrap();
        station.load(offcut);

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
        assert!(!station.can_load());
        assert_eq!(station.take(Part::Piece), None);
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
        let mut station = MiterSawStation::default();
        station.parts.push(ripped_piece());

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
            let offcut = station.take(Part::Offcut).unwrap();
            station.load(offcut);
        }
        chop(&mut station);
        let last = station.take(Part::Piece).unwrap();
        station.parts.push(last);

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
        let piece = station.take(Part::Piece).unwrap();
        station.parts.push(piece);
        let pile = station.pile();

        assert_eq!(pile.len(), 2);
        for part in &pile {
            let (near, _) = part.bounds();
            assert_close(near.z, PILE_LEFT);
            // Behind the fence, which is at x = 0.
            assert_close(near.x, -PILE_FRONT);
        }
        // On the tray.
        assert_close(pile[0].bounds().0.y, PILE_TRAY.max.y);
        assert_close(
            pile[1].bounds().0.y,
            PILE_TRAY.max.y + inches(0.75) + PILE_GAP,
        );
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
            miter_gauge: None,
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
    fn a_board_turned_across_the_saw_is_cut_only_as_far_as_the_blade_reaches() {
        let mut station = loaded();
        assert!(station.turn_left());
        station.set_stop(Length::from_inches(3));
        assert!(station.status().ends_with(
            "The blade reaches only 7-1/2\" across, so it can't cut all the way through this \
             24\" board."
        ));
        chop(&mut station);

        let Work::Ready { board, .. } = &station.work else {
            panic!("the board should still be whole against the stop");
        };
        assert_eq!(board.kerfs().len(), 1);
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
    fn stopping_short_scars_the_board_and_the_scar_stays() {
        let mut station = loaded();
        station.set_depth_stop(Some(Length::from_inch_fraction(1, 2)));
        assert!(
            station
                .status()
                .ends_with("The depth stop will cut 1/4\" into it.")
        );

        station.lower(1.0);
        assert!(station.lowered < 1.0);
        assert_eq!(station.bottom(), Length::from_inch_fraction(1, 2));
        assert_eq!(
            station.status(),
            "Cutting. So far, a 1/8\"-wide, 1/4\"-deep dado 3-7/16\" long."
        );

        station.raise(1.0);
        let Work::Ready { board, .. } = &station.work else {
            panic!("the scarred board should still be against the stop");
        };
        assert_eq!(board.kerfs().len(), 1);
        assert!(station.status().starts_with(
            "Board on the saw: 3/4\" x 6\" x 24\", with a 1/8\"-wide, 1/4\"-deep dado 3-7/16\" long."
        ));
    }

    #[test]
    fn side_by_side_strokes_nibble_out_a_wider_scar() {
        let mut station = loaded();
        station.set_depth_stop(Some(Length::from_inch_fraction(1, 2)));
        chop(&mut station);
        station.set_stop(Length::from_inches(12) + Length::from_inch_fraction(1, 8));
        chop(&mut station);

        let Work::Ready { board, .. } = &station.work else {
            panic!("the board should still be against the stop");
        };
        assert_eq!(board.kerfs().len(), 2);
        assert!(station.status().starts_with(
            "Board on the saw: 3/4\" x 6\" x 24\", with a 1/4\"-wide, 1/4\"-deep dado 3-7/16\" long."
        ));
    }

    #[test]
    fn the_depth_stop_changes_only_with_the_blade_up() {
        let mut station = loaded();
        station.lower(0.1);
        station.set_depth_stop(Some(Length::from_inch_fraction(1, 2)));
        assert_eq!(station.depth_stop, None);

        station.raise(1.0);
        station.set_depth_stop(Some(Length::from_inches(9)));
        assert_eq!(station.depth_stop, Some(DEPTH_STOP_MAX));
        // Above the board, the blade can't reach it.
        assert!(
            station
                .status()
                .ends_with("The depth stop keeps the blade above it.")
        );
        chop(&mut station);
        let Work::Ready { board, .. } = &station.work else {
            panic!("the board should still be against the stop");
        };
        assert!(board.kerfs().is_empty());
    }

    #[test]
    fn all_the_way_down_the_blade_reaches_through_the_table() {
        let mut station = loaded();
        station.lower(1.0);
        assert_eq!(station.bottom(), station.saw.stroke.1);
        assert!(station.bottom() < Length::ZERO);
    }

    #[test]
    fn empty_pile_says_so() {
        assert_eq!(
            MiterSawStation::default().cut_list(),
            "Cut list: no parts set aside yet"
        );
    }

    #[test]
    fn the_table_clicks_from_stop_to_stop_and_turns_only_with_the_blade_up() {
        let mut station = loaded();
        station.swing_to_next_stop(true);
        assert_eq!(station.saw.miter, Angle::from_degrees(15));
        station.swing_to_next_stop(true);
        assert_eq!(station.saw.miter, Angle::from_hundredths(2250));
        // It turns 50° either way, past its last stop.
        station.set_miter(-Angle::from_degrees(60));
        assert_eq!(station.saw.miter, -Angle::from_degrees(50));
        station.swing_to_next_stop(false);
        assert_eq!(station.saw.miter, -Angle::from_degrees(50));
        station.swing_to_next_stop(true);
        assert_eq!(station.saw.miter, -Angle::from_degrees(45));

        station.lower(0.1);
        station.set_miter(Angle::ZERO);
        assert_eq!(station.saw.miter, -Angle::from_degrees(45));
    }

    #[test]
    fn a_cut_at_an_angle_leaves_a_mitered_piece() {
        let mut station = loaded();
        station.set_miter(Angle::from_degrees(30));
        chop(&mut station);
        let Work::Cut { cut, .. } = &station.work else {
            panic!("the blade should have cut the board");
        };
        assert_eq!(cut.piece.board.miters(), [Angle::from_degrees(30)]);
        assert!(
            station
                .status()
                .contains("The piece is mitered 30 deg at one end."),
            "{}",
            station.status()
        );
    }

    #[test]
    fn turned_further_the_blade_reaches_across_less() {
        let mut station = loaded();
        station.set_miter(Angle::from_degrees(45));
        assert!(station.status().ends_with(
            "The blade reaches only 5-19/64\" across at 45 deg, so it can't cut all the way \
             through this 6\" board."
        ));

        // It cuts as far as it reaches, and the board stays in one piece.
        chop(&mut station);
        let Work::Ready { board, .. } = &station.work else {
            panic!("the board should still be whole against the stop");
        };
        assert_eq!(board.kerfs().len(), 1);
        assert!(
            station
                .status()
                .starts_with("Board on the saw: 3/4\" x 6\" x 24\", with a stopped cut"),
            "{}",
            station.status()
        );
    }

    #[test]
    fn on_edge_the_blade_chops_down_through_the_board() {
        let mut station = MiterSawStation::default();
        station.load(Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        ));
        assert!(station.tip());
        chop(&mut station);
        let Work::Cut { cut, .. } = &station.work else {
            panic!("the blade should have cut the board");
        };
        assert_eq!(cut.piece.board.to_string(), "3/4\" x 3-1/2\" x 12\"");
    }

    #[test]
    fn on_edge_the_table_has_to_be_square() {
        let mut station = loaded();
        assert!(station.tip());
        station.set_miter(Angle::from_degrees(45));
        chop(&mut station);
        assert_eq!(
            station.status(),
            "No cut: the table has to be square to cut a board standing on edge."
        );
    }
}

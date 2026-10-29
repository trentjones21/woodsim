//! The miter saw station's state: the saw, the stop block, the board against it, and how far
//! the blade is down.
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
use woodsim::{Angle, CutError, Length, MiterSaw, Orientation};

use crate::{
    feed::shelf,
    hand::Workpiece,
    inventory::Shelf,
    shop::{Part, SawBoards, describe_kerf},
    work::{self, Parted, Passed},
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
    /// Wood the saw has set aside, for the inventory: what a new board displaced, the other side
    /// of a cut when one side's taken, strips a cut set loose, and what's been put away.
    set_aside: Vec<(Shelf, Workpiece)>,
    /// How the board on the saw lies.
    lie: Orientation,
}

pub enum Work {
    /// Nothing on the saw.
    Empty,
    /// A board or a glue-up against the stop block, waiting for the blade. `missed` says why
    /// the last stroke didn't cut it, until the stop moves. `scarred` is it as the blade has
    /// scarred it on the stroke under way; the scar stays when the blade comes back up.
    Ready {
        board: Workpiece,
        missed: Option<CutError>,
        scarred: Option<Workpiece>,
    },
    /// A board the blade has cut, with the stop where it was at the time. While the blade is
    /// still down, `cut` follows it deeper; once it comes back up, the strips it set loose are
    /// set aside.
    Cut {
        board: Workpiece,
        stop: Length,
        cut: Parted,
        loose_set_aside: bool,
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
            set_aside: Vec::new(),
            lie: Orientation::default(),
        }
    }
}

impl MiterSawStation {
    /// Puts `board`, a board or a glue-up, against the stop block, unturned, setting aside
    /// whatever was on the saw.
    pub fn load(&mut self, board: Workpiece) {
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
    /// down. A glue-up stays flat.
    pub fn tip(&mut self) -> bool {
        if matches!(
            &self.work,
            Work::Ready {
                board: Workpiece::GlueUp(_),
                ..
            }
        ) {
            return false;
        }
        self.relay(self.lie.tipped())
    }

    /// Whether the board against the stop can be turned or flipped: it's waiting there with the
    /// blade up.
    pub fn can_turn(&self) -> bool {
        self.blade_up() && matches!(self.work, Work::Ready { .. })
    }

    /// Whether it can stand on an edge: it can turn, and it's a board rather than a glue-up.
    pub fn can_tip(&self) -> bool {
        self.can_turn()
            && !matches!(
                self.work,
                Work::Ready {
                    board: Workpiece::GlueUp(_),
                    ..
                }
            )
    }

    /// Whether there's anything on the saw for the blade to come down on.
    pub fn has_work(&self) -> bool {
        !matches!(self.work, Work::Empty)
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
    /// the board, it scars it; all the way through, it cuts it. Going on down after it breaks
    /// through, it clears what its curve hadn't reached yet, the way a real stroke does, so a
    /// full stroke leaves clean ends.
    pub fn lower(&mut self, amount: f32) {
        self.lowered = (self.lowered + amount).min(self.lowest());
        if self.lowered <= self.deepest {
            return;
        }
        self.deepest = self.lowered;
        let bottom = self.bottom();
        match &mut self.work {
            Work::Ready {
                board,
                missed,
                scarred,
            } => match work::chop(&self.saw, board, self.lie, self.stop, bottom) {
                Ok(Passed::Cut(cut)) => {
                    self.work = Work::Cut {
                        board: board.clone(),
                        stop: self.stop,
                        cut,
                        loose_set_aside: false,
                    }
                }
                Ok(Passed::Kerfed(scar)) => *scarred = Some(scar),
                Ok(Passed::Missed) => {}
                Err(error) => *missed = Some(error),
            },
            Work::Cut {
                board, stop, cut, ..
            } => {
                if let Ok(Passed::Cut(deeper)) =
                    work::chop(&self.saw, board, self.lie, *stop, bottom)
                {
                    *cut = deeper;
                }
            }
            Work::Empty => {}
        }
    }

    /// Raises the blade by `amount` of its travel. Back at the top, the stroke is over: any
    /// scar it left stays in the board, and strips a cut set loose are set aside.
    pub fn raise(&mut self, amount: f32) {
        self.lowered = (self.lowered - amount).max(0.0);
        if self.lowered > 0.0 {
            return;
        }
        self.deepest = 0.0;
        match &mut self.work {
            Work::Ready { board, scarred, .. } => {
                if let Some(scar) = scarred.take() {
                    *board = scar;
                }
            }
            Work::Cut {
                cut,
                loose_set_aside,
                ..
            } if !*loose_set_aside => {
                *loose_set_aside = true;
                let loose = cut.loose(self.lie);
                self.set_aside
                    .extend(loose.into_iter().map(|strip| (Shelf::Offcuts, strip)));
            }
            _ => {}
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
                (Work::Cut { cut, .. }, Part::Offcut) => cut.has_offcut(),
                _ => false,
            }
    }

    /// Lifts a board off the saw to carry it elsewhere: the board against the stop, or one side
    /// of the last cut, with the other side set aside.
    pub fn take(&mut self, part: Part) -> Option<Workpiece> {
        if !self.can_take(part) {
            return None;
        }
        match std::mem::replace(&mut self.work, Work::Empty) {
            Work::Empty => None,
            Work::Ready { board, .. } => Some(work::hand_off(board, self.lie)),
            Work::Cut { cut, .. } => {
                let (taken, other) = match part {
                    Part::Offcut => (Part::Offcut, Part::Piece),
                    _ => (Part::Piece, Part::Offcut),
                };
                if let Some(other_side) = cut.take(other, self.lie) {
                    self.set_aside.push((shelf(other), other_side));
                }
                cut.take(taken, self.lie)
            }
        }
    }

    /// Sets aside everything on the saw: the board against the stop to the parts, or the
    /// measured piece of a cut to the parts and the offcut to the offcuts.
    fn clear_saw(&mut self) {
        match std::mem::replace(&mut self.work, Work::Empty) {
            Work::Empty => {}
            Work::Ready { board, .. } => self
                .set_aside
                .push((Shelf::Parts, work::hand_off(board, self.lie))),
            Work::Cut { cut, .. } => {
                let parts = [Part::Piece, Part::Offcut]
                    .into_iter()
                    .filter_map(|part| Some((shelf(part), cut.take(part, self.lie)?)));
                self.set_aside.extend(parts.collect::<Vec<_>>());
            }
        }
    }

    /// Puts away everything on the saw, with the blade up.
    pub fn put_away(&mut self) {
        if self.can_put_away() {
            self.clear_saw();
        }
    }

    /// Whether there's anything on the saw to put away, with the blade up.
    pub fn can_put_away(&self) -> bool {
        self.blade_up() && !matches!(self.work, Work::Empty)
    }

    /// Puts the piece the last cut measured away and slides the offcut back against the stop,
    /// to cut another the same length.
    pub fn cut_again(&mut self) {
        if let Some(offcut) = self.take(Part::Offcut) {
            self.load(offcut);
        }
    }

    /// Whether the last cut left an offcut to cut again, with the blade up.
    pub fn can_cut_again(&self) -> bool {
        self.can_take(Part::Offcut)
    }

    /// Whether there's wood set aside for the inventory.
    pub fn has_set_aside(&self) -> bool {
        !self.set_aside.is_empty()
    }

    /// Hands what's been set aside to the inventory.
    pub fn take_set_aside(&mut self) -> Vec<(Shelf, Workpiece)> {
        std::mem::take(&mut self.set_aside)
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
            Work::Cut {
                board, stop, cut, ..
            } => (board, *stop, Some(cut)),
        };
        // The board's far end along the fence rests against the stop block; the rest reaches
        // left past the blade. It reaches from the fence out toward the operator.
        let near = Vec3::new(
            -work::extent(board, self.lie.across().0).as_meters_f32(),
            0.0,
            stop.as_meters_f32() - work::extent(board, self.lie.along().0).as_meters_f32(),
        );
        match cut {
            None => SawBoards {
                whole: work::placed(board, self.lie, near),
                ..default()
            },
            // Each piece sits where it was on the board. Loose ones are set aside.
            Some(cut) => SawBoards {
                whole: Vec::new(),
                piece: cut.placed(Part::Piece, board, self.lie, near),
                offcut: cut.placed(Part::Offcut, board, self.lie, near),
            },
        }
    }

    /// A warning if the blade won't reach all the way across `board` as it lies, at the miter
    /// angle: it cuts as far as it reaches and leaves the board in one piece.
    fn reach_note(&self, board: &Workpiece) -> String {
        let span = work::extent(board, self.lie.across().0);
        let what = match board {
            Workpiece::Board(_) => "board",
            Workpiece::GlueUp(_) => "glue-up",
        };
        let widest = self.saw.widest();
        if span <= widest {
            return String::new();
        }
        let angle = match self.saw.miter.abs() {
            square if square == Angle::ZERO => String::new(),
            miter => format!(" at {miter}"),
        };
        format!(
            " The blade reaches only {widest} across{angle}, so it can't cut all the way \
             through this {span} {what}."
        )
    }

    /// What the depth stop will do to `board`, if it's on.
    fn depth_note(&self, board: &Workpiece) -> String {
        let Some(stop) = self.depth_stop else {
            return String::new();
        };
        let thickness = work::extent(board, self.lie.up().0);
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
                "Nothing on the saw. Put a board against the stop from the inventory, or set down \
                 the one in hand."
                    .to_string()
            }
            Work::Ready {
                missed: Some(error),
                ..
            } => format!("No cut: {error}."),
            Work::Ready {
                scarred: Some(Workpiece::Board(scarred)),
                ..
            } => {
                let scar = scarred.kerfs().last().expect("a scar");
                format!("Cutting. So far, {}.", describe_kerf(scarred, scar))
            }
            Work::Ready {
                scarred: Some(Workpiece::GlueUp(_)),
                ..
            } => "Cutting into the glue-up.".to_string(),
            Work::Ready { board, .. } => format!(
                "{}. Hold the blade down to cut.{}{}",
                work::on_the_saw(board),
                self.reach_note(board),
                self.depth_note(board)
            ),
            Work::Cut { cut, .. } if !cut.has_loose() => cut.describe("Cut", self.lie),
            Work::Cut { cut, .. } => {
                format!(
                    "{} Put away with the offcuts.",
                    cut.describe("Cut", self.lie)
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use woodsim::Board;

    use crate::hand::Workpiece;
    use woodsim::Blade;

    use super::*;
    use crate::shop::{Placed, inches};

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
        station.load(Workpiece::Board(ripped_piece()));
        station
    }

    /// Brings the blade all the way down and back up.
    fn chop(station: &mut MiterSawStation) {
        station.lower(1.0);
        station.raise(1.0);
    }

    #[test]
    fn a_glue_up_crosscuts_into_two_glue_ups_and_the_other_is_set_aside() {
        let mut station = MiterSawStation::default();
        let panel = crate::work::panel(
            &[Length::from_inch_fraction(5, 2); 2],
            Length::from_inches(24),
        );
        station.load(Workpiece::GlueUp(panel));
        assert!(!station.tip());
        chop(&mut station);
        assert_eq!(
            station.status(),
            "Cut the glue-up apart: a glue-up of 2 parts, 3/4\" x 5\" x 12\", and a glue-up of \
             2 parts, 3/4\" x 5\" x 11-7/8\"."
        );
        let Some(Workpiece::GlueUp(piece)) = station.take(Part::Piece) else {
            panic!("expected the glued piece");
        };
        assert_eq!(piece.joints().len(), 1);
        assert!(matches!(
            station.take_set_aside()[..],
            [(Shelf::Offcuts, Workpiece::GlueUp(_))]
        ));
    }

    #[test]
    fn a_stroke_pulled_down_a_little_at_a_time_leaves_clean_mitered_ends() {
        // A narrow board, ripped off a wider one, comes apart before the blade's curve has
        // cleared the corners of a 45° cut; going on down, the blade clears them, as a full
        // stroke does, and both ends are flat.
        let mut station = MiterSawStation::default();
        station.load(Workpiece::Board(Board::new(
            Length::from_inches(24),
            Length::from_inches(1),
            Length::from_inch_fraction(3, 4),
        )));
        station.set_miter(Angle::from_degrees(45));
        for _ in 0..250 {
            station.lower(0.004);
        }
        station.raise(1.0);
        let Work::Cut {
            cut: Parted::Board(cut),
            ..
        } = &station.work
        else {
            panic!("expected the board cut apart");
        };
        for side in [Some(&cut.piece), cut.offcut.as_ref()] {
            let board = &side.expect("both sides of the cut").board;
            assert!(board.kerfs().is_empty(), "{:?}", board.kerfs());
            assert_eq!(board.face().len(), 1);
        }
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
        let whole = loaded().boards().whole.into_iter().next().unwrap();

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
        assert!(boards.whole.is_empty());
        let piece = boards.piece.into_iter().next().unwrap();
        assert_close(x_span(&piece).0, 0.0);
        assert_close(x_span(&piece).1, inches(12.0));
        let offcut = boards.offcut.into_iter().next().unwrap();
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
        assert_close(
            x_span(&station.boards().piece.into_iter().next().unwrap()).1,
            inches(12.0),
        );
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
        assert!(!station.boards().whole.is_empty());
        assert_eq!(
            station.status(),
            "No cut: the blade misses the board: set to 30\", but the board is only 24\"."
        );

        station.set_stop(Length::from_inches(12));
        assert_close(
            x_span(&station.boards().whole.into_iter().next().unwrap()).1,
            inches(12.0),
        );
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

        let (start, end) = x_span(&station.boards().whole.into_iter().next().unwrap());
        assert_close(end, inches(12.0));
        assert_close(end - start, inches(11.875));
        let set_aside = station.take_set_aside();
        assert_eq!(set_aside.len(), 1);
        assert_eq!(set_aside[0].0, Shelf::Parts);
        assert_eq!(
            set_aside[0].1.board().unwrap().length(),
            Length::from_inches(12)
        );
    }

    #[test]
    fn nothing_can_be_picked_up_with_the_blade_down() {
        let mut station = loaded();
        station.lower(1.0);
        assert!(!station.can_take(Part::Piece));
        assert!(!station.can_load());
        assert_eq!(station.take(Part::Piece), None);
        assert!(!station.has_set_aside());
        assert!(matches!(station.work, Work::Cut { .. }));
    }

    #[test]
    fn taking_the_board_against_the_stop_empties_the_saw() {
        let mut station = loaded();

        assert_eq!(station.take(Part::Piece), None);
        assert_eq!(
            station.take(Part::Whole),
            Some(Workpiece::Board(ripped_piece()))
        );
        assert!(matches!(station.work, Work::Empty));
        assert!(!station.has_set_aside());
    }

    #[test]
    fn taking_one_side_of_a_cut_sets_the_other_aside() {
        let mut station = loaded();
        chop(&mut station);
        let taken = station.take(Part::Piece).unwrap();

        assert_eq!(taken.board().unwrap().length(), Length::from_inches(12));
        assert!(matches!(station.work, Work::Empty));
        let set_aside = station.take_set_aside();
        assert_eq!(set_aside.len(), 1);
        assert_eq!(set_aside[0].0, Shelf::Offcuts);
        assert_eq!(
            set_aside[0].1.board().unwrap().length(),
            Length::from_inch_fraction(95, 8)
        );
    }

    #[test]
    fn cutting_again_turns_one_board_into_parts() {
        let mut station = loaded();
        station.set_stop(Length::from_inches(6));
        assert!(!station.can_cut_again());
        for _ in 0..2 {
            chop(&mut station);
            assert!(station.can_cut_again());
            station.cut_again();
        }
        chop(&mut station);
        assert!(station.can_put_away());
        station.put_away();

        assert!(matches!(station.work, Work::Empty));
        assert!(!station.can_put_away());
        // Three 6" parts, what's left, and three kerfs add back up to the 24" board.
        let set_aside = station.take_set_aside();
        let lengths: Vec<(Shelf, Length)> = set_aside
            .iter()
            .map(|(shelf, part)| (*shelf, part.board().unwrap().length()))
            .collect();
        let six = Length::from_inches(6);
        assert_eq!(
            lengths,
            [
                (Shelf::Parts, six),
                (Shelf::Parts, six),
                (Shelf::Parts, six),
                (Shelf::Offcuts, Length::from_inch_fraction(45, 8)),
            ]
        );
        let total: Length = lengths.iter().map(|(_, length)| *length).sum();
        assert_eq!(
            total + Length::from_inch_fraction(3, 8),
            Length::from_inches(24)
        );
    }

    #[test]
    fn loading_a_board_sets_aside_whatever_was_on_the_saw() {
        let mut station = loaded();
        chop(&mut station);
        station.load(Workpiece::Board(ripped_piece()));

        assert!(matches!(station.work, Work::Ready { .. }));
        let shelves: Vec<Shelf> = station
            .take_set_aside()
            .iter()
            .map(|(shelf, _)| *shelf)
            .collect();
        assert_eq!(shelves, [Shelf::Parts, Shelf::Offcuts]);
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
        station.load(Workpiece::Board(slotted_board()));
        station.set_stop(Length::from_inches(14));
        chop(&mut station);

        // The other 9-7/8" fell apart along the slot. The narrow strip is set aside.
        let set_aside = station.take_set_aside();
        assert_eq!(set_aside.len(), 1);
        assert_eq!(set_aside[0].0, Shelf::Offcuts);
        assert_eq!(
            set_aside[0].1.board().unwrap().to_string(),
            "3/4\" x 1-1/8\" x 9-7/8\""
        );
        assert!(station.status().ends_with(
            "The slot already in the board split off a 3/4\" x 1-1/8\" x 9-7/8\". \
             Put away with the offcuts."
        ));
        // The offcut is the 6" strip, still against the fence where it was on the board.
        let offcut = station.boards().offcut.into_iter().next().unwrap();
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
        assert_eq!(board.board().unwrap().kerfs().len(), 1);
    }

    #[test]
    fn turned_end_for_end_the_slotted_end_goes_against_the_stop() {
        let mut station = MiterSawStation::default();
        station.load(Workpiece::Board(slotted_board()));
        assert!(station.turn_left() && station.turn_left());
        station.set_stop(Length::from_inches(10));
        chop(&mut station);

        // The 10" against the stop falls apart along the slot; its narrow strip is set aside.
        let set_aside = station.take_set_aside();
        assert_eq!(set_aside.len(), 1);
        assert_eq!(
            set_aside[0].1.board().unwrap().to_string(),
            "3/4\" x 1-1/8\" x 10\""
        );
        let piece = station.boards().piece.into_iter().next().unwrap();
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
        assert_eq!(board.board().unwrap().kerfs().len(), 1);
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
        assert_eq!(board.board().unwrap().kerfs().len(), 2);
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
        assert!(board.board().unwrap().kerfs().is_empty());
    }

    #[test]
    fn all_the_way_down_the_blade_reaches_through_the_table() {
        let mut station = loaded();
        station.lower(1.0);
        assert_eq!(station.bottom(), station.saw.stroke.1);
        assert!(station.bottom() < Length::ZERO);
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
        let Work::Cut {
            cut: Parted::Board(cut),
            ..
        } = &station.work
        else {
            panic!("the blade should have cut the board");
        };
        assert_eq!(cut.piece.board.miters(), [Angle::from_degrees(30)]);
        assert!(
            station
                .status()
                .contains("The piece is mitered 30° at one end."),
            "{}",
            station.status()
        );
    }

    #[test]
    fn turned_further_the_blade_reaches_across_less() {
        let mut station = loaded();
        station.set_miter(Angle::from_degrees(45));
        assert!(station.status().ends_with(
            "The blade reaches only 5-19/64\" across at 45°, so it can't cut all the way \
             through this 6\" board."
        ));

        // It cuts as far as it reaches, and the board stays in one piece.
        chop(&mut station);
        let Work::Ready { board, .. } = &station.work else {
            panic!("the board should still be whole against the stop");
        };
        assert_eq!(board.board().unwrap().kerfs().len(), 1);
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
        station.load(Workpiece::Board(Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        )));
        assert!(station.tip());
        chop(&mut station);
        let Work::Cut {
            cut: Parted::Board(cut),
            ..
        } = &station.work
        else {
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

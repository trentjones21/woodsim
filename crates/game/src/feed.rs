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
use woodsim::{
    Angle, Axis, Blade, Board, CutError, Length, MAX_GAUGE, MAX_TILT, Orientation, Recipe, Surface,
    TableSaw, Tool,
};

use crate::{
    chop::MITER_STOPS,
    hand::Workpiece,
    inventory::Shelf,
    shop::{Part, SawBoards, describe_kerf},
    work::{self, Passed, Rest},
};

pub const FENCE_MIN: Length = Length::from_inch_fraction(1, 16);
pub const FENCE_MAX: Length = Length::from_inches(24);
/// How far the miter gauge's stop goes from the blade, along its 21" auxiliary fence.
const GAUGE_STOP_MIN: Length = Length::from_inch_fraction(1, 16);
const GAUGE_STOP_MAX: Length = Length::from_inches(20);
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
    /// The board or glue-up on the saw, with the slots earlier passes cut. During a pass, this
    /// is how it was before the pass.
    board: Workpiece,
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
    /// Where the miter gauge's stop is set, kept while the rip fence is in use.
    gauge_stop: Length,
    /// Wood the saw has set aside, for the inventory: what a new board displaced, and what's
    /// been put away.
    set_aside: Vec<(Shelf, Workpiece)>,
    /// Choosing what the board rests on, and against the fence.
    rest: Rest,
}

struct Feeding {
    /// The saw as it was set when the pass began.
    saw: TableSaw,
    /// The furthest back and forward the blade's center has been along the board.
    reached: (Length, Length),
    result: Result<Passed, CutError>,
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
                miter_gauge: None,
                gauge_angle: woodsim::Angle::ZERO,
                tilt: woodsim::Angle::ZERO,
            },
            board: Workpiece::Board(new_stock()),
            lie: Orientation::default(),
            advance: START,
            pass: None,
            taken: Vec::new(),
            gauge_stop: Length::from_inches(12),
            set_aside: Vec::new(),
            rest: Rest::default(),
        }
    }
}

/// Which shelf a part of the saw's board goes on: the measured piece and a board that wasn't cut
/// to the parts, the offcut to the offcuts.
pub fn shelf(part: Part) -> Shelf {
    match part {
        Part::Offcut => Shelf::Offcuts,
        Part::Whole | Part::Piece => Shelf::Parts,
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
    /// Moves the rip fence, within its range, while it's in use. It stays put while a board is
    /// at the blade.
    pub fn set_fence(&mut self, fence: Length) {
        if !self.at_blade() && !self.using_gauge() {
            self.saw.fence = fence.clamp(FENCE_MIN, FENCE_MAX);
        }
    }

    /// Whether the miter gauge is guiding the board, with the rip fence slid out of the way.
    pub fn using_gauge(&self) -> bool {
        self.saw.miter_gauge.is_some()
    }

    /// Where the miter gauge's stop is set: its distance from the blade.
    pub fn gauge_stop(&self) -> Length {
        self.gauge_stop
    }

    /// Switches between the rip fence and the miter gauge. Returns false while a board is at the
    /// blade.
    pub fn switch_guide(&mut self) -> bool {
        if self.at_blade() {
            return false;
        }
        self.saw.miter_gauge = match self.saw.miter_gauge {
            Some(_) => None,
            None => Some(self.gauge_stop),
        };
        true
    }

    /// Moves the miter gauge's stop, within its range. It stays put while a board is at the
    /// blade.
    pub fn set_gauge_stop(&mut self, stop: Length) {
        if self.at_blade() {
            return;
        }
        self.gauge_stop = stop.clamp(GAUGE_STOP_MIN, GAUGE_STOP_MAX);
        if self.using_gauge() {
            self.saw.miter_gauge = Some(self.gauge_stop);
        }
    }

    /// Turns the miter gauge's head, as far as it goes either way. It stays put while a board
    /// is at the blade.
    pub fn set_gauge_angle(&mut self, angle: Angle) {
        if !self.at_blade() {
            self.saw.gauge_angle = angle.clamp(-MAX_GAUGE, MAX_GAUGE);
        }
    }

    /// Turns the gauge's head to the next place it clicks into, one way or the other, or as
    /// far as it goes past the last one.
    pub fn swing_gauge_to_next_stop(&mut self, more: bool) {
        let now = self.saw.gauge_angle;
        let stops = MITER_STOPS.iter().flat_map(|&stop| [stop, -stop]);
        let next = if more {
            stops.filter(|&stop| stop > now).min().unwrap_or(MAX_GAUGE)
        } else {
            stops.filter(|&stop| stop < now).max().unwrap_or(-MAX_GAUGE)
        };
        self.set_gauge_angle(next);
    }

    /// Whether the gauge's head can be turned: not while a board is at the blade.
    pub fn can_swing_gauge(&self) -> bool {
        !self.at_blade()
    }

    /// Tilts the blade, from square to as far as it goes. It stays put while a board is at the
    /// blade.
    pub fn set_tilt(&mut self, tilt: Angle) {
        if !self.at_blade() {
            self.saw.tilt = tilt.clamp(Angle::ZERO, MAX_TILT);
        }
    }

    /// Tilts the blade to the next stop on its scale, further or back, or as far as it goes
    /// past the last one.
    pub fn tilt_to_next_stop(&mut self, further: bool) {
        let now = self.saw.tilt;
        let stops = MITER_STOPS.into_iter();
        let next = if further {
            stops.filter(|&stop| stop > now).min().unwrap_or(MAX_TILT)
        } else {
            stops
                .filter(|&stop| stop < now)
                .max()
                .unwrap_or(Angle::ZERO)
        };
        self.set_tilt(next);
    }

    /// Whether the blade can be tilted: not while a board is at it.
    pub fn can_tilt(&self) -> bool {
        !self.at_blade()
    }

    /// Where the miter gauge sits, in meters along the table, and how its head is turned: the
    /// point where its face crosses the blade's stop-side face, and the turn about it.
    pub fn gauge_pose(&self) -> (f32, Quat) {
        let saw = self.pass.as_ref().map_or(self.saw, |pass| pass.saw);
        match self.angled(saw) {
            Some((rotation, pivot, shift)) => (pivot.z + shift, rotation),
            None => (
                (self.reach() - self.advance).as_meters_f32(),
                Quat::IDENTITY,
            ),
        }
    }

    /// How the board, and the gauge's head with it, is turned and moved on the table while the
    /// gauge holds it at an angle, as `woodsim` cuts it: turned about where the blade's
    /// stop-side face crosses the gauge's face, at the board's trailing side, then slid along
    /// the table so its leading corner is where the blade has fed it to. The turn, the point
    /// it's about, and the slide, in meters; `None` while the gauge is square or not in use.
    fn angled(&self, saw: TableSaw) -> Option<(Quat, Vec3, f32)> {
        let stop = saw.miter_gauge?;
        if saw.gauge_angle == Angle::ZERO {
            return None;
        }
        // The board's length across the table turns toward its trailing side as the blade's
        // front swings toward the stop.
        let rotation = Quat::from_rotation_y(-(saw.gauge_angle.radians() as f32));
        let along = work::reach(&self.board, self.lie).along.as_meters_f32();
        let across = self.span().as_meters_f32();
        let near_side = (-saw.blade.kerf - stop).as_meters_f32();
        let lead = (-self.advance).as_meters_f32();
        let pivot = Vec3::new((-saw.blade.kerf).as_meters_f32(), 0.0, lead + along);
        let leading = [0.0, across]
            .into_iter()
            .flat_map(|x| [0.0, along].map(|z| Vec3::new(near_side + x, 0.0, lead + z)))
            .map(|corner| (pivot + rotation * (corner - pivot)).z)
            .fold(f32::MAX, f32::min);
        Some((rotation, pivot, lead - leading))
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

    /// Stands the board up further (`work::stood_up`): on an edge, with its bottom face toward
    /// the fence, then on its end, then back down flat. A glue-up stays flat.
    pub fn tip(&mut self) -> bool {
        if !self.can_tip() {
            return false;
        }
        self.relay(work::stood_up(self.lie))
    }

    /// Lays the board down another way, if it's in one piece and waiting in front of the blade.
    fn relay(&mut self, lie: Orientation) -> bool {
        if !self.can_turn() {
            return false;
        }
        self.lie = lie;
        true
    }

    /// What's on the saw, written out: as it was before the pass under way, with the saw as it
    /// was set for the pass, how it lies, and how far the pass has gone, so it can be made again.
    pub fn recipe(&self) -> Option<Recipe> {
        self.on_saw()?;
        let (saw, travel) = match &self.pass {
            Some(pass) => (pass.saw, pass.reached),
            None => (self.saw, (self.advance, self.advance)),
        };
        Some(Recipe {
            wood: work::wood(&self.board),
            saw: Some(Tool::TableSaw {
                saw,
                lie: work::lying(&self.board, self.lie),
                travel,
            }),
        })
    }

    /// Starts choosing what the board rests on and what goes against the fence, each with a
    /// click on it, or stops: while it waits in front of the blade.
    pub fn choose_rest(&mut self) -> bool {
        if !self.can_turn() {
            return false;
        }
        self.rest.toggle();
        true
    }

    /// Whether a click on the board picks what it rests on, rather than picking it up.
    pub fn choosing_rest(&self) -> bool {
        self.rest.choosing.is_some() && self.can_turn()
    }

    /// Picks `surface` of the board in `slot` (its place in a glue-up) to rest on, or to go
    /// against the fence, as the choosing has got to. Returns false, saying why in the
    /// status, if it won't rest that way.
    pub fn pick_rest(&mut self, slot: usize, surface: Surface) -> bool {
        if !self.choosing_rest() {
            return false;
        }
        match self.rest.pick(&self.board, self.lie, (slot, surface)) {
            Some(lie) => {
                self.lie = lie;
                true
            }
            None => false,
        }
    }

    /// The surface the board rests on while the one for the fence is chosen: the board's place
    /// in a glue-up, and its surface.
    pub fn resting_on(&self) -> Option<(usize, Surface)> {
        self.rest.down().filter(|_| self.choosing_rest())
    }

    /// Whether the board can be turned or flipped: it's in one piece, waiting in front of the
    /// blade.
    pub fn can_turn(&self) -> bool {
        self.on_saw() == Some(Part::Whole) && self.pass.is_none() && self.advance <= -CLEAR
    }

    /// Whether it can stand on an edge: it can turn, and it's a board rather than a glue-up.
    pub fn can_tip(&self) -> bool {
        self.can_turn() && !matches!(self.board, Workpiece::GlueUp(_)) && self.lie.is_square()
    }

    /// Whether there's a board to push along the fence.
    pub fn can_push(&self) -> bool {
        self.on_saw().is_some()
    }

    /// Whether `bring_back` would bring the board back around.
    pub fn can_bring_back(&self) -> bool {
        self.on_saw() == Some(Part::Whole) && !self.at_blade() && self.advance != START
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
        self.rest = Rest::default();
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
            result: Ok(Passed::Missed),
            loose_set_aside: false,
        });
        pass.reached = (pass.reached.0.min(advance), pass.reached.1.max(advance));
        pass.result = work::feed(
            &pass.saw,
            &self.board,
            self.lie,
            pass.reached.0,
            pass.reached.1,
        );
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
    pub fn take(&mut self, part: Part) -> Option<Workpiece> {
        if !self.can_take(part) {
            return None;
        }
        let board = match (part, self.result()) {
            (Part::Whole, _) => work::hand_off(self.current_board(), self.lie),
            (_, Some(Ok(Passed::Cut(cut)))) => cut.take(part, self.lie)?,
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
            (Part::Whole, Some(Ok(Passed::Cut(_)))) => false,
            (Part::Whole, _) => !self.at_blade(),
            (Part::Piece, Some(Ok(Passed::Cut(_)))) => self.past_blade(),
            (Part::Offcut, Some(Ok(Passed::Cut(cut)))) => cut.has_offcut() && self.past_blade(),
            _ => false,
        }
    }

    /// Whether the last cut set pieces loose that haven't been handed off yet.
    pub fn has_loose(&self) -> bool {
        matches!(&self.pass, Some(pass) if !pass.loose_set_aside
            && matches!(&pass.result, Ok(Passed::Cut(cut)) if cut.has_loose()))
    }

    /// Hands off the pieces the last cut set loose, once, to be set aside.
    pub fn take_loose(&mut self) -> Vec<Workpiece> {
        if !self.has_loose() {
            return Vec::new();
        }
        let lie = self.lie;
        let pass = self.pass.as_mut().expect("loose pieces come from a pass");
        pass.loose_set_aside = true;
        match &pass.result {
            Ok(Passed::Cut(cut)) => cut.loose(lie),
            _ => Vec::new(),
        }
    }

    /// Whether a board can be set down on the saw: not while one is at the blade.
    pub fn can_load(&self) -> bool {
        !self.at_blade()
    }

    /// Puts `board` against the fence, unturned and ready to rip, setting aside whatever was on
    /// the saw that wasn't carried off: the board, slots and all, or the pieces it was cut into.
    pub fn load(&mut self, board: Workpiece) {
        let loose = self.take_loose();
        self.set_aside
            .extend(loose.into_iter().map(|strip| (Shelf::Offcuts, strip)));
        let left: Vec<(Shelf, Workpiece)> = match self.result() {
            Some(Ok(Passed::Cut(cut))) => [Part::Piece, Part::Offcut]
                .into_iter()
                .filter(|part| !self.taken.contains(part))
                .filter_map(|part| Some((shelf(part), cut.take(part, self.lie)?)))
                .collect(),
            _ if self.taken.contains(&Part::Whole) => Vec::new(),
            _ => vec![(Shelf::Parts, work::hand_off(self.current_board(), self.lie))],
        };
        self.set_aside.extend(left);
        self.board = board;
        self.lie = Orientation::default();
        self.advance = START;
        self.pass = None;
        self.taken.clear();
        self.rest = Rest::default();
    }

    /// Puts away everything on the saw that's clear of the blade: the measured piece to the
    /// parts, the offcut to the offcuts, a board that wasn't cut to the parts.
    pub fn put_away(&mut self) {
        for part in [Part::Whole, Part::Piece, Part::Offcut] {
            if let Some(work) = self.take(part) {
                self.set_aside.push((shelf(part), work));
            }
        }
    }

    /// Whether there's anything clear of the blade to put away.
    pub fn can_put_away(&self) -> bool {
        [Part::Whole, Part::Piece, Part::Offcut]
            .into_iter()
            .any(|part| self.can_take(part))
    }

    /// Puts the piece the last cut measured away and sets the offcut back against the fence, to
    /// cut another the same way: rip strip after strip, say.
    pub fn cut_again(&mut self) {
        if !self.can_cut_again() {
            return;
        }
        if let Some(piece) = self.take(Part::Piece) {
            self.set_aside.push((Shelf::Parts, piece));
        }
        if let Some(offcut) = self.take(Part::Offcut) {
            self.load(offcut);
        }
    }

    /// Whether the last cut left an offcut, clear of the blade, to cut again.
    pub fn can_cut_again(&self) -> bool {
        self.can_take(Part::Offcut)
    }

    /// Whether there's wood set aside, or strips a cut set loose, for the inventory.
    pub fn has_set_aside(&self) -> bool {
        !self.set_aside.is_empty() || self.has_loose()
    }

    /// Hands what's been set aside to the inventory, with strips a cut set loose as offcuts.
    pub fn take_set_aside(&mut self) -> Vec<(Shelf, Workpiece)> {
        let loose = self.take_loose();
        let mut set_aside = std::mem::take(&mut self.set_aside);
        set_aside.extend(loose.into_iter().map(|strip| (Shelf::Offcuts, strip)));
        set_aside
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
        match &self.board {
            Workpiece::Board(board) => self.saw.reach(board, self.lie),
            Workpiece::GlueUp(_) => work::reach(&self.board, self.lie).along,
        }
    }

    /// How far the board reaches across the table, from the fence.
    fn span(&self) -> Length {
        work::reach(&self.board, self.lie).across
    }

    fn cut_apart(&self) -> bool {
        matches!(self.result(), Some(Ok(Passed::Cut(_))))
    }

    fn result(&self) -> Option<&Result<Passed, CutError>> {
        self.pass.as_ref().map(|pass| &pass.result)
    }

    /// The board in one piece as it is now, with any slot the pass under way has cut.
    fn current_board(&self) -> Workpiece {
        match self.result() {
            Some(Ok(Passed::Kerfed(board))) => board.clone(),
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
                (Part::Whole, Some(Ok(Passed::Cut(_)))) => false,
                (Part::Whole, _) => !self.taken.contains(&part),
                (Part::Piece, Some(Ok(Passed::Cut(_)))) => !self.taken.contains(&part),
                (Part::Offcut, Some(Ok(Passed::Cut(cut)))) => {
                    cut.has_offcut() && !self.taken.contains(&part)
                }
                _ => false,
            })
    }

    pub fn boards(&self) -> SawBoards {
        let saw = self.pass.as_ref().map_or(self.saw, |pass| pass.saw);
        // The board reaches back across the table from the fence, or on from the gauge's stop
        // beyond the blade; and from its leading end back toward the operator.
        let near_side = match saw.miter_gauge {
            Some(stop) => -saw.blade.kerf - stop,
            None => saw.fence - self.span(),
        };
        let near = Vec3::new(
            near_side.as_meters_f32(),
            0.0,
            (-self.advance).as_meters_f32(),
        );
        let unless_taken = |part: Part, placed: Vec<_>| {
            if self.taken.contains(&part) {
                Vec::new()
            } else {
                placed
            }
        };
        let mut boards = match self.result() {
            // Each piece sits where it was on the board. Loose pieces are set aside.
            Some(Ok(Passed::Cut(cut))) => SawBoards {
                whole: Vec::new(),
                piece: unless_taken(
                    Part::Piece,
                    cut.placed(Part::Piece, &self.board, self.lie, near),
                ),
                offcut: unless_taken(
                    Part::Offcut,
                    cut.placed(Part::Offcut, &self.board, self.lie, near),
                ),
            },
            _ => SawBoards {
                whole: unless_taken(
                    Part::Whole,
                    work::placed(&self.current_board(), self.lie, near),
                ),
                ..default()
            },
        };
        // Held at an angle by the gauge, it's all turned and slid as the gauge has it.
        if let Some((rotation, pivot, shift)) = self.angled(saw) {
            for placed in boards
                .whole
                .iter_mut()
                .chain(&mut boards.piece)
                .chain(&mut boards.offcut)
            {
                placed.corner = pivot + rotation * (placed.corner - pivot) + Vec3::Z * shift;
                placed.rotation = rotation * placed.rotation;
            }
        }
        boards
    }

    /// One line describing what's on the saw.
    pub fn status(&self) -> String {
        if self.on_saw().is_none() {
            return "Nothing on the saw. Put a board on it from the inventory, or set down the one \
                    in hand."
                .to_string();
        }
        if self.can_turn()
            && let Some(rest) = self.rest.status()
        {
            return rest;
        }
        let verb = match self.lie.across().0 {
            Axis::Width => "Ripped",
            Axis::Length => "Crosscut",
            Axis::Thickness => "Resawed",
        };
        match self.result() {
            Some(Err(error)) => format!("No cut: {error}."),
            Some(Ok(Passed::Cut(cut))) if !cut.has_loose() => cut.describe(verb, self.lie),
            Some(Ok(Passed::Cut(cut))) => format!(
                "{} Put away with the offcuts.",
                cut.describe(verb, self.lie)
            ),
            Some(Ok(Passed::Kerfed(board))) => {
                let kerf = match board {
                    Workpiece::Board(board) => {
                        describe_kerf(board, board.kerfs().last().expect("a pass's slot"))
                    }
                    Workpiece::GlueUp(_) => "a slot into the glue-up".to_string(),
                };
                if self.at_blade() {
                    format!("Cutting. So far, {kerf}.{}", self.warning())
                } else {
                    format!("Cut {kerf}. Bring the board back around for another pass.")
                }
            }
            Some(Ok(Passed::Missed)) if self.past_blade() => {
                "The blade is below the table, so the board went over it uncut. \
                 Bring it back around for another pass."
                    .to_string()
            }
            _ => format!(
                "{}. Drag it, or hold the push button, to push it through.{}",
                work::on_the_saw(&self.board),
                self.warning()
            ),
        }
    }

    /// A warning, if the guide can't hold the board straight. Against the fence, a board wider
    /// across the table than it is long along it rides the fence on its short side, and can
    /// twist into the blade and kick back. On the gauge, a board longer than it is wide runs
    /// on past the gauge's face, which can't keep it on line.
    fn warning(&self) -> &'static str {
        let (reach, span) = (self.reach(), self.span());
        match self.saw.miter_gauge {
            None if reach < span && self.saw.fence < span => {
                " Careful: it rides the fence on its short side, so it can twist into the blade \
                 and kick back. Switch to the miter gauge to crosscut it."
            }
            Some(stop) if reach > span && stop < span => {
                " Careful: the gauge only holds the back of a board this long, so it can wander \
                 off line. Switch to the rip fence to rip it."
            }
            _ => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use woodsim::Board;

    use super::*;
    use crate::hand::Workpiece;
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

        let whole = boards.whole.into_iter().next().unwrap();
        assert_close(whole.corner.x, inches(6.0 - 7.25));
        assert_close(whole.corner.z, inches(6.0));
        assert!(boards.piece.is_empty() && boards.offcut.is_empty());
        assert!(!station.at_blade());
    }

    #[test]
    fn stopping_partway_leaves_a_stopped_cut() {
        let mut station = TableSawStation::default();
        push_to(&mut station, 10);
        let boards = station.boards();

        let whole = boards.whole.into_iter().next().unwrap();
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
             and 13\" on the bottom. Drag it, or hold the push button, to push it through."
        );
    }

    #[test]
    fn pushed_all_the_way_it_comes_apart_clear_of_the_blade() {
        let mut station = TableSawStation::default();
        push_through(&mut station);
        let boards = station.boards();

        assert!(station.past_blade() && !station.at_blade());
        assert!(boards.whole.is_empty());
        let piece = boards.piece.into_iter().next().unwrap();
        assert_eq!(piece.board.to_string(), "3/4\" x 6\" x 24\"");
        assert_close(piece.corner.x, 0.0);
        let offcut = boards.offcut.into_iter().next().unwrap();
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
        assert_close(
            station.boards().piece.into_iter().next().unwrap().corner.x,
            0.0,
        );
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
            "Cut a 1/8\"-wide, 1/4\"-deep groove the full length. \
             Bring the board back around for another pass."
        );

        // Move the fence over and cut a second groove beside the first.
        assert!(station.bring_back());
        station.set_fence(Length::from_inches(5));
        push_through(&mut station);
        let whole = station.boards().whole.into_iter().next().unwrap();
        assert_eq!(whole.board.kerfs().len(), 2);
        assert!(station.can_take(Part::Whole));
    }

    #[test]
    fn a_blade_below_the_table_lets_the_board_pass_over() {
        let mut station = TableSawStation::default();
        station.set_height(Length::ZERO);
        push_through(&mut station);

        assert!(
            station
                .boards()
                .whole
                .into_iter()
                .next()
                .unwrap()
                .board
                .kerfs()
                .is_empty()
        );
        assert!(station.status().starts_with("The blade is below the table"));
    }

    #[test]
    fn fence_past_the_board_pushes_it_by_uncut() {
        let mut station = TableSawStation::default();
        station.set_fence(Length::from_inches(8));
        push_through(&mut station);

        assert!(!station.boards().whole.is_empty());
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
        assert_eq!(piece.board().unwrap().to_string(), "3/4\" x 6\" x 24\"");
        assert_eq!(station.take(Part::Piece), None);
        let boards = station.boards();
        assert!(boards.piece.is_empty());
        assert!(!boards.offcut.is_empty());
    }

    #[test]
    fn a_whole_board_can_be_taken_only_clear_of_the_blade() {
        let mut station = TableSawStation::default();
        push_to(&mut station, 10);
        assert_eq!(station.take(Part::Whole), None);

        station.move_to(START);
        let board = station.take(Part::Whole).unwrap();
        assert_eq!(board.board().unwrap().kerfs().len(), 1);
        assert_eq!(station.boards(), SawBoards::default());
        assert_eq!(
            station.status(),
            "Nothing on the saw. Put a board on it from the inventory, or set down the one in hand."
        );
    }

    #[test]
    fn turned_a_quarter_turn_the_board_lies_across_the_table_and_crosscuts() {
        let mut station = TableSawStation::default();
        assert!(station.turn_left());
        let (near, far) = station.boards().whole.into_iter().next().unwrap().bounds();
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
        let (near, far) = station.boards().piece.into_iter().next().unwrap().bounds();
        assert_close(near.x, 0.0);
        assert_close(far.x, inches(12.0));
    }

    #[test]
    fn the_miter_gauge_crosscuts_from_its_stop_without_a_warning() {
        let mut station = TableSawStation::default();
        assert!(station.turn_left() && station.switch_guide());
        assert!(!station.status().contains("Careful"));
        // The board's near end rests against the stop, 12" past the blade's far side.
        let (near, _) = station.boards().whole.into_iter().next().unwrap().bounds();
        assert_close(near.x, -inches(12.125));

        push_through(&mut station);
        assert_eq!(
            station.status(),
            "Crosscut a 3/4\" x 7-1/4\" x 12\" piece, plus a 3/4\" x 7-1/4\" x 11-7/8\" \
             offcut. The blade turned 1/8\" to sawdust."
        );
        // The piece lies between the stop and the blade.
        let (near, far) = station.boards().piece.into_iter().next().unwrap().bounds();
        assert_close(near.x, -inches(12.125));
        assert_close(far.x, -inches(0.125));
    }

    #[test]
    fn the_guide_switches_only_while_no_board_is_at_the_blade() {
        let mut station = TableSawStation::default();
        push_to(&mut station, 2);
        assert!(!station.switch_guide());

        station.move_to(START);
        assert!(station.switch_guide());
        assert!(station.switch_guide());
        assert!(!station.using_gauge());
    }

    #[test]
    fn a_long_board_on_the_gauge_gets_a_warning() {
        let mut station = TableSawStation::default();
        assert!(station.switch_guide());
        // A stop past the board's width would miss it, so there'd be nothing to warn about.
        assert!(!station.status().contains("Careful"));
        station.set_gauge_stop(Length::from_inches(3));
        assert!(
            station
                .status()
                .ends_with("Switch to the rip fence to rip it.")
        );
    }

    #[test]
    fn the_gauge_stop_stays_in_range_and_the_parked_fence_stays_put() {
        let mut station = TableSawStation::default();
        station.switch_guide();
        station.set_gauge_stop(Length::ZERO);
        assert_eq!(station.saw.miter_gauge, Some(GAUGE_STOP_MIN));
        station.set_gauge_stop(Length::from_inches(100));
        assert_eq!(station.saw.miter_gauge, Some(GAUGE_STOP_MAX));
        station.set_fence(Length::from_inches(3));
        assert_eq!(station.saw.fence, Length::from_inches(6));
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

        let board = station.boards().whole.into_iter().next().unwrap().board;
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
        assert_eq!(
            loose[0].board().unwrap().to_string(),
            "3/4\" x 1-1/8\" x 4-7/8\""
        );
        assert!(!station.has_loose());
        assert!(station.status().ends_with("Put away with the offcuts."));
    }

    #[test]
    fn setting_a_board_down_clears_the_saw() {
        let mut station = TableSawStation::default();
        push_through(&mut station);
        station.take(Part::Piece);
        // The offcut left on the saw is set aside, for the inventory.
        station.load(Workpiece::Board(new_stock()));
        assert_eq!(station.take_set_aside().len(), 1);

        assert!(station.pass.is_none());
        assert!(!station.boards().whole.is_empty());
        assert!(station.taken.is_empty());
    }

    #[test]
    fn no_board_goes_down_while_one_is_at_the_blade() {
        let mut station = TableSawStation::default();
        assert!(station.can_load());
        push_to(&mut station, 6);
        assert!(!station.can_load());
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
    fn loading_a_board_sets_aside_the_board_that_was_waiting() {
        let mut station = TableSawStation::default();
        station.load(Workpiece::Board(crosscut_piece()));

        assert_eq!(
            station.take_set_aside(),
            [(Shelf::Parts, Workpiece::Board(new_stock()))]
        );
        assert_eq!(station.board, Workpiece::Board(crosscut_piece()));
        assert_eq!(
            station.status(),
            "Board on the saw: 3/4\" x 6\" x 12\". Drag it, or hold the push button, to push it through."
        );
    }

    #[test]
    fn loading_a_board_sets_aside_a_board_left_in_the_blade_with_its_slot() {
        let mut station = TableSawStation::default();
        push_to(&mut station, 10);
        station.load(Workpiece::Board(crosscut_piece()));
        let leftovers = station.take_set_aside();

        assert_eq!(leftovers.len(), 1);
        assert_eq!(leftovers[0].1.board().unwrap().kerfs().len(), 1);
    }

    #[test]
    fn loading_a_board_sets_aside_the_pieces_not_carried_off() {
        let mut station = TableSawStation::default();
        push_through(&mut station);
        station.take(Part::Piece);
        station.load(Workpiece::Board(crosscut_piece()));

        let left: Vec<(Shelf, Length)> = station
            .take_set_aside()
            .iter()
            .map(|(shelf, part)| (*shelf, part.board().unwrap().width()))
            .collect();
        assert_eq!(left, [(Shelf::Offcuts, Length::from_inch_fraction(9, 8))]);
        assert!(station.pass.is_none());
        assert!(station.taken.is_empty());
    }

    /// Where each corner of `placed`'s wood is on the table, in meters.
    fn wood_corners(placed: &crate::shop::Placed) -> Vec<Vec3> {
        placed
            .board
            .face()
            .iter()
            .flat_map(|region| region.corners())
            .flat_map(|(l, w)| {
                [Length::ZERO, placed.board.thickness()].map(|t| {
                    let mesh = Vec3::new(w.as_meters_f32(), t.as_meters_f32(), l.as_meters_f32());
                    placed.corner + placed.rotation * mesh
                })
            })
            .collect()
    }

    #[test]
    fn at_a_gauge_angle_the_board_is_drawn_turned_so_the_cut_is_on_the_blade() {
        let mut station = TableSawStation::default();
        assert!(station.switch_guide());
        assert!(station.turn_left());
        station.set_gauge_angle(Angle::from_degrees(30));
        station.push(10.0);
        let Some(Ok(Passed::Cut(crate::work::Parted::Board(cut)))) = station.result() else {
            panic!("expected the board crosscut");
        };
        assert_eq!(cut.piece.board.miters(), [Angle::from_degrees(30)]);
        // The piece against the gauge's stop is left of the blade, and the offcut right of it,
        // both right up to the kerf.
        let kerf = station.saw.blade.kerf.as_meters_f32();
        let boards = station.boards();
        let xs = |placed: &[crate::shop::Placed]| -> (f32, f32) {
            placed
                .iter()
                .flat_map(wood_corners)
                .fold((f32::MAX, f32::MIN), |(low, high), corner| {
                    (low.min(corner.x), high.max(corner.x))
                })
        };
        let (piece, offcut) = (xs(&boards.piece), xs(&boards.offcut));
        assert!((piece.1 + kerf).abs() < 1e-4, "{piece:?}");
        assert!(offcut.0.abs() < 1e-4, "{offcut:?}");
        // The gauge's head turns with it.
        assert_eq!(
            station.gauge_pose().1,
            Quat::from_rotation_y(-(30f32.to_radians()))
        );
    }

    #[test]
    fn a_mitered_piece_turned_over_and_end_for_end_is_mitered_again_at_the_same_stop() {
        let mut station = TableSawStation::default();
        assert!(station.switch_guide());
        assert!(station.turn_left());
        station.set_gauge_angle(Angle::from_degrees(30));
        station.set_gauge_stop(Length::from_inches(14));
        station.push(10.0);
        let piece = station.take(Part::Piece).expect("the mitered piece");
        // Set back on the saw across it and turned over, end for end, the miter's long point goes
        // against the stop and the blade comes in at the far end's corner.
        station.load(piece);
        assert!(station.turn_left());
        assert!(station.flip());
        station.push(10.0);
        let Some(Ok(Passed::Cut(crate::work::Parted::Board(cut)))) = station.result() else {
            panic!("expected the second miter cut, got {:?}", station.result());
        };
        assert_eq!(cut.piece.board.miters(), [Angle::from_degrees(30); 2]);
        assert_eq!(cut.piece.board.length(), Length::from_inches(14));
    }

    /// A 1x3, narrow enough to cut through standing on edge.
    fn narrow() -> Board {
        Board::new(
            Length::from_inches(24),
            Length::from_inches(3),
            Length::from_inch_fraction(3, 4),
        )
    }

    #[test]
    fn on_edge_and_across_a_tilted_blade_cuts_the_end_on_the_slant_across_the_width() {
        let mut station = TableSawStation::default();
        station.load(Workpiece::Board(narrow()));
        assert!(station.switch_guide());
        assert!(station.tip());
        assert!(station.turn_left());
        station.set_height(HEIGHT_MAX);
        station.set_tilt(Angle::from_degrees(30));
        station.push(10.0);
        let Some(Ok(Passed::Cut(crate::work::Parted::Board(cut)))) = station.result() else {
            panic!("expected the board crosscut, got {:?}", station.result());
        };
        assert_eq!(cut.piece.board.miters(), [Angle::from_degrees(30)]);
        assert!(cut.piece.board.bevels().is_empty());
    }

    #[test]
    fn on_edge_and_across_at_a_gauge_angle_a_tilted_blade_cuts_a_compound_angle() {
        let mut station = TableSawStation::default();
        station.load(Workpiece::Board(narrow()));
        assert!(station.switch_guide());
        assert!(station.tip());
        assert!(station.turn_left());
        station.set_height(HEIGHT_MAX);
        station.set_tilt(Angle::from_degrees(30));
        station.set_gauge_angle(Angle::from_degrees(15));
        station.push(10.0);
        let Some(Ok(Passed::Cut(crate::work::Parted::Board(cut)))) = station.result() else {
            panic!("expected the board crosscut, got {:?}", station.result());
        };
        // The end slants across the width and leans through the thickness.
        assert_eq!(cut.piece.board.miters().len(), 1);
        assert_eq!(cut.piece.board.bevels().len(), 1);
        // The board on edge, and the gauge's head with it, are turned on the table.
        assert_eq!(
            station.gauge_pose().1,
            Quat::from_rotation_y(-(15f32.to_radians()))
        );
    }

    #[test]
    fn on_edge_and_across_stopped_partway_the_slot_is_in_the_bottom_edge() {
        let mut station = TableSawStation::default();
        station.load(Workpiece::Board(narrow()));
        assert!(station.switch_guide());
        assert!(station.tip());
        assert!(station.turn_left());
        station.set_height(HEIGHT_MAX);
        // The blade's center 4.3" short of the leading face: its teeth are into the bottom
        // edge, partway through the thickness.
        station.move_to(-Length::from_nm(Length::from_inches(43).as_nm() / 10));
        assert_eq!(
            station.status(),
            "Cutting. So far, a 1/8\"-wide, 11/16\"-deep kerf in an edge, 11/32\" long."
        );
    }

    /// A 1x3 ripped 1-1/2" wide with the blade tilted 30°, beveled along one edge: low enough,
    /// resting on its bevel, for the blade to cut through.
    fn beveled() -> Board {
        let saw = TableSaw {
            tilt: Angle::from_degrees(30),
            height: Length::from_inches(2),
            fence: Length::from_inch_fraction(3, 2),
            ..TableSawStation::default().saw
        };
        let Ok(woodsim::Pass::Cut(cut)) = saw.rip(
            &Board::new(
                Length::from_inches(24),
                Length::from_inch_fraction(5, 2),
                Length::from_inch_fraction(3, 4),
            ),
            Orientation::default(),
        ) else {
            panic!("expected the bevel ripped");
        };
        cut.piece.board
    }

    #[test]
    fn a_board_rests_on_its_bevel_with_an_edge_to_the_fence_and_rips() {
        let board = beveled();
        let [(bevel, _)] = board.bevels()[..] else {
            panic!("expected one bevel");
        };
        let mut station = TableSawStation::default();
        station.load(Workpiece::Board(board.clone()));
        assert!(station.choose_rest());
        assert_eq!(
            station.status(),
            "Click the surface to rest it on, flat on the table."
        );
        assert!(station.pick_rest(0, Surface::Side(bevel)));
        assert!(!station.lie.is_square());
        assert_eq!(station.resting_on(), Some((0, Surface::Side(bevel))));
        assert!(
            station
                .status()
                .starts_with("Click the surface to go against the fence")
        );
        // The bottom face, 60° from the bevel, faces the fence as squarely as it can.
        assert!(station.pick_rest(0, Surface::Bottom));
        assert!(!station.choosing_rest());
        assert_eq!(station.resting_on(), None);
        station.set_height(HEIGHT_MAX);
        station.set_fence(Length::from_inches(1));
        station.push(30.0);
        let Some(Ok(Passed::Cut(crate::work::Parted::Board(cut)))) = station.result() else {
            panic!("expected the board ripped, got {:?}", station.result());
        };
        assert_eq!(cut.piece.board.origin().replay(), cut.piece.board);
    }

    #[test]
    fn a_slot_cut_resting_on_a_bevel_is_described_from_the_bevel() {
        let board = beveled();
        let [(bevel, _)] = board.bevels()[..] else {
            panic!("expected one bevel");
        };
        let mut station = TableSawStation::default();
        station.load(Workpiece::Board(board));
        assert!(station.choose_rest());
        assert!(station.pick_rest(0, Surface::Side(bevel)));
        assert!(station.pick_rest(0, Surface::Bottom));
        station.set_fence(Length::from_inches(1));
        station.set_height(HEIGHT_MAX);
        station.move_to(Length::from_inches(8));
        // In through the bevel it rests on, and out its top, not "0" long" in an edge.
        assert_eq!(
            station.status(),
            "Cutting. So far, a stopped resaw cut 12-5/8\" long on its beveled edge and \
             12-1/32\" on top."
        );
    }

    #[test]
    fn it_wont_rest_where_other_wood_reaches_past_and_says_so() {
        let mut stacked = woodsim::Assembly::of(beveled());
        assert!(stacked.attach(
            (0, Surface::Top),
            &woodsim::Assembly::of(Board::new(
                Length::from_inches(24),
                Length::from_inch_fraction(3, 2),
                Length::from_inch_fraction(3, 4),
            )),
            (0, Surface::Bottom),
            woodsim::Mating::default(),
            woodsim::Fastener::Glue,
        ));
        let mut station = TableSawStation::default();
        station.load(Workpiece::GlueUp(stacked));
        assert!(station.choose_rest());
        let before = station.lie;
        assert!(!station.pick_rest(0, Surface::Top));
        assert_eq!(station.lie, before);
        assert_eq!(
            station.status(),
            "It won't sit on that: other wood reaches past it. Click the surface to rest it on, \
             flat on the table."
        );
        // Pressed again, it stops choosing.
        assert!(station.choose_rest());
        assert!(!station.choosing_rest());
    }

    #[test]
    fn standing_up_goes_from_flat_to_its_edge_to_its_end_and_back_down() {
        let mut station = TableSawStation::default();
        station.load(Workpiece::Board(narrow()));
        let height = |station: &TableSawStation| {
            let (near, far) = station.boards().whole[0].bounds();
            far.y - near.y
        };
        assert_close(height(&station), inches(0.75));
        assert!(station.tip());
        assert!(station.lie.on_edge());
        assert_close(height(&station), inches(3.0));
        assert!(station.tip());
        assert!(station.lie.on_end());
        assert_close(height(&station), inches(24.0));
        assert!(station.tip());
        assert_eq!(station.lie, Orientation::default());
    }

    #[test]
    fn on_end_against_the_fence_the_blade_cuts_a_tenon_cheek() {
        let mut station = TableSawStation::default();
        station.load(Workpiece::Board(narrow()));
        assert!(station.tip());
        assert!(station.tip());
        station.set_fence(Length::from_inch_fraction(1, 4));
        station.set_height(Length::from_inch_fraction(3, 2));
        push_through(&mut station);
        assert_eq!(
            station.status(),
            "Cut a 1/8\"-wide, 1-1/2\"-deep resaw kerf in an end, all the way across. Bring the \
             board back around for another pass."
        );
    }

    #[test]
    fn on_end_a_short_block_is_resawn_through_its_length() {
        let mut station = TableSawStation::default();
        station.load(Workpiece::Board(Board::new(
            Length::from_inches(2),
            Length::from_inches(3),
            Length::from_inch_fraction(3, 4),
        )));
        assert!(station.tip());
        assert!(station.tip());
        station.set_fence(Length::from_inch_fraction(1, 4));
        station.set_height(HEIGHT_MAX);
        push_through(&mut station);
        assert!(
            station
                .status()
                .starts_with("Resawed a 1/4\" x 3\" x 2\" piece, plus a 3/8\" x 3\" x 2\" offcut."),
            "{}",
            station.status()
        );
    }

    #[test]
    fn on_edge_a_tilted_blade_resaws_on_the_slant() {
        let mut station = TableSawStation::default();
        station.load(Workpiece::Board(narrow()));
        assert!(station.tip());
        station.set_fence(Length::from_inch_fraction(5, 16));
        station.set_height(HEIGHT_MAX);
        station.set_tilt(Angle::from_degrees(3));
        push_through(&mut station);
        let Some(Ok(Passed::Cut(crate::work::Parted::Board(cut)))) = station.result() else {
            panic!("expected the board resawn, got {:?}", station.result());
        };
        // Each layer is thicker along one edge than the other: a face of each slopes, the way
        // bevel siding is made.
        let offcut = cut.offcut.as_ref().expect("the other layer");
        for layer in [&cut.piece.board, &offcut.board] {
            assert_eq!(layer.tapers().len(), 1);
            assert_eq!(layer.tapers()[0].1, Angle::from_degrees(3));
            assert!(layer.bevels().is_empty());
        }
        assert!(
            station
                .status()
                .ends_with("The piece is tapered 3° across its width."),
            "{}",
            station.status()
        );
    }

    #[test]
    fn the_gauge_clicks_from_stop_to_stop_and_turns_only_clear_of_the_blade() {
        let mut station = TableSawStation::default();
        station.swing_gauge_to_next_stop(true);
        assert_eq!(station.saw.gauge_angle, Angle::from_degrees(15));
        station.swing_gauge_to_next_stop(false);
        station.swing_gauge_to_next_stop(false);
        assert_eq!(station.saw.gauge_angle, Angle::from_degrees(-15));
        station.set_gauge_angle(Angle::from_degrees(80));
        assert_eq!(station.saw.gauge_angle, MAX_GAUGE);
        push_to(&mut station, 6);
        assert!(!station.can_swing_gauge());
        station.set_gauge_angle(Angle::ZERO);
        assert_eq!(station.saw.gauge_angle, MAX_GAUGE);
    }

    #[test]
    fn put_away_shelves_the_piece_and_the_offcut_and_clears_the_saw() {
        let mut station = TableSawStation::default();
        assert!(station.can_put_away());
        push_through(&mut station);
        station.put_away();
        let shelves: Vec<(Shelf, Length)> = station
            .take_set_aside()
            .iter()
            .map(|(shelf, part)| (*shelf, part.board().unwrap().width()))
            .collect();
        assert_eq!(
            shelves,
            [
                (Shelf::Parts, Length::from_inches(6)),
                (Shelf::Offcuts, Length::from_inch_fraction(9, 8)),
            ]
        );
        assert!(!station.can_put_away());
        assert!(station.boards().whole.is_empty());
    }

    #[test]
    fn cutting_again_rips_strip_after_strip() {
        let mut station = TableSawStation::default();
        station.saw.fence = Length::from_inches(2);
        assert!(!station.can_cut_again());
        push_through(&mut station);
        assert!(station.can_cut_again());
        station.cut_again();
        // The 2" strip is put away; what's left waits against the fence for the next pass.
        let set_aside = station.take_set_aside();
        assert_eq!(set_aside.len(), 1);
        assert_eq!(set_aside[0].0, Shelf::Parts);
        assert_eq!(
            set_aside[0].1.board().unwrap().width(),
            Length::from_inches(2)
        );
        let left =
            Length::from_inch_fraction(29, 4) - Length::from_inches(2) - station.saw.blade.kerf;
        assert_eq!(station.current_board().board().unwrap().width(), left);
        assert!(station.can_turn());
    }

    #[test]
    fn a_glue_up_rips_into_a_glued_piece_and_a_loose_strip_and_stays_flat() {
        // Two 4" boards edge to edge, ripped 5" from the fence: the far board and an inch of
        // the near one stay glued; the rest of the near one comes off on its own.
        let mut station = TableSawStation::default();
        let panel = crate::work::panel(&[Length::from_inches(4); 2], Length::from_inches(24));
        station.load(Workpiece::GlueUp(panel));
        assert!(!station.tip());
        station.set_fence(Length::from_inches(5));
        assert!(
            station
                .status()
                .starts_with("On the saw: a glue-up of 2 parts, 3/4\" x 8\" x 24\"."),
            "{}",
            station.status()
        );
        push_through(&mut station);

        let boards = station.boards();
        assert_eq!((boards.piece.len(), boards.offcut.len()), (2, 1));
        assert_eq!(
            station.status(),
            "Cut the glue-up apart: a glue-up of 2 parts, 3/4\" x 5\" x 24\", and 3/4\" x \
             2-7/8\" x 24\"."
        );
        let Some(Workpiece::GlueUp(piece)) = station.take(Part::Piece) else {
            panic!("expected the glued piece");
        };
        assert_eq!(piece.joints().len(), 1);
        assert!(matches!(
            station.take(Part::Offcut),
            Some(Workpiece::Board(_))
        ));
    }

    #[test]
    fn on_edge_a_narrow_board_resaws_into_two_thin_ones_side_by_side() {
        let mut station = TableSawStation::default();
        station.load(Workpiece::Board(Board::new(
            Length::from_inches(24),
            Length::from_inches(3),
            Length::from_inch_fraction(3, 4),
        )));
        assert!(station.tip());
        station.set_fence(Length::from_inch_fraction(5, 16));
        station.set_height(HEIGHT_MAX);
        // Standing on edge, it's as tall as it was wide and as wide across as it was thick.
        let standing = station.boards().whole.into_iter().next().unwrap();
        let (near, far) = standing.bounds();
        assert_close(far.y - near.y, inches(3.0));
        assert_close(far.x - near.x, inches(0.75));
        push_through(&mut station);

        assert_eq!(
            station.status(),
            "Resawed a 5/16\" x 3\" x 24\" piece, plus a 5/16\" x 3\" x 24\" offcut. The blade \
             turned 1/8\" to sawdust."
        );
        // The piece rides the fence; the offcut is a kerf away, toward the operator.
        let boards = station.boards();
        let (piece, offcut) = (
            boards.piece.into_iter().next().unwrap().bounds(),
            boards.offcut.into_iter().next().unwrap().bounds(),
        );
        assert_close(piece.1.x, inches(5.0 / 16.0));
        assert_close(offcut.1.x, inches(-1.0 / 8.0));
    }
}

//! The jointer station's state: the jointer, the board on it, and how far it has been fed.
//!
//! `woodsim` decides what the cutterhead does to the board; this module keeps track of where the
//! board is and where to draw it. Positions are in meters, in table space: x = 0 is the end of
//! the knives, out over the rabbeting ledge, with +x toward the fence; y = 0 is the outfeed
//! table, level with the top of the knives' circle, the infeed table lower by the depth of cut;
//! and z = 0 is over the cutterhead's center, with the board feeding toward -z, away from the
//! operator.
//!
//! Table space is the jointer's own frame: across the tables (x), up (y), and along them (z).
//! The board lies against the fence as its `Orientation` says, and leans with the fence when it's
//! tilted (`Jointer::leaned`), resting on its lowest edge.

use std::sync::Arc;

use bevy::prelude::*;
use woodsim::{
    Angle, Axis, CutError, Jointer, Length, MAX_FENCE_TILT, Orientation, Recipe, Refusal, Remake,
    Surface, Tool,
};

use crate::{
    chop::MITER_STOPS,
    feed::shelf,
    hand::Workpiece,
    inventory::Shelf,
    shop::{Part, Placed, SawBoards, describe_kerf, describe_size, measured},
    work::{self, Passed, Rest},
};

/// How close the fence comes to the knives' end, leaving a strip that wide to cut, and how far
/// back it goes, all of them in use.
pub const FENCE_MIN: Length = Length::from_inch_fraction(1, 8);
pub const FENCE_MAX: Length = Length::from_inches(6);
/// As deep as the infeed table drops: for a rabbet in one pass. Jointing a face or an edge takes
/// far less.
pub const DEPTH_MAX: Length = Length::from_inch_fraction(1, 2);
/// The knives reach at most the cutterhead's radius in front of its center, so a board this far
/// in front of the center (or further) is clear of them.
const CLEAR: Length = Length::from_inches(2);
/// Where a board waits: its leading end 1" short of where the knives could reach it.
const START: Length = Length::from_inches(-3);
/// Shorter than this, a board's too short to joint safely: its end can tip into the cutterhead's
/// gap as it crosses.
const SHORTEST: Length = Length::from_inches(12);

#[derive(Resource)]
pub struct JointerStation {
    pub jointer: Jointer,
    /// The board or glue-up on the jointer, with what earlier passes left. During a pass, this
    /// is how it was before the pass.
    board: Workpiece,
    /// How the board lies against the fence, before it leans with it.
    lie: Orientation,
    /// Where the cutterhead's center is along the board, from its leading end: negative while the
    /// board is in front of it.
    advance: Length,
    /// The pass under way: from when the board starts moving until it's pulled back clear of the
    /// cutterhead, brought back around for another, or replaced.
    pass: Option<Feeding>,
    /// Parts carried off the jointer.
    taken: Vec<Part>,
    /// Wood the jointer has set aside, for the inventory: what a new board displaced, and
    /// what's been put away.
    set_aside: Vec<(Shelf, Workpiece)>,
    /// Choosing what the board rests on, and against the fence.
    rest: Rest,
    /// How many times the board has moved and the knives have taken more wood, for chips.
    bites: u32,
}

struct Feeding {
    /// The jointer as it was set when the pass began.
    jointer: Jointer,
    /// The furthest back and forward the cutterhead's center has been along the board.
    reached: (Length, Length),
    result: Result<Passed, CutError>,
    /// Whether pieces the pass set loose have been handed off to set aside.
    loose_set_aside: bool,
}

impl Default for JointerStation {
    fn default() -> JointerStation {
        JointerStation {
            jointer: Jointer::six_inch(),
            board: Workpiece::Board(new_stock()),
            lie: Orientation::default(),
            advance: START,
            pass: None,
            // It starts with nothing on it.
            taken: vec![Part::Whole],
            set_aside: Vec::new(),
            rest: Rest::default(),
            bites: 0,
        }
    }
}

/// A fresh 1x6, 24" long: what's on the jointer before anything is, never shown.
fn new_stock() -> woodsim::Board {
    woodsim::Board::new(
        Length::from_inches(24),
        Length::from_inch_fraction(11, 2),
        Length::from_inch_fraction(3, 4),
    )
}

impl JointerStation {
    /// Raises or lowers the infeed table, from level with the outfeed table to as deep as it
    /// goes. It stays put while a board is at the cutterhead.
    pub fn set_depth(&mut self, depth: Length) {
        if !self.at_head() {
            self.jointer.depth = depth.clamp(Length::ZERO, DEPTH_MAX);
        }
    }

    /// Slides the fence across the tables, within its range. It stays put while a board is at
    /// the cutterhead.
    pub fn set_fence(&mut self, fence: Length) {
        if !self.at_head() {
            self.jointer.fence = fence.clamp(FENCE_MIN, FENCE_MAX);
        }
    }

    /// Tilts the fence, from square to as far as it goes. It stays put while a board is at the
    /// cutterhead.
    pub fn set_tilt(&mut self, tilt: Angle) {
        if !self.at_head() {
            self.jointer.tilt = tilt.clamp(Angle::ZERO, MAX_FENCE_TILT);
        }
    }

    /// Tilts the fence to the next stop on its scale, further or back, or as far as it goes past
    /// the last one.
    pub fn tilt_to_next_stop(&mut self, further: bool) {
        let now = self.jointer.tilt;
        let stops = MITER_STOPS.into_iter();
        let next = if further {
            stops
                .filter(|&stop| stop > now)
                .min()
                .unwrap_or(MAX_FENCE_TILT)
        } else {
            stops
                .filter(|&stop| stop < now)
                .max()
                .unwrap_or(Angle::ZERO)
        };
        self.set_tilt(next);
    }

    /// Whether the tables and the fence can be set: not while a board is at the cutterhead.
    pub fn can_set(&self) -> bool {
        !self.at_head()
    }

    /// Turns the board a quarter turn to the left, seen from above. Returns false unless it's
    /// in one piece, waiting in front of the cutterhead.
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

    /// Lays the board down another way, if it's in one piece and waiting in front of the
    /// cutterhead.
    fn relay(&mut self, lie: Orientation) -> bool {
        if !self.can_turn() {
            return false;
        }
        self.lie = lie;
        true
    }

    /// What's on the jointer, written out: as it was before the pass under way, with the jointer
    /// as it was set for the pass, how it lies, and how far the pass has gone, so it can be made
    /// again.
    pub fn recipe(&self) -> Option<Recipe> {
        self.on_jointer()?;
        let (jointer, travel) = match &self.pass {
            Some(pass) => (pass.jointer, pass.reached),
            None => (self.jointer, (self.advance, self.advance)),
        };
        Some(Recipe {
            wood: work::wood(&self.board),
            saw: Some(Tool::Jointer {
                jointer,
                lie: work::lying(&self.board, self.lie),
                travel,
            }),
        })
    }

    /// Starts choosing what goes flat against the fence, with a click on it, or stops: while it
    /// waits in front of the cutterhead. Whatever it then rests on is what's jointed.
    pub fn choose_rest(&mut self) -> bool {
        if !self.can_turn() {
            return false;
        }
        self.rest.toggle_against();
        true
    }

    /// Whether a click on the board picks what goes against the fence, rather than picking it
    /// up.
    pub fn choosing_rest(&self) -> bool {
        self.rest.choosing.is_some() && self.can_turn()
    }

    /// Picks `surface` of the board in `slot` (its place in a glue-up) to go flat against the
    /// fence. Returns false, saying why in the status, if it won't go that way.
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

    /// The surface the board rests on while the one for the fence is chosen: none at the
    /// jointer, where only the fence's is.
    pub fn resting_on(&self) -> Option<(usize, Surface)> {
        self.rest.down().filter(|_| self.choosing_rest())
    }

    /// Whether the board can be turned or flipped: it's in one piece, waiting in front of the
    /// cutterhead.
    pub fn can_turn(&self) -> bool {
        self.on_jointer() == Some(Part::Whole) && self.pass.is_none() && self.advance <= -CLEAR
    }

    /// Whether it can stand on an edge: it can turn, and it's a board rather than a glue-up.
    pub fn can_tip(&self) -> bool {
        self.can_turn() && !matches!(self.board, Workpiece::GlueUp(_)) && self.lie.is_square()
    }

    /// Whether there's a board to push across.
    pub fn can_push(&self) -> bool {
        self.on_jointer().is_some()
    }

    /// Whether `bring_back` would bring the board back around for another pass.
    pub fn can_bring_back(&self) -> bool {
        match self.on_jointer() {
            Some(Part::Whole) => !self.at_head() && self.advance != START,
            Some(Part::Piece) => self.past_head() && !self.has_offcut(),
            _ => false,
        }
    }

    /// Where the cutterhead's center is along the board, from its leading end: negative while
    /// the board is in front of it.
    pub fn advance(&self) -> Length {
        self.advance
    }

    /// Pushes the board `distance` meters along the fence, or pulls it back if `distance` is
    /// negative. It moves in 64ths of an inch.
    pub fn push(&mut self, distance: f32) {
        let step = crate::shop::snap(distance, 64);
        self.move_to(self.advance + step);
    }

    /// Slides the board along the fence until the cutterhead's center is `advance` along it. A
    /// board the knives have taken a layer off, or pushed all the way past them, only goes
    /// forward: you bring it back around.
    pub fn move_to(&mut self, advance: Length) {
        if self.on_jointer().is_none() {
            return;
        }
        self.rest = Rest::default();
        let back_limit = if self.cut_apart() || self.past_head() {
            self.advance
        } else {
            START
        };
        let advance = advance.clamp(back_limit, self.reach() + CLEAR + Length::from_inches(1));
        if advance == self.advance {
            return;
        }
        let pass = self.pass.get_or_insert(Feeding {
            jointer: self.jointer,
            reached: (self.advance, self.advance),
            result: Ok(Passed::Missed),
            loose_set_aside: false,
        });
        pass.reached = (pass.reached.0.min(advance), pass.reached.1.max(advance));
        let result = work::joint(
            &pass.jointer,
            &self.board,
            self.lie,
            pass.reached.0,
            pass.reached.1,
        );
        if result != pass.result && matches!(result, Ok(Passed::Kerfed(_) | Passed::Cut(_))) {
            self.bites += 1;
        }
        pass.result = result;
        self.advance = advance;
        // Pulled back clear of the cutterhead, the board keeps whatever step it got, and the
        // tables and fence are free to move for the next pass.
        if self.advance <= -CLEAR {
            self.finish_pass();
        }
    }

    /// Brings the board back around to the infeed table, ready for another pass: as it is, if
    /// it's clear of the cutterhead, or what the knives left of it, once it's all the way past.
    /// Returns false if it's at the cutterhead, or gone.
    pub fn bring_back(&mut self) -> bool {
        if !self.can_bring_back() {
            return false;
        }
        if let Some(Ok(Passed::Cut(cut))) = self.result() {
            let (piece, lie) = cut.piece_in_place(self.lie);
            self.load_set_aside_loose();
            self.board = piece;
            self.lie = lie;
            self.pass = None;
            self.taken.clear();
        } else {
            self.finish_pass();
        }
        self.advance = START;
        true
    }

    /// Lifts a board off the jointer to carry it elsewhere: the whole board, once it's clear of
    /// the cutterhead, or what a pass left of one, once it's past.
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

    /// Whether `part` is on the jointer and clear of the cutterhead, so it can be carried off.
    pub fn can_take(&self, part: Part) -> bool {
        if self.taken.contains(&part) {
            return false;
        }
        match (part, self.result()) {
            (Part::Whole, Some(Ok(Passed::Cut(_)))) => false,
            (Part::Whole, _) => !self.at_head(),
            (Part::Piece, Some(Ok(Passed::Cut(_)))) => self.past_head(),
            (Part::Offcut, Some(Ok(Passed::Cut(cut)))) => cut.has_offcut() && self.past_head(),
            _ => false,
        }
    }

    /// Whether a pass left a second part, where the knives took the glue between two of a
    /// glue-up's boards.
    fn has_offcut(&self) -> bool {
        matches!(self.result(), Some(Ok(Passed::Cut(cut))) if cut.has_offcut())
            && !self.taken.contains(&Part::Offcut)
    }

    /// Whether the last pass set pieces loose that haven't been handed off yet.
    pub fn has_loose(&self) -> bool {
        matches!(&self.pass, Some(pass) if !pass.loose_set_aside
            && matches!(&pass.result, Ok(Passed::Cut(cut)) if cut.has_loose()))
    }

    /// Hands off the pieces the last pass set loose, once, to be set aside.
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

    /// Sets aside what the last pass set loose, for the inventory.
    fn load_set_aside_loose(&mut self) {
        let loose = self.take_loose();
        self.set_aside
            .extend(loose.into_iter().map(|strip| (Shelf::Offcuts, strip)));
    }

    /// What's on the jointer that hasn't been carried off, as each would be handed over: the
    /// board, or what a pass left of it.
    pub fn wood(&self) -> Vec<(Part, Workpiece)> {
        match self.result() {
            Some(Ok(Passed::Cut(cut))) => [Part::Piece, Part::Offcut]
                .into_iter()
                .filter(|part| !self.taken.contains(part))
                .filter_map(|part| Some((part, cut.take(part, self.lie)?)))
                .collect(),
            _ if self.taken.contains(&Part::Whole) => Vec::new(),
            _ => vec![(Part::Whole, work::hand_off(self.current_board(), self.lie))],
        }
    }

    /// Makes the wood on the jointer again as `remake` makes it, and the pass under way, or
    /// just made, again on it: made the new way, if it's the pass being changed. Changes nothing
    /// unless `apply`; returns whether anything would change, or why it can't.
    pub fn remake(&mut self, remake: &mut Remake, apply: bool) -> Result<bool, Refusal> {
        let board = work::remade(remake, &self.board)?;
        let Some(pass) = &self.pass else {
            let changed = board.is_some();
            if let (Some(board), true) = (board, apply) {
                self.board = board;
            }
            return Ok(changed);
        };
        let here = self.wood().iter().any(|(_, work)| {
            work::last_pass(work).is_some_and(|last| Arc::ptr_eq(&last, remake.pass()))
        });
        if board.is_none() && !here {
            return Ok(false);
        }
        let jointer = match (here, remake.tool()) {
            (true, Tool::Jointer { jointer, .. }) => jointer,
            _ => pass.jointer,
        };
        let board = board.unwrap_or_else(|| self.board.clone());
        let result = work::joint(&jointer, &board, self.lie, pass.reached.0, pass.reached.1);
        // What's been carried off came from what the pass left: it still has to leave it.
        let still_cut = matches!(result, Ok(Passed::Cut(_)));
        if !self.taken.is_empty() && !still_cut {
            let why = "the pass on the jointer doesn't take a layer off it".to_string();
            return Err(Refusal::Pass(remake.pass().number, why));
        }
        if apply {
            self.board = board;
            let pass = self.pass.as_mut().expect("a pass under way");
            pass.jointer = jointer;
            pass.result = result;
        }
        Ok(true)
    }

    /// Whether a board can be set down on the jointer: not while one is at the cutterhead.
    pub fn can_load(&self) -> bool {
        !self.at_head()
    }

    /// Puts `board` against the fence, lying flat, ready to joint its bottom face, setting aside
    /// whatever was on the jointer that wasn't carried off.
    pub fn load(&mut self, board: Workpiece) {
        self.load_set_aside_loose();
        let left = self
            .wood()
            .into_iter()
            .map(|(part, work)| (shelf(part), work));
        self.set_aside.extend(left.collect::<Vec<_>>());
        self.board = board;
        self.lie = Orientation::default();
        self.advance = START;
        self.pass = None;
        self.taken.clear();
        self.rest = Rest::default();
    }

    /// Puts away everything on the jointer that's clear of the cutterhead.
    pub fn put_away(&mut self) {
        for part in [Part::Whole, Part::Piece, Part::Offcut] {
            if let Some(work) = self.take(part) {
                self.set_aside.push((shelf(part), work));
            }
        }
    }

    /// Whether there's anything clear of the cutterhead to put away.
    pub fn can_put_away(&self) -> bool {
        [Part::Whole, Part::Piece, Part::Offcut]
            .into_iter()
            .any(|part| self.can_take(part))
    }

    /// Whether there's wood set aside, or pieces a pass set loose, for the inventory.
    pub fn has_set_aside(&self) -> bool {
        !self.set_aside.is_empty() || self.has_loose()
    }

    /// Hands what's been set aside to the inventory, with pieces a pass set loose as offcuts.
    pub fn take_set_aside(&mut self) -> Vec<(Shelf, Workpiece)> {
        let loose = self.take_loose();
        let mut set_aside = std::mem::take(&mut self.set_aside);
        set_aside.extend(loose.into_iter().map(|strip| (Shelf::Offcuts, strip)));
        set_aside
    }

    /// Whether any of the board is over the cutterhead, where the tables and fence can't move.
    pub fn at_head(&self) -> bool {
        self.on_jointer().is_some() && self.advance > -CLEAR && self.advance < self.reach() + CLEAR
    }

    /// How many times the board has moved and the knives have taken more wood: they're cutting
    /// whenever this goes up.
    pub fn bites(&self) -> u32 {
        self.bites
    }

    /// Where the knives throw their chips, in table space, and which way: out of the gap between
    /// the tables on the open side of the wood, toward you, while the wood's over the
    /// cutterhead.
    pub fn chips(&self) -> Option<(Vec3, Vec3)> {
        let boards = self.boards();
        let (low, high) = [&boards.whole, &boards.piece, &boards.offcut]
            .into_iter()
            .flatten()
            .map(Placed::bounds)
            .fold((Vec3::MAX, Vec3::MIN), |(low, high), (near, far)| {
                (low.min(near), high.max(far))
            });
        let over = low.z < 0.0 && high.z > 0.0;
        over.then(|| {
            let side = low.x.max(0.0) - 0.004;
            (
                Vec3::new(side, 0.004, 0.0),
                Vec3::new(-0.6, 0.7, 0.4).normalize(),
            )
        })
    }

    /// Whether the whole board is past the cutterhead, on the outfeed table.
    pub fn past_head(&self) -> bool {
        self.advance >= self.reach() + CLEAR
    }

    /// How far the board reaches along the tables, from its leading end.
    fn reach(&self) -> Length {
        match &self.board {
            Workpiece::Board(board) => self.jointer.reach(board, self.lie),
            Workpiece::GlueUp(_) => work::reach(&self.board, self.leaned()).along,
        }
    }

    /// How the board lies on the tables, leaning with the fence as it's tilted for the pass.
    pub fn leaned(&self) -> Orientation {
        let jointer = self.pass.as_ref().map_or(self.jointer, |pass| pass.jointer);
        Orientation::from_rotation(jointer.leaned(self.lie.rotation()))
    }

    fn cut_apart(&self) -> bool {
        matches!(self.result(), Some(Ok(Passed::Cut(_))))
    }

    fn result(&self) -> Option<&Result<Passed, CutError>> {
        self.pass.as_ref().map(|pass| &pass.result)
    }

    /// The board in one piece as it is now, with any step the pass under way has cut.
    fn current_board(&self) -> Workpiece {
        match self.result() {
            Some(Ok(Passed::Kerfed(board))) => board.clone(),
            _ => self.board.clone(),
        }
    }

    /// Ends the pass under way, keeping what it cut.
    fn finish_pass(&mut self) {
        self.board = self.current_board();
        self.pass = None;
    }

    /// What's left on the jointer: the whole board, or what a pass left of it, or nothing.
    fn on_jointer(&self) -> Option<Part> {
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

    /// Where the board's boards are, in table space: against the fence, across from the knives'
    /// end, its lowest point on the infeed table, and its leading end as far along as it's been
    /// fed.
    pub fn boards(&self) -> SawBoards {
        let jointer = self.pass.as_ref().map_or(self.jointer, |pass| pass.jointer);
        let lie = self.leaned();
        let near = Vec3::new(
            (-work::knives_end(&jointer, &self.board, self.lie)).as_meters_f32(),
            (-jointer.depth).as_meters_f32(),
            (-self.advance).as_meters_f32(),
        );
        let unless_taken = |part: Part, placed: Vec<_>| {
            if self.taken.contains(&part) {
                Vec::new()
            } else {
                placed
            }
        };
        match self.result() {
            // What the knives left sits where it was, on the outfeed table.
            Some(Ok(Passed::Cut(cut))) => SawBoards {
                whole: Vec::new(),
                piece: unless_taken(Part::Piece, cut.placed(Part::Piece, &self.board, lie, near)),
                offcut: unless_taken(
                    Part::Offcut,
                    cut.placed(Part::Offcut, &self.board, lie, near),
                ),
            },
            _ => SawBoards {
                whole: unless_taken(Part::Whole, work::placed(&self.current_board(), lie, near)),
                ..default()
            },
        }
    }

    /// One line describing what's on the jointer.
    pub fn status(&self) -> String {
        if self.on_jointer().is_none() {
            return "Nothing on the jointer. Put a board on it from the inventory, or set down \
                    the one in hand."
                .to_string();
        }
        if self.can_turn()
            && let Some(rest) = self.rest.status()
        {
            return rest;
        }
        let jointer = self.pass.as_ref().map_or(self.jointer, |pass| pass.jointer);
        match self.result() {
            Some(Err(error)) => format!("No cut: {error}."),
            Some(Ok(Passed::Cut(cut))) => {
                let (piece, _) = cut.piece_in_place(self.lie);
                let left = match &piece {
                    Workpiece::Board(board) => format!("a {} piece", describe_size(board)),
                    Workpiece::GlueUp(_) => work::describe(&piece),
                };
                let mut words = format!(
                    "Jointed {} off {}, leaving {left}.",
                    measured(jointer.depth),
                    self.surface_down()
                );
                if let Workpiece::Board(board) = &piece {
                    let shaped: Vec<String> = [
                        crate::shop::describe_bevels(board),
                        crate::shop::describe_tapers(board),
                    ]
                    .into_iter()
                    .flatten()
                    .collect();
                    if !shaped.is_empty() {
                        words.push_str(&format!(" It's {}.", shaped.join(" and ")));
                    }
                }
                if self.has_loose() || cut.has_offcut() {
                    words.push_str(" It came apart where it was glued; put away with the offcuts.");
                } else {
                    words.push_str(" Bring it back around for another pass.");
                }
                words
            }
            Some(Ok(Passed::Kerfed(_))) if self.at_head() => {
                let done = self.advance.min(self.reach()).max(Length::ZERO);
                let reach = self.reach();
                let kind = if self.reaches_past_knives() {
                    " It reaches past the knives' end, so it'll come out rabbeted."
                } else {
                    ""
                };
                format!(
                    "Jointing: {} of {} across so far.{kind}{}",
                    measured(done),
                    measured(reach),
                    self.warning()
                )
            }
            Some(Ok(Passed::Kerfed(board))) => {
                let cut = match board {
                    Workpiece::Board(board) => {
                        describe_kerf(board, board.kerfs().last().expect("a pass's slot"))
                    }
                    Workpiece::GlueUp(_) => "a step into the glue-up".to_string(),
                };
                format!("Cut {cut}. Bring the board back around for another pass.")
            }
            Some(Ok(Passed::Missed)) if self.past_head() => {
                if jointer.depth == Length::ZERO {
                    "The infeed table is level with the outfeed, so the knives took nothing. \
                     Lower it, and bring the board back around."
                        .to_string()
                } else {
                    "The board went over without touching the knives: where it rests is past \
                     their end. Move the fence back, and bring it back around."
                        .to_string()
                }
            }
            _ => format!(
                "{}. Drag it, or hold the push button, to feed it across.{}",
                on_the_jointer(&self.board),
                self.warning()
            ),
        }
    }

    /// Whether the wood reaches past the knives' end, across the tables from the fence, so the
    /// knives leave a ledge along it.
    fn reaches_past_knives(&self) -> bool {
        let jointer = self.pass.as_ref().map_or(self.jointer, |pass| pass.jointer);
        work::knives_end(&jointer, &self.board, self.lie) > Length::ZERO
    }

    /// What lies on the tables, in words: "the bottom face", "an edge", "an end".
    fn surface_down(&self) -> &'static str {
        if !self.lie.is_square() {
            return "what it rested on";
        }
        match (&self.board, self.lie.up().0) {
            (Workpiece::GlueUp(_), _) => "the bottom",
            (_, Axis::Thickness) => "a face",
            (_, Axis::Width) => "an edge",
            (_, Axis::Length) => "an end",
        }
    }

    /// A warning, if the board's too short to joint safely.
    fn warning(&self) -> &'static str {
        if self.reach() < SHORTEST {
            " Careful: it's under 12\" long, too short to joint safely; its end can tip into the \
             cutterhead."
        } else {
            ""
        }
    }
}

/// What's on the jointer, for its status: "Board on the jointer: 3/4" x 6" x 24"".
fn on_the_jointer(work: &Workpiece) -> String {
    match work {
        Workpiece::Board(board) => format!(
            "Board on the jointer: {}",
            crate::shop::describe_board(board)
        ),
        Workpiece::GlueUp(_) => format!("On the jointer: {}", work::describe(work)),
    }
}

#[cfg(test)]
mod tests {
    use woodsim::Board;

    use super::*;
    use crate::shop::inches;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "{actual} m is not {expected} m"
        );
    }

    /// The jointer with a 1x6, 24" long, against the fence.
    fn one_by_six() -> JointerStation {
        let mut station = JointerStation::default();
        station.load(Workpiece::Board(new_stock()));
        station
    }

    /// Pushes the board all the way across the cutterhead.
    fn push_across(station: &mut JointerStation) {
        station.push(10.0);
    }

    fn piece(station: &JointerStation) -> Board {
        let boards = station.boards();
        let [piece] = boards.piece.as_slice() else {
            panic!("expected what the knives left: {boards:?}");
        };
        piece.board.clone()
    }

    #[test]
    fn it_starts_with_nothing_on_it() {
        let station = JointerStation::default();
        assert!(station.wood().is_empty());
        assert_eq!(station.boards(), SawBoards::default());
        assert!(station.status().starts_with("Nothing on the jointer."));
    }

    #[test]
    fn a_board_waits_against_the_fence_on_the_infeed_table() {
        let station = one_by_six();
        let boards = station.boards();
        let whole = boards.whole.into_iter().next().unwrap();
        // A 5-1/2" board against the fence 6" from the knives' end, its near edge 1/2" in.
        assert_close(whole.corner.x, inches(0.5));
        assert_close(whole.corner.y, -inches(1.0 / 32.0));
        assert_close(whole.corner.z, inches(3.0));
        assert!(!station.at_head());
    }

    #[test]
    fn fed_all_the_way_across_it_comes_off_thinner_on_the_outfeed_table() {
        let mut station = one_by_six();
        station.set_depth(Length::from_inch_fraction(1, 16));
        push_across(&mut station);
        assert!(station.past_head());
        let piece = piece(&station);
        assert_eq!(piece.to_string(), "11/16\" x 5-1/2\" x 24\"");
        // Its bottom is on the outfeed table, level with the knives' top.
        let placed = station.boards().piece.remove(0);
        assert_close(placed.bounds().0.y, 0.0);
        assert_eq!(
            station.status(),
            "Jointed 1/16\" off a face, leaving a 11/16\" x 5-1/2\" x 24\" piece. Bring it back \
             around for another pass."
        );
    }

    #[test]
    fn brought_back_around_it_takes_another_pass() {
        let mut station = one_by_six();
        station.set_depth(Length::from_inch_fraction(1, 16));
        for _ in 0..2 {
            push_across(&mut station);
            assert!(station.bring_back());
        }
        assert!(station.can_turn());
        let wood = station.wood();
        let [(Part::Whole, Workpiece::Board(board))] = wood.as_slice() else {
            panic!("one board, waiting");
        };
        assert_eq!(board.thickness(), Length::from_inch_fraction(5, 8));
        assert_eq!(board.origin().operations().len(), 2);
    }

    #[test]
    fn on_edge_it_joints_the_edge() {
        let mut station = one_by_six();
        assert!(station.tip());
        push_across(&mut station);
        assert_eq!(piece(&station).width(), Length::from_inch_fraction(175, 32));
        assert!(station.status().starts_with("Jointed 1/32\" off an edge"));
    }

    #[test]
    fn the_tables_and_fence_stay_put_at_the_cutterhead_and_in_their_range() {
        let mut station = one_by_six();
        station.set_depth(Length::from_inches(2));
        assert_eq!(station.jointer.depth, DEPTH_MAX);
        station.set_fence(Length::from_inches(9));
        assert_eq!(station.jointer.fence, FENCE_MAX);
        station.set_tilt(Angle::from_degrees(90));
        assert_eq!(station.jointer.tilt, MAX_FENCE_TILT);
        station.set_tilt(Angle::ZERO);

        station.move_to(Length::from_inches(4));
        assert!(station.at_head());
        station.set_depth(Length::from_inch_fraction(1, 8));
        station.set_fence(Length::from_inches(3));
        station.set_tilt(Angle::from_degrees(45));
        assert_eq!(station.jointer.depth, DEPTH_MAX);
        assert_eq!(station.jointer.fence, FENCE_MAX);
        assert_eq!(station.jointer.tilt, Angle::ZERO);
        assert!(!station.can_set());
    }

    #[test]
    fn partway_across_it_says_how_far_and_pulled_back_keeps_the_step() {
        let mut station = one_by_six();
        station.move_to(Length::from_inches(10));
        assert_eq!(station.status(), "Jointing: 10\" of 24\" across so far.");
        station.move_to(START);
        assert!(!station.at_head());
        let wood = station.wood();
        let [(Part::Whole, Workpiece::Board(board))] = wood.as_slice() else {
            panic!("one board, stepped");
        };
        assert_eq!(board.kerfs().len(), 1);
        assert_eq!(board.thickness(), Length::from_inch_fraction(3, 4));
    }

    #[test]
    fn a_board_wider_than_the_knives_reach_comes_out_rabbeted() {
        let mut station = one_by_six();
        station.set_fence(Length::from_inches(2));
        station.set_depth(Length::from_inch_fraction(3, 8));
        station.move_to(Length::from_inches(6));
        assert!(station.status().contains("it'll come out rabbeted"));
        push_across(&mut station);
        assert_eq!(
            station.status(),
            "Cut a 2\"-wide, 3/8\"-deep rabbet the full length. Bring the board back around \
             for another pass."
        );
        assert!(station.can_take(Part::Whole));
        assert!(station.bring_back());
    }

    #[test]
    fn with_the_fence_tilted_the_board_leans_on_it_and_its_edge_is_beveled() {
        let mut station = one_by_six();
        assert!(station.tip());
        station.set_tilt(Angle::from_degrees(45));
        station.set_depth(Length::from_inch_fraction(1, 8));
        push_across(&mut station);
        let bevels = piece(&station).bevels();
        assert_eq!(bevels.len(), 1);
        assert_eq!(bevels[0].1, Angle::from_degrees(45));
        assert!(
            station
                .status()
                .contains("It's beveled 45° along one edge.")
        );
    }

    #[test]
    fn set_level_the_knives_take_nothing() {
        let mut station = one_by_six();
        station.set_depth(Length::ZERO);
        push_across(&mut station);
        assert!(station.status().starts_with("The infeed table is level"));
        assert!(station.can_bring_back());
    }

    #[test]
    fn choosing_the_face_for_the_fence_stands_it_on_the_edge_to_joint() {
        let mut station = one_by_six();
        assert!(station.choose_rest());
        assert!(station.choosing_rest());
        assert!(
            station
                .status()
                .starts_with("Click the surface to go flat against the fence.")
        );
        // One click: its bottom face to the fence, and it's standing on an edge, done choosing.
        assert!(station.pick_rest(0, Surface::Bottom));
        assert!(!station.choosing_rest() && station.resting_on().is_none());
        push_across(&mut station);
        assert!(station.status().starts_with("Jointed 1/32\" off an edge"));
        assert_eq!(piece(&station).width(), Length::from_inch_fraction(175, 32));
    }

    #[test]
    fn it_wont_take_a_surface_other_wood_would_meet_the_fence_past() {
        // A 1x2 glued on top of the 1x6: the 1x6's top can't go to the fence, the 1x2 in the way.
        let one_by_two = Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(3, 2),
            Length::from_inch_fraction(3, 4),
        );
        let mut glue_up = woodsim::Assembly::of(new_stock());
        assert!(glue_up.attach(
            (0, Surface::Top),
            &woodsim::Assembly::of(one_by_two),
            (0, Surface::Bottom),
            woodsim::Mating::default(),
            woodsim::Fastener::Glue,
        ));
        let mut station = JointerStation::default();
        station.load(Workpiece::GlueUp(glue_up));
        assert!(station.choose_rest());
        assert!(!station.pick_rest(0, Surface::Top));
        assert!(station.choosing_rest());
        assert!(
            station
                .status()
                .starts_with("Other wood reaches past that, toward the fence.")
        );
    }

    #[test]
    fn a_short_board_gets_a_warning() {
        let mut station = one_by_six();
        station.load(Workpiece::Board(Board::new(
            Length::from_inches(8),
            Length::from_inches(3),
            Length::from_inch_fraction(3, 4),
        )));
        assert!(station.status().contains("too short to joint safely"));
    }

    #[test]
    fn what_a_pass_left_is_carried_off_once_its_past() {
        let mut station = one_by_six();
        // Across, but still over the cutterhead.
        station.move_to(Length::from_inches(25));
        assert!(!station.can_take(Part::Piece));
        push_across(&mut station);
        let taken = station.take(Part::Piece).unwrap();
        assert_eq!(
            taken.board().unwrap().thickness(),
            Length::from_inch_fraction(23, 32)
        );
        assert_eq!(station.boards(), SawBoards::default());
        assert!(!station.can_bring_back());
    }
}

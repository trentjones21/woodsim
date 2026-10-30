//! What's fed along a table past a cutterhead that takes a layer off it, at the jointer or the
//! planer: the machine, the board or glue-up on it, how it lies, how far it's been fed, and the
//! pass under way. Each machine says how it cuts and where the wood sits on it (`Machine`); its
//! station is an `Infeed` of it, with what's its own beside it (`dress.rs`, `thickness.rs`).
//!
//! Positions are in meters, in table space, the machine's own frame: across the table (x), up
//! (y), and along it (z), with z = 0 under the cutterhead's center and the board feeding toward
//! -z, away from the operator. Where x and y start is the machine's to say.

use std::sync::Arc;

use bevy::prelude::*;
use woodsim::{CutError, Length, Lie, Orientation, Recipe, Refusal, Remake, Surface, Tool};

use crate::{
    feed::shelf,
    hand::Workpiece,
    inventory::Shelf,
    shop::{Part, SawBoards},
    work::{self, Choosing, Passed, Rest},
};

/// A machine that takes a layer off wood fed past its cutterhead: what it does to the wood,
/// where the wood sits on it, and how it's written into a pass's history.
pub trait Machine: Copy + PartialEq + Send + Sync + 'static {
    /// The machine as it comes, set as it's first used.
    fn new() -> Self;

    /// What a pass does to `work`, lying as `lie` says, while the cutterhead's center travels
    /// from `start` to `end` along it, measured from its leading end.
    fn pass(
        &self,
        work: &Workpiece,
        lie: Orientation,
        start: Length,
        end: Length,
    ) -> Result<Passed, CutError>;

    /// How far `work`, lying as `lie` says, reaches along the table.
    fn reach(&self, work: &Workpiece, lie: Orientation) -> Length;

    /// How wood lying as `lie` says lies on the table: as it is, or leaning with a fence.
    fn laid(&self, lie: Orientation) -> Orientation;

    /// Where the near corner of the box `work` fills, lying as `lie` says, sits across the table
    /// and up it, in table space.
    fn near(&self, work: &Workpiece, lie: Orientation) -> (Length, Length);

    /// The radius of the knives' circle.
    fn radius(&self) -> Length;

    /// A pass of it set this way, as history keeps it.
    fn tool(self, lie: Lie, travel: (Length, Length)) -> Tool;

    /// The machine a pass was made with, if it was one of these.
    fn of(tool: Tool) -> Option<Self>;

    /// Whether rollers carry the wood through once they have it, so it can't be pulled back,
    /// and wood that won't go through stops at the mouth.
    fn rollers(&self) -> bool;

    /// What choosing what the wood rests on starts by picking.
    fn choosing() -> Choosing;
}

#[derive(Resource)]
pub struct Infeed<M: Machine> {
    /// The machine, as it's set.
    pub machine: M,
    /// The board or glue-up on it, with what earlier passes left. During a pass, this is how it
    /// was before the pass.
    board: Workpiece,
    /// How the board lies, before it leans with any fence.
    lie: Orientation,
    /// Where the cutterhead's center is along the board, from its leading end: negative while the
    /// board is in front of it.
    advance: Length,
    /// The pass under way: from when the board starts moving until it's pulled back clear of the
    /// cutterhead, brought back around for another, or replaced.
    pass: Option<Feeding<M>>,
    /// Parts carried off.
    taken: Vec<Part>,
    /// Wood set aside, for the inventory: what a new board displaced, and what's been put away.
    set_aside: Vec<(Shelf, Workpiece)>,
    /// Choosing what the board rests on.
    rest: Rest,
    /// How many times the board has moved and the knives have taken more wood, for chips.
    bites: u32,
    /// How many times it's been laid another way, turned or rested on something else, for
    /// drawing it turning.
    turns: u32,
}

struct Feeding<M> {
    /// The machine as it was set when the pass began.
    machine: M,
    /// The furthest back and forward the cutterhead's center has been along the board.
    reached: (Length, Length),
    result: Result<Passed, CutError>,
    /// Whether pieces the pass set loose have been handed off to set aside.
    loose_set_aside: bool,
}

impl<M: Machine> Default for Infeed<M> {
    fn default() -> Infeed<M> {
        let machine = M::new();
        Infeed {
            machine,
            board: Workpiece::Board(new_stock()),
            lie: Orientation::default(),
            advance: start(&machine),
            pass: None,
            // It starts with nothing on it.
            taken: vec![Part::Whole],
            set_aside: Vec::new(),
            rest: Rest::default(),
            bites: 0,
            turns: 0,
        }
    }
}

/// A fresh 1x6, 24" long: what's on a machine before anything is, never shown.
fn new_stock() -> woodsim::Board {
    woodsim::Board::new(
        Length::from_inches(24),
        Length::from_inch_fraction(11, 2),
        Length::from_inch_fraction(3, 4),
    )
}

/// The knives reach at most the cutterhead's radius in front of its center, so a board this far
/// in front of the center (or further) is clear of them.
fn clear<M: Machine>(machine: &M) -> Length {
    machine.radius() + Length::from_inch_fraction(3, 4)
}

/// Where a board waits: its leading end 1" short of where the knives could reach it.
fn start<M: Machine>(machine: &M) -> Length {
    -clear(machine) - Length::from_inches(1)
}

impl<M: Machine> Infeed<M> {
    /// Whether the machine can be set: not while a board is at the cutterhead.
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

    /// Stands the board up further (`work::stood_up`): on an edge, then on its end, then back
    /// down flat. A glue-up stays flat.
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
        self.turns += 1;
        true
    }

    /// What's on the machine, written out: as it was before the pass under way, with the
    /// machine as it was set for the pass, how it lies, and how far the pass has gone, so it can
    /// be made again.
    pub fn recipe(&self) -> Option<Recipe> {
        self.on_station()?;
        let (machine, travel) = match &self.pass {
            Some(pass) => (pass.machine, pass.reached),
            None => (self.machine, (self.advance, self.advance)),
        };
        Some(Recipe {
            wood: work::wood(&self.board),
            saw: Some(machine.tool(work::lying(&self.board, self.lie), travel)),
        })
    }

    /// Starts choosing what the board rests on, with a click on it, or stops: while it waits
    /// in front of the cutterhead.
    pub fn choose_rest(&mut self) -> bool {
        if !self.can_turn() {
            return false;
        }
        self.rest.toggle_from(M::choosing());
        true
    }

    /// Whether a click on the board picks what it rests on, rather than picking it up.
    pub fn choosing_rest(&self) -> bool {
        self.rest.choosing.is_some() && self.can_turn()
    }

    /// Picks `surface` of the board in `slot` (its place in a glue-up), as the choosing has got
    /// to. Returns false, saying why in the status, if it won't rest that way.
    pub fn pick_rest(&mut self, slot: usize, surface: Surface) -> bool {
        if !self.choosing_rest() {
            return false;
        }
        match self.rest.pick(&self.board, self.lie, (slot, surface)) {
            Some(lie) => {
                self.lie = lie;
                self.turns += 1;
                true
            }
            None => false,
        }
    }

    /// The surface the board rests on while another is chosen: none at these machines, where one
    /// click chooses.
    pub fn resting_on(&self) -> Option<(usize, Surface)> {
        self.rest.down().filter(|_| self.choosing_rest())
    }

    /// Whether the board can be turned or flipped: it's in one piece, waiting in front of the
    /// cutterhead.
    pub fn can_turn(&self) -> bool {
        self.on_station() == Some(Part::Whole)
            && self.pass.is_none()
            && self.advance <= -clear(&self.machine)
    }

    /// Whether it can stand on an edge: it can turn, and it's a board rather than a glue-up.
    pub fn can_tip(&self) -> bool {
        self.can_turn() && !matches!(self.board, Workpiece::GlueUp(_)) && self.lie.is_square()
    }

    /// Whether there's a board to push.
    pub fn can_push(&self) -> bool {
        self.on_station().is_some()
    }

    /// Whether `bring_back` would bring the board back around for another pass.
    pub fn can_bring_back(&self) -> bool {
        match self.on_station() {
            Some(Part::Whole) => !self.at_head() && self.advance != start(&self.machine),
            Some(Part::Piece) => self.past_head() && !self.has_offcut(),
            _ => false,
        }
    }

    /// Where the cutterhead's center is along the board, from its leading end: negative while
    /// the board is in front of it.
    pub fn advance(&self) -> Length {
        self.advance
    }

    /// Pushes the board `distance` meters along the table, or pulls it back if `distance` is
    /// negative. It moves in 64ths of an inch.
    pub fn push(&mut self, distance: f32) {
        let step = crate::shop::snap(distance, 64);
        self.move_to(self.advance + step);
    }

    /// Slides the board along the table until the cutterhead's center is `advance` along it. A
    /// board the knives have taken a layer off, or pushed all the way past them, or that rollers
    /// have hold of, only goes forward: you bring it back around. Wood that won't go through
    /// rollers stops at their mouth.
    pub fn move_to(&mut self, advance: Length) {
        if self.on_station().is_none() {
            return;
        }
        self.rest = Rest::default();
        // Refused, it never went in: the next try starts afresh, the machine as it's set now.
        if self.refused() {
            self.pass = None;
        }
        let held = self.machine.rollers() && self.at_head();
        let back_limit = if self.cut_apart() || self.past_head() || held {
            self.advance
        } else {
            start(&self.machine)
        };
        let clear = clear(&self.machine);
        let mut advance = advance.clamp(back_limit, self.reach() + clear + Length::from_inches(1));
        if advance == self.advance {
            return;
        }
        let pass = self.pass.get_or_insert(Feeding {
            machine: self.machine,
            reached: (self.advance, self.advance),
            result: Ok(Passed::Missed),
            loose_set_aside: false,
        });
        let reached = (pass.reached.0.min(advance), pass.reached.1.max(advance));
        let result = pass
            .machine
            .pass(&self.board, self.lie, reached.0, reached.1);
        if result.is_err() && pass.machine.rollers() {
            advance = advance.min(-clear);
        }
        pass.reached = (pass.reached.0.min(advance), pass.reached.1.max(advance));
        if result != pass.result && matches!(result, Ok(Passed::Kerfed(_) | Passed::Cut(_))) {
            self.bites += 1;
        }
        pass.result = result;
        self.advance = advance;
        // Pulled back clear of the cutterhead, the board keeps whatever step it got, and the
        // machine is free to be set for the next pass.
        if self.advance <= -clear && !self.refused() {
            self.finish_pass();
        }
    }

    /// Whether the machine refused the pass under way, saying why in the status.
    fn refused(&self) -> bool {
        matches!(self.result(), Some(Err(_)))
    }

    /// Brings the board back around to where boards wait, ready for another pass: as it is, if
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
        self.advance = start(&self.machine);
        true
    }

    /// Lifts a board off to carry it elsewhere: the whole board, once it's clear of the
    /// cutterhead, or what a pass left of one, once it's past.
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

    /// Whether `part` is on the machine and clear of the cutterhead, so it can be carried off.
    pub fn can_take(&self, part: Part) -> bool {
        if self.taken.contains(&part) {
            return false;
        }
        match (part, self.result()) {
            (Part::Whole, Some(Ok(Passed::Cut(_)))) => false,
            (Part::Whole, _) => !self.at_head() || self.refused(),
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

    /// What's on the machine that hasn't been carried off, as each would be handed over: the
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

    /// Makes the wood on the machine again as `remake` makes it, and the pass under way, or just
    /// made, again on it: made the new way, if it's the pass being changed. Changes nothing
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
        let machine = match (here, M::of(remake.tool())) {
            (true, Some(machine)) => machine,
            _ => pass.machine,
        };
        let board = board.unwrap_or_else(|| self.board.clone());
        let result = machine.pass(&board, self.lie, pass.reached.0, pass.reached.1);
        // What's been carried off came from what the pass left: it still has to leave it.
        let still_cut = matches!(result, Ok(Passed::Cut(_)));
        if !self.taken.is_empty() && !still_cut {
            let why = "the pass doesn't take a layer off it".to_string();
            return Err(Refusal::Pass(remake.pass().number, why));
        }
        if apply {
            self.board = board;
            let pass = self.pass.as_mut().expect("a pass under way");
            pass.machine = machine;
            pass.result = result;
        }
        Ok(true)
    }

    /// Whether a board can be set down on the machine: not while one is at the cutterhead.
    pub fn can_load(&self) -> bool {
        !self.at_head() || self.refused()
    }

    /// Puts `board` on the machine, lying flat, setting aside whatever was on it that wasn't
    /// carried off.
    pub fn load(&mut self, board: Workpiece) {
        self.load_set_aside_loose();
        let left = self
            .wood()
            .into_iter()
            .map(|(part, work)| (shelf(part), work));
        self.set_aside.extend(left.collect::<Vec<_>>());
        self.board = board;
        self.lie = Orientation::default();
        self.advance = start(&self.machine);
        self.pass = None;
        self.taken.clear();
        self.rest = Rest::default();
    }

    /// Puts away everything on the machine that's clear of the cutterhead.
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

    /// Whether any of the board is over the cutterhead, where the machine can't be set.
    pub fn at_head(&self) -> bool {
        let clear = clear(&self.machine);
        self.on_station().is_some() && self.advance > -clear && self.advance < self.reach() + clear
    }

    /// How many times the board has moved and the knives have taken more wood: they're cutting
    /// whenever this goes up.
    pub fn bites(&self) -> u32 {
        self.bites
    }

    /// How many times the board's been laid another way: it's turning whenever this goes up.
    pub fn turns(&self) -> u32 {
        self.turns
    }

    /// Whether the whole board is past the cutterhead, on the outfeed side.
    pub fn past_head(&self) -> bool {
        self.advance >= self.reach() + clear(&self.machine)
    }

    /// How far the board reaches along the table, from its leading end.
    pub(crate) fn reach(&self) -> Length {
        self.machine.reach(&self.board, self.lie)
    }

    /// The machine as it's set for the pass under way, or as it's set now.
    pub(crate) fn for_pass(&self) -> M {
        self.pass.as_ref().map_or(self.machine, |pass| pass.machine)
    }

    /// How the board lies on the table, as the machine lays it for the pass.
    pub fn laid(&self) -> Orientation {
        self.for_pass().laid(self.lie)
    }

    /// How the board lies, before it leans with any fence.
    pub(crate) fn lie(&self) -> Orientation {
        self.lie
    }

    /// The board or glue-up on the machine, as it was before the pass under way.
    pub(crate) fn board(&self) -> &Workpiece {
        &self.board
    }

    /// Choosing what it rests on, for the status.
    pub(crate) fn rest(&self) -> &Rest {
        &self.rest
    }

    fn cut_apart(&self) -> bool {
        matches!(self.result(), Some(Ok(Passed::Cut(_))))
    }

    pub(crate) fn result(&self) -> Option<&Result<Passed, CutError>> {
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

    /// What's left on the machine: the whole board, or what a pass left of it, or nothing.
    pub(crate) fn on_station(&self) -> Option<Part> {
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

    /// Where the board's boards are, in table space: where the machine puts the near corner of
    /// the box it fills, across and up, and its leading end as far along as it's been fed.
    pub fn boards(&self) -> SawBoards {
        let lie = self.laid();
        let (across, up) = self.for_pass().near(&self.board, self.lie);
        let near = Vec3::new(
            across.as_meters_f32(),
            up.as_meters_f32(),
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
            // What the knives left sits where it was.
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
}

//! The jointer station's state: the jointer, the board on it, and how far it has been fed, an
//! `Infeed` of the jointer, with what's the jointer's own: its depth of cut, its fence, and what
//! the status says.
//!
//! In table space, x = 0 is the end of the knives, out over the rabbeting ledge, with +x toward
//! the fence; y = 0 is the outfeed table, level with the top of the knives' circle, the infeed
//! table lower by the depth of cut. The board lies against the fence as its `Orientation` says,
//! and leans with the fence when it's tilted (`Jointer::leaned`), resting on its lowest edge.

use bevy::prelude::*;
use plans::{describe_kerf, describe_size, measured};
use woodsim::{Angle, Axis, CutError, Jointer, Length, Lie, MAX_FENCE_TILT, Orientation, Tool};

use crate::{
    chop::MITER_STOPS,
    hand::Workpiece,
    infeed::{Infeed, Machine},
    shop::Placed,
    work::{self, Choosing, Passed},
};

/// How close the fence comes to the knives' end, leaving a strip that wide to cut. It goes back
/// as far as the knives are long, all of them in use.
pub const FENCE_MIN: Length = Length::from_inch_fraction(1, 8);
/// As deep as the infeed table drops: for a rabbet in one pass. Jointing a face or an edge takes
/// far less.
pub const DEPTH_MAX: Length = Length::from_inch_fraction(1, 2);
/// Shorter than this, a board's too short to joint safely: its end can tip into the cutterhead's
/// gap as it crosses.
const SHORTEST: Length = Length::from_inches(12);

pub type JointerStation = Infeed<Jointer>;

impl Machine for Jointer {
    fn new() -> Jointer {
        Jointer::six_inch()
    }

    fn pass(
        &self,
        work: &Workpiece,
        lie: Orientation,
        start: Length,
        end: Length,
    ) -> Result<Passed, CutError> {
        work::joint(self, work, lie, start, end)
    }

    fn reach(&self, work: &Workpiece, lie: Orientation) -> Length {
        match work {
            Workpiece::Board(board) => Jointer::reach(self, board, lie),
            Workpiece::GlueUp(_) => work::reach(work, self.laid(lie)).along,
        }
    }

    fn laid(&self, lie: Orientation) -> Orientation {
        Orientation::from_rotation(self.leaned(lie.rotation()))
    }

    /// Against the fence, across from the knives' end, its lowest point on the infeed table.
    fn near(&self, work: &Workpiece, lie: Orientation) -> (Length, Length) {
        (-work::knives_end(self, work, lie), -self.depth)
    }

    fn radius(&self) -> Length {
        Jointer::radius(self)
    }

    fn tool(self, lie: Lie, travel: (Length, Length)) -> Tool {
        Tool::Jointer {
            jointer: self,
            lie,
            travel,
        }
    }

    fn of(tool: Tool) -> Option<Jointer> {
        match tool {
            Tool::Jointer { jointer, .. } => Some(jointer),
            _ => None,
        }
    }

    fn rollers(&self) -> bool {
        false
    }

    /// Just what goes against the fence: what's then lowest is what's jointed.
    fn choosing() -> Choosing {
        Choosing::Against
    }
}

impl JointerStation {
    /// Raises or lowers the infeed table, from level with the outfeed table to as deep as it
    /// goes. It stays put while a board is at the cutterhead.
    pub fn set_depth(&mut self, depth: Length) {
        if self.can_set() {
            self.machine.depth = depth.clamp(Length::ZERO, DEPTH_MAX);
        }
    }

    /// Slides the fence across the tables, from just in from the knives' end to as far back as
    /// they reach. It stays put while a board is at the cutterhead.
    pub fn set_fence(&mut self, fence: Length) {
        if self.can_set() {
            self.machine.fence = fence.clamp(FENCE_MIN, self.machine.knives);
        }
    }

    /// Tilts the fence, from square to as far as it goes. It stays put while a board is at the
    /// cutterhead.
    pub fn set_tilt(&mut self, tilt: Angle) {
        if self.can_set() {
            self.machine.tilt = tilt.clamp(Angle::ZERO, MAX_FENCE_TILT);
        }
    }

    /// Tilts the fence to the next stop on its scale, further or back, or as far as it goes past
    /// the last one.
    pub fn tilt_to_next_stop(&mut self, further: bool) {
        let now = self.machine.tilt;
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

    /// One line describing what's on the jointer.
    pub fn status(&self) -> String {
        if self.on_station().is_none() {
            return "Nothing on the jointer. Put a board on it from the inventory, or set down \
                    the one in hand."
                .to_string();
        }
        if self.can_turn()
            && let Some(rest) = self.rest().status()
        {
            return rest;
        }
        let jointer = self.for_pass();
        match self.result() {
            Some(Err(error)) => format!("No cut: {error}."),
            Some(Ok(Passed::Cut(cut))) => {
                let (piece, _) = cut.piece_in_place(self.lie());
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
                    let shaped: Vec<String> =
                        [plans::describe_bevels(board), plans::describe_tapers(board)]
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
                let done = self.advance().min(self.reach()).max(Length::ZERO);
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
                on_the_jointer(self.board()),
                self.warning()
            ),
        }
    }

    /// Whether the wood reaches past the knives' end, across the tables from the fence, so the
    /// knives leave a ledge along it.
    fn reaches_past_knives(&self) -> bool {
        work::knives_end(&self.for_pass(), self.board(), self.lie()) > Length::ZERO
    }

    /// What lies on the tables, in words: "the bottom face", "an edge", "an end".
    fn surface_down(&self) -> &'static str {
        let lie = self.lie();
        if !lie.is_square() {
            return "what it rested on";
        }
        match (self.board(), lie.up().0) {
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
        Workpiece::Board(board) => {
            format!("Board on the jointer: {}", plans::describe_board(board))
        }
        Workpiece::GlueUp(_) => format!("On the jointer: {}", work::describe(work)),
    }
}

#[cfg(test)]
mod tests {
    use woodsim::Board;

    use super::*;
    use crate::shop::{Part, SawBoards, inches};
    use woodsim::Surface;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "{actual} m is not {expected} m"
        );
    }

    /// The jointer with a 1x6, 24" long, against the fence.
    fn one_by_six() -> JointerStation {
        let mut station = JointerStation::default();
        station.load(Workpiece::Board(Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(11, 2),
            Length::from_inch_fraction(3, 4),
        )));
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
        assert_eq!(station.machine.depth, DEPTH_MAX);
        station.set_fence(Length::from_inches(9));
        assert_eq!(station.machine.fence, station.machine.knives);
        station.set_tilt(Angle::from_degrees(90));
        assert_eq!(station.machine.tilt, MAX_FENCE_TILT);
        station.set_tilt(Angle::ZERO);

        station.move_to(Length::from_inches(4));
        assert!(station.at_head());
        station.set_depth(Length::from_inch_fraction(1, 8));
        station.set_fence(Length::from_inches(3));
        station.set_tilt(Angle::from_degrees(45));
        assert_eq!(station.machine.depth, DEPTH_MAX);
        assert_eq!(station.machine.fence, station.machine.knives);
        assert_eq!(station.machine.tilt, Angle::ZERO);
        assert!(!station.can_set());
    }

    #[test]
    fn partway_across_it_says_how_far_and_pulled_back_keeps_the_step() {
        let mut station = one_by_six();
        station.move_to(Length::from_inches(10));
        assert_eq!(station.status(), "Jointing: 10\" of 24\" across so far.");
        station.move_to(-Length::from_inches(3));
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
        let mut glue_up = woodsim::Assembly::of(Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(11, 2),
            Length::from_inch_fraction(3, 4),
        ));
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

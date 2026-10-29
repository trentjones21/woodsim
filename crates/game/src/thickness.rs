//! The planer station's state: the planer, the board on it, and how far it's gone through, an
//! `Infeed` of the planer, with what's the planer's own: how thick it's set, its body built to
//! its size (`Body`), and what the status says.
//!
//! In table space, x = 0 is the middle of the bed across, the wood going through centered on
//! it, and y = 0 is the bed's top, where the wood rests.

use bevy::prelude::*;
use plans::{describe_board, describe_kerf, describe_size, measured};
use woodsim::{CutError, Length, Lie, Orientation, Planer, Tool};

use crate::{
    hand::Workpiece,
    infeed::{Infeed, Machine},
    shop::{Placed, inches},
    work::{self, Choosing, Passed},
};

/// Shorter than this, a board's too short to plane safely: it can tip into the cutterhead
/// between the rollers.
const SHORTEST: Length = Length::from_inches(12);

pub type PlanerStation = Infeed<Planer>;

impl Machine for Planer {
    fn new() -> Planer {
        Planer::thirteen_inch()
    }

    fn pass(
        &self,
        work: &Workpiece,
        lie: Orientation,
        start: Length,
        end: Length,
    ) -> Result<Passed, CutError> {
        work::plane(self, work, lie, start, end)
    }

    fn reach(&self, work: &Workpiece, lie: Orientation) -> Length {
        match work {
            Workpiece::Board(board) => Planer::reach(self, board, lie),
            Workpiece::GlueUp(_) => work::reach(work, lie).along,
        }
    }

    fn laid(&self, lie: Orientation) -> Orientation {
        lie
    }

    /// Centered across the bed, resting on it.
    fn near(&self, work: &Workpiece, lie: Orientation) -> (Length, Length) {
        let across = work::reach(work, lie).across;
        (-Length::from_nm(across.as_nm() / 2), Length::ZERO)
    }

    fn radius(&self) -> Length {
        Planer::radius(self)
    }

    fn tool(self, lie: Lie, travel: (Length, Length)) -> Tool {
        Tool::Planer {
            planer: self,
            lie,
            travel,
        }
    }

    fn of(tool: Tool) -> Option<Planer> {
        match tool {
            Tool::Planer { planer, .. } => Some(planer),
            _ => None,
        }
    }

    fn rollers(&self) -> bool {
        true
    }

    /// Just what rests on the bed: the top's planed parallel to it.
    fn choosing() -> Choosing {
        Choosing::Bed
    }
}

/// A planer's body, sized to it, in meters in table space: the bed and its wings, the head over
/// it, the posts the head rides on, and its rollers. A bigger planer is bigger all over: along
/// the bed, in proportion to how wide its knives are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    /// How far the bed reaches across from its middle, and along each way, and the wings past it.
    pub bed_across: f32,
    pub bed_along: f32,
    pub wings: f32,
    /// How far the head reaches across from its middle and along each way, how tall it is, and
    /// how far its bottom stands over the knives' circle, clear of what a pass takes.
    pub head_across: f32,
    pub head_along: f32,
    pub head_height: f32,
    pub head_clearance: f32,
    /// Where the four posts stand, across and along from the middle, and how high they reach.
    pub posts: Vec2,
    pub post_height: f32,
    /// The rollers either side of the cutterhead: how far along, and how big around.
    pub rollers_along: f32,
    pub roller_radius: f32,
}

impl Body {
    pub fn of(planer: &Planer) -> Body {
        let knives = planer.knives.as_meters_f32();
        let scale = knives / inches(13.0);
        let head_height = inches(6.0) * scale;
        Body {
            bed_across: knives / 2.0 + inches(1.0),
            bed_along: inches(10.0) * scale,
            wings: inches(12.0) * scale,
            head_across: knives / 2.0 + inches(2.5),
            head_along: inches(6.0) * scale,
            head_height,
            head_clearance: (planer.deepest + Length::from_inch_fraction(1, 4)).as_meters_f32(),
            posts: Vec2::new(knives / 2.0 + inches(1.75), inches(4.5) * scale),
            post_height: planer.thickest.as_meters_f32() + head_height + inches(1.0),
            rollers_along: inches(3.0) * scale,
            roller_radius: inches(0.6) * scale,
        }
    }
}

impl PlanerStation {
    /// Raises or lowers the head, from as thin as it planes to as thick as it takes. It stays put
    /// while a board is going through.
    pub fn set_thickness(&mut self, thickness: Length) {
        if self.can_set() {
            let planer = &mut self.machine;
            planer.thickness = thickness.clamp(planer.thinnest, planer.thickest);
        }
    }

    /// Whether the rollers have hold of the board and are carrying it through.
    pub fn feeding(&self) -> bool {
        self.at_head() && !matches!(self.result(), Some(Err(_)))
    }

    /// Where the knives throw their chips, in table space, and which way: out the dust port on
    /// top of the head, at the back, while the wood's under the cutterhead.
    pub fn chips(&self) -> Option<(Vec3, Vec3)> {
        let boards = self.boards();
        let (low, high) = [&boards.whole, &boards.piece, &boards.offcut]
            .into_iter()
            .flatten()
            .map(Placed::bounds)
            .fold((Vec3::MAX, Vec3::MIN), |(low, high), (near, far)| {
                (low.min(near), high.max(far))
            });
        let body = Body::of(&self.machine);
        let top = self.machine.thickness.as_meters_f32() + body.head_clearance + body.head_height;
        (low.z < 0.0 && high.z > 0.0).then(|| {
            (
                Vec3::new(body.head_across * 0.4, top + inches(2.0), -body.head_along),
                Vec3::new(0.2, 0.6, -1.0).normalize(),
            )
        })
    }

    /// How much a pass takes off what's on the planer, as it lies.
    fn takes(&self) -> Length {
        let planer = self.for_pass();
        match self.board() {
            Workpiece::Board(board) => planer.takes(board, self.lie()),
            Workpiece::GlueUp(glue_up) => planer.takes_glue_up(glue_up, self.lie().rotation()),
        }
    }

    /// One line describing what's on the planer.
    pub fn status(&self) -> String {
        if self.on_station().is_none() {
            return "Nothing on the planer. Put a board on it from the inventory, or set down \
                    the one in hand."
                .to_string();
        }
        if self.can_turn()
            && let Some(rest) = self.rest().status()
        {
            return rest;
        }
        let planer = self.for_pass();
        match self.result() {
            Some(Err(error @ CutError::TooDeep { .. })) => format!(
                "It won't go in: {error}. Raise the head, and take it down a pass at a time."
            ),
            Some(Err(error @ CutError::TooWide { .. })) => {
                format!("It won't go in: {error}. Rip it narrower first.")
            }
            Some(Err(error)) => format!("No cut: {error}."),
            Some(Ok(Passed::Cut(cut))) => {
                let (piece, _) = cut.piece_in_place(self.lie());
                let left = match &piece {
                    Workpiece::Board(board) => format!("a {} piece", describe_size(board)),
                    Workpiece::GlueUp(_) => work::describe(&piece),
                };
                let mut words = format!(
                    "Planed it {} thick, leaving {left}.",
                    measured(planer.thickness)
                );
                if self.has_loose() || cut.has_offcut() {
                    words.push_str(" It came apart where it was glued; put away with the offcuts.");
                } else {
                    words.push_str(" Bring it back around for another pass.");
                }
                words
            }
            Some(Ok(_)) if self.at_head() => {
                let done = self.advance().min(self.reach()).max(Length::ZERO);
                format!(
                    "Planing: {} of {} through so far.",
                    measured(done),
                    measured(self.reach())
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
                "The head's set higher than it's thick, so the knives took nothing. Lower it, \
                 and bring it back around."
                    .to_string()
            }
            _ => {
                let takes = self.takes();
                let taking = if takes == Length::ZERO {
                    " The head's above it: lower it to take some off.".to_string()
                } else if takes > planer.deepest {
                    format!(
                        " That's {} off, more than the {} a pass takes: raise the head.",
                        measured(takes),
                        measured(planer.deepest)
                    )
                } else {
                    format!(" A pass takes {} off.", measured(takes))
                };
                format!(
                    "{}.{taking} Drag it, or hold the push button, to feed it in.{}",
                    on_the_planer(self.board()),
                    self.warning()
                )
            }
        }
    }

    /// A warning, if the board's too short to plane safely.
    fn warning(&self) -> &'static str {
        if self.reach() < SHORTEST {
            " Careful: it's under 12\" long, too short to plane safely; it can tip into the \
             cutterhead between the rollers."
        } else {
            ""
        }
    }
}

/// What's on the planer, for its status: "Board on the planer: 3/4" x 6" x 24"".
fn on_the_planer(work: &Workpiece) -> String {
    match work {
        Workpiece::Board(board) => format!("Board on the planer: {}", describe_board(board)),
        Workpiece::GlueUp(_) => format!("On the planer: {}", work::describe(work)),
    }
}

#[cfg(test)]
mod tests {
    use woodsim::{Board, Surface};

    use super::*;
    use crate::shop::{Part, SawBoards};

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "{actual} m is not {expected} m"
        );
    }

    /// The planer with a 1x6, 24" long, on its bed.
    fn one_by_six() -> PlanerStation {
        let mut station = PlanerStation::default();
        station.load(Workpiece::Board(Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(11, 2),
            Length::from_inch_fraction(3, 4),
        )));
        station
    }

    /// Feeds it in, and lets the rollers carry it through, as the station does a frame at a
    /// time.
    fn feed_through(station: &mut PlanerStation) {
        station.push(0.2);
        for _ in 0..200 {
            if station.feeding() {
                station.push(0.01);
            }
        }
    }

    fn piece(station: &PlanerStation) -> Board {
        let boards = station.boards();
        let [piece] = boards.piece.as_slice() else {
            panic!("expected what the knives left: {boards:?}");
        };
        piece.board.clone()
    }

    #[test]
    fn it_starts_with_nothing_on_it() {
        let station = PlanerStation::default();
        assert!(station.wood().is_empty());
        assert_eq!(station.boards(), SawBoards::default());
        assert!(station.status().starts_with("Nothing on the planer."));
    }

    #[test]
    fn a_board_waits_centered_on_the_bed_saying_what_a_pass_takes() {
        let mut station = one_by_six();
        station.set_thickness(Length::from_inch_fraction(11, 16));
        let whole = station.boards().whole.remove(0);
        assert_close(whole.corner.x, -inches(2.75));
        assert_close(whole.corner.y, 0.0);
        assert!(!station.at_head());
        assert!(
            station.status().contains("A pass takes 1/16\" off."),
            "{}",
            station.status()
        );
    }

    #[test]
    fn the_rollers_carry_it_through_and_it_comes_out_the_setting_thick() {
        let mut station = one_by_six();
        station.set_thickness(Length::from_inch_fraction(11, 16));
        station.push(0.2);
        assert!(station.feeding());
        // Once they have it, it can't be pulled back.
        let advance = station.advance();
        station.move_to(-Length::from_inches(6));
        assert_eq!(station.advance(), advance);
        feed_through(&mut station);
        assert!(station.past_head());
        assert_eq!(piece(&station).to_string(), "11/16\" x 5-1/2\" x 24\"");
        assert_eq!(
            station.status(),
            "Planed it 11/16\" thick, leaving a 11/16\" x 5-1/2\" x 24\" piece. Bring it back \
             around for another pass."
        );
        assert!(station.bring_back());
        assert!(station.can_turn());
    }

    #[test]
    fn too_deep_a_pass_stops_at_the_mouth_until_the_head_goes_up() {
        let mut station = one_by_six();
        station.set_thickness(Length::from_inch_fraction(1, 2));
        assert!(
            station
                .status()
                .contains("more than the 1/8\" a pass takes")
        );
        station.push(0.2);
        assert!(!station.at_head() && !station.feeding());
        assert!(
            station
                .status()
                .starts_with("It won't go in: that's 1/4\" off in one pass"),
            "{}",
            station.status()
        );
        // Raised a step, it goes through.
        station.set_thickness(Length::from_inch_fraction(5, 8));
        feed_through(&mut station);
        assert_eq!(
            piece(&station).thickness(),
            Length::from_inch_fraction(5, 8)
        );
    }

    #[test]
    fn a_board_too_wide_wont_go_in() {
        let mut station = PlanerStation::default();
        station.load(Workpiece::Board(Board::new(
            Length::from_inches(24),
            Length::from_inches(14),
            Length::from_inch_fraction(3, 4),
        )));
        station.set_thickness(Length::from_inch_fraction(11, 16));
        station.push(0.2);
        assert!(station.status().contains("too wide to go through"));
        assert!(station.can_take(Part::Whole));
    }

    #[test]
    fn the_head_stays_put_while_a_board_goes_through_and_in_its_range() {
        let mut station = one_by_six();
        station.set_thickness(Length::from_inches(9));
        assert_eq!(station.machine.thickness, station.machine.thickest);
        station.set_thickness(Length::ZERO);
        assert_eq!(station.machine.thickness, station.machine.thinnest);
        station.set_thickness(Length::from_inch_fraction(11, 16));
        station.push(0.2);
        station.set_thickness(Length::from_inch_fraction(5, 8));
        assert_eq!(
            station.machine.thickness,
            Length::from_inch_fraction(11, 16)
        );
    }

    #[test]
    fn choosing_what_it_rests_on_is_one_click() {
        let mut station = one_by_six();
        assert!(station.choose_rest());
        assert!(
            station
                .status()
                .starts_with("Click the surface to rest it on")
        );
        // Its top face down: flipped over, and done choosing.
        assert!(station.pick_rest(0, Surface::Top));
        assert!(!station.choosing_rest());
        assert_eq!(station.lie(), Orientation::default().flipped());
    }

    #[test]
    fn a_bigger_planer_has_a_bigger_body() {
        let small = Body::of(&Planer::thirteen_inch());
        let big = Body::of(&Planer {
            knives: Length::from_inches(24),
            thickest: Length::from_inches(9),
            ..Planer::thirteen_inch()
        });
        assert!(big.bed_across > small.bed_across + inches(5.0));
        assert!(big.bed_along > small.bed_along && big.head_along > small.head_along);
        assert!(big.post_height > small.post_height);
        // The knives fit between the posts either way.
        for (body, knives) in [(small, 13.0), (big, 24.0)] {
            assert!(body.posts.x > inches(knives) / 2.0);
        }
    }
}

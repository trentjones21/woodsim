//! What's on a saw, a board or a glue-up alike: how far it reaches as it lies, what a pass of the
//! blade does to it, where its boards are drawn, and what's handed over when it's carried away.
//!
//! A glue-up lies on a saw the way a board would: its `x`, `y`, and `z` take the place of a
//! board's length, width, and thickness, and an `Orientation` lays them on the saw. It can't
//! stand on edge.

use bevy::{
    math::{DQuat, DVec2, DVec3},
    prelude::*,
};
use woodsim::{
    Assembly, Cut, CutError, GlueUpCut, GlueUpPass, Length, Lie, MiterSaw, Motion, Orientation,
    Pass, Surface, TableSaw, Unsteady, Wood, outline, surfaces,
};

use crate::{
    hand::{Workpiece, describe_glue_up},
    shop::{Part, Placed, describe_board, describe_cut},
};

/// What a pass of a saw did to what was on it.
#[derive(Clone, Debug, PartialEq)]
pub enum Passed {
    /// The blade didn't touch it.
    Missed,
    /// Slotted, or cut through some of a glue-up's boards, but still in one piece.
    Kerfed(Workpiece),
    Cut(Parted),
}

/// What was on a saw, cut apart.
#[derive(Clone, Debug, PartialEq)]
pub enum Parted {
    Board(Cut),
    GlueUp(GlueUpCut),
}

impl Passed {
    fn of_board(pass: Pass) -> Passed {
        match pass {
            Pass::Missed => Passed::Missed,
            Pass::Kerfed(board) => Passed::Kerfed(Workpiece::Board(board)),
            Pass::Cut(cut) => Passed::Cut(Parted::Board(cut)),
        }
    }

    fn of_glue_up(pass: GlueUpPass) -> Passed {
        match pass {
            GlueUpPass::Missed => Passed::Missed,
            GlueUpPass::Kerfed(glue_up) => Passed::Kerfed(Workpiece::GlueUp(glue_up)),
            GlueUpPass::Cut(cut) => Passed::Cut(Parted::GlueUp(cut)),
        }
    }
}

/// Feeds `work`, lying as `lie` says, past a table saw's blade while its center travels from
/// `start` to `end` along it, as `TableSaw::feed` does a board.
pub fn feed(
    saw: &TableSaw,
    work: &Workpiece,
    lie: Orientation,
    start: Length,
    end: Length,
) -> Result<Passed, CutError> {
    match work {
        Workpiece::Board(board) => saw.feed(board, lie, start, end).map(Passed::of_board),
        Workpiece::GlueUp(glue_up) => saw
            .feed_glue_up(glue_up, lie.rotation(), start, end)
            .map(Passed::of_glue_up),
    }
}

/// Brings a miter saw's blade down on `work`, lying as `lie` says against the stop `length`
/// from the blade, until its lowest point is `bottom` high, as `MiterSaw::chop` does a board.
pub fn chop(
    saw: &MiterSaw,
    work: &Workpiece,
    lie: Orientation,
    length: Length,
    bottom: Length,
) -> Result<Passed, CutError> {
    match work {
        Workpiece::Board(board) => saw.chop(board, lie, length, bottom).map(Passed::of_board),
        Workpiece::GlueUp(glue_up) => saw
            .chop_glue_up(glue_up, lie.rotation(), length, bottom)
            .map(Passed::of_glue_up),
    }
}

/// `work` as woodsim writes it out (`woodsim::Recipe`).
pub fn wood(work: &Workpiece) -> Wood {
    match work {
        Workpiece::Board(board) => Wood::Board(board.clone()),
        Workpiece::GlueUp(glue_up) => Wood::GlueUp(glue_up.clone()),
    }
}

/// How `work`, lying as `lie` says, lies on a saw, as a pass records it.
pub fn lying(work: &Workpiece, lie: Orientation) -> Lie {
    match work {
        Workpiece::Board(_) => Lie::Board(lie),
        Workpiece::GlueUp(_) => Lie::GlueUp(lie.rotation()),
    }
}

/// Choosing what something on a saw rests on: the surface to put down on the table, then the
/// one to go against the fence, each picked with a click.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rest {
    /// What the next click picks, while choosing.
    pub choosing: Option<Choosing>,
    /// Why the last surface picked wouldn't do.
    pub refused: Option<Unsteady>,
}

/// What the next click picks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Choosing {
    /// The surface to rest on, flat on the table.
    Down,
    /// The surface to go against the fence, resting on this one (a board's place in what's on
    /// the saw, and its surface).
    Fence((usize, Surface)),
}

impl Rest {
    /// Starts choosing, or stops.
    pub fn toggle(&mut self) {
        self.choosing = match self.choosing {
            Some(_) => None,
            None => Some(Choosing::Down),
        };
        self.refused = None;
    }

    /// Picks `surface` of `work`'s `member` (0 for a board), which lies as `lie` says: how it
    /// lies now, if that picks how it rests. Picked to rest on, it turns whichever surface it
    /// can to the fence that leaves it most nearly as it lay; picked for the fence, that one.
    pub fn pick(
        &mut self,
        work: &Workpiece,
        lie: Orientation,
        picked: (usize, Surface),
    ) -> Option<Orientation> {
        let resting = match self.choosing? {
            Choosing::Down => rest_on(work, picked, lie),
            Choosing::Fence(down) => glue_up_of(work).resting(down, picked),
        };
        match resting {
            Ok(rest) => {
                self.choosing = match self.choosing {
                    Some(Choosing::Down) => Some(Choosing::Fence(picked)),
                    _ => None,
                };
                self.refused = None;
                Some(rest)
            }
            Err(why) => {
                self.refused = Some(why);
                None
            }
        }
    }

    /// The surface it's resting on while the one for the fence is chosen.
    pub fn down(&self) -> Option<(usize, Surface)> {
        match self.choosing {
            Some(Choosing::Fence(down)) => Some(down),
            _ => None,
        }
    }

    /// What to click next, or why the last surface picked wouldn't do, for the status.
    pub fn status(&self) -> Option<String> {
        let refused = self.refused.map(|why| match why {
            Unsteady::Rocks => "It won't sit on that: other wood reaches past it. ",
            Unsteady::FacesUp => "That faces straight up or down, not toward the fence. ",
            Unsteady::Blocked => "Other wood reaches past that, toward the fence. ",
            Unsteady::NoSurface => "",
        });
        let next = match self.choosing? {
            Choosing::Down => "Click the surface to rest it on, flat on the table.",
            Choosing::Fence(_) => {
                "Click the surface to go against the fence, or carry on with it as it is."
            }
        };
        Some(format!("{}{next}", refused.unwrap_or_default()))
    }
}

/// `work` as a glue-up: a board, as a glue-up of one.
fn glue_up_of(work: &Workpiece) -> Assembly {
    match work {
        Workpiece::Board(board) => Assembly::of(board.clone()),
        Workpiece::GlueUp(glue_up) => glue_up.clone(),
    }
}

/// How `work`, lying as `lie` says, lies resting on `down`: tipped over onto it the shortest
/// way, as you'd tip it by hand (over about the table's length, like flipping, if it faces
/// straight up), then turned the least it can be to bring one of its surfaces to the fence.
fn rest_on(
    work: &Workpiece,
    down: (usize, Surface),
    lie: Orientation,
) -> Result<Orientation, Unsteady> {
    let glue_up = glue_up_of(work);
    let on_saw = |surface| Some(lie.rotation().apply(glue_up.facing(surface)?));
    let facing_down = on_saw(down).ok_or(Unsteady::NoSurface)?;
    // Tipped about the level line square to how it faces now, to face straight down.
    let below = Vec3::NEG_Y.as_dvec3();
    let now = DVec3::new(facing_down[1], facing_down[2], facing_down[0]);
    let axis = now.cross(below);
    let axis = if axis.length() < 1e-9 {
        DVec3::Z
    } else {
        axis.normalize()
    };
    let tip = DQuat::from_axis_angle(axis, now.angle_between(below));
    let mut refused = Unsteady::NoSurface;
    let mut best: Option<(f64, Orientation)> = None;
    for (index, member) in glue_up.members().iter().enumerate() {
        for surface in surfaces(&member.board) {
            match glue_up.resting(down, (index, surface)) {
                Ok(rest) => {
                    // How far it turns, once tipped, to face the fence.
                    let Some([along, across, up]) = on_saw((index, surface)) else {
                        continue;
                    };
                    let tipped = tip * DVec3::new(across, up, along);
                    let turn = DVec2::new(tipped.x, tipped.z).angle_to(DVec2::X).abs();
                    if best.is_none_or(|(least, _)| turn < least) {
                        best = Some((turn, rest));
                    }
                }
                // Rocking on it says the most: no surface for the fence helps that.
                Err(why) if refused != Unsteady::Rocks => refused = why,
                Err(_) => {}
            }
        }
    }
    best.map(|(_, rest)| rest).ok_or(refused)
}

/// How far something reaches on a saw, as it lies there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    /// Along the table.
    pub along: Length,
    /// Across it, toward the fence.
    pub across: Length,
    pub up: Length,
}

/// How far `work` reaches on a saw it lies on as `lie` says: the box its wood fills, turned as
/// it lies, which is the box the saw measures its settings from. Lying a square way, its
/// length, width, and thickness, or a glue-up's, whichever lies each way.
pub fn reach(work: &Workpiece, lie: Orientation) -> Reach {
    let rotation = lie.rotation();
    let nm = |length: Length| length.as_nm() as f64;
    let corners: Vec<[f64; 3]> = match work {
        Workpiece::Board(board) => board.corners().into_iter().map(|p| p.map(nm)).collect(),
        Workpiece::GlueUp(glue_up) => glue_up
            .members()
            .iter()
            .flat_map(|member| {
                member
                    .board
                    .corners()
                    .into_iter()
                    .map(|[l, w, t]| member.point(l, w, t).map(nm))
            })
            .collect(),
    };
    let size = |k: usize| {
        let (low, high) = corners
            .iter()
            .map(|&p| rotation.apply(p)[k])
            .fold((f64::MAX, f64::MIN), |(low, high), x| {
                (low.min(x), high.max(x))
            });
        Length::from_nm((high - low).round() as i64)
    };
    Reach {
        along: size(0),
        across: size(1),
        up: size(2),
    }
}

/// `work` in words: a board as `describe_board` puts it, or "a glue-up of 4 parts, ...".
pub fn describe(work: &Workpiece) -> String {
    match work {
        Workpiece::Board(board) => describe_board(board),
        Workpiece::GlueUp(glue_up) => format!("a {}", describe_glue_up(glue_up)),
    }
}

/// What's on a saw, for its status: "Board on the saw: 3/4" x 6" x 24"", or "On the saw: a
/// glue-up of 4 parts, ...".
pub fn on_the_saw(work: &Workpiece) -> String {
    match work {
        Workpiece::Board(board) => format!("Board on the saw: {}", describe_board(board)),
        Workpiece::GlueUp(_) => format!("On the saw: {}", describe(work)),
    }
}

/// The next way up for a board on a saw, each time it's stood up further: lying flat, it's
/// stood on an edge; on an edge, on its end; and on its end, it's laid back down flat the way it
/// first lay.
pub fn stood_up(lie: Orientation) -> Orientation {
    if lie.on_edge() {
        lie.stood()
    } else if lie.on_end() {
        lie.stood().tipped()
    } else {
        lie.tipped()
    }
}

/// `work` as it's carried away from a saw it lay on as `lie` says: a board as it is, and a
/// glue-up turned as it lay, so it's set down the same way up. A glue-up the saw left a single
/// board of is that board.
pub fn hand_off(work: Workpiece, lie: Orientation) -> Workpiece {
    match work {
        Workpiece::GlueUp(glue_up) => glue_up_off_saw(&glue_up, lie),
        board => board,
    }
}

fn glue_up_off_saw(glue_up: &Assembly, lie: Orientation) -> Workpiece {
    if let [member] = glue_up.members() {
        return Workpiece::Board(member.board.clone());
    }
    Workpiece::GlueUp(glue_up.moved(Motion {
        rotation: lie.rotation(),
        shift: [Length::ZERO; 3],
    }))
}

/// The saw's own frame (across, up, along as `x`, `y`, `z`) for an assembly lying as `lie`
/// says: its axes laid the way a board's are.
fn on_saw(lie: Orientation) -> impl Fn([f64; 3]) -> Vec3 {
    let rotation = lie.rotation();
    move |v| {
        let v = rotation.apply(v);
        Vec3::new(v[1] as f32, v[2] as f32, v[0] as f32)
    }
}

/// Where `work`'s boards sit on a saw, lying as `lie` says with the near corner of the space it
/// takes up at `near`.
pub fn placed(work: &Workpiece, lie: Orientation, near: Vec3) -> Vec<Placed> {
    match work {
        Workpiece::Board(board) => vec![Placed::lying(board, lie, near)],
        Workpiece::GlueUp(glue_up) => {
            let axes = on_saw(lie);
            let (low, _) = glue_up_bounds(glue_up, &axes);
            place_glue_up(glue_up, &axes, near - low)
        }
    }
}

impl Parted {
    /// Whether there's an offcut: the blade didn't turn the whole far side to sawdust.
    pub fn has_offcut(&self) -> bool {
        match self {
            Parted::Board(cut) => cut.offcut.is_some(),
            Parted::GlueUp(cut) => cut.offcut.is_some(),
        }
    }

    /// Whether the cut set any parts loose, past the piece and the offcut.
    pub fn has_loose(&self) -> bool {
        match self {
            Parted::Board(cut) => !cut.loose.is_empty(),
            Parted::GlueUp(cut) => !cut.loose.is_empty(),
        }
    }

    /// The piece or the offcut, as it's carried away from a saw it lay on as `lie` says.
    pub fn take(&self, part: Part, lie: Orientation) -> Option<Workpiece> {
        match (self, part) {
            (Parted::Board(cut), Part::Piece) => Some(Workpiece::Board(cut.piece.board.clone())),
            (Parted::Board(cut), Part::Offcut) => cut
                .offcut
                .as_ref()
                .map(|offcut| Workpiece::Board(offcut.board.clone())),
            (Parted::GlueUp(cut), Part::Piece) => Some(glue_up_off_saw(&cut.piece, lie)),
            (Parted::GlueUp(cut), Part::Offcut) => cut
                .offcut
                .as_ref()
                .map(|offcut| glue_up_off_saw(offcut, lie)),
            (_, Part::Whole) => None,
        }
    }

    /// The parts the cut set loose, as they're carried away.
    pub fn loose(&self, lie: Orientation) -> Vec<Workpiece> {
        match self {
            Parted::Board(cut) => cut
                .loose
                .iter()
                .map(|piece| Workpiece::Board(piece.board.clone()))
                .collect(),
            Parted::GlueUp(cut) => cut
                .loose
                .iter()
                .map(|part| glue_up_off_saw(part, lie))
                .collect(),
        }
    }

    /// Where the boards of the piece or the offcut sit: where they were in `whole`, which lay as
    /// `lie` says with the near corner of the space it took up at `near`.
    pub fn placed(
        &self,
        part: Part,
        whole: &Workpiece,
        lie: Orientation,
        near: Vec3,
    ) -> Vec<Placed> {
        match (self, whole) {
            (Parted::Board(cut), Workpiece::Board(board)) => {
                let whole = Placed::lying(board, lie, near);
                let side = match part {
                    Part::Piece => Some(&cut.piece),
                    Part::Offcut => cut.offcut.as_ref(),
                    Part::Whole => None,
                };
                side.map(|piece| vec![whole.piece(piece)])
                    .unwrap_or_default()
            }
            (Parted::GlueUp(cut), Workpiece::GlueUp(glue_up)) => {
                let side = match part {
                    Part::Piece => Some(&cut.piece),
                    Part::Offcut => cut.offcut.as_ref(),
                    Part::Whole => None,
                };
                let axes = on_saw(lie);
                let (low, _) = glue_up_bounds(glue_up, &axes);
                side.map(|side| place_glue_up(side, &axes, near - low))
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    /// What the cut did, in words, with `verb` for a board ("Ripped", "Cut"): the piece and
    /// the offcut.
    pub fn describe(&self, verb: &str, lie: Orientation) -> String {
        match self {
            Parted::Board(cut) => describe_cut(verb, cut),
            Parted::GlueUp(cut) => {
                let piece = describe(&glue_up_off_saw(&cut.piece, lie));
                match &cut.offcut {
                    Some(offcut) => format!(
                        "Cut the glue-up apart: {piece}, and {}.",
                        describe(&glue_up_off_saw(offcut, lie))
                    ),
                    None => format!("Trimmed the glue-up: {piece}."),
                }
            }
        }
    }
}

/// A point of an assembly in meters along its axes.
fn meters(point: [Length; 3]) -> [f64; 3] {
    point.map(|length| length.as_nm() as f64 / 1e9)
}

/// The box a glue-up fills in a station's frame, as `axes` turns the assembly's axes into it,
/// as its least and most corners in meters from the assembly's origin.
pub fn glue_up_bounds(assembly: &Assembly, axes: &dyn Fn([f64; 3]) -> Vec3) -> (Vec3, Vec3) {
    assembly
        .members()
        .iter()
        .flat_map(|member| {
            [Surface::Bottom, Surface::Top]
                .into_iter()
                .flat_map(|surface| outline(&member.board, surface))
                .flatten()
                .map(|[l, w, t]| axes(meters(member.point(l, w, t))))
                .collect::<Vec<_>>()
        })
        .fold((Vec3::MAX, Vec3::MIN), |(low, high), corner| {
            (low.min(corner), high.max(corner))
        })
}

/// Where each board of a glue-up sits in a station's frame, as `axes` turns the assembly's axes
/// into it and `origin` is where the assembly's origin is. Each board's grain is where it lies
/// in the assembly, so the boards don't all look alike and a piece cut from one keeps its grain.
pub fn place_glue_up(
    assembly: &Assembly,
    axes: &dyn Fn([f64; 3]) -> Vec3,
    origin: Vec3,
) -> Vec<Placed> {
    assembly
        .members()
        .iter()
        .map(|member| {
            let [l, w, t] = member.axes().map(axes);
            let local = member.rotation.inverse().apply(meters(member.at));
            Placed {
                board: member.board.clone(),
                corner: origin + axes(meters(member.at)),
                rotation: Quat::from_mat3(&Mat3::from_cols(w, t, l)),
                grain: Vec3::new(local[1] as f32, local[2] as f32, local[0] as f32),
            }
        })
        .collect()
}

/// A panel of boards glued edge to edge, each `widths` wide, `length` long, and 3/4" thick,
/// face up, each next one on the last's far edge.
#[cfg(test)]
pub fn panel(widths: &[Length], length: Length) -> Assembly {
    use woodsim::{Board, Fastener, Mating};
    let board = |width: Length| Board::new(length, width, Length::from_inch_fraction(3, 4));
    let edge = |board: &Board, far: bool| {
        woodsim::surfaces(board)
            .into_iter()
            .find(|surface| {
                matches!(surface, Surface::Side(edge) if edge.out.l == 0 && (edge.out.w > 0) == far)
            })
            .expect("an edge")
    };
    let mut panel = Assembly::of(board(widths[0]));
    for (k, &width) in widths.iter().enumerate().skip(1) {
        let (last, next) = (panel.members()[k - 1].board.clone(), board(width));
        let (onto, surface) = ((k - 1, edge(&last, true)), (0, edge(&next, false)));
        let other = Assembly::of(next);
        let mating = (0..4)
            .map(|turn| Mating {
                turn,
                slide: [Length::ZERO; 2],
            })
            .find(|&mating| {
                let motion = panel.mating(onto, &other, surface, mating).unwrap();
                motion.rotation.apply([0.0, 0.0, 1.0])[2] > 0.999
            })
            .expect("a way that keeps it face up");
        assert!(panel.attach(onto, &other, surface, mating, Fastener::Glue));
    }
    panel
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glue_up_flipped_on_a_saw_is_carried_away_flipped() {
        let glue_up = panel(&[Length::from_inches(2); 2], Length::from_inches(24));
        let Workpiece::GlueUp(carried) = hand_off(
            Workpiece::GlueUp(glue_up.clone()),
            Orientation::default().flipped(),
        ) else {
            panic!("expected a glue-up");
        };
        // Its boards' tops now face down, and it fills the same box.
        for member in carried.members() {
            assert_eq!(member.axes()[2], [0.0, 0.0, -1.0]);
        }
        let size = |glue_up: &Assembly| glue_up.bounds().map(|(least, most)| most - least);
        assert_eq!(size(&carried), size(&glue_up));
    }

    #[test]
    fn the_pieces_of_a_cut_glue_up_are_drawn_where_they_were() {
        let glue_up =
            Workpiece::GlueUp(panel(&[Length::from_inches(2); 2], Length::from_inches(24)));
        let saw = MiterSaw::twelve_inch();
        let lie = Orientation::default();
        let Ok(Passed::Cut(cut)) = chop(&saw, &glue_up, lie, Length::from_inches(10), saw.stroke.1)
        else {
            panic!("expected the glue-up cut apart");
        };
        let whole = placed(&glue_up, lie, Vec3::ZERO);
        let piece = cut.placed(Part::Piece, &glue_up, lie, Vec3::ZERO);
        let offcut = cut.placed(Part::Offcut, &glue_up, lie, Vec3::ZERO);
        assert_eq!((piece.len(), offcut.len()), (2, 2));
        let along = |placed: &[Placed]| {
            placed
                .iter()
                .map(Placed::bounds)
                .fold((f32::MAX, f32::MIN), |(low, high), (near, far)| {
                    (low.min(near.z), high.max(far.z))
                })
        };
        // Along the saw, the offcut and the piece reach from one end of the whole to the
        // other, with the kerf between them.
        let ((start, end), (offcut_start, offcut_end), (piece_start, piece_end)) =
            (along(&whole), along(&offcut), along(&piece));
        assert!((offcut_start - start).abs() < 1e-6 && (piece_end - end).abs() < 1e-6);
        let kerf = woodsim::Blade::FULL_KERF.as_meters_f32();
        assert!((piece_start - offcut_end - kerf).abs() < 1e-6);
        assert!((piece_end - piece_start - Length::from_inches(10).as_meters_f32()).abs() < 1e-6);
    }
}

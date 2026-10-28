//! What's on a saw, a board or a glue-up alike: how far it reaches as it lies, what a pass of the
//! blade does to it, where its boards are drawn, and what's handed over when it's carried away.
//!
//! A glue-up lies on a saw the way a board would: its `x`, `y`, and `z` take the place of a
//! board's length, width, and thickness, and an `Orientation` lays them on the saw. It can't
//! stand on edge.

use bevy::prelude::*;
use woodsim::{
    Assembly, Axis, Cut, CutError, GlueUpCut, GlueUpPass, Length, MiterSaw, Motion, Orientation,
    Pass, Surface, TableSaw, outline,
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

/// How far `work` reaches along one of its own axes: a board's length, width, or thickness,
/// or a glue-up's `x`, `y`, or `z`.
pub fn extent(work: &Workpiece, axis: Axis) -> Length {
    match work {
        Workpiece::Board(board) => axis.of(board),
        Workpiece::GlueUp(glue_up) => {
            let k = match axis {
                Axis::Length => 0,
                Axis::Width => 1,
                Axis::Thickness => 2,
            };
            let (least, most) = glue_up.bounds()[k];
            most - least
        }
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

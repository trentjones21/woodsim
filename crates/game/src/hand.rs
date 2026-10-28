//! Carrying wood around the shop. Click a piece of wood to pick it up, wherever it is: on a saw,
//! on the pile, or a new board off the lumber rack; at the workbench, where clicks pick
//! surfaces to put together, Space picks up a part or a glue-up. Then click where it goes to set
//! it down: a saw, the workbench, the pile, or the scrap bin to throw it away.

use bevy::prelude::*;
use woodsim::{Assembly, Board, Length, Surface, outline};

use crate::{
    chop::MiterSawStation,
    feed::TableSawStation,
    join::BenchStation,
    shop::{Placed, ShopView, describe_size, measured},
};

/// Wood to carry or set aside: one board, or boards fastened together.
#[derive(Clone, Debug, PartialEq)]
pub enum Workpiece {
    Board(Board),
    /// Boards glued, nailed, or screwed together, lying flat as they lay on the bench: the
    /// assembly's `z` is up.
    GlueUp(Assembly),
}

#[cfg(test)]
impl Workpiece {
    /// The board, if it's one board.
    pub fn board(&self) -> Option<&Board> {
        match self {
            Workpiece::Board(board) => Some(board),
            Workpiece::GlueUp(_) => None,
        }
    }
}

/// The wood being carried, if any.
#[derive(Resource, Default)]
pub struct Hand(pub Option<Workpiece>);

/// Whether the pointer press that's ending moved far enough to be a drag rather than a click: a
/// drag slides a board along the table saw or a stop along its fence; a click picks a board up.
#[derive(Resource, Default)]
pub struct Gesture {
    dragged: bool,
}

impl Gesture {
    pub fn dragged(&self) -> bool {
        self.dragged
    }
}

/// How far the pointer moves while pressed, in pixels, before it's a drag.
const DRAG_THRESHOLD: f32 = 6.0;

pub struct HandPlugin;

impl Plugin for HandPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hand>()
            .init_resource::<Gesture>()
            .add_observer(start_press)
            .add_observer(track_drag);
    }
}

fn start_press(_: On<PointerPress>, mut gesture: ResMut<Gesture>) {
    gesture.dragged = false;
}

fn track_drag(drag: On<PointerDrag>, mut gesture: ResMut<Gesture>) {
    if drag.distance.length() > DRAG_THRESHOLD {
        gesture.dragged = true;
    }
}

/// A new board off the lumber rack, as wide and thick as the stock it came from, cut 24" long so
/// it fits the saws.
pub fn from_rack(width: Length, thickness: Length) -> Board {
    Board::new(Length::from_inches(24), width, thickness)
}

/// Sets the wood in hand down at `station`: a board on a saw, ready to cut, with anything
/// already on it going on the pile; anything on the workbench, loose, as near `near` (a point
/// in the shop) as there's room. Returns false, still holding it, if there's nothing in hand or
/// it can't go there: the saws don't cut glue-ups yet.
pub fn set_down(
    station: ShopView,
    near: Option<Vec3>,
    hand: &mut Hand,
    table_saw: &mut TableSawStation,
    miter_saw: &mut MiterSawStation,
    bench: &mut BenchStation,
) -> bool {
    let Some(work) = hand.0.take() else {
        return false;
    };
    match (station, work) {
        (ShopView::TableSaw, Workpiece::Board(board)) => {
            let leftovers = table_saw.load(board);
            miter_saw
                .parts
                .extend(leftovers.into_iter().map(Workpiece::Board));
            true
        }
        (ShopView::MiterSaw, Workpiece::Board(board)) => {
            miter_saw.load(board);
            true
        }
        (ShopView::Bench, work) => {
            bench.set_down(work, near.map(|point| (point - crate::bench::ORIGIN).xz()));
            true
        }
        (_, work) => {
            hand.0 = Some(work);
            false
        }
    }
}

/// What the hand holds, for the readout, or nothing.
pub fn describe(hand: &Hand) -> String {
    match &hand.0 {
        Some(Workpiece::Board(board)) => format!("In hand: {}", describe_size(board)),
        Some(Workpiece::GlueUp(assembly)) => format!(
            "In hand: {}. The saws can't cut glue-ups yet.",
            describe_glue_up(assembly)
        ),
        None => String::new(),
    }
}

/// A glue-up in words: how many parts, and the box it fills, thick by wide by long:
/// "glue-up of 4 parts, 3/4" x 10" x 10"".
pub fn describe_glue_up(assembly: &Assembly) -> String {
    let [x, y, z] = assembly
        .bounds()
        .map(|(least, most)| measured(most - least));
    format!(
        "glue-up of {} parts, {z} x {} x {}",
        assembly.members().len(),
        x.min(y),
        x.max(y)
    )
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
/// into it, with the least corner of the box it fills at `near`.
pub fn place_glue_up(
    assembly: &Assembly,
    axes: &dyn Fn([f64; 3]) -> Vec3,
    near: Vec3,
) -> Vec<Placed> {
    let (low, _) = glue_up_bounds(assembly, axes);
    assembly
        .members()
        .iter()
        .enumerate()
        .map(|(index, member)| {
            let [l, w, t] = member.axes().map(axes);
            Placed {
                board: member.board.clone(),
                corner: near - low + axes(meters(member.at)),
                rotation: Quat::from_mat3(&Mat3::from_cols(w, t, l)),
                grain: Vec3::new(0.13, 0.0, 0.41) * index as f32,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use woodsim::{Fastener, Mating, surfaces};

    use super::*;

    /// Two 1x4s, 24" long, edge to edge: a panel 8" wide.
    fn panel() -> Assembly {
        let board = Board::new(
            Length::from_inches(24),
            Length::from_inches(4),
            Length::from_inch_fraction(3, 4),
        );
        let edge = |far: bool| {
            surfaces(&board)
                .into_iter()
                .find(|surface| {
                    matches!(surface, Surface::Side(edge) if edge.out.l == 0 && (edge.out.w > 0) == far)
                })
                .unwrap()
        };
        let mut panel = Assembly::of(board.clone());
        let other = Assembly::of(board.clone());
        // The way around that keeps the second board face up.
        let mating = (0..4)
            .map(|turn| Mating {
                turn,
                slide: [Length::ZERO; 2],
            })
            .find(|&mating| {
                let motion = panel
                    .mating((0, edge(true)), &other, (0, edge(false)), mating)
                    .unwrap();
                motion.rotation.apply([0.0, 0.0, 1.0])[2] > 0.999
            })
            .unwrap();
        assert!(panel.attach(
            (0, edge(true)),
            &other,
            (0, edge(false)),
            mating,
            Fastener::Glue
        ));
        panel
    }

    #[test]
    fn a_glue_up_reads_as_its_parts_and_the_box_it_fills() {
        assert_eq!(
            describe_glue_up(&panel()),
            "glue-up of 2 parts, 3/4\" x 8\" x 24\""
        );
    }

    #[test]
    fn a_glue_up_is_placed_board_by_board_from_its_near_corner() {
        // The assembly's x along z, y along x, and z up, the way a board lies on the pile.
        let axes = |v: [f64; 3]| Vec3::new(v[1] as f32, v[2] as f32, v[0] as f32);
        let placed = place_glue_up(&panel(), &axes, Vec3::ONE);
        assert_eq!(placed.len(), 2);
        let (low, high) = placed
            .iter()
            .map(Placed::bounds)
            .fold((Vec3::MAX, Vec3::MIN), |(low, high), (near, far)| {
                (low.min(near), high.max(far))
            });
        assert!(low.abs_diff_eq(Vec3::ONE, 1e-6), "{low}");
        let size = Vec3::new(0.2032, 0.01905, 0.6096);
        assert!((high - low).abs_diff_eq(size, 1e-6), "{}", high - low);
    }

    #[test]
    fn the_saws_refuse_a_glue_up_but_the_bench_takes_it() {
        let mut hand = Hand(Some(Workpiece::GlueUp(panel())));
        let (mut table_saw, mut miter_saw, mut bench) = (
            TableSawStation::default(),
            MiterSawStation::default(),
            BenchStation::default(),
        );
        for saw in [ShopView::TableSaw, ShopView::MiterSaw] {
            assert!(!set_down(
                saw,
                None,
                &mut hand,
                &mut table_saw,
                &mut miter_saw,
                &mut bench
            ));
            assert!(hand.0.is_some());
        }
        assert!(set_down(
            ShopView::Bench,
            None,
            &mut hand,
            &mut table_saw,
            &mut miter_saw,
            &mut bench
        ));
        assert!(hand.0.is_none());
        assert_eq!(bench.groups.len(), 1);
        assert_eq!(bench.groups[0].assembly.joints().len(), 1);
    }
}

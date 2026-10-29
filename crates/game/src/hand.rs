//! Carrying wood around the shop. Click a piece of wood to pick it up, wherever it is: on a saw,
//! or a new board off the lumber rack; at the workbench, where clicks pick surfaces to put
//! together, Space picks up a part or a glue-up. Then click where it goes to set it down: a saw,
//! the workbench, or the scrap bin to throw it away; or put it away in the inventory.

use bevy::prelude::*;
use woodsim::{Assembly, Board, Length};

use crate::{
    chop::MiterSawStation,
    dress::JointerStation,
    feed::TableSawStation,
    join::BenchStation,
    shop::{ShopView, describe_size, measured},
};

/// Wood to carry or set aside: one board, or boards fastened together.
#[derive(Clone, Debug, PartialEq)]
pub enum Workpiece {
    Board(Board),
    /// Boards glued, nailed, or screwed together, lying flat as they lay on the bench: the
    /// assembly's `z` is up.
    GlueUp(Assembly),
}

impl Workpiece {
    /// Another just like it, made the same way from new lumber of its own, so plans buy it and
    /// cut it again.
    pub fn duplicate(&self) -> Workpiece {
        match self {
            Workpiece::Board(board) => Workpiece::Board(board.duplicate()),
            Workpiece::GlueUp(assembly) => Workpiece::GlueUp(assembly.duplicate()),
        }
    }
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

/// Sets the wood in hand down at `station`: on a saw, ready to cut, with anything already on it
/// set aside for the inventory; on the workbench, loose, as near `near` (a point in the shop) as there's
/// room. Returns false, still holding it, if there's nothing in hand or it can't go there.
pub fn set_down(
    station: ShopView,
    near: Option<Vec3>,
    hand: &mut Hand,
    table_saw: &mut TableSawStation,
    miter_saw: &mut MiterSawStation,
    jointer: &mut JointerStation,
    bench: &mut BenchStation,
) -> bool {
    let Some(work) = hand.0.take() else {
        return false;
    };
    match station {
        ShopView::TableSaw => {
            table_saw.load(work);
            true
        }
        ShopView::MiterSaw => {
            miter_saw.load(work);
            true
        }
        ShopView::Jointer => {
            jointer.load(work);
            true
        }
        ShopView::Bench => {
            bench.set_down(work, near.map(|point| (point - crate::bench::ORIGIN).xz()));
            true
        }
        ShopView::Overview | ShopView::Studio => {
            hand.0 = Some(work);
            false
        }
    }
}

/// What the hand holds, for the readout, or nothing.
pub fn describe(hand: &Hand) -> String {
    match &hand.0 {
        Some(Workpiece::Board(board)) => format!("In hand: {}", describe_size(board)),
        Some(Workpiece::GlueUp(assembly)) => format!("In hand: {}", describe_glue_up(assembly)),
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

#[cfg(test)]
mod tests {
    use crate::shop::Placed;

    use super::*;

    /// Two 1x4s, 24" long, edge to edge: a panel 8" wide.
    fn panel() -> Assembly {
        crate::work::panel(&[Length::from_inches(4); 2], Length::from_inches(24))
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
        // Lying on a saw as it comes, the way a board does.
        let placed = crate::work::placed(
            &Workpiece::GlueUp(panel()),
            woodsim::Orientation::default(),
            Vec3::ONE,
        );
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
    fn a_glue_up_goes_on_either_saw_the_jointer_or_the_bench() {
        let (mut table_saw, mut miter_saw, mut jointer, mut bench) = (
            TableSawStation::default(),
            MiterSawStation::default(),
            JointerStation::default(),
            BenchStation::default(),
        );
        for saw in [ShopView::TableSaw, ShopView::MiterSaw, ShopView::Jointer] {
            let mut hand = Hand(Some(Workpiece::GlueUp(panel())));
            assert!(set_down(
                saw,
                None,
                &mut hand,
                &mut table_saw,
                &mut miter_saw,
                &mut jointer,
                &mut bench
            ));
            assert!(hand.0.is_none());
        }
        let mut hand = Hand(Some(Workpiece::GlueUp(panel())));
        assert!(set_down(
            ShopView::Bench,
            None,
            &mut hand,
            &mut table_saw,
            &mut miter_saw,
            &mut jointer,
            &mut bench
        ));
        assert!(hand.0.is_none());
        assert_eq!(bench.groups.len(), 1);
        assert_eq!(bench.groups[0].assembly.joints().len(), 1);
    }
}

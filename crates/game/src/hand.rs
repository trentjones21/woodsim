//! Carrying wood around the shop. Click a piece of wood to pick it up, wherever it is: on a saw,
//! on the pile, on the workbench, or a new board off the lumber rack. Then click where it goes
//! to set it down: a saw, the workbench, the pile, or the scrap bin to throw it away.

use bevy::prelude::*;
use woodsim::{Board, Length};

use crate::{chop::MiterSawStation, feed::TableSawStation, join::BenchStation, shop::ShopView};

/// The board being carried, if any.
#[derive(Resource, Default)]
pub struct Hand(pub Option<Board>);

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

/// Sets the board in hand down at `station`: on a saw, ready to cut, with anything already on
/// it going on the pile; on the bare workbench, to start putting parts together. Returns false,
/// still holding it, if there's nothing in hand or it can't go there: on a workbench that has
/// parts on it already, it goes against one of them instead.
pub fn set_down(
    station: ShopView,
    hand: &mut Hand,
    table_saw: &mut TableSawStation,
    miter_saw: &mut MiterSawStation,
    bench: &mut BenchStation,
) -> bool {
    let Some(board) = hand.0.take() else {
        return false;
    };
    match station {
        ShopView::TableSaw => {
            let leftovers = table_saw.load(board);
            miter_saw.parts.extend(leftovers);
            true
        }
        ShopView::MiterSaw => {
            miter_saw.load(board);
            true
        }
        ShopView::Bench => match bench.put(board, None) {
            Ok(()) => true,
            Err(board) => {
                hand.0 = Some(board);
                false
            }
        },
        ShopView::Overview => {
            hand.0 = Some(board);
            false
        }
    }
}

/// What the hand holds, for the readout, or nothing.
pub fn describe(hand: &Hand) -> String {
    match &hand.0 {
        Some(board) => format!("In hand: {}", crate::shop::describe_size(board)),
        None => String::new(),
    }
}

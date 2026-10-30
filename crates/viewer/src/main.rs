//! The marketplace's viewer: one cut from a plan, made again on its saw the way the game makes
//! it, for a web page. It's the game's own shop and table saw (`game`), without the HUD, the
//! inventory, or anything that saves files: the cut's recipe, as Copy (Y) writes one at a saw,
//! sets the saw up (`TableSawStation::set_up`), and the viewer pushes the board through as
//! holding Space would, so the blade spins up, cuts, and throws its sawdust as it does in the
//! game, then sets it up again and goes around. Dragging turns the view around the saw, and
//! scrolling zooms, as at the saw in the game.
//!
//! On the web it draws into the page's `#viewer` canvas, with WebGL2.

use bevy::prelude::*;
use game::{
    bench::BenchView,
    blades::BladesPlugin,
    chop::MiterSawStation,
    controls::{Action, Actions},
    dress::JointerStation,
    feed::TableSawStation,
    hand::{Gesture, Hand},
    hud::OnHud,
    join::BenchStation,
    scenery::ScenePlugin,
    shoot::Studio,
    shop::{ShopPlugin, ShopView},
    table_saw::{FEED_SPEED, TableSawPlugin},
    thickness::PlanerStation,
    turning::TurningPlugin,
};
use woodsim::{Length, Recipe};

/// The cut it shows, for now: a 1x8 ripped with the blade tilted 45°.
const CUT: &str = include_str!("../cuts/bevel-rip.txt");

/// How long the board waits in front of the blade before each pass, and how long the cut
/// stays to look at after it, in seconds.
const WAIT: f32 = 1.0;
const LOOK: f32 = 2.5;

fn main() {
    let recipe = Recipe::read(CUT).expect("the cut's recipe reads");
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Woodshop viewer".to_string(),
                canvas: Some("#viewer".to_string()),
                fit_canvas_to_parent: true,
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ShopView::TableSaw)
        // No mesh picking: nothing in the shop can be clicked or dragged, so a drag always
        // turns the view.
        .add_plugins((
            ShopPlugin,
            ScenePlugin,
            TableSawPlugin,
            BladesPlugin,
            TurningPlugin,
        ))
        // What the shop reads of the game it's left out: nothing in hand, no HUD, and the
        // other stations and the studio as they start.
        .init_resource::<Actions>()
        .init_resource::<Hand>()
        .init_resource::<Gesture>()
        .init_resource::<OnHud>()
        .init_resource::<Studio>()
        .init_resource::<BenchView>()
        .init_resource::<BenchStation>()
        .init_resource::<MiterSawStation>()
        .init_resource::<JointerStation>()
        .init_resource::<PlanerStation>()
        .insert_resource(Replay {
            recipe,
            to: Length::ZERO,
            phase: Phase::Setting,
            since: 0.0,
        })
        // Before the stations act on what's asked this frame.
        .add_systems(PreUpdate, replay)
        .run();
}

/// The cut being made again, over and over.
#[derive(Resource)]
struct Replay {
    recipe: Recipe,
    /// How far along the board the blade's center goes, as the pass went.
    to: Length,
    phase: Phase,
    /// Seconds since the phase began.
    since: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Putting the wood back on the saw as it was.
    Setting,
    /// Waiting in front of the blade.
    Waiting,
    /// Pushing the board through.
    Pushing,
    /// Done, to look at.
    Looking,
}

/// Pushes the board through the way holding Space does, stopping where the pass stopped, and
/// goes around again.
fn replay(
    time: Res<Time>,
    mut replay: ResMut<Replay>,
    mut station: ResMut<TableSawStation>,
    mut actions: ResMut<Actions>,
) {
    *actions = Actions::default();
    replay.since += time.delta_secs();
    let next = match replay.phase {
        Phase::Setting => {
            let recipe = &replay.recipe;
            replay.to = station.set_up(recipe).expect("the cut is a table saw pass");
            Some(Phase::Waiting)
        }
        Phase::Waiting => (replay.since > WAIT).then_some(Phase::Pushing),
        Phase::Pushing => {
            // The last step lands exactly where the pass stopped, rather than past it.
            let step = Length::from_nm((FEED_SPEED * time.delta_secs() / 1e-9) as i64);
            if station.advance() + step < replay.to {
                actions.hold(Action::Push);
                None
            } else {
                station.move_to(replay.to);
                Some(Phase::Looking)
            }
        }
        Phase::Looking => (replay.since > LOOK).then_some(Phase::Setting),
    };
    if let Some(phase) = next {
        replay.phase = phase;
        replay.since = 0.0;
    }
}

//! Wood shown moving, so it's clear where it went. A picture of a piece flies from where it was
//! to where it's going: off a saw or the bench into the inventory, out of the inventory onto a
//! station or into your hand, or away to nothing when it's thrown out. Where it lands in the
//! inventory, the line it joined flashes, or the inventory's button bounces if the line can't be
//! seen. What's in hand follows the pointer, so you can see you're holding it.

use bevy::{
    ecs::system::{EntityCommands, SystemParam},
    prelude::*,
    window::PrimaryWindow,
};

use crate::{
    chop::{MiterSawStation, board_turn},
    feed::TableSawStation,
    hand::{Hand, Workpiece},
    icons::picture,
    inventory::{InventorySystems, Moved, Spot},
    join::BenchStation,
    shop::{Placed, SawBoards, ShopCamera, ShopView},
};

/// How long a picture takes to fly, and to settle into the hand.
const FLIGHT_SECONDS: f32 = 0.55;
const POP_SECONDS: f32 = 0.22;
/// How long a line that wood landed in stays lit, and the inventory's button bounces.
const FLASH_SECONDS: f32 = 0.9;
const PULSE_SECONDS: f32 = 0.35;
/// How big a picture is, in logical pixels, flying and in hand.
const PICTURE: f32 = 64.0;
/// How far below and right of the pointer what's in hand hangs.
const HELD_OFFSET: Vec2 = Vec2::new(16.0, 18.0);

const PAPER: Color = Color::srgba(1.0, 0.98, 0.94, 0.96);
const EDGE: Color = Color::srgba(0.17, 0.2, 0.27, 0.2);
const SHADOW: Color = Color::srgba(0.1, 0.12, 0.18, 0.28);
const ACCENT: Color = Color::srgb(0.93, 0.47, 0.15);

pub struct FlightsPlugin;

impl Plugin for FlightsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LastPress>()
            .add_systems(Startup, spawn_held)
            .add_systems(
                Update,
                (launch, fly, show_held, pulse, flash)
                    .chain()
                    .after(InventorySystems),
            );
    }
}

/// Where the last button clicked is, in logical pixels, for wood to fly from.
#[derive(Resource, Default)]
pub struct LastPress(pub Vec2);

/// A line of the inventory, and the pieces on it.
#[derive(Component)]
pub struct RowOf(pub Vec<u64>);

/// The button that opens the inventory, where wood flies when its line can't be seen.
#[derive(Component)]
pub struct InventoryButton;

/// The inventory's scrolling list, which a line has to be inside to be seen.
#[derive(Component)]
pub struct ListArea;

/// The picture of what's in hand on the card that says what it is.
#[derive(Component)]
pub struct HandPicture;

/// A picture of wood on its way.
#[derive(Component)]
struct Flight {
    from: Vec2,
    to: Spot,
    started: f32,
}

/// What's in hand, following the pointer: settling in since `since`.
#[derive(Component)]
struct Held {
    since: f32,
}

/// A button bouncing, or a line lit up, since the given time.
#[derive(Component)]
struct Pulse(f32);

#[derive(Component)]
struct Flash(f32);

/// What a picture is drawn in: a white card with a soft shadow.
fn card(picture: Handle<Image>) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: px(PICTURE),
            height: px(PICTURE),
            padding: UiRect::all(px(4)),
            border: UiRect::all(px(1.5)),
            border_radius: BorderRadius::all(px(12)),
            ..default()
        },
        BackgroundColor(PAPER),
        BorderColor::all(EDGE),
        BoxShadow::new(SHADOW, px(0), px(4), px(0), px(10)),
        GlobalZIndex(30),
        Pickable::IGNORE,
        children![(
            ImageNode::new(picture),
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
            Pickable::IGNORE,
        )],
    )
}

fn spawn_held(mut commands: Commands) {
    commands.spawn((
        card(Handle::default()),
        Visibility::Hidden,
        Held { since: 0.0 },
    ));
}

/// Where things are on the screen, in logical pixels.
#[derive(SystemParam)]
struct Screen<'w, 's> {
    camera: Single<'w, 's, (&'static Camera, &'static GlobalTransform), With<ShopCamera>>,
    window: Option<Single<'w, 's, &'static Window, With<PrimaryWindow>>>,
    last_press: Res<'w, LastPress>,
    table_saw: Res<'w, TableSawStation>,
    miter_saw: Res<'w, MiterSawStation>,
    bench: Res<'w, BenchStation>,
    rows: Query<
        'w,
        's,
        (
            &'static RowOf,
            &'static UiGlobalTransform,
            &'static ComputedNode,
        ),
    >,
    list: Query<'w, 's, (&'static UiGlobalTransform, &'static ComputedNode), With<ListArea>>,
    button:
        Query<'w, 's, (&'static UiGlobalTransform, &'static ComputedNode), With<InventoryButton>>,
}

impl Screen<'_, '_> {
    fn pointer(&self) -> Option<Vec2> {
        self.window.as_ref()?.cursor_position()
    }

    /// Where `spot` is now, if it's anywhere.
    fn at(&self, spot: Spot) -> Option<Vec2> {
        match spot {
            Spot::Station(view) => {
                let (camera, transform) = *self.camera;
                camera
                    .world_to_viewport(transform, self.wood_at(view)?)
                    .ok()
            }
            Spot::Item(id) => Some(self.line(id).unwrap_or(self.inventory_button()?)),
            Spot::Pressed => Some(self.last_press.0),
            Spot::Hand => self
                .pointer()
                .map(|pointer| pointer + HELD_OFFSET + Vec2::splat(PICTURE / 2.0)),
            Spot::Gone => None,
        }
    }

    /// The middle of the wood at a station: what's on a saw, or the part last set down on the
    /// bench.
    fn wood_at(&self, view: ShopView) -> Option<Vec3> {
        let middle = |boards: SawBoards| {
            let placed: Vec<&Placed> = boards
                .whole
                .iter()
                .chain(&boards.piece)
                .chain(&boards.offcut)
                .collect();
            let (low, high) = placed
                .iter()
                .map(|placed| placed.bounds())
                .fold((Vec3::MAX, Vec3::MIN), |(low, high), (near, far)| {
                    (low.min(near), high.max(far))
                });
            (!placed.is_empty()).then(|| (low + high) / 2.0)
        };
        Some(match view {
            ShopView::TableSaw => {
                let middle = middle(self.table_saw.boards()).unwrap_or(Vec3::new(0.08, 0.02, 0.45));
                crate::table_saw::ORIGIN + middle
            }
            ShopView::MiterSaw => {
                let middle = middle(self.miter_saw.boards()).unwrap_or(Vec3::new(-0.08, 0.02, 0.0));
                crate::miter_saw::ORIGIN + board_turn() * middle
            }
            ShopView::Bench => {
                let middle = self
                    .bench
                    .groups
                    .last()
                    .map_or(Vec2::ZERO, |group| group.footprint().center());
                crate::bench::ORIGIN + Vec3::new(middle.x, 0.03, middle.y)
            }
            ShopView::Overview | ShopView::Studio => return None,
        })
    }

    /// The middle of the inventory line `id` is on, if it can be seen in the list.
    fn line(&self, id: u64) -> Option<Vec2> {
        let (_, at, size) = self.rows.iter().find(|(row, ..)| row.0.contains(&id))?;
        let middle = at.affine().translation;
        let (list_at, list_size) = self.list.iter().next()?;
        let (list_middle, half) = (list_at.affine().translation, list_size.size / 2.0);
        let seen = size.size.y > 0.0
            && middle.y > list_middle.y - half.y
            && middle.y < list_middle.y + half.y;
        seen.then(|| {
            // Toward the line's left end, where its name is.
            let left = middle.x - size.size.x / 2.0 + size.size.x * 0.25;
            Vec2::new(left, middle.y) * size.inverse_scale_factor
        })
    }

    fn inventory_button(&self) -> Option<Vec2> {
        let (at, size) = self.button.iter().next()?;
        Some(at.affine().translation * size.inverse_scale_factor)
    }
}

/// Sends a picture flying for each piece of wood that moved into or out of the inventory.
fn launch(
    mut commands: Commands,
    mut moved: MessageReader<Moved>,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time>,
    screen: Screen,
) {
    for moved in moved.read() {
        let Some(from) = screen.at(moved.from) else {
            continue;
        };
        let mut flight = commands.spawn((
            card(images.add(picture(&moved.work))),
            Flight {
                from,
                to: moved.to,
                started: time.elapsed_secs(),
            },
        ));
        place(&mut flight, from, 0.0);
    }
}

/// Puts a picture's middle at `at`, scaled by `scale` (0 for not yet, so it doesn't flash up
/// full size for a frame).
fn place(entity: &mut EntityCommands, at: Vec2, scale: f32) {
    entity.insert((
        UiTransform::from_scale(Vec2::splat(scale)),
        Node {
            left: px(at.x - PICTURE / 2.0),
            top: px(at.y - PICTURE / 2.0),
            ..card_node()
        },
    ));
}

fn card_node() -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: px(PICTURE),
        height: px(PICTURE),
        padding: UiRect::all(px(4)),
        border: UiRect::all(px(1.5)),
        border_radius: BorderRadius::all(px(12)),
        ..default()
    }
}

/// Moves each picture along an arc to where it's going, shrinking as it goes into the inventory
/// and growing as it comes out; then lights up where it landed.
fn fly(
    mut commands: Commands,
    mut flights: Query<(Entity, &Flight, &mut Node, &mut UiTransform)>,
    time: Res<Time>,
    screen: Screen,
    buttons: Query<Entity, With<InventoryButton>>,
    rows: Query<(Entity, &RowOf)>,
) {
    let now = time.elapsed_secs();
    for (entity, flight, mut node, mut transform) in &mut flights {
        let t = ((now - flight.started) / FLIGHT_SECONDS).clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        let to = screen.at(flight.to).unwrap_or(flight.from);
        // Up and over, higher the further it goes.
        let lift = Vec2::new(0.0, -(to - flight.from).length() * 0.3 - 30.0);
        let middle = (flight.from + to) / 2.0 + lift;
        let at = flight
            .from
            .lerp(middle, eased)
            .lerp(middle.lerp(to, eased), eased);
        let (start, end) = match flight.to {
            Spot::Item(_) => (1.0, 0.55),
            Spot::Gone => (1.0, 0.0),
            _ => (0.6, 1.0),
        };
        let scale = start + (end - start) * eased;
        node.left = px(at.x - PICTURE / 2.0);
        node.top = px(at.y - PICTURE / 2.0);
        *transform = UiTransform::from_scale(Vec2::splat(scale));
        if t < 1.0 {
            continue;
        }
        commands.entity(entity).despawn();
        // The inventory may be rebuilt this same frame, taking the line or button with it
        // before it lights up: then there's nothing to light.
        if let Spot::Item(id) = flight.to {
            match screen.line(id) {
                Some(_) => {
                    for (row, of) in &rows {
                        if of.0.contains(&id) {
                            commands.entity(row).try_insert(Flash(now));
                        }
                    }
                }
                None => {
                    for button in &buttons {
                        commands.entity(button).try_insert(Pulse(now));
                    }
                }
            }
        }
    }
}

/// Keeps a picture of what's in hand under the pointer, popping in when it's picked up.
fn show_held(
    hand: Res<Hand>,
    time: Res<Time>,
    screen: Screen,
    mut images: ResMut<Assets<Image>>,
    held: Single<(
        &mut Held,
        &mut Node,
        &mut UiTransform,
        &mut Visibility,
        &Children,
    )>,
    mut pictures: Query<(&mut ImageNode, Has<HandPicture>)>,
    mut was: Local<Option<Workpiece>>,
) {
    let (mut held, mut node, mut transform, mut visibility, children) = held.into_inner();
    let now = time.elapsed_secs();
    if hand.is_changed() && *was != hand.0 {
        if let Some(work) = &hand.0 {
            let image = images.add(picture(work));
            // The one under the pointer, and the one on the card that says what it is.
            let under_pointer = children.first().copied();
            for (mut picture, on_card) in &mut pictures {
                if on_card {
                    picture.image = image.clone();
                }
            }
            if let Some(child) = under_pointer
                && let Ok((mut picture, _)) = pictures.get_mut(child)
            {
                picture.image = image.clone();
            }
            // Fresh in hand, it pops in; swapped for another, it just changes.
            if was.is_none() {
                held.since = now;
            }
        }
        *was = hand.0.clone();
    }
    let pointer = screen.pointer();
    let shown = hand.0.is_some() && pointer.is_some();
    let want = if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if *visibility != want {
        *visibility = want;
    }
    let Some(pointer) = pointer.filter(|_| shown) else {
        return;
    };
    node.left = px(pointer.x + HELD_OFFSET.x);
    node.top = px(pointer.y + HELD_OFFSET.y);
    // Overshoots a little, then settles.
    let t = ((now - held.since) / POP_SECONDS).clamp(0.0, 1.0);
    let scale = 1.0 + 0.25 * (std::f32::consts::PI * t).sin() * (1.0 - t) - 0.4 * (1.0 - t);
    *transform = UiTransform::from_scale(Vec2::splat(scale));
}

/// Bounces a button that wood just flew into.
fn pulse(
    mut commands: Commands,
    time: Res<Time>,
    mut pulsing: Query<(Entity, &Pulse, &mut UiTransform)>,
) {
    for (entity, pulse, mut transform) in &mut pulsing {
        let t = ((time.elapsed_secs() - pulse.0) / PULSE_SECONDS).clamp(0.0, 1.0);
        let scale = 1.0 + 0.2 * (std::f32::consts::PI * t).sin();
        *transform = UiTransform::from_scale(Vec2::splat(scale));
        if t >= 1.0 {
            commands.entity(entity).try_remove::<Pulse>();
        }
    }
}

/// Lights up a line of the inventory that wood just landed in, fading.
fn flash(
    mut commands: Commands,
    time: Res<Time>,
    mut lit: Query<(Entity, &Flash, &mut BackgroundColor)>,
) {
    for (entity, flash, mut background) in &mut lit {
        let t = ((time.elapsed_secs() - flash.0) / FLASH_SECONDS).clamp(0.0, 1.0);
        background.0 = ACCENT.with_alpha(0.3 * (1.0 - t));
        if t >= 1.0 {
            commands.entity(entity).try_remove::<Flash>();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// A line of the inventory, inside the list so it can be seen, for wood with `id`.
    fn line(world: &mut World, id: u64) -> Entity {
        let size = ComputedNode {
            size: Vec2::new(300.0, 30.0),
            inverse_scale_factor: 1.0,
            ..default()
        };
        world
            .spawn((
                RowOf(vec![id]),
                UiGlobalTransform::default(),
                size,
                BackgroundColor::default(),
            ))
            .id()
    }

    /// Wood lands in two lines as the inventory is rebuilt the same frame, one of them gone by
    /// the time it lights up: the line that's still there flashes, and the other is let go.
    #[test]
    fn wood_landing_in_a_line_rebuilt_away_lets_it_go() {
        let mut world = World::new();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs(1));
        world.insert_resource(time);
        world.init_resource::<LastPress>();
        world.init_resource::<TableSawStation>();
        world.init_resource::<MiterSawStation>();
        world.init_resource::<BenchStation>();
        world.spawn((Camera::default(), GlobalTransform::default(), ShopCamera));
        world.spawn((
            ListArea,
            UiGlobalTransform::default(),
            ComputedNode {
                size: Vec2::splat(1000.0),
                inverse_scale_factor: 1.0,
                ..default()
            },
        ));
        let (kept, rebuilt) = (line(&mut world, 1), line(&mut world, 2));
        for id in [1, 2] {
            world.spawn((
                Flight {
                    from: Vec2::ZERO,
                    to: Spot::Item(id),
                    started: 0.0,
                },
                Node::default(),
                UiTransform::default(),
            ));
        }
        let mut schedule = Schedule::default();
        // Rebuilding the inventory and the landing go in the same frame, in no set order.
        schedule.add_systems(
            (
                move |mut commands: Commands| commands.entity(rebuilt).despawn(),
                fly,
            )
                .chain_ignore_deferred(),
        );
        schedule.run(&mut world);
        assert!(world.get::<Flash>(kept).is_some());
        assert!(world.get_entity(rebuilt).is_err());
        assert_eq!(world.query::<&Flight>().iter(&world).count(), 0);
    }
}

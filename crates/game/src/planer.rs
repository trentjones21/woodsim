//! The planer station: its bed, the head over it on four posts, the cutterhead and rollers in
//! the head, the board, and the controls. Everything's built to the planer's size (`Body`).

use std::f32::consts::FRAC_PI_2;

use bevy::{picking::pointer::PointerButton, prelude::*};
use woodsim::{Length, Planer};

use crate::{
    blades::{BladeSystems, Motor, Sawing, spawn_cutterhead},
    controls::{Action, Actions, Step},
    hand::{Grabbable, Hand},
    meshes::{lumber_mesh, rounded_box},
    rest,
    shop::{
        Block, Part, SawPart, Shape, ShopAssets, ShopView, Slot, Stations, inches, place_parts,
        pointer_on_plane, rounded_block, set_material_on, snap, station_part,
    },
    thickness::{Body, PlanerStation},
};

/// Where the station's coordinates start in the shop: the middle of the bed, at its top, under
/// the cutterhead's center.
pub const ORIGIN: Vec3 = Vec3::new(-1.55, inches(30.0), 1.75);

/// The space `planer` and its stand take up, for outlining it in the overview: as big as it is,
/// up to the motor on its head, wherever the head's set.
pub fn bounds(planer: &Planer) -> Block {
    let body = Body::of(planer);
    let across = body.head_across.max(body.posts.x + POST_RADIUS);
    let along = body.bed_along + body.wings;
    let motor =
        planer.thickness.as_meters_f32() + body.head_clearance + body.head_height + MOTOR_HEIGHT;
    Block {
        min: Vec3::new(-across, -ORIGIN.y, -along),
        max: Vec3::new(across, motor.max(body.post_height), along),
    }
}

const POST_RADIUS: f32 = inches(0.6);
/// How far the motor stands up off the top of the head.
const MOTOR_HEIGHT: f32 = inches(3.0);

const THICKNESS_STEP: Length = Length::from_inch_fraction(1, 64);
const THICKNESS_BIG_STEP: Length = Length::from_inch_fraction(1, 16);
/// How fast the board goes in while Space is held, and how fast the rollers carry it through:
/// about 26 feet a minute.
const FEED_SPEED: f32 = inches(5.0);
/// Seconds the cutterhead takes to come up to speed and to coast to a stop, and how long it
/// runs on after the board's through, as if switched off then.
const SPIN_UP: f32 = 0.8;
const COAST: f32 = 3.0;
const RUN_ON: f32 = 1.5;

pub struct PlanerPlugin;

impl Plugin for PlanerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlanerStation>()
            .init_resource::<BoardGrab>()
            .add_systems(Startup, spawn_planer)
            .add_systems(
                Update,
                (
                    (set_thickness, turn_board, push_board, bring_back)
                        .run_if(resource_equals(ShopView::Planer)),
                    feed_through,
                    run_planer.before(BladeSystems),
                    (draw_head, draw_board),
                )
                    .chain(),
            );
    }
}

#[derive(Component)]
pub(crate) struct AtPlaner;

/// The head, with the cutterhead and rollers in it, which rises and falls with the setting.
#[derive(Component)]
struct Head;

/// The cutterhead, in the head.
#[derive(Component)]
struct Cutterhead;

/// Where the pointer grabbed the board to drag it, along the bed, and how far along the
/// cutterhead the board was then.
#[derive(Resource, Default)]
struct BoardGrab {
    pointer: f32,
    advance: Length,
}

fn spawn_planer(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    station: Res<PlanerStation>,
) {
    let mut paint = |color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.55,
            ..default()
        })
    };
    let bed_top = paint(Color::srgb(0.76, 0.79, 0.83));
    let body_color = paint(Color::srgb(0.95, 0.75, 0.2));
    let dark_metal = paint(Color::srgb(0.22, 0.24, 0.3));
    let safety_red = paint(Color::srgb(0.92, 0.25, 0.22));
    let steel = paint(Color::srgb(0.62, 0.66, 0.72));
    let body = Body::of(&station.machine);

    let spawn_box = |commands: &mut Commands,
                     meshes: &mut Assets<Mesh>,
                     material: &Handle<StandardMaterial>,
                     min: Vec3,
                     max: Vec3| {
        let entity = commands.spawn((
            rounded_block(meshes, Block { min, max }, ORIGIN),
            MeshMaterial3d(material.clone()),
        ));
        station_part(entity, ShopView::Planer);
    };
    // The bed through the machine, and the wings that hold the wood level going in and out.
    let along = body.bed_along + body.wings;
    spawn_box(
        &mut commands,
        &mut meshes,
        &bed_top,
        Vec3::new(-body.bed_across, inches(-1.5), -body.bed_along),
        Vec3::new(body.bed_across, 0.0, body.bed_along),
    );
    for side in [-1.0, 1.0] {
        let (near, far) = (side * body.bed_along, side * along);
        spawn_box(
            &mut commands,
            &mut meshes,
            &bed_top,
            Vec3::new(-body.bed_across, inches(-0.6), near.min(far)),
            Vec3::new(body.bed_across, 0.0, near.max(far)),
        );
    }
    // The base under the bed, on a stand.
    spawn_box(
        &mut commands,
        &mut meshes,
        &body_color,
        Vec3::new(-body.head_across, inches(-6.0), -body.bed_along * 0.8),
        Vec3::new(body.head_across, inches(-1.5), body.bed_along * 0.8),
    );
    spawn_box(
        &mut commands,
        &mut meshes,
        &dark_metal,
        Vec3::new(
            -body.head_across * 0.8,
            inches(3.0) - ORIGIN.y,
            -body.bed_along * 0.7,
        ),
        Vec3::new(body.head_across * 0.8, inches(-6.0), body.bed_along * 0.7),
    );
    spawn_box(
        &mut commands,
        &mut meshes,
        &dark_metal,
        Vec3::new(-body.head_across, -ORIGIN.y, -body.bed_along * 0.8),
        Vec3::new(
            body.head_across,
            inches(3.0) - ORIGIN.y,
            body.bed_along * 0.8,
        ),
    );
    // The switch, on the front of the base.
    spawn_box(
        &mut commands,
        &mut meshes,
        &safety_red,
        Vec3::new(-body.head_across - inches(0.6), inches(-5.0), inches(2.0)),
        Vec3::new(-body.head_across, inches(-2.5), inches(4.5)),
    );
    // The four posts the head rides up and down.
    for (x, z) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
        let at = Vec3::new(x * body.posts.x, 0.0, z * body.posts.y);
        commands.spawn((
            Mesh3d(meshes.add(Cylinder::new(POST_RADIUS, body.post_height))),
            MeshMaterial3d(steel.clone()),
            Transform::from_translation(ORIGIN + at + Vec3::Y * body.post_height / 2.0),
            Pickable::IGNORE,
        ));
    }

    // The head, in its own frame: y = 0 at the bottom of the knives' circle, where the setting
    // is, placed by `draw_head`.
    let head = commands
        .spawn((Transform::default(), Visibility::default(), Head))
        .id();
    let housing = Block {
        min: Vec3::new(-body.head_across, body.head_clearance, -body.head_along),
        max: Vec3::new(
            body.head_across,
            body.head_clearance + body.head_height,
            body.head_along,
        ),
    };
    let top = housing.max.y;
    for (material, block) in [
        (&body_color, housing),
        // The motor on top, the dust port at the back, and the crank that raises the head.
        (
            &dark_metal,
            Block {
                min: Vec3::new(-body.head_across * 0.6, top, -body.head_along * 0.5),
                max: Vec3::new(
                    body.head_across * 0.2,
                    top + MOTOR_HEIGHT,
                    body.head_along * 0.4,
                ),
            },
        ),
        (
            &dark_metal,
            Block {
                min: Vec3::new(body.head_across * 0.25, top, -body.head_along - inches(1.5)),
                max: Vec3::new(
                    body.head_across * 0.65,
                    top + inches(2.0),
                    -body.head_along * 0.2,
                ),
            },
        ),
        (
            &safety_red,
            Block {
                min: Vec3::new(body.head_across - inches(1.0), top, inches(-0.5)),
                max: Vec3::new(body.head_across, top + inches(2.5), inches(0.5)),
            },
        ),
    ] {
        let entity = commands.spawn((
            Mesh3d(meshes.add(rounded_box(block.max - block.min))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(block.min),
            ChildOf(head),
        ));
        station_part(entity, ShopView::Planer);
    }
    // The rollers either side of the cutterhead, pressing the wood to the bed.
    let roller = Mesh::from(Cylinder::new(
        body.roller_radius,
        2.0 * body.bed_across - inches(0.5),
    ))
    .rotated_by(Quat::from_rotation_z(FRAC_PI_2));
    let roller = meshes.add(roller);
    for side in [-1.0, 1.0] {
        commands.spawn((
            Mesh3d(roller.clone()),
            MeshMaterial3d(dark_metal.clone()),
            Transform::from_xyz(
                0.0,
                body.roller_radius + inches(1.0 / 32.0),
                side * body.rollers_along,
            ),
            Pickable::IGNORE,
            ChildOf(head),
        ));
    }
    // The cutterhead turns about x, its top toward you, as the jointer's does.
    let planer = station.machine;
    let cutterhead = spawn_cutterhead(
        &mut commands,
        &mut meshes,
        &mut materials,
        (planer.cutterhead, planer.knives),
        Motor::new(SPIN_UP, COAST, RUN_ON),
    );
    commands.entity(cutterhead).insert((
        Cutterhead,
        Transform::from_xyz(0.0, planer.radius().as_meters_f32(), 0.0),
        ChildOf(head),
    ));
}

/// Raises or lowers the head a 64th at a time (big, a 16th).
fn set_thickness(actions: Res<Actions>, mut station: ResMut<PlanerStation>) {
    for action in actions.started() {
        let Action::Blade(Step { more: thicker, big }) = action else {
            continue;
        };
        let step = if big {
            THICKNESS_BIG_STEP
        } else {
            THICKNESS_STEP
        };
        let thickness = station.machine.thickness;
        station.set_thickness(if thicker {
            thickness + step
        } else {
            thickness - step
        });
    }
}

fn push_board(actions: Res<Actions>, time: Res<Time>, mut station: ResMut<PlanerStation>) {
    if actions.held(Action::Push) && !station.feeding() {
        station.push(FEED_SPEED * time.delta_secs());
    }
}

/// Once the rollers have the board, they carry it through, wherever you are.
fn feed_through(time: Res<Time>, mut station: ResMut<PlanerStation>) {
    if station.feeding() {
        station.push(FEED_SPEED * time.delta_secs());
    }
}

fn bring_back(actions: Res<Actions>, mut station: ResMut<PlanerStation>) {
    if actions.just(Action::BringBack) {
        station.bring_back();
    }
}

/// Runs the cutterhead while a board's going through, or being pushed in, and throws chips out
/// the dust port wherever the knives took wood this frame.
fn run_planer(
    station: Res<PlanerStation>,
    actions: Res<Actions>,
    view: Res<ShopView>,
    mut motor: Single<&mut Motor, With<Cutterhead>>,
    mut sawing: MessageWriter<Sawing>,
    mut last: Local<u32>,
) {
    let pushing = *view == ShopView::Planer && actions.held(Action::Push);
    if station.at_head() || pushing && station.can_push() {
        motor.run();
    }
    if station.bites() != *last
        && let Some((at, toward)) = station.chips()
    {
        sawing.write(Sawing {
            at: ORIGIN + at,
            toward,
            amount: 1.0,
        });
    }
    *last = station.bites();
}

/// Remembers where the pointer grabbed the board, so it doesn't jump to the cursor.
fn grab_board(
    drag: On<PointerDragStart>,
    camera: Single<(&Camera, &GlobalTransform)>,
    station: Res<PlanerStation>,
    mut grab: ResMut<BoardGrab>,
) {
    let (camera, camera_transform) = *camera;
    if drag.button == PointerButton::Primary
        && let Some(point) =
            pointer_on_plane(camera, camera_transform, drag.pointer.position, ORIGIN.y)
    {
        *grab = BoardGrab {
            pointer: point.z,
            advance: station.advance(),
        };
    }
}

/// Slides the board along the bed after the pointer, toward the cutterhead to feed it in, or
/// back, until the rollers have it.
fn drag_board(
    drag: On<PointerDrag>,
    camera: Single<(&Camera, &GlobalTransform)>,
    grab: Res<BoardGrab>,
    mut station: ResMut<PlanerStation>,
) {
    let (camera, camera_transform) = *camera;
    if drag.button == PointerButton::Primary
        && !station.feeding()
        && let Some(point) =
            pointer_on_plane(camera, camera_transform, drag.pointer.position, ORIGIN.y)
    {
        // The board feeds toward -z, so moving the pointer that way pushes it.
        station.move_to(grab.advance - snap(point.z - grab.pointer, 64));
    }
}

/// Turns the board a quarter turn left or right, flips it over, stands it on an edge, or
/// chooses what it rests on, while it waits in front of the head.
fn turn_board(actions: Res<Actions>, mut station: ResMut<PlanerStation>) {
    for action in actions.started() {
        match action {
            Action::TurnLeft => station.turn_left(),
            Action::TurnRight => station.turn_right(),
            Action::Flip => station.flip(),
            Action::StandUp => station.tip(),
            Action::Rest => station.choose_rest(),
            _ => continue,
        };
    }
}

/// With empty hands, a click on a board clear of the head picks it up; holding one, a click on
/// the planer's board sets the one in hand down here instead.
fn board_clicked(
    click: On<PointerClick>,
    parts: Query<(&Part, &Slot, &Shape, &GlobalTransform)>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut stations: Stations,
) {
    if click.button != PointerButton::Primary || stations.gesture.dragged() {
        return;
    }
    let Ok((part, slot, shape, transform)) = parts.get(click.entity) else {
        return;
    };
    // Choosing what it rests on, a click picks a surface rather than picking it up.
    if stations.planer.choosing_rest() {
        if *part == Part::Whole
            && let Some(surface) = rest::surface_hit(shape, transform, &click.hit)
        {
            stations.planer.pick_rest(slot.0, surface);
        }
        return;
    }
    let handled = if stations.hand.0.is_some() {
        stations.set_down(ShopView::Planer, None)
    } else {
        stations.hand.0 = stations.planer.take(*part);
        stations.hand.0.is_some()
    };
    if handled && let Ok(mut material) = materials.get_mut(click.entity) {
        material.0 = shop.wood.clone();
    }
}

/// Lights up a board under the pointer when a click would pick it up, or set the one in hand
/// down here.
fn highlight_carryable(
    over: On<PointerOver>,
    station: Res<PlanerStation>,
    hand: Res<Hand>,
    shop: Res<ShopAssets>,
    parts: Query<&Part>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    let clickable = |part: Part| match hand.0 {
        Some(_) => station.can_load(),
        None => !station.choosing_rest() && station.can_take(part),
    };
    if let Ok(part) = parts.get(over.entity)
        && clickable(*part)
        && let Ok(mut material) = materials.get_mut(over.entity)
    {
        material.0 = shop.wood_highlight.clone();
    }
}

/// Raises the head to the setting: the bottom of the knives' circle that far above the bed.
fn draw_head(station: Res<PlanerStation>, mut head: Single<&mut Transform, With<Head>>) {
    if station.is_changed() {
        let thickness = station.machine.thickness.as_meters_f32();
        head.translation = ORIGIN + Vec3::Y * thickness;
    }
}

/// Draws what's on the planer, each board with its own mesh, rebuilt as the knives take its top
/// off: a board, or every board of a glue-up.
fn draw_board(
    mut commands: Commands,
    station: Res<PlanerStation>,
    shop: Res<ShopAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut parts: Query<SawPart, With<AtPlaner>>,
    mut turns: Local<u32>,
) {
    if !station.is_changed() {
        return;
    }
    let boards = station.boards();
    let turned = station.turns() != *turns;
    *turns = station.turns();
    let missing = place_parts(
        &boards,
        ORIGIN,
        Quat::IDENTITY,
        turned,
        &mut meshes,
        &mut parts,
    );
    for (part, slot, pose, placed) in missing {
        commands
            .spawn((
                Mesh3d(meshes.add(lumber_mesh(&placed.board, placed.grain))),
                MeshMaterial3d(shop.wood.clone()),
                pose,
                part,
                slot,
                AtPlaner,
                Shape::built(&placed),
                Grabbable,
            ))
            .observe(highlight_carryable)
            .observe(set_material_on::<PointerOut>(shop.wood.clone()))
            .observe(rest::point_at)
            .observe(rest::point_away)
            .observe(board_clicked)
            .observe(grab_board)
            .observe(drag_board);
    }
}

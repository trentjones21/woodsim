//! The jointer station: its tables, fence, and cutterhead, the board, and the controls.

use bevy::{picking::pointer::PointerButton, prelude::*};
use woodsim::{Angle, Length};

use crate::{
    blades::{BladeSystems, Motor, Sawing, spawn_cutterhead},
    controls::{Action, Actions, Step},
    dress::JointerStation,
    hand::Hand,
    meshes::{Grain, board_mesh, lumber_mesh, rounded_box},
    rest,
    shop::{
        Block, Part, SawPart, Shape, ShopAssets, ShopView, Slot, Stations, inches, place_parts,
        pointer_on_plane, rounded_block, set_material_on, snap, snap_to_sixteenth, station_part,
    },
};

/// Where the station's coordinates start in the shop: the end of the knives, at the outfeed
/// table's top, over the cutterhead's center.
pub const ORIGIN: Vec3 = Vec3::new(1.9, inches(32.0), -0.95);

/// The space the jointer and its stand take up, for outlining it in the overview.
pub const BOUNDS: Block = Block {
    min: Vec3::new(TABLE_FRONT, -ORIGIN.y, -TABLE_END),
    max: Vec3::new(TABLE_BACK, FENCE_HEIGHT, TABLE_END),
};

/// The tables, in table space: out over the rabbeting ledge in front of the knives' end, back
/// past the far end of the knives, and how far each runs from the cutterhead.
const TABLE_FRONT: f32 = inches(-1.75);
const TABLE_BACK: f32 = inches(6.75);
const TABLE_END: f32 = inches(23.0);
const TABLE_THICKNESS: f32 = inches(2.0);
/// Half the gap between the tables, that the cutterhead turns in.
const GAP: f32 = inches(1.5);

const FENCE_THICKNESS: f32 = inches(1.0);
const FENCE_HEIGHT: f32 = inches(4.0);
const FENCE_HALF_LENGTH: f32 = inches(17.5);

const DEPTH_STEP: Length = Length::from_inch_fraction(1, 64);
const DEPTH_BIG_STEP: Length = Length::from_inch_fraction(1, 16);
const FENCE_STEP: Length = Length::from_inch_fraction(1, 16);
const FENCE_BIG_STEP: Length = Length::from_inches(1);
/// How fast the board moves while Space is held: 8" a second, a steady feed.
const FEED_SPEED: f32 = inches(8.0);
/// Seconds the cutterhead takes to come up to speed and to coast to a stop, and how long it
/// runs on after the board's clear, as if switched off then.
const SPIN_UP: f32 = 0.8;
const COAST: f32 = 3.0;
const RUN_ON: f32 = 1.5;

pub struct JointerPlugin;

impl Plugin for JointerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<JointerStation>()
            .init_resource::<BoardGrab>()
            .add_systems(Startup, spawn_jointer)
            .add_systems(
                Update,
                (
                    (
                        set_depth, move_fence, tilt_fence, turn_board, push_board, bring_back,
                    )
                        .run_if(resource_equals(ShopView::Jointer)),
                    run_jointer.before(BladeSystems),
                    (draw_tables, draw_fence, draw_board),
                )
                    .chain(),
            );
    }
}

#[derive(Component)]
pub(crate) struct AtJointer;

/// The infeed table, which drops by the depth of cut.
#[derive(Component)]
struct InfeedTable;

/// The fence, which slides across the tables and tilts.
#[derive(Component)]
struct Fence {
    /// How far right of the fence's face the pointer grabbed it, in meters.
    grab_offset: f32,
}

/// The cutterhead, under the gap between the tables.
#[derive(Component)]
struct Cutterhead;

/// Where the pointer grabbed the board to drag it, along the tables, and how far along the
/// cutterhead the board was then.
#[derive(Resource, Default)]
struct BoardGrab {
    pointer: f32,
    advance: Length,
}

fn spawn_jointer(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    shop: Res<ShopAssets>,
    station: Res<JointerStation>,
) {
    let mut paint = |color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.55,
            ..default()
        })
    };
    let table_top = paint(Color::srgb(0.76, 0.79, 0.83));
    let body = paint(Color::srgb(0.3, 0.62, 0.86));
    let dark_metal = paint(Color::srgb(0.22, 0.24, 0.3));
    let safety_red = paint(Color::srgb(0.92, 0.25, 0.22));
    let tape_yellow = paint(Color::srgb(1.0, 0.88, 0.35));

    let spawn_box = |commands: &mut Commands,
                     meshes: &mut Assets<Mesh>,
                     material: &Handle<StandardMaterial>,
                     min: Vec3,
                     max: Vec3| {
        let entity = commands.spawn((
            rounded_block(meshes, Block { min, max }, ORIGIN),
            MeshMaterial3d(material.clone()),
        ));
        station_part(entity, ShopView::Jointer);
    };
    // The outfeed table, level with the knives' top; the infeed table, with the rabbeting
    // ledge out in front, set lower by `draw_tables`.
    spawn_box(
        &mut commands,
        &mut meshes,
        &table_top,
        Vec3::new(0.0, -TABLE_THICKNESS, -TABLE_END),
        Vec3::new(TABLE_BACK, 0.0, -GAP),
    );
    let infeed = Block {
        min: Vec3::new(TABLE_FRONT, -TABLE_THICKNESS, GAP),
        max: Vec3::new(TABLE_BACK, 0.0, TABLE_END),
    };
    let entity = commands.spawn((
        Mesh3d(meshes.add(rounded_box(infeed.max - infeed.min))),
        MeshMaterial3d(table_top.clone()),
        Transform::from_translation(ORIGIN + infeed.min),
        InfeedTable,
    ));
    station_part(entity, ShopView::Jointer);
    // A scale on the front of the infeed table, read against its depth.
    spawn_box(
        &mut commands,
        &mut meshes,
        &tape_yellow,
        Vec3::new(TABLE_FRONT - inches(0.1), inches(-1.6), inches(3.0)),
        Vec3::new(TABLE_FRONT, inches(-0.4), inches(8.0)),
    );
    // The base the tables ride on, the dust chute out the side, and the stand under it all.
    spawn_box(
        &mut commands,
        &mut meshes,
        &body,
        Vec3::new(inches(-0.5), inches(-7.0), inches(-14.0)),
        Vec3::new(inches(6.5), -TABLE_THICKNESS, inches(14.0)),
    );
    spawn_box(
        &mut commands,
        &mut meshes,
        &dark_metal,
        Vec3::new(inches(6.5), inches(-6.0), inches(-3.0)),
        Vec3::new(inches(10.0), inches(-3.0), inches(3.0)),
    );
    spawn_box(
        &mut commands,
        &mut meshes,
        &body,
        Vec3::new(inches(-2.5), inches(3.0) - ORIGIN.y, inches(-10.0)),
        Vec3::new(inches(8.5), inches(-7.0), inches(10.0)),
    );
    spawn_box(
        &mut commands,
        &mut meshes,
        &dark_metal,
        Vec3::new(inches(-3.5), -ORIGIN.y, inches(-11.0)),
        Vec3::new(inches(9.5), inches(3.0) - ORIGIN.y, inches(11.0)),
    );
    // The switch, on the front of the stand.
    spawn_box(
        &mut commands,
        &mut meshes,
        &safety_red,
        Vec3::new(inches(-3.25), inches(-12.0), inches(4.0)),
        Vec3::new(inches(-2.5), inches(-9.5), inches(7.0)),
    );
    // The handwheel that raises and lowers the infeed table, at its front end.
    commands.spawn((
        Mesh3d(meshes.add(Torus::new(inches(1.6), inches(2.0)))),
        MeshMaterial3d(dark_metal.clone()),
        Transform::from_translation(ORIGIN + Vec3::new(inches(-1.0), inches(-5.0), inches(19.0)))
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
        Pickable::IGNORE,
    ));

    // The cutterhead turns about x, its top toward you, its middle under the knives' middle.
    let radius = station.jointer.radius().as_meters_f32();
    let head = spawn_cutterhead(
        &mut commands,
        &mut meshes,
        &mut materials,
        station.jointer,
        Motor::new(SPIN_UP, COAST, RUN_ON),
    );
    commands.entity(head).insert((
        Cutterhead,
        Transform::from_translation(
            ORIGIN + Vec3::new(station.jointer.knives.as_meters_f32() / 2.0, -radius, 0.0),
        ),
    ));

    // The fence, in its own frame: its face at x = 0 where it meets the tables, standing up
    // from there, turned about that line as it tilts.
    let fence_color = materials.add(StandardMaterial {
        base_color: Color::srgb(0.9, 0.2, 0.22),
        perceptual_roughness: 0.4,
        ..default()
    });
    let fence_hover = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.45, 0.4),
        emissive: LinearRgba::new(1.0, 0.35, 0.2, 1.0) * 40.0,
        perceptual_roughness: 0.4,
        ..default()
    });
    let mut fence_mesh = rounded_box(Vec3::new(
        FENCE_THICKNESS,
        FENCE_HEIGHT,
        2.0 * FENCE_HALF_LENGTH,
    ));
    fence_mesh.translate_by(Vec3::new(0.0, 0.0, -FENCE_HALF_LENGTH));
    commands
        .spawn((
            Mesh3d(meshes.add(fence_mesh)),
            MeshMaterial3d(fence_color.clone()),
            Transform::default(),
            Fence { grab_offset: 0.0 },
        ))
        .observe(set_material_on::<PointerOver>(fence_hover))
        .observe(set_material_on::<PointerOut>(fence_color))
        .observe(grab_fence)
        .observe(drag_fence);
    // The fence's carriage behind it, on the outfeed table's back edge.
    let carriage = Block {
        min: Vec3::new(TABLE_BACK, inches(-4.0), inches(-4.0)),
        max: Vec3::new(TABLE_BACK + inches(1.5), inches(-1.0), inches(4.0)),
    };
    let entity = commands.spawn((
        Mesh3d(meshes.add(board_mesh(
            carriage.max - carriage.min,
            Vec3::ZERO,
            Grain::X,
        ))),
        MeshMaterial3d(shop.maple.clone()),
        Transform::from_translation(ORIGIN + carriage.min),
    ));
    station_part(entity, ShopView::Jointer);
}

/// Lowers the infeed table for a deeper cut, or raises it, a 64th at a time (big, a 16th).
fn set_depth(actions: Res<Actions>, mut station: ResMut<JointerStation>) {
    for action in actions.started() {
        let Action::Depth(Step { more: deeper, big }) = action else {
            continue;
        };
        let step = if big { DEPTH_BIG_STEP } else { DEPTH_STEP };
        let depth = station.jointer.depth;
        station.set_depth(if deeper { depth + step } else { depth - step });
    }
}

/// Slides the fence back, using more of the knives, or in toward their end.
fn move_fence(actions: Res<Actions>, mut station: ResMut<JointerStation>) {
    for action in actions.started() {
        let Action::Guide(Step { more: back, big }) = action else {
            continue;
        };
        let step = if big { FENCE_BIG_STEP } else { FENCE_STEP };
        let fence = station.jointer.fence;
        station.set_fence(if back { fence + step } else { fence - step });
    }
}

/// Tilts the fence 1° further or back, or a big step to the next stop.
fn tilt_fence(actions: Res<Actions>, mut station: ResMut<JointerStation>) {
    for action in actions.started() {
        let Action::Tilt(Step { more, big }) = action else {
            continue;
        };
        if big {
            station.tilt_to_next_stop(more);
        } else {
            let step = Angle::from_degrees(if more { 1 } else { -1 });
            let tilt = station.jointer.tilt + step;
            station.set_tilt(tilt);
        }
    }
}

/// Remembers where on the fence the pointer grabbed it, so the fence doesn't jump to the cursor.
fn grab_fence(
    drag: On<PointerDragStart>,
    camera: Single<(&Camera, &GlobalTransform)>,
    mut fences: Query<&mut Fence>,
    station: Res<JointerStation>,
) {
    let (camera, camera_transform) = *camera;
    if drag.button == PointerButton::Primary
        && let Some(point) = pointer_on_plane(
            camera,
            camera_transform,
            drag.pointer.position,
            ORIGIN.y + FENCE_HEIGHT,
        )
        && let Ok(mut fence) = fences.get_mut(drag.entity)
    {
        fence.grab_offset = point.x - ORIGIN.x - station.jointer.fence.as_meters_f32();
    }
}

fn drag_fence(
    drag: On<PointerDrag>,
    camera: Single<(&Camera, &GlobalTransform)>,
    fences: Query<&Fence>,
    mut station: ResMut<JointerStation>,
) {
    let (camera, camera_transform) = *camera;
    if drag.button == PointerButton::Primary
        && let Some(point) = pointer_on_plane(
            camera,
            camera_transform,
            drag.pointer.position,
            ORIGIN.y + FENCE_HEIGHT,
        )
        && let Ok(fence) = fences.get(drag.entity)
    {
        station.set_fence(snap_to_sixteenth(point.x - ORIGIN.x - fence.grab_offset));
    }
}

fn push_board(actions: Res<Actions>, time: Res<Time>, mut station: ResMut<JointerStation>) {
    if actions.held(Action::Push) {
        station.push(FEED_SPEED * time.delta_secs());
    }
}

fn bring_back(actions: Res<Actions>, mut station: ResMut<JointerStation>) {
    if actions.just(Action::BringBack) {
        station.bring_back();
    }
}

/// Runs the cutterhead while a board's over it, or being pushed toward it, and throws chips out
/// of the gap wherever the knives took wood this frame, more the faster the board's going.
fn run_jointer(
    station: Res<JointerStation>,
    actions: Res<Actions>,
    view: Res<ShopView>,
    time: Res<Time>,
    mut motor: Single<&mut Motor, With<Cutterhead>>,
    mut sawing: MessageWriter<Sawing>,
    mut last: Local<(u32, Length)>,
) {
    let pushing = *view == ShopView::Jointer && actions.held(Action::Push);
    if station.at_head() || pushing && station.can_push() {
        motor.run();
    }
    let (bites, advance) = (station.bites(), station.advance());
    if bites != last.0
        && let Some((at, toward)) = station.chips()
    {
        let moved = (advance - last.1).as_meters_f32().abs();
        let amount = moved / (FEED_SPEED * time.delta_secs().max(1e-3));
        sawing.write(Sawing {
            at: ORIGIN + at,
            toward,
            amount: amount.clamp(0.3, 2.0),
        });
    }
    *last = (bites, advance);
}

/// Remembers where the pointer grabbed the board, so it doesn't jump to the cursor.
fn grab_board(
    drag: On<PointerDragStart>,
    camera: Single<(&Camera, &GlobalTransform)>,
    station: Res<JointerStation>,
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

/// Slides the board along the fence after the pointer: forward to joint it, back to back it
/// out.
fn drag_board(
    drag: On<PointerDrag>,
    camera: Single<(&Camera, &GlobalTransform)>,
    grab: Res<BoardGrab>,
    mut station: ResMut<JointerStation>,
) {
    let (camera, camera_transform) = *camera;
    if drag.button == PointerButton::Primary
        && let Some(point) =
            pointer_on_plane(camera, camera_transform, drag.pointer.position, ORIGIN.y)
    {
        // The board feeds toward -z, so moving the pointer that way pushes it.
        station.move_to(grab.advance - snap(point.z - grab.pointer, 64));
    }
}

/// Turns the board a quarter turn left or right, flips it over, stands it on an edge, or
/// chooses what it rests on, while it waits in front of the cutterhead.
fn turn_board(actions: Res<Actions>, mut station: ResMut<JointerStation>) {
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

/// With empty hands, a click on a board clear of the cutterhead picks it up; holding one, a
/// click on the jointer's board sets the one in hand down here instead.
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
    if stations.jointer.choosing_rest() {
        if *part == Part::Whole
            && let Some(surface) = rest::surface_hit(shape, transform, &click.hit)
        {
            stations.jointer.pick_rest(slot.0, surface);
        }
        return;
    }
    let handled = if stations.hand.0.is_some() {
        stations.set_down(ShopView::Jointer, None)
    } else {
        stations.hand.0 = stations.jointer.take(*part);
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
    station: Res<JointerStation>,
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

/// Drops the infeed table by the depth of cut.
fn draw_tables(
    station: Res<JointerStation>,
    mut infeed: Single<&mut Transform, With<InfeedTable>>,
) {
    if station.is_changed() {
        let depth = station.jointer.depth.as_meters_f32();
        infeed.translation = ORIGIN + Vec3::new(TABLE_FRONT, -TABLE_THICKNESS - depth, GAP);
    }
}

/// Stands the fence where it meets the tables, its face toward the knives' end, leaning out
/// over the tables as it's tilted.
fn draw_fence(station: Res<JointerStation>, mut fence: Single<&mut Transform, With<Fence>>) {
    if station.is_changed() {
        let jointer = station.jointer;
        let tilt = jointer.tilt.radians() as f32;
        let turn = Quat::from_rotation_z(tilt);
        // The mesh's corner is its face's foot, on the line it turns about.
        fence.translation = ORIGIN + Vec3::X * jointer.fence.as_meters_f32();
        fence.rotation = turn;
    }
}

/// Draws what's on the jointer, each board with its own mesh, rebuilt as the knives take a layer
/// off it: a board, or every board of a glue-up.
fn draw_board(
    mut commands: Commands,
    station: Res<JointerStation>,
    shop: Res<ShopAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut parts: Query<SawPart, With<AtJointer>>,
) {
    if !station.is_changed() {
        return;
    }
    let boards = station.boards();
    let missing = place_parts(&boards, ORIGIN, Quat::IDENTITY, &mut meshes, &mut parts);
    for (part, slot, pose, placed) in missing {
        commands
            .spawn((
                Mesh3d(meshes.add(lumber_mesh(&placed.board, placed.grain))),
                MeshMaterial3d(shop.wood.clone()),
                pose,
                part,
                slot,
                AtJointer,
                Shape::built(&placed),
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

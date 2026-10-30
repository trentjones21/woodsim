//! The table saw station: the saw, its fence, the board, and the controls.

use std::f32::consts::FRAC_PI_2;

use bevy::{picking::pointer::PointerButton, prelude::*};
use woodsim::Length;

use crate::{
    blades::{BladeSystems, Motor, Sawing, spawn_blade},
    controls::{Action, Actions, Step},
    feed::TableSawStation,
    hand::{Grabbable, Hand},
    meshes::{Grain, board_mesh, lumber_mesh, rounded_box},
    rest,
    shop::{
        Block, Part, SawPart, Shape, ShopAssets, ShopView, Slot, Stations, inches, place_parts,
        pointer_on_plane, rounded_block, set_material_on, snap, snap_to_sixteenth, station_part,
    },
};

/// Where the station's coordinates start in the shop: the blade's fence-side face, at the
/// table top.
pub const ORIGIN: Vec3 = Vec3::new(0.0, inches(34.0), 0.0);

/// The space the saw and its outfeed table take up, for outlining it in the overview.
pub const BOUNDS: Block = Block {
    min: Vec3::new(TABLE_LEFT, -ORIGIN.y, OUTFEED_END),
    max: Vec3::new(TABLE_RIGHT, FENCE_HEIGHT, TABLE_FRONT + inches(2.0)),
};

/// The cast-iron top, in table space.
const TABLE_LEFT: f32 = inches(-14.0);
const TABLE_RIGHT: f32 = inches(28.0);
const TABLE_FRONT: f32 = inches(13.5);
const TABLE_BACK: f32 = inches(-13.5);
/// The far end of the outfeed table behind the saw.
const OUTFEED_END: f32 = inches(-32.0);

const FENCE_WIDTH: f32 = inches(2.0);
const FENCE_HEIGHT: f32 = inches(2.5);
/// The fence runs the depth of the table and over the front rail.
const FENCE_LENGTH: f32 = TABLE_FRONT + inches(2.0) - TABLE_BACK;

/// Where the rip fence sits, slid out of the way, while the miter gauge is in use.
const FENCE_PARKED: f32 = TABLE_RIGHT - FENCE_WIDTH;
/// The middle of the left miter slot, where the miter gauge's bar rides.
const GAUGE_SLOT: f32 = inches(-6.125);
/// The stop block on the miter gauge's auxiliary fence, and how tall that fence is.
const STOP_BLOCK: Vec3 = Vec3::new(inches(1.5), inches(2.0), inches(1.5));
/// The auxiliary fence stops short of the miter saw's bench, next door.
const AUX_FENCE: Vec3 = Vec3::new(inches(21.0), inches(2.5), inches(0.75));

const FENCE_STEP: Length = Length::from_inch_fraction(1, 16);
const FENCE_BIG_STEP: Length = Length::from_inches(1);
const HEIGHT_STEP: Length = Length::from_inch_fraction(1, 16);
const HEIGHT_BIG_STEP: Length = Length::from_inch_fraction(1, 4);
/// The riving knife's top sits this far below the blade's, so it can follow a cut that doesn't
/// go through.
const KNIFE_BELOW_BLADE: f32 = inches(1.0 / 8.0);
const KNIFE_HEIGHT: f32 = inches(4.0);
/// How fast the board moves while Space is held: 10" a second.
const FEED_SPEED: f32 = inches(10.0);
/// Seconds the saw takes to come up to speed and to coast to a stop, and how long it runs on
/// after the board's clear of the blade, as if switched off then.
const SPIN_UP: f32 = 0.6;
const COAST: f32 = 2.5;
const RUN_ON: f32 = 1.5;

pub struct TableSawPlugin;

impl Plugin for TableSawPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TableSawStation>()
            .init_resource::<BoardGrab>()
            .add_systems(Startup, spawn_saw)
            .add_systems(
                Update,
                (
                    (
                        move_guide,
                        switch_guide,
                        swing_gauge,
                        tilt_blade,
                        raise_blade,
                        turn_board,
                        push_board,
                        bring_back,
                    )
                        .run_if(resource_equals(ShopView::TableSaw)),
                    run_saw.before(BladeSystems),
                    (draw_fence, draw_gauge, draw_blade, draw_board),
                )
                    .chain(),
            );
    }
}

#[derive(Component)]
struct Fence {
    /// How far right of the fence face the pointer grabbed it, in meters.
    grab_offset: f32,
}

#[derive(Component)]
pub(crate) struct AtTableSaw;

/// The miter gauge: it rides with the back of the board, and shows only while it's in use.
#[derive(Component)]
struct MiterGauge;

/// The miter gauge's head, its auxiliary fence, and the stop on it: what turns as the gauge is
/// set to an angle, while its bar stays in the slot.
#[derive(Component)]
struct GaugeHead;

/// The stop block on the miter gauge's auxiliary fence.
#[derive(Component)]
struct GaugeStop {
    /// How far right of the block's face the pointer grabbed it, in meters.
    grab_offset: f32,
}

/// The blade, which rises and falls with its height setting, and tilts.
#[derive(Component)]
struct BladeDisc;

/// The riving knife, which rises and falls with the blade.
#[derive(Component)]
struct RivingKnife;

/// Where the pointer grabbed the board to drag it, along the table, and how far along the blade
/// the board was then. The board follows how far the pointer moves.
#[derive(Resource, Default)]
struct BoardGrab {
    pointer: f32,
    advance: Length,
}

fn spawn_saw(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    shop: Res<ShopAssets>,
    station: Res<TableSawStation>,
) {
    let mut paint = |color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.55,
            ..default()
        })
    };
    let table_top = paint(Color::srgb(0.76, 0.79, 0.83));
    let cabinet = paint(Color::srgb(0.98, 0.72, 0.2));
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
        station_part(entity, ShopView::TableSaw);
    };
    spawn_box(
        &mut commands,
        &mut meshes,
        &table_top,
        Vec3::new(TABLE_LEFT, inches(-1.5), TABLE_BACK),
        Vec3::new(TABLE_RIGHT, 0.0, TABLE_FRONT),
    );
    // Miter slots: shallow channels either side of the blade, drawn as dark strips.
    for x in [-6.5, 5.5] {
        spawn_box(
            &mut commands,
            &mut meshes,
            &dark_metal,
            Vec3::new(inches(x), inches(-0.2), TABLE_BACK),
            Vec3::new(inches(x + 0.75), inches(0.02), TABLE_FRONT),
        );
    }
    // The throat plate around the blade.
    spawn_box(
        &mut commands,
        &mut meshes,
        &safety_red,
        Vec3::new(inches(-1.75), inches(-0.2), inches(-7.0)),
        Vec3::new(inches(1.75), inches(0.03), inches(7.0)),
    );
    // Front rail, which the fence rides on, with a measuring tape along its top.
    spawn_box(
        &mut commands,
        &mut meshes,
        &dark_metal,
        Vec3::new(TABLE_LEFT, inches(-2.5), TABLE_FRONT),
        Vec3::new(TABLE_RIGHT, 0.0, TABLE_FRONT + inches(2.0)),
    );
    spawn_box(
        &mut commands,
        &mut meshes,
        &tape_yellow,
        Vec3::new(TABLE_LEFT + inches(1.0), 0.0, TABLE_FRONT + inches(0.4)),
        Vec3::new(
            TABLE_RIGHT - inches(1.0),
            inches(0.03),
            TABLE_FRONT + inches(1.4),
        ),
    );
    // The power paddle hangs under the rail, left of the blade, where a hip can bump it off.
    spawn_box(
        &mut commands,
        &mut meshes,
        &safety_red,
        Vec3::new(inches(-12.0), inches(-5.0), TABLE_FRONT + inches(1.5)),
        Vec3::new(inches(-8.5), inches(-2.5), TABLE_FRONT + inches(3.0)),
    );
    // The cabinet on a dark plinth, with a dust port out the back.
    spawn_box(
        &mut commands,
        &mut meshes,
        &cabinet,
        Vec3::new(inches(-10.0), inches(3.0) - ORIGIN.y, inches(-10.0)),
        Vec3::new(inches(10.0), inches(-1.5), inches(10.0)),
    );
    spawn_box(
        &mut commands,
        &mut meshes,
        &dark_metal,
        Vec3::new(inches(-11.0), -ORIGIN.y, inches(-11.0)),
        Vec3::new(inches(11.0), inches(3.0) - ORIGIN.y, inches(11.0)),
    );
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(inches(2.0), inches(3.0)))),
        MeshMaterial3d(dark_metal.clone()),
        Transform::from_translation(ORIGIN + Vec3::new(0.0, inches(-24.0), inches(-11.0)))
            .with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
        Pickable::IGNORE,
    ));
    // Outfeed table: a maple top on a painted support panel.
    let outfeed_top = Block {
        min: Vec3::new(inches(-10.0), inches(-0.75), OUTFEED_END),
        max: Vec3::new(inches(20.0), 0.0, TABLE_BACK),
    };
    let entity = commands.spawn((
        Mesh3d(meshes.add(board_mesh(
            outfeed_top.max - outfeed_top.min,
            Vec3::new(0.3, 0.0, 0.1),
            Grain::X,
        ))),
        MeshMaterial3d(shop.maple.clone()),
        Transform::from_translation(ORIGIN + outfeed_top.min),
    ));
    station_part(entity, ShopView::TableSaw);
    spawn_box(
        &mut commands,
        &mut meshes,
        &cabinet,
        Vec3::new(inches(-9.0), -ORIGIN.y, OUTFEED_END),
        Vec3::new(inches(19.0), inches(-0.75), OUTFEED_END + inches(0.75)),
    );

    // The blade and riving knife are placed at the blade's height by `draw_blade`. The blade
    // turns about x, its top toward the operator.
    let kerf = station.saw.blade.kerf.as_meters_f32();
    let blade = spawn_blade(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        station.saw.blade,
        Motor::new(SPIN_UP, COAST, RUN_ON),
    );
    commands.entity(blade).insert(BladeDisc);
    // The riving knife stands in the kerf just behind the blade, so pieces pass either side.
    let knife = Block {
        min: Vec3::new(-kerf * 0.8, -KNIFE_HEIGHT, inches(-6.5)),
        max: Vec3::new(-kerf * 0.2, 0.0, inches(-3.0)),
    };
    commands.spawn((
        Mesh3d(meshes.add(rounded_box(knife.max - knife.min))),
        MeshMaterial3d(dark_metal.clone()),
        Transform::default(),
        Pickable::IGNORE,
        RivingKnife,
    ));

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
    commands
        .spawn((
            Mesh3d(meshes.add(rounded_box(Vec3::new(
                FENCE_WIDTH,
                FENCE_HEIGHT,
                FENCE_LENGTH,
            )))),
            MeshMaterial3d(fence_color.clone()),
            Transform::default(),
            Fence { grab_offset: 0.0 },
            Grabbable,
        ))
        .observe(set_material_on::<PointerOver>(fence_hover))
        .observe(set_material_on::<PointerOut>(fence_color))
        .observe(grab_fence)
        .observe(drag_fence);

    // The miter gauge, in its own frame: x across the table as usual, and z = 0 at the back of
    // the board, which its auxiliary fence pushes. Its bar rides the left miter slot; its head
    // sits behind the fence; the stop block, placed by `draw_gauge`, sets the piece's length.
    let gauge = commands
        .spawn((Transform::default(), Visibility::Hidden, MiterGauge))
        .id();
    let head = commands
        .spawn((
            Transform::default(),
            Visibility::Inherited,
            GaugeHead,
            ChildOf(gauge),
        ))
        .id();
    for (index, (material, min, max)) in [
        (
            &dark_metal,
            Vec3::new(GAUGE_SLOT - inches(0.3), inches(-0.35), inches(-16.0)),
            Vec3::new(GAUGE_SLOT + inches(0.3), 0.0, inches(2.5)),
        ),
        (
            &safety_red,
            Vec3::new(inches(-9.5), 0.0, AUX_FENCE.z),
            Vec3::new(inches(-2.75), inches(2.25), inches(2.5)),
        ),
        (
            &dark_metal,
            Vec3::new(GAUGE_SLOT - inches(0.5), inches(2.25), inches(1.2)),
            Vec3::new(GAUGE_SLOT + inches(0.5), inches(3.0), inches(2.1)),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        // The bar stays in the slot; the rest turns with the head.
        commands.spawn((
            Mesh3d(meshes.add(rounded_box(max - min))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(min),
            Pickable::IGNORE,
            ChildOf(if index == 0 { gauge } else { head }),
        ));
    }
    // The auxiliary fence reaches from just short of the blade out past the farthest stop.
    commands.spawn((
        Mesh3d(meshes.add(board_mesh(AUX_FENCE, Vec3::new(0.2, 0.0, 0.0), Grain::X))),
        MeshMaterial3d(shop.maple.clone()),
        Transform::from_xyz(inches(-1.0) - AUX_FENCE.x, 0.0, 0.0),
        Pickable::IGNORE,
        ChildOf(head),
    ));
    let stop_color = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.78, 0.15),
        perceptual_roughness: 0.4,
        ..default()
    });
    let stop_hover = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.85, 0.4),
        emissive: LinearRgba::new(1.0, 0.7, 0.1, 1.0) * 40.0,
        ..default()
    });
    commands
        .spawn((
            Mesh3d(meshes.add(rounded_box(STOP_BLOCK))),
            MeshMaterial3d(stop_color.clone()),
            Transform::default(),
            GaugeStop { grab_offset: 0.0 },
            Grabbable,
            ChildOf(head),
        ))
        .observe(set_material_on::<PointerOver>(stop_hover))
        .observe(set_material_on::<PointerOut>(stop_color))
        .observe(grab_gauge_stop)
        .observe(drag_gauge_stop);
}

/// Moves whichever guide is in use the way its arrow points: the gauge's stop is left of the
/// blade, so moving it left makes the piece longer.
fn move_guide(actions: Res<Actions>, mut station: ResMut<TableSawStation>) {
    for action in actions.started() {
        let Action::Guide(Step { more: right, big }) = action else {
            continue;
        };
        let step = if big { FENCE_BIG_STEP } else { FENCE_STEP };
        if station.using_gauge() {
            let stop = station.gauge_stop();
            station.set_gauge_stop(if right { stop - step } else { stop + step });
        } else {
            let fence = station.saw.fence;
            station.set_fence(if right { fence + step } else { fence - step });
        }
    }
}

/// Swings the miter gauge's head 1°, or a big step to the next place it clicks into.
fn tilt_blade(actions: Res<Actions>, mut station: ResMut<TableSawStation>) {
    for action in actions.started() {
        let Action::Tilt(Step { more, big }) = action else {
            continue;
        };
        if big {
            station.tilt_to_next_stop(more);
        } else {
            let step = woodsim::Angle::from_degrees(if more { 1 } else { -1 });
            let tilt = station.saw.tilt + step;
            station.set_tilt(tilt);
        }
    }
}

fn swing_gauge(actions: Res<Actions>, mut station: ResMut<TableSawStation>) {
    for action in actions.started() {
        let Action::Miter(Step { more, big }) = action else {
            continue;
        };
        if big {
            station.swing_gauge_to_next_stop(more);
        } else {
            let step = woodsim::Angle::from_degrees(if more { 1 } else { -1 });
            let angle = station.saw.gauge_angle + step;
            station.set_gauge_angle(angle);
        }
    }
}

fn switch_guide(actions: Res<Actions>, mut station: ResMut<TableSawStation>) {
    if actions.just(Action::SwitchGuide) {
        station.switch_guide();
    }
}

/// Remembers where on the stop block the pointer grabbed it, so it doesn't jump to the cursor.
fn grab_gauge_stop(
    drag: On<PointerDragStart>,
    camera: Single<(&Camera, &GlobalTransform)>,
    mut stops: Query<&mut GaugeStop>,
    station: Res<TableSawStation>,
) {
    let (camera, camera_transform) = *camera;
    if drag.button == PointerButton::Primary
        && let Some(point) = pointer_on_plane(
            camera,
            camera_transform,
            drag.pointer.position,
            ORIGIN.y + STOP_BLOCK.y,
        )
        && let Ok(mut stop) = stops.get_mut(drag.entity)
    {
        stop.grab_offset = along_gauge(&station, point) - stop_face(&station);
    }
}

fn drag_gauge_stop(
    drag: On<PointerDrag>,
    camera: Single<(&Camera, &GlobalTransform)>,
    stops: Query<&GaugeStop>,
    mut station: ResMut<TableSawStation>,
) {
    let (camera, camera_transform) = *camera;
    if drag.button == PointerButton::Primary
        && let Some(point) = pointer_on_plane(
            camera,
            camera_transform,
            drag.pointer.position,
            ORIGIN.y + STOP_BLOCK.y,
        )
        && let Ok(stop) = stops.get(drag.entity)
    {
        // The stop's face is the gauge setting plus a kerf left of the blade's fence-side face.
        let face = along_gauge(&station, point) - stop.grab_offset;
        let kerf = station.saw.blade.kerf.as_meters_f32();
        station.set_gauge_stop(snap_to_sixteenth(-face - kerf));
    }
}

/// How far across the table `point` is, along the gauge's auxiliary fence as its head is
/// turned: where it would be if the head were square.
fn along_gauge(station: &TableSawStation, point: Vec3) -> f32 {
    let (z, turn) = station.gauge_pose();
    let pivot = Vec3::new(-station.saw.blade.kerf.as_meters_f32(), 0.0, 0.0);
    let local = point - (ORIGIN + Vec3::Z * z);
    (turn.inverse() * (local - pivot) + pivot).x
}

/// Where the face of the gauge's stop block is across the table, in meters.
fn stop_face(station: &TableSawStation) -> f32 {
    -(station.saw.blade.kerf + station.gauge_stop()).as_meters_f32()
}

/// Remembers where on the fence the pointer grabbed it, so the fence doesn't jump to the cursor.
fn grab_fence(
    drag: On<PointerDragStart>,
    camera: Single<(&Camera, &GlobalTransform)>,
    mut fences: Query<&mut Fence>,
    station: Res<TableSawStation>,
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
        fence.grab_offset = point.x - ORIGIN.x - station.saw.fence.as_meters_f32();
    }
}

fn drag_fence(
    drag: On<PointerDrag>,
    camera: Single<(&Camera, &GlobalTransform)>,
    fences: Query<&Fence>,
    mut station: ResMut<TableSawStation>,
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

fn raise_blade(actions: Res<Actions>, mut station: ResMut<TableSawStation>) {
    for action in actions.started() {
        let Action::Blade(Step { more: up, big }) = action else {
            continue;
        };
        let step = if big { HEIGHT_BIG_STEP } else { HEIGHT_STEP };
        let height = station.saw.height;
        station.set_height(if up { height + step } else { height - step });
    }
}

fn push_board(actions: Res<Actions>, time: Res<Time>, mut station: ResMut<TableSawStation>) {
    if actions.held(Action::Push) {
        station.push(FEED_SPEED * time.delta_secs());
    }
}

/// Runs the saw while a board's at the blade, or being pushed toward it, and throws sawdust up
/// wherever the blade took wood this frame, more the faster the board's going.
fn run_saw(
    station: Res<TableSawStation>,
    actions: Res<Actions>,
    view: Res<ShopView>,
    time: Res<Time>,
    mut motor: Single<&mut Motor, With<BladeDisc>>,
    mut sawing: MessageWriter<Sawing>,
    mut last: Local<(u32, Length)>,
) {
    let pushing = *view == ShopView::TableSaw && actions.held(Action::Push);
    if station.at_blade() || pushing && station.can_push() {
        motor.run();
    }
    let (bites, advance) = (station.bites(), station.advance());
    if bites != last.0
        && let Some((at, toward)) = station.sawdust()
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

fn bring_back(actions: Res<Actions>, mut station: ResMut<TableSawStation>) {
    if actions.just(Action::BringBack) {
        station.bring_back();
    }
}

/// Remembers where the pointer grabbed the board, so it doesn't jump to the cursor.
fn grab_board(
    drag: On<PointerDragStart>,
    camera: Single<(&Camera, &GlobalTransform)>,
    station: Res<TableSawStation>,
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

/// Slides the board along the fence after the pointer: forward to cut, back to back it out.
fn drag_board(
    drag: On<PointerDrag>,
    camera: Single<(&Camera, &GlobalTransform)>,
    grab: Res<BoardGrab>,
    mut station: ResMut<TableSawStation>,
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

/// Turns the board a quarter turn left or right, flips it over, or stands it on an edge, while
/// it waits in front of the blade.
fn turn_board(actions: Res<Actions>, mut station: ResMut<TableSawStation>) {
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

/// With empty hands, a click on a board clear of the blade picks it up; holding one, a click on
/// the saw's board sets the one in hand down here instead.
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
    if stations.table_saw.choosing_rest() {
        if *part == Part::Whole
            && let Some(surface) = rest::surface_hit(shape, transform, &click.hit)
        {
            stations.table_saw.pick_rest(slot.0, surface);
        }
        return;
    }
    let handled = if stations.hand.0.is_some() {
        stations.set_down(ShopView::TableSaw, None)
    } else {
        stations.hand.0 = stations.table_saw.take(*part);
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
    station: Res<TableSawStation>,
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

fn draw_fence(station: Res<TableSawStation>, mut fence: Single<&mut Transform, With<Fence>>) {
    if station.is_changed() {
        let x = if station.using_gauge() {
            FENCE_PARKED
        } else {
            station.saw.fence.as_meters_f32()
        };
        fence.translation = ORIGIN + Vec3::new(x, 0.0, TABLE_BACK);
    }
}

/// Shows the miter gauge while it's in use, at the back of the board, with its stop block set.
fn draw_gauge(
    station: Res<TableSawStation>,
    gauge: Single<Entity, With<MiterGauge>>,
    head: Single<Entity, With<GaugeHead>>,
    stop: Single<Entity, With<GaugeStop>>,
    mut parts: Query<(&mut Transform, &mut Visibility)>,
) {
    if !station.is_changed() {
        return;
    }
    let (z, turn) = station.gauge_pose();
    if let Ok((mut transform, mut visibility)) = parts.get_mut(*gauge) {
        *visibility = if station.using_gauge() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        transform.translation = ORIGIN + Vec3::Z * z;
    }
    // The head turns about where its face crosses the blade's stop-side face.
    if let Ok((mut transform, _)) = parts.get_mut(*head) {
        let pivot = Vec3::new(-station.saw.blade.kerf.as_meters_f32(), 0.0, 0.0);
        *transform = Transform::from_translation(pivot - turn * pivot).with_rotation(turn);
    }
    // The block sits against the board's end, in front of the auxiliary fence.
    if let Ok((mut transform, _)) = parts.get_mut(*stop) {
        transform.translation = Vec3::new(stop_face(&station) - STOP_BLOCK.x, 0.0, -STOP_BLOCK.z);
    }
}

/// Raises the blade and riving knife to the blade's height, and tilts them about where the
/// blade's fence-side face meets the table, leaning away from the fence. The blade's top is
/// its radius above its center, up its tilted plane, and reaches the blade's height.
fn draw_blade(
    station: Res<TableSawStation>,
    mut blade: Single<&mut Transform, (With<BladeDisc>, Without<RivingKnife>)>,
    mut knife: Single<&mut Transform, (With<RivingKnife>, Without<BladeDisc>)>,
) {
    if station.is_changed() {
        let saw = station.saw;
        let height = saw.height.as_meters_f32();
        let kerf = saw.blade.kerf.as_meters_f32();
        let radius = saw.blade.radius().as_meters_f32();
        let tilt = saw.tilt.radians() as f32;
        let turn = Quat::from_rotation_z(tilt);
        // Up the blade's own plane, from the table to its top: the height up the table's, less
        // how much lower the mid-plane is there, tilted.
        let top = (height + kerf / 2.0 * tilt.sin()) / tilt.cos();
        let tilted = |x: f32, y: f32, z: f32| ORIGIN + turn * Vec3::new(x, y, z);
        blade.translation = tilted(-kerf / 2.0, top - radius, 0.0);
        blade.rotation = turn;
        let knife_top = top - KNIFE_BELOW_BLADE;
        knife.translation = tilted(-kerf * 0.8, knife_top - KNIFE_HEIGHT, inches(-6.5));
        knife.rotation = turn;
    }
}

/// Draws what's on the saw, each board with its own mesh, rebuilt as it's cut: a board, or
/// every board of a glue-up.
fn draw_board(
    mut commands: Commands,
    station: Res<TableSawStation>,
    shop: Res<ShopAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut parts: Query<SawPart, With<AtTableSaw>>,
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
                AtTableSaw,
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

//! The table saw station: the saw, its fence, the board, and the controls.

use std::f32::consts::FRAC_PI_2;

use bevy::{picking::pointer::PointerButton, prelude::*};
use woodsim::Length;

use crate::{
    chop::MiterSawStation,
    feed::TableSawStation,
    hand::Hand,
    meshes::{Grain, board_mesh, rounded_box},
    shop::{
        Block, Part, Shape, ShopAssets, ShopView, Stations, inches, place_parts, pointer_on_plane,
        rounded_block, set_material_on, snap, snap_to_sixteenth, station_part,
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

pub const HELP: &str = "Left/Right: move the fence or gauge stop 1/16\"   Shift: 1\"   \
                        Or drag it\n\
                        G: switch between the rip fence and the miter gauge\n\
                        Up/Down: raise or lower the blade 1/16\"   Shift+Up/Down: 1/4\"\n\
                        Drag the board, or hold Space, to push it   Drag it back to back it out\n\
                        Q/E: turn the board   F: flip it   V: stand it on edge   \
                        B: bring it back around\n\
                        Click a board clear of the blade: pick it up   \
                        Holding one: click the saw to set it here\n\
                        Tab: go to the miter saw   Esc: back to the shop";

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
                        move_fence_with_keys,
                        switch_guide_with_key,
                        raise_blade_with_keys,
                        turn_with_keys,
                        push_board,
                        bring_back_with_key,
                    )
                        .run_if(resource_equals(ShopView::TableSaw)),
                    set_loose_aside,
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
struct AtTableSaw;

/// The miter gauge: it rides with the back of the board, and shows only while it's in use.
#[derive(Component)]
struct MiterGauge;

/// The stop block on the miter gauge's auxiliary fence.
#[derive(Component)]
struct GaugeStop {
    /// How far right of the block's face the pointer grabbed it, in meters.
    grab_offset: f32,
}

/// The blade, which rises and falls with its height setting.
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
    let wood = shop.wood.clone();

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

    // The blade and riving knife are placed at the blade's height by `draw_blade`.
    let kerf = station.saw.blade.kerf.as_meters_f32();
    let radius = station.saw.blade.radius().as_meters_f32();
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(radius, kerf))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.88, 0.89, 0.92),
            perceptual_roughness: 0.3,
            ..default()
        })),
        // A cylinder's axis starts along Y; turn it to X so the blade spins toward the operator.
        Transform::from_rotation(Quat::from_rotation_z(FRAC_PI_2)),
        Pickable::IGNORE,
        BladeDisc,
    ));
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
    for (material, min, max) in [
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
    ] {
        commands.spawn((
            Mesh3d(meshes.add(rounded_box(max - min))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(min),
            Pickable::IGNORE,
            ChildOf(gauge),
        ));
    }
    // The auxiliary fence reaches from just short of the blade out past the farthest stop.
    commands.spawn((
        Mesh3d(meshes.add(board_mesh(AUX_FENCE, Vec3::new(0.2, 0.0, 0.0), Grain::X))),
        MeshMaterial3d(shop.maple.clone()),
        Transform::from_xyz(inches(-1.0) - AUX_FENCE.x, 0.0, 0.0),
        Pickable::IGNORE,
        ChildOf(gauge),
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
            ChildOf(gauge),
        ))
        .observe(set_material_on::<PointerOver>(stop_hover))
        .observe(set_material_on::<PointerOut>(stop_color))
        .observe(grab_gauge_stop)
        .observe(drag_gauge_stop);

    // Each part gets its own mesh, rebuilt as the board is cut.
    for part in [Part::Whole, Part::Piece, Part::Offcut] {
        commands
            .spawn((
                Mesh3d(meshes.add(rounded_box(Vec3::splat(0.01)))),
                MeshMaterial3d(wood.clone()),
                Transform::default(),
                Visibility::Hidden,
                part,
                AtTableSaw,
                Shape::default(),
            ))
            .observe(highlight_carryable)
            .observe(set_material_on::<PointerOut>(wood.clone()))
            .observe(board_clicked)
            .observe(grab_board)
            .observe(drag_board);
    }
}

fn move_fence_with_keys(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<TableSawStation>) {
    let step = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        FENCE_BIG_STEP
    } else {
        FENCE_STEP
    };
    // The arrows move whichever guide is in use the way they point: the gauge's stop is left
    // of the blade, so moving it left makes the piece longer.
    let (right, left) = (
        keys.just_pressed(KeyCode::ArrowRight),
        keys.just_pressed(KeyCode::ArrowLeft),
    );
    if !(right || left) {
        return;
    }
    if station.using_gauge() {
        let stop = station.gauge_stop();
        station.set_gauge_stop(if right { stop - step } else { stop + step });
    } else {
        let fence = station.saw.fence;
        station.set_fence(if right { fence + step } else { fence - step });
    }
}

fn switch_guide_with_key(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<TableSawStation>) {
    if keys.just_pressed(KeyCode::KeyG) {
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
        stop.grab_offset = point.x - ORIGIN.x - stop_face(&station);
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
        let face = point.x - ORIGIN.x - stop.grab_offset;
        let kerf = station.saw.blade.kerf.as_meters_f32();
        station.set_gauge_stop(snap_to_sixteenth(-face - kerf));
    }
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

fn raise_blade_with_keys(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<TableSawStation>) {
    let step = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        HEIGHT_BIG_STEP
    } else {
        HEIGHT_STEP
    };
    let height = station.saw.height;
    if keys.just_pressed(KeyCode::ArrowUp) {
        station.set_height(height + step);
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        station.set_height(height - step);
    }
}

fn push_board(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut station: ResMut<TableSawStation>,
) {
    if keys.pressed(KeyCode::Space) {
        station.push(FEED_SPEED * time.delta_secs());
    }
}

fn bring_back_with_key(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<TableSawStation>) {
    if keys.just_pressed(KeyCode::KeyB) {
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

/// Q and E turn the board a quarter turn left and right, and F flips it over, while it waits
/// in front of the blade.
fn turn_with_keys(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<TableSawStation>) {
    if keys.just_pressed(KeyCode::KeyQ) {
        station.turn_left();
    }
    if keys.just_pressed(KeyCode::KeyE) {
        station.turn_right();
    }
    if keys.just_pressed(KeyCode::KeyF) {
        station.flip();
    }
    if keys.just_pressed(KeyCode::KeyV) {
        station.tip();
    }
}

/// Puts any strips a cut set loose on the parts pile by the miter saw.
fn set_loose_aside(mut table_saw: ResMut<TableSawStation>, mut miter_saw: ResMut<MiterSawStation>) {
    if table_saw.has_loose() {
        let loose = table_saw.take_loose();
        miter_saw.parts.extend(loose);
    }
}

/// With empty hands, a click on a board clear of the blade picks it up; holding one, a click on
/// the saw's board sets the one in hand down here instead.
fn board_clicked(
    click: On<PointerClick>,
    parts: Query<&Part>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut stations: Stations,
) {
    if click.button != PointerButton::Primary || stations.gesture.dragged() {
        return;
    }
    let Ok(part) = parts.get(click.entity) else {
        return;
    };
    let handled = if stations.hand.0.is_some() {
        stations.set_down(ShopView::TableSaw)
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
        None => station.can_take(part),
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
    stop: Single<Entity, With<GaugeStop>>,
    mut parts: Query<(&mut Transform, &mut Visibility)>,
) {
    if !station.is_changed() {
        return;
    }
    if let Ok((mut transform, mut visibility)) = parts.get_mut(*gauge) {
        *visibility = if station.using_gauge() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        transform.translation = ORIGIN + Vec3::Z * station.trailing_edge();
    }
    // The block sits against the board's end, in front of the auxiliary fence.
    if let Ok((mut transform, _)) = parts.get_mut(*stop) {
        transform.translation = Vec3::new(stop_face(&station) - STOP_BLOCK.x, 0.0, -STOP_BLOCK.z);
    }
}

/// Raises the blade and riving knife to the blade's height. The blade's center is its radius
/// below its top.
fn draw_blade(
    station: Res<TableSawStation>,
    mut blade: Single<&mut Transform, (With<BladeDisc>, Without<RivingKnife>)>,
    mut knife: Single<&mut Transform, (With<RivingKnife>, Without<BladeDisc>)>,
) {
    if station.is_changed() {
        let saw = station.saw;
        let height = saw.height.as_meters_f32();
        let kerf = saw.blade.kerf.as_meters_f32();
        let center = height - saw.blade.radius().as_meters_f32();
        blade.translation = ORIGIN + Vec3::new(-kerf / 2.0, center, 0.0);
        let knife_top = height - KNIFE_BELOW_BLADE;
        knife.translation = ORIGIN + Vec3::new(-kerf * 0.8, knife_top - KNIFE_HEIGHT, inches(-6.5));
    }
}

fn draw_board(
    station: Res<TableSawStation>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut parts: Query<
        (&Part, &Mesh3d, &mut Transform, &mut Visibility, &mut Shape),
        With<AtTableSaw>,
    >,
) {
    if station.is_changed() {
        let boards = station.boards();
        place_parts(&boards, ORIGIN, Quat::IDENTITY, &mut meshes, &mut parts);
    }
}

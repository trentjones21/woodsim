//! The miter saw station: a saw built into a bench, its stop block, the board, and the controls.

use std::f32::consts::FRAC_PI_2;

use bevy::{picking::pointer::PointerButton, prelude::*};
use woodsim::{Angle, Board, Length, MiterSaw};

use crate::{
    chop::{MITER_STOPS, MiterSawStation, board_turn},
    feed::TableSawStation,
    meshes::{Grain, board_mesh, lumber_mesh, rounded_box},
    shop::{
        Block, Part, Placed, Shape, ShopAssets, ShopView, inches, place_parts, pointer_on_plane,
        rounded_block, set_material_on, snap_to_sixteenth, station_part,
    },
};

/// Where the station's coordinates start in the shop: the blade's stop-side face, at the fence
/// face, level with the saw table. It sits to the left of the table saw.
pub const ORIGIN: Vec3 = Vec3::new(-1.6, inches(36.0), -0.1);

/// The space the bench and saw take up, raised arm included, for outlining it in the overview.
pub const BOUNDS: Block = Block {
    min: Vec3::new(-BENCH_HALF_LENGTH, -ORIGIN.y, inches(-8.0)),
    max: Vec3::new(BENCH_HALF_LENGTH, inches(16.0), inches(14.0)),
};

pub const HELP: &str = "Left/Right: move the stop 1/16\"   Shift+Left/Right: 1\"   \
                        Or drag the stop block\n\
                        Up/Down: set the depth stop 1/16\"   Shift: 1/4\"   \
                        Down past the table turns it off\n\
                        [ and ]: swing the miter 1 deg   \
                        Shift: to the next stop (15, 22.5, 30, 45 deg)\n\
                        Hold Space: lower the blade   Q/E: turn the board   F: flip it   \
                        V: stand it on edge\n\
                        Click a cut piece: slide it back against the stop\n\
                        Click a part on the pile: put it back on the saw   \
                        S: set everything on the saw aside\n\
                        Right-click a piece or press T: carry it to the table saw\n\
                        Tab: go to the workbench   Esc: back to the shop";

const FENCE_HEIGHT: f32 = inches(3.0);
const BENCH_HALF_LENGTH: f32 = inches(40.0);
const STOP_HEIGHT: f32 = inches(3.5);
/// The stop block reaches from behind the fence to this far in front of it.
const STOP_BACK: f32 = inches(-1.25);
const STOP_FRONT: f32 = inches(2.5);
const STOP_WIDTH: f32 = inches(1.5);
/// Fractions of the blade's full travel per second.
const LOWER_SPEED: f32 = 1.0 / 0.6;
const RAISE_SPEED: f32 = 1.0 / 0.4;

const STOP_STEP: Length = Length::from_inch_fraction(1, 16);
const DEPTH_STEP: Length = Length::from_inch_fraction(1, 16);
const DEPTH_BIG_STEP: Length = Length::from_inch_fraction(1, 4);
const MITER_STEP: Angle = Angle::from_degrees(1);
/// The miter scale's marks are this far out from where the table turns.
const SCALE_RADIUS: f32 = inches(9.6);
/// The saw's base is a little below its turning table, and a little above the bench top.
const BASE_TOP: f32 = inches(-0.025);

/// Where the arm's pivot is, on a post behind the fence.
fn pivot(saw: &MiterSaw) -> Vec3 {
    Vec3::new(
        0.0,
        saw.pivot.1.as_meters_f32(),
        -saw.pivot.0.as_meters_f32(),
    )
}

/// The blade's center relative to the pivot, with its lowest point `bottom` above the table.
fn blade_from_pivot(saw: &MiterSaw, bottom: Length) -> Vec3 {
    let (out, up) = saw.center(bottom);
    Vec3::new(0.0, up.as_meters_f32(), out.as_meters_f32()) - pivot(saw)
}
const STOP_BIG_STEP: Length = Length::from_inches(1);

pub struct MiterSawPlugin;

impl Plugin for MiterSawPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MiterSawStation>()
            .add_systems(Startup, spawn_miter_saw)
            .add_systems(
                Update,
                (
                    (
                        (
                            move_stop_with_keys,
                            set_depth_stop_with_keys,
                            swing_with_keys,
                            turn_with_keys,
                            set_aside_with_key,
                            carry_with_key,
                        )
                            .run_if(resource_equals(ShopView::MiterSaw)),
                        work_blade,
                    ),
                    (draw_stop, draw_table, draw_head, draw_board, draw_pile),
                )
                    .chain(),
            );
    }
}

#[derive(Component)]
struct StopBlock {
    /// How far right of the stop block's face the pointer grabbed it, in meters.
    grab_offset: f32,
}

/// The saw's turning table, with the post the arm pivots on: it turns to the miter angle about
/// where the blade's stop-side face crosses the fence.
#[derive(Component)]
struct Turntable;

/// The arm, motor, and blade that swing down together, on the turntable.
#[derive(Component)]
struct Head;

#[derive(Component)]
struct AtMiterSaw;

/// A part on the pile behind the fence, by its place in `MiterSawStation::parts`.
#[derive(Component)]
struct PilePart(usize);

fn spawn_miter_saw(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    shop: Res<ShopAssets>,
    station: Res<MiterSawStation>,
) {
    let mut paint = |color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.55,
            ..default()
        })
    };
    let cabinet = paint(Color::srgb(0.9, 0.42, 0.36));
    let drawer = paint(Color::srgb(0.97, 0.56, 0.48));
    let dark = paint(Color::srgb(0.22, 0.24, 0.3));
    let aluminum = paint(Color::srgb(0.86, 0.88, 0.91));
    let pull = paint(Color::srgb(0.98, 0.95, 0.88));
    let steel = paint(Color::srgb(0.4, 0.43, 0.5));
    let pointer = paint(Color::srgb(1.0, 0.78, 0.15));
    // The saw's housing: bright tool plastic.
    let body = paint(Color::srgb(0.08, 0.64, 0.66));
    let zero_clearance = paint(Color::srgb(0.92, 0.25, 0.22));
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
        station_part(entity, ShopView::MiterSaw);
    };
    // A cabinet workbench: drawers on a dark toe kick, under a maple top.
    let front = inches(14.0);
    spawn_box(
        &mut commands,
        &mut meshes,
        &dark,
        Vec3::new(-BENCH_HALF_LENGTH + inches(1.0), -ORIGIN.y, inches(-7.0)),
        Vec3::new(
            BENCH_HALF_LENGTH - inches(1.0),
            inches(3.0) - ORIGIN.y,
            front - inches(2.0),
        ),
    );
    spawn_box(
        &mut commands,
        &mut meshes,
        &cabinet,
        Vec3::new(-BENCH_HALF_LENGTH, inches(3.0) - ORIGIN.y, inches(-8.0)),
        Vec3::new(BENCH_HALF_LENGTH, inches(-1.75), front),
    );
    for (bottom, top) in [(-12.0, -3.0), (-22.0, -13.0), (-32.5, -23.0)] {
        for i in 0..4 {
            let left = -BENCH_HALF_LENGTH + inches(1.0 + 19.5 * i as f32);
            let right = left + inches(18.5);
            spawn_box(
                &mut commands,
                &mut meshes,
                &drawer,
                Vec3::new(left, inches(bottom), front),
                Vec3::new(right, inches(top), front + inches(0.4)),
            );
            let middle = (left + right) / 2.0;
            spawn_box(
                &mut commands,
                &mut meshes,
                &pull,
                Vec3::new(middle - inches(2.5), inches(top - 2.0), front + inches(0.4)),
                Vec3::new(middle + inches(2.5), inches(top - 1.4), front + inches(1.0)),
            );
        }
    }
    // The top sits a hair below the saw table so the two don't flicker against each other.
    let top = Block {
        min: Vec3::new(-BENCH_HALF_LENGTH, inches(-1.75), inches(-8.0)),
        max: Vec3::new(BENCH_HALF_LENGTH, inches(-0.05), front + inches(0.5)),
    };
    let entity = commands.spawn((
        Mesh3d(meshes.add(board_mesh(top.max - top.min, Vec3::ZERO, Grain::X))),
        MeshMaterial3d(shop.maple.clone()),
        Transform::from_translation(ORIGIN + top.min),
    ));
    station_part(entity, ShopView::MiterSaw);

    // The saw: a dark base, with the miter scale's marks on its front, around a turntable. Each
    // layer sits a hair above the one under it, the bench top, the base, the table's arm, and
    // the table, so no two flicker against each other.
    spawn_box(
        &mut commands,
        &mut meshes,
        &dark,
        Vec3::new(inches(-7.0), inches(-1.0), inches(-6.0)),
        Vec3::new(inches(7.0), BASE_TOP, inches(10.0)),
    );
    let mark = meshes.add(Cuboid::new(inches(0.12), inches(0.06), inches(0.4)));
    for stop in MITER_STOPS.iter().flat_map(|&stop| [stop, -stop]) {
        let turn = Quat::from_rotation_y(stop.radians() as f32);
        commands.spawn((
            Mesh3d(mark.clone()),
            MeshMaterial3d(pull.clone()),
            Transform::from_translation(ORIGIN + turn * Vec3::new(0.0, BASE_TOP, SCALE_RADIUS))
                .with_rotation(turn),
            Pickable::IGNORE,
        ));
    }
    let turntable = commands
        .spawn((
            Transform::from_translation(ORIGIN),
            Visibility::default(),
            Turntable,
        ))
        .id();
    let table_part = |commands: &mut Commands,
                      meshes: &mut Assets<Mesh>,
                      material: &Handle<StandardMaterial>,
                      min: Vec3,
                      max: Vec3| {
        let entity = commands.spawn((
            rounded_block(meshes, Block { min, max }, Vec3::ZERO),
            MeshMaterial3d(material.clone()),
            ChildOf(turntable),
        ));
        station_part(entity, ShopView::MiterSaw);
    };
    // The round table, flush with the base's top; its back half is under the fence.
    let entity = commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(inches(5.5), inches(0.5)))),
        MeshMaterial3d(steel.clone()),
        Transform::from_xyz(0.0, inches(-0.25), 0.0),
        ChildOf(turntable),
    ));
    station_part(entity, ShopView::MiterSaw);
    // Its arm out front, which points at the scale, under a red zero-clearance strip where the
    // blade comes through.
    table_part(
        &mut commands,
        &mut meshes,
        &steel,
        Vec3::new(inches(-1.4), inches(-0.5), inches(3.0)),
        Vec3::new(inches(1.4), inches(-0.012), inches(9.4)),
    );
    table_part(
        &mut commands,
        &mut meshes,
        &pointer,
        Vec3::new(inches(-0.1), inches(-0.04), inches(8.4)),
        Vec3::new(inches(0.1), inches(0.03), inches(9.3)),
    );
    commands.spawn((
        rounded_block(
            &mut meshes,
            Block {
                min: Vec3::new(inches(-0.6), inches(-0.5), inches(-1.0)),
                max: Vec3::new(inches(0.6), inches(0.02), inches(7.5)),
            },
            Vec3::ZERO,
        ),
        MeshMaterial3d(zero_clearance),
        Pickable::IGNORE,
        ChildOf(turntable),
    ));
    // The fence, split wide enough around the blade for it to turn 50° either way.
    for (left, right) in [
        (-BENCH_HALF_LENGTH + inches(2.0), inches(-1.6)),
        (inches(1.6), BENCH_HALF_LENGTH - inches(2.0)),
    ] {
        spawn_box(
            &mut commands,
            &mut meshes,
            &aluminum,
            Vec3::new(left, 0.0, inches(-1.0)),
            Vec3::new(right, FENCE_HEIGHT, 0.0),
        );
    }
    // The post the arm pivots on, which turns with the table.
    table_part(
        &mut commands,
        &mut meshes,
        &dark,
        Vec3::new(inches(-1.5), 0.0, inches(-6.0)),
        Vec3::new(
            inches(1.5),
            pivot(&station.saw).y + inches(1.0),
            inches(-2.5),
        ),
    );

    // The head is drawn with the arm all the way down; `draw_head` swings it up from there.
    let saw = station.saw;
    let kerf = saw.blade.kerf.as_meters_f32();
    let down = blade_from_pivot(&saw, saw.stroke.1);
    let arm_length = down.length();
    let arm_tilt = (-down.y).atan2(down.z);
    let motor = meshes.add(Cylinder::new(inches(1.9), inches(4.5)));
    // Rounded boxes start at a corner; these are centered on their transforms.
    let mut centered = |size: Vec3| {
        let mut mesh = rounded_box(size);
        mesh.translate_by(-size / 2.0);
        meshes.add(mesh)
    };
    let arm = centered(Vec3::new(inches(1.5), inches(1.8), arm_length));
    let handle = centered(Vec3::new(inches(1.3), inches(1.3), inches(4.5)));
    commands
        .spawn((
            Transform::from_translation(pivot(&saw)),
            Visibility::default(),
            Head,
            ChildOf(turntable),
        ))
        .with_children(|head| {
            head.spawn((
                Mesh3d(meshes.add(Cylinder::new(saw.blade.radius().as_meters_f32(), kerf))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.88, 0.89, 0.92),
                    perceptual_roughness: 0.3,
                    ..default()
                })),
                // A cylinder's axis starts along Y; turn it to X so the blade cuts across the board.
                Transform::from_translation(down - Vec3::X * kerf / 2.0)
                    .with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
                Pickable::IGNORE,
            ));
            head.spawn((
                Mesh3d(arm),
                MeshMaterial3d(body.clone()),
                Transform::from_translation(down / 2.0 + Vec3::X * inches(1.0))
                    .with_rotation(Quat::from_rotation_x(arm_tilt)),
                Pickable::IGNORE,
            ));
            // The motor, a drum beside the blade on the stop side, with a dark end cap.
            head.spawn((
                Mesh3d(motor.clone()),
                MeshMaterial3d(body.clone()),
                Transform::from_translation(down + Vec3::new(inches(2.6), inches(2.0), 0.0))
                    .with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
                Pickable::IGNORE,
            ));
            head.spawn((
                Mesh3d(motor),
                MeshMaterial3d(dark.clone()),
                Transform::from_translation(down + Vec3::new(inches(5.0), inches(2.0), 0.0))
                    .with_rotation(Quat::from_rotation_z(FRAC_PI_2))
                    .with_scale(Vec3::new(0.85, 0.15, 0.85)),
                Pickable::IGNORE,
            ));
            // The handle, out front where a hand pulls the blade down.
            head.spawn((
                Mesh3d(handle),
                MeshMaterial3d(dark.clone()),
                Transform::from_translation(
                    down + Vec3::new(inches(0.5), inches(4.5), inches(4.0)),
                ),
                Pickable::IGNORE,
            ));
        });

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
            Mesh3d(meshes.add(rounded_box(Vec3::new(
                STOP_WIDTH,
                STOP_HEIGHT,
                STOP_FRONT - STOP_BACK,
            )))),
            MeshMaterial3d(stop_color.clone()),
            Transform::default(),
            StopBlock { grab_offset: 0.0 },
        ))
        .observe(set_material_on::<PointerOver>(stop_hover))
        .observe(set_material_on::<PointerOut>(stop_color))
        .observe(grab_stop)
        .observe(drag_stop);

    for part in [Part::Whole, Part::Piece, Part::Offcut] {
        // Each part gets its own mesh, rebuilt as the board is cut.
        let mesh = meshes.add(rounded_box(Vec3::splat(0.01)));
        commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(wood.clone()),
                Transform::default(),
                Visibility::Hidden,
                part,
                AtMiterSaw,
                Shape::default(),
            ))
            .observe(highlight_takeable)
            .observe(set_material_on::<PointerOut>(wood.clone()))
            .observe(saw_part_clicked);
    }
}

/// Q and E turn the board against the stop a quarter turn left and right, and F flips it over.
fn turn_with_keys(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<MiterSawStation>) {
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

fn set_aside_with_key(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<MiterSawStation>) {
    if keys.just_pressed(KeyCode::KeyS) {
        station.set_aside();
    }
}

/// T carries the board against the stop to the table saw: the uncut board, or the measured
/// piece of the last cut.
fn carry_with_key(
    keys: Res<ButtonInput<KeyCode>>,
    mut miter_saw: ResMut<MiterSawStation>,
    mut table_saw: ResMut<TableSawStation>,
    mut view: ResMut<ShopView>,
) {
    if keys.just_pressed(KeyCode::KeyT)
        && let Some(board) = miter_saw
            .take(Part::Whole)
            .or_else(|| miter_saw.take(Part::Piece))
    {
        carry_to_table_saw(board, &mut table_saw, &mut miter_saw, &mut view);
    }
}

/// Puts `board` on the table saw. Whatever was on the table saw goes on the pile. Up close, the
/// camera follows the board; in the overview, it stays put.
fn carry_to_table_saw(
    board: Board,
    table_saw: &mut TableSawStation,
    miter_saw: &mut MiterSawStation,
    view: &mut ShopView,
) {
    miter_saw.parts.extend(table_saw.load(board));
    if *view != ShopView::Overview {
        *view = ShopView::TableSaw;
    }
}

/// Lights up a board on the saw under the pointer when it can be handled.
fn highlight_takeable(
    over: On<PointerOver>,
    parts: Query<&Part>,
    station: Res<MiterSawStation>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    if let Ok(part) = parts.get(over.entity)
        && station.can_take(*part)
        && let Ok(mut material) = materials.get_mut(over.entity)
    {
        material.0 = shop.wood_highlight.clone();
    }
}

/// Click a cut piece to slide it back against the stop; right-click any board on the saw to
/// carry it to the table saw.
fn saw_part_clicked(
    click: On<PointerClick>,
    parts: Query<&Part>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut miter_saw: ResMut<MiterSawStation>,
    mut table_saw: ResMut<TableSawStation>,
    mut view: ResMut<ShopView>,
) {
    let Ok(&part) = parts.get(click.entity) else {
        return;
    };
    let handled = match click.button {
        PointerButton::Primary => miter_saw.pick_up(part),
        PointerButton::Secondary => match miter_saw.take(part) {
            Some(board) => {
                carry_to_table_saw(board, &mut table_saw, &mut miter_saw, &mut view);
                true
            }
            None => false,
        },
        _ => false,
    };
    if handled && let Ok(mut material) = materials.get_mut(click.entity) {
        material.0 = shop.wood.clone();
    }
}

fn move_stop_with_keys(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<MiterSawStation>) {
    let step = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        STOP_BIG_STEP
    } else {
        STOP_STEP
    };
    let stop = station.stop;
    if keys.just_pressed(KeyCode::ArrowRight) {
        station.set_stop(stop + step);
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        station.set_stop(stop - step);
    }
}

/// Holding Space at this station brings the blade down; letting go lets it spring back up.
fn work_blade(
    keys: Res<ButtonInput<KeyCode>>,
    view: Res<ShopView>,
    time: Res<Time>,
    mut station: ResMut<MiterSawStation>,
) {
    let pressing = *view == ShopView::MiterSaw && keys.pressed(KeyCode::Space);
    if pressing && station.lowered < station.lowest() {
        station.lower(LOWER_SPEED * time.delta_secs());
    } else if !pressing && station.lowered > 0.0 {
        station.raise(RAISE_SPEED * time.delta_secs());
    }
}

/// Remembers where on the stop block the pointer grabbed it, so it doesn't jump to the cursor.
fn grab_stop(
    drag: On<PointerDragStart>,
    camera: Single<(&Camera, &GlobalTransform)>,
    mut stops: Query<&mut StopBlock>,
    station: Res<MiterSawStation>,
) {
    let (camera, camera_transform) = *camera;
    if drag.button == PointerButton::Primary
        && let Some(point) = pointer_on_plane(
            camera,
            camera_transform,
            drag.pointer.position,
            ORIGIN.y + STOP_HEIGHT,
        )
        && let Ok(mut stop) = stops.get_mut(drag.entity)
    {
        stop.grab_offset = point.x - ORIGIN.x - station.stop.as_meters_f32();
    }
}

fn drag_stop(
    drag: On<PointerDrag>,
    camera: Single<(&Camera, &GlobalTransform)>,
    stops: Query<&StopBlock>,
    mut station: ResMut<MiterSawStation>,
) {
    let (camera, camera_transform) = *camera;
    if drag.button == PointerButton::Primary
        && let Some(point) = pointer_on_plane(
            camera,
            camera_transform,
            drag.pointer.position,
            ORIGIN.y + STOP_HEIGHT,
        )
        && let Ok(stop) = stops.get(drag.entity)
    {
        station.set_stop(snap_to_sixteenth(point.x - ORIGIN.x - stop.grab_offset));
    }
}

fn draw_stop(station: Res<MiterSawStation>, mut stop: Single<&mut Transform, With<StopBlock>>) {
    if station.is_changed() {
        let left = station.stop.as_meters_f32();
        stop.translation = ORIGIN + Vec3::new(left, 0.0, STOP_BACK);
    }
}

/// Swings the arm up from all the way down, so the blade's center is where the saw says it is
/// for the height of its lowest point.
fn draw_head(station: Res<MiterSawStation>, mut head: Single<&mut Transform, With<Head>>) {
    if station.is_changed() {
        let saw = station.saw;
        // The angle of the arm, about the pivot.
        let angle = |arm: Vec3| arm.y.atan2(arm.z);
        let down = angle(blade_from_pivot(&saw, saw.stroke.1));
        let now = angle(blade_from_pivot(&saw, station.bottom()));
        head.rotation = Quat::from_rotation_x(down - now);
    }
}

/// [ and ] swing the miter 1° away from the stop and toward it; with Shift, to the next place
/// the table clicks into.
fn swing_with_keys(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<MiterSawStation>) {
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    for (key, toward_stop) in [(KeyCode::BracketLeft, false), (KeyCode::BracketRight, true)] {
        if !keys.just_pressed(key) {
            continue;
        }
        if shift {
            station.swing_to_next_stop(toward_stop);
        } else {
            let step = if toward_stop { MITER_STEP } else { -MITER_STEP };
            let miter = station.saw.miter + step;
            station.set_miter(miter);
        }
    }
}

/// Turns the table, with the arm and blade on it, to the miter angle.
fn draw_table(station: Res<MiterSawStation>, mut table: Single<&mut Transform, With<Turntable>>) {
    if station.is_changed() {
        table.rotation = Quat::from_rotation_y(station.saw.miter.radians() as f32);
    }
}

/// Up and Down raise and lower the depth stop. Lowered past the table, it's off.
fn set_depth_stop_with_keys(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<MiterSawStation>) {
    let step = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        DEPTH_BIG_STEP
    } else {
        DEPTH_STEP
    };
    if keys.just_pressed(KeyCode::ArrowUp) {
        let stop = station.depth_stop.map_or(Length::ZERO, |stop| stop + step);
        station.set_depth_stop(Some(stop));
    }
    if keys.just_pressed(KeyCode::ArrowDown)
        && let Some(stop) = station.depth_stop
    {
        let lower = stop - step;
        station.set_depth_stop((lower >= Length::ZERO).then_some(lower));
    }
}

fn draw_board(
    station: Res<MiterSawStation>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut parts: Query<
        (&Part, &Mesh3d, &mut Transform, &mut Visibility, &mut Shape),
        With<AtMiterSaw>,
    >,
) {
    if station.is_changed() {
        let boards = station.boards();
        place_parts(&boards, ORIGIN, board_turn(), &mut meshes, &mut parts);
    }
}

/// Keeps one board on the pile for each part set aside.
fn draw_pile(
    mut commands: Commands,
    station: Res<MiterSawStation>,
    shop: Res<ShopAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut pile: Query<(Entity, &PilePart, &Mesh3d, &mut Transform, &mut Shape)>,
) {
    if !station.is_changed() {
        return;
    }
    let pile_parts = station.pile();
    let place = |placed: &Placed| {
        Transform::from_translation(ORIGIN + board_turn() * placed.corner)
            .with_rotation(board_turn() * placed.rotation)
    };
    let mut drawn = 0;
    for (entity, part, mesh, mut transform, mut shape) in &mut pile {
        match pile_parts.get(part.0) {
            Some(placed) => {
                *transform = place(placed);
                shape.update(placed, mesh, &mut meshes);
                drawn += 1;
            }
            None => commands.entity(entity).despawn(),
        }
    }
    for (index, placed) in pile_parts.iter().enumerate().skip(drawn) {
        commands
            .spawn((
                Mesh3d(meshes.add(lumber_mesh(&placed.board, placed.grain))),
                MeshMaterial3d(shop.wood.clone()),
                place(placed),
                PilePart(index),
                Shape::built(placed),
            ))
            .observe(highlight_on_pile)
            .observe(set_material_on::<PointerOut>(shop.wood.clone()))
            .observe(pile_part_clicked);
    }
}

/// Lights up a part on the pile under the pointer when it can go back on the saw.
fn highlight_on_pile(
    over: On<PointerOver>,
    station: Res<MiterSawStation>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    if station.can_take_from_pile()
        && let Ok(mut material) = materials.get_mut(over.entity)
    {
        material.0 = shop.wood_highlight.clone();
    }
}

/// Click a part on the pile to put it back on the miter saw; right-click it to carry it to the
/// table saw.
fn pile_part_clicked(
    click: On<PointerClick>,
    pile: Query<&PilePart>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut miter_saw: ResMut<MiterSawStation>,
    mut table_saw: ResMut<TableSawStation>,
    mut view: ResMut<ShopView>,
) {
    let Ok(part) = pile.get(click.entity) else {
        return;
    };
    let handled = match click.button {
        PointerButton::Primary => miter_saw.take_from_pile(part.0),
        PointerButton::Secondary => match miter_saw.remove_from_pile(part.0) {
            Some(board) => {
                carry_to_table_saw(board, &mut table_saw, &mut miter_saw, &mut view);
                true
            }
            None => false,
        },
        _ => false,
    };
    // The box clicked will show a different part once the pile shifts, so drop its highlight.
    if handled && let Ok(mut material) = materials.get_mut(click.entity) {
        material.0 = shop.wood.clone();
    }
}

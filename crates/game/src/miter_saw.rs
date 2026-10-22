//! The miter saw station: a saw built into a bench, its stop block, the board, and the controls.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::{picking::pointer::PointerButton, prelude::*};
use woodsim::{Board, Length};

use crate::{
    chop::MiterSawStation,
    feed::TableSawStation,
    shop::{
        ActiveStation, Block, Part, ShopAssets, block_transform, inches, place_parts,
        pointer_on_plane, set_material_on, snap_to_sixteenth,
    },
};

/// Where the station's coordinates start in the shop: the blade's stop-side face, at the fence
/// face, level with the saw table. It sits to the left of the table saw.
pub const ORIGIN: Vec3 = Vec3::new(-1.6, inches(36.0), -0.1);

pub const HELP: &str = "Left/Right: move the stop 1/16\"   Shift+Left/Right: 1\"   \
                        Or drag the stop block\n\
                        Hold Space: lower the blade   \
                        Click a cut piece: slide it back against the stop\n\
                        Click a part on the pile: put it back on the saw   \
                        S: set everything on the saw aside\n\
                        Right-click a piece or press T: carry it to the table saw   \
                        Tab: back to the table saw";

/// A 12" blade.
const BLADE_RADIUS: f32 = inches(6.0);
const FENCE_HEIGHT: f32 = inches(3.0);
const BENCH_HALF_LENGTH: f32 = inches(40.0);
const STOP_HEIGHT: f32 = inches(3.5);
/// The stop block reaches from behind the fence to this far in front of it.
const STOP_BACK: f32 = inches(-1.25);
const STOP_FRONT: f32 = inches(2.5);
const STOP_WIDTH: f32 = inches(1.5);
/// The arm pivots on a post behind the fence and swings the blade down in front of it.
const PIVOT: Vec3 = Vec3::new(0.0, inches(6.0), inches(-4.0));
/// The blade's center relative to the pivot, with the arm all the way down.
const BLADE_FROM_PIVOT: Vec3 = Vec3::new(0.0, inches(-3.0), inches(7.6));
/// How far the arm swings up from the bottom of its travel.
const RAISE_ANGLE: f32 = 40.0 * PI / 180.0;
/// Fractions of the blade's full travel per second.
const LOWER_SPEED: f32 = 1.0 / 0.6;
const RAISE_SPEED: f32 = 1.0 / 0.4;

const STOP_STEP: Length = Length::from_inch_fraction(1, 16);
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
                        (move_stop_with_keys, set_aside_with_key, carry_with_key)
                            .run_if(resource_equals(ActiveStation::MiterSaw)),
                        work_blade,
                    ),
                    (draw_stop, draw_head, draw_board, draw_pile),
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

/// The arm, motor, and blade that swing down together.
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
    let cube = shop.cube.clone();
    let bench = materials.add(Color::srgb(0.5, 0.38, 0.25));
    let metal = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.62, 0.65),
        metallic: 0.8,
        perceptual_roughness: 0.45,
        ..default()
    });
    let body = materials.add(Color::srgb(0.15, 0.35, 0.6));
    let wood = shop.wood.clone();

    let mut spawn_box = |material: &Handle<StandardMaterial>, min: Vec3, max: Vec3| {
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(material.clone()),
            block_transform(Block { min, max }, ORIGIN),
            Pickable::IGNORE,
        ));
    };
    // The bench top sits a hair below the saw table so the two don't flicker against each other.
    spawn_box(
        &bench,
        Vec3::new(-BENCH_HALF_LENGTH, -ORIGIN.y, inches(-8.0)),
        Vec3::new(BENCH_HALF_LENGTH, inches(-0.05), inches(14.0)),
    );
    spawn_box(
        &metal,
        Vec3::new(inches(-7.0), inches(-1.0), inches(-6.0)),
        Vec3::new(inches(7.0), 0.0, inches(10.0)),
    );
    // The fence, split around the blade.
    for (left, right) in [
        (-BENCH_HALF_LENGTH + inches(2.0), inches(-0.75)),
        (inches(0.75), BENCH_HALF_LENGTH - inches(2.0)),
    ] {
        spawn_box(
            &metal,
            Vec3::new(left, 0.0, inches(-1.0)),
            Vec3::new(right, FENCE_HEIGHT, 0.0),
        );
    }
    // The post the arm pivots on.
    spawn_box(
        &body,
        Vec3::new(inches(-1.5), 0.0, inches(-6.0)),
        Vec3::new(inches(1.5), PIVOT.y + inches(1.0), inches(-2.5)),
    );

    let kerf = station.saw.blade.kerf.as_meters_f32();
    let arm_length = BLADE_FROM_PIVOT.length();
    let arm_tilt = (-BLADE_FROM_PIVOT.y).atan2(BLADE_FROM_PIVOT.z);
    commands
        .spawn((
            Transform::from_translation(ORIGIN + PIVOT),
            Visibility::default(),
            Head,
        ))
        .with_children(|head| {
            head.spawn((
                Mesh3d(meshes.add(Cylinder::new(BLADE_RADIUS, kerf))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.75, 0.75, 0.78),
                    metallic: 1.0,
                    perceptual_roughness: 0.3,
                    ..default()
                })),
                // A cylinder's axis starts along Y; turn it to X so the blade cuts across the board.
                Transform::from_translation(BLADE_FROM_PIVOT - Vec3::X * kerf / 2.0)
                    .with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
                Pickable::IGNORE,
            ));
            head.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(body.clone()),
                Transform::from_translation(BLADE_FROM_PIVOT / 2.0 + Vec3::X * inches(1.0))
                    .with_rotation(Quat::from_rotation_x(arm_tilt))
                    .with_scale(Vec3::new(inches(1.5), inches(1.5), arm_length)),
                Pickable::IGNORE,
            ));
            // The motor, beside the blade on the stop side.
            head.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(body.clone()),
                Transform::from_translation(
                    BLADE_FROM_PIVOT + Vec3::new(inches(2.25), inches(2.0), 0.0),
                )
                .with_scale(Vec3::new(inches(3.5), inches(3.5), inches(4.5))),
                Pickable::IGNORE,
            ));
            head.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(body.clone()),
                Transform::from_translation(
                    BLADE_FROM_PIVOT + Vec3::new(0.0, inches(4.5), inches(4.0)),
                )
                .with_scale(Vec3::new(inches(1.2), inches(1.2), inches(4.0))),
                Pickable::IGNORE,
            ));
        });

    let stop_color = materials.add(Color::srgb(0.95, 0.72, 0.1));
    let stop_hover = materials.add(Color::srgb(1.0, 0.85, 0.4));
    commands
        .spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(stop_color.clone()),
            Transform::default(),
            StopBlock { grab_offset: 0.0 },
        ))
        .observe(set_material_on::<PointerOver>(stop_hover))
        .observe(set_material_on::<PointerOut>(stop_color))
        .observe(grab_stop)
        .observe(drag_stop);

    for part in [Part::Uncut, Part::Piece, Part::Offcut] {
        commands
            .spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(wood.clone()),
                Transform::default(),
                Visibility::Hidden,
                part,
                AtMiterSaw,
            ))
            .observe(highlight_takeable)
            .observe(set_material_on::<PointerOut>(wood.clone()))
            .observe(saw_part_clicked);
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
    mut active: ResMut<ActiveStation>,
) {
    if keys.just_pressed(KeyCode::KeyT)
        && let Some(board) = miter_saw
            .take(Part::Uncut)
            .or_else(|| miter_saw.take(Part::Piece))
    {
        carry_to_table_saw(board, &mut table_saw, &mut miter_saw, &mut active);
    }
}

/// Puts `board` on the table saw and goes there. Whatever was on the table saw goes on the pile.
fn carry_to_table_saw(
    board: Board,
    table_saw: &mut TableSawStation,
    miter_saw: &mut MiterSawStation,
    active: &mut ActiveStation,
) {
    miter_saw.parts.extend(table_saw.load(board));
    *active = ActiveStation::TableSaw;
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
    mut active: ResMut<ActiveStation>,
) {
    let Ok(&part) = parts.get(click.entity) else {
        return;
    };
    let handled = match click.button {
        PointerButton::Primary => miter_saw.pick_up(part),
        PointerButton::Secondary => match miter_saw.take(part) {
            Some(board) => {
                carry_to_table_saw(board, &mut table_saw, &mut miter_saw, &mut active);
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
    active: Res<ActiveStation>,
    time: Res<Time>,
    mut station: ResMut<MiterSawStation>,
) {
    let pressing = *active == ActiveStation::MiterSaw && keys.pressed(KeyCode::Space);
    if pressing && station.lowered < 1.0 {
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
        let block = Block {
            min: Vec3::new(left, 0.0, STOP_BACK),
            max: Vec3::new(left + STOP_WIDTH, STOP_HEIGHT, STOP_FRONT),
        };
        **stop = block_transform(block, ORIGIN);
    }
}

fn draw_head(station: Res<MiterSawStation>, mut head: Single<&mut Transform, With<Head>>) {
    if station.is_changed() {
        head.rotation = Quat::from_rotation_x(-RAISE_ANGLE * (1.0 - station.lowered));
    }
}

fn draw_board(
    station: Res<MiterSawStation>,
    mut parts: Query<(&Part, &mut Transform, &mut Visibility), With<AtMiterSaw>>,
) {
    if station.is_changed() {
        place_parts(&station.board_blocks(), ORIGIN, &mut parts);
    }
}

/// Keeps one box on the pile for each part set aside.
fn draw_pile(
    mut commands: Commands,
    station: Res<MiterSawStation>,
    shop: Res<ShopAssets>,
    mut pile: Query<(Entity, &PilePart, &mut Transform)>,
) {
    if !station.is_changed() {
        return;
    }
    let blocks = station.pile_blocks();
    let mut drawn = 0;
    for (entity, part, mut transform) in &mut pile {
        match blocks.get(part.0) {
            Some(block) => {
                *transform = block_transform(*block, ORIGIN);
                drawn += 1;
            }
            None => commands.entity(entity).despawn(),
        }
    }
    for (index, block) in blocks.iter().enumerate().skip(drawn) {
        commands
            .spawn((
                Mesh3d(shop.cube.clone()),
                MeshMaterial3d(shop.wood.clone()),
                block_transform(*block, ORIGIN),
                PilePart(index),
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
    mut active: ResMut<ActiveStation>,
) {
    let Ok(part) = pile.get(click.entity) else {
        return;
    };
    let handled = match click.button {
        PointerButton::Primary => miter_saw.take_from_pile(part.0),
        PointerButton::Secondary => match miter_saw.remove_from_pile(part.0) {
            Some(board) => {
                carry_to_table_saw(board, &mut table_saw, &mut miter_saw, &mut active);
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

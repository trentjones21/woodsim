//! The table saw station: the saw, its fence, the board, the controls, and the readout.

use std::f32::consts::FRAC_PI_2;

use bevy::{picking::pointer::PointerButton, prelude::*};
use woodsim::Length;

use crate::feed::{BLADE_DROP, BLADE_RADIUS, Bench, Block, inches};

const TABLE_TOP: f32 = inches(34.0);
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
const FENCE_Z: f32 = TABLE_BACK + FENCE_LENGTH / 2.0;

const FENCE_STEP: Length = Length::from_inch_fraction(1, 16);
const FENCE_BIG_STEP: Length = Length::from_inches(1);
/// How fast the board moves while Space is held: 10" a second.
const FEED_SPEED: f32 = inches(10.0);

pub struct TableSawPlugin;

impl Plugin for TableSawPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Bench>()
            .add_systems(Startup, (spawn_saw, spawn_readout))
            .add_systems(
                Update,
                (
                    (move_fence_with_keys, push_board, new_board),
                    (draw_fence, draw_board, update_readout),
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

#[derive(Component, Clone, Copy)]
enum BoardPart {
    Uncut,
    Piece,
    Offcut,
}

#[derive(Component)]
struct FenceReading;

#[derive(Component)]
struct CutStatus;

fn spawn_saw(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    bench: Res<Bench>,
) {
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let cast_iron = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.62, 0.65),
        metallic: 0.8,
        perceptual_roughness: 0.45,
        ..default()
    });
    let cabinet = materials.add(Color::srgb(0.2, 0.3, 0.45));
    let outfeed = materials.add(Color::srgb(0.32, 0.32, 0.34));
    let wood = materials.add(Color::srgb(0.87, 0.72, 0.53));

    let mut spawn_box = |material: &Handle<StandardMaterial>, min: Vec3, max: Vec3| {
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(material.clone()),
            block_transform(Block { min, max }),
            Pickable::IGNORE,
        ));
    };
    spawn_box(
        &cast_iron,
        Vec3::new(TABLE_LEFT, inches(-1.5), TABLE_BACK),
        Vec3::new(TABLE_RIGHT, 0.0, TABLE_FRONT),
    );
    // Front rail, which the fence rides on.
    spawn_box(
        &cast_iron,
        Vec3::new(TABLE_LEFT, inches(-2.5), TABLE_FRONT),
        Vec3::new(TABLE_RIGHT, 0.0, TABLE_FRONT + inches(2.0)),
    );
    spawn_box(
        &cabinet,
        Vec3::new(inches(-10.0), -TABLE_TOP, inches(-10.0)),
        Vec3::new(inches(10.0), inches(-1.5), inches(10.0)),
    );
    // Outfeed table and the panel holding up its far end.
    spawn_box(
        &outfeed,
        Vec3::new(inches(-10.0), inches(-0.75), OUTFEED_END),
        Vec3::new(inches(20.0), 0.0, TABLE_BACK),
    );
    spawn_box(
        &outfeed,
        Vec3::new(inches(-9.0), -TABLE_TOP, OUTFEED_END),
        Vec3::new(inches(19.0), inches(-0.75), OUTFEED_END + inches(0.75)),
    );

    let kerf = bench.saw.blade.kerf.as_meters_f32();
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(BLADE_RADIUS, kerf))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.75, 0.75, 0.78),
            metallic: 1.0,
            perceptual_roughness: 0.3,
            ..default()
        })),
        // A cylinder's axis starts along Y; turn it to X so the blade spins toward the operator.
        Transform::from_xyz(-kerf / 2.0, TABLE_TOP - BLADE_DROP, 0.0)
            .with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
        Pickable::IGNORE,
    ));

    let fence_color = materials.add(Color::srgb(0.8, 0.15, 0.12));
    let fence_hover = materials.add(Color::srgb(0.95, 0.4, 0.3));
    commands
        .spawn((
            Mesh3d(meshes.add(Cuboid::new(FENCE_WIDTH, FENCE_HEIGHT, FENCE_LENGTH))),
            MeshMaterial3d(fence_color.clone()),
            Transform::default(),
            Fence { grab_offset: 0.0 },
        ))
        .observe(set_material_on::<PointerOver>(fence_hover))
        .observe(set_material_on::<PointerOut>(fence_color))
        .observe(grab_fence)
        .observe(drag_fence);

    for part in [BoardPart::Uncut, BoardPart::Piece, BoardPart::Offcut] {
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(wood.clone()),
            Transform::default(),
            Visibility::Hidden,
            Pickable::IGNORE,
            part,
        ));
    }
}

fn spawn_readout(mut commands: Commands) {
    let panel = |top_or_bottom: Node| Node {
        position_type: PositionType::Absolute,
        left: px(12),
        padding: UiRect::axes(px(10), px(6)),
        ..top_or_bottom
    };
    let backdrop = BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6));
    commands.spawn((
        Text::default(),
        TextFont {
            font_size: FontSize::Px(36.0),
            ..default()
        },
        panel(Node {
            top: px(12),
            ..default()
        }),
        backdrop,
        FenceReading,
    ));
    commands.spawn((
        Text::default(),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        panel(Node {
            top: px(76),
            ..default()
        }),
        backdrop,
        CutStatus,
    ));
    commands.spawn((
        Text::new(
            "Left/Right: move the fence 1/16\"   Shift+Left/Right: 1\"   Or drag the fence\n\
             Hold Space: push the board   R: new board",
        ),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        panel(Node {
            bottom: px(12),
            ..default()
        }),
        backdrop,
    ));
}

fn move_fence_with_keys(keys: Res<ButtonInput<KeyCode>>, mut bench: ResMut<Bench>) {
    let step = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        FENCE_BIG_STEP
    } else {
        FENCE_STEP
    };
    let fence = bench.saw.fence;
    if keys.just_pressed(KeyCode::ArrowRight) {
        bench.set_fence(fence + step);
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        bench.set_fence(fence - step);
    }
}

/// Remembers where on the fence the pointer grabbed it, so the fence doesn't jump to the cursor.
fn grab_fence(
    drag: On<PointerDragStart>,
    camera: Single<(&Camera, &GlobalTransform)>,
    mut fences: Query<&mut Fence>,
    bench: Res<Bench>,
) {
    let (camera, camera_transform) = *camera;
    if drag.button == PointerButton::Primary
        && let Some(x) = pointer_x(camera, camera_transform, drag.pointer.position)
        && let Ok(mut fence) = fences.get_mut(drag.entity)
    {
        fence.grab_offset = x - bench.saw.fence.as_meters_f32();
    }
}

fn drag_fence(
    drag: On<PointerDrag>,
    camera: Single<(&Camera, &GlobalTransform)>,
    fences: Query<&Fence>,
    mut bench: ResMut<Bench>,
) {
    let (camera, camera_transform) = *camera;
    if drag.button == PointerButton::Primary
        && let Some(x) = pointer_x(camera, camera_transform, drag.pointer.position)
        && let Ok(fence) = fences.get(drag.entity)
    {
        bench.set_fence(snap_to_sixteenth(x - fence.grab_offset));
    }
}

/// Where the pointer is across the table (x, in meters), on a level plane at the fence's top.
fn pointer_x(camera: &Camera, camera_transform: &GlobalTransform, position: Vec2) -> Option<f32> {
    let ray = camera.viewport_to_world(camera_transform, position).ok()?;
    let fence_top = Vec3::Y * (TABLE_TOP + FENCE_HEIGHT);
    let point = ray.plane_intersection_point(fence_top, InfinitePlane3d::new(Vec3::Y))?;
    Some(point.x)
}

/// Rounds a fence position in meters to the nearest 1/16", the finest mark on the fence scale.
fn snap_to_sixteenth(meters: f32) -> Length {
    let sixteenths = (meters / inches(1.0) * 16.0).round() as i64;
    Length::from_inch_fraction(sixteenths, 16)
}

fn push_board(keys: Res<ButtonInput<KeyCode>>, time: Res<Time>, mut bench: ResMut<Bench>) {
    if keys.pressed(KeyCode::Space) && !bench.is_through() {
        bench.push(FEED_SPEED * time.delta_secs());
    }
}

fn new_board(keys: Res<ButtonInput<KeyCode>>, mut bench: ResMut<Bench>) {
    if keys.just_pressed(KeyCode::KeyR) {
        bench.new_board();
    }
}

fn draw_fence(bench: Res<Bench>, mut fence: Single<&mut Transform, With<Fence>>) {
    if bench.is_changed() {
        let x = bench.saw.fence.as_meters_f32() + FENCE_WIDTH / 2.0;
        fence.translation = Vec3::new(x, TABLE_TOP + FENCE_HEIGHT / 2.0, FENCE_Z);
    }
}

fn draw_board(bench: Res<Bench>, mut parts: Query<(&BoardPart, &mut Transform, &mut Visibility)>) {
    if !bench.is_changed() {
        return;
    }
    let blocks = bench.board_blocks();
    for (part, mut transform, mut visibility) in &mut parts {
        let block = match part {
            BoardPart::Uncut => blocks.uncut,
            BoardPart::Piece => blocks.piece,
            BoardPart::Offcut => blocks.offcut,
        };
        match block {
            Some(block) => {
                *transform = block_transform(block);
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

fn update_readout(
    bench: Res<Bench>,
    mut fence_reading: Single<&mut Text, With<FenceReading>>,
    mut status: Single<&mut Text, (With<CutStatus>, Without<FenceReading>)>,
) {
    if bench.is_changed() {
        fence_reading.0 = format!("Fence: {}", bench.saw.fence);
        status.0 = bench.status();
    }
}

/// Places a unit cube so it fills `block`, which is in table space.
fn block_transform(block: Block) -> Transform {
    let center = (block.min + block.max) / 2.0 + Vec3::Y * TABLE_TOP;
    Transform::from_translation(center).with_scale(block.max - block.min)
}

/// Returns an observer that swaps the entity's material when event `E` happens to it.
fn set_material_on<E: EntityEvent>(
    material: Handle<StandardMaterial>,
) -> impl Fn(On<E>, Query<&mut MeshMaterial3d<StandardMaterial>>) {
    move |event, mut materials| {
        if let Ok(mut current) = materials.get_mut(event.event_target()) {
            current.0 = material.clone();
        }
    }
}

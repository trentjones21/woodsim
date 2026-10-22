//! The table saw station: the saw, its fence, the board, and the controls.

use std::f32::consts::FRAC_PI_2;

use bevy::{picking::pointer::PointerButton, prelude::*};
use woodsim::Length;

use crate::{
    chop::MiterSawStation,
    feed::{BLADE_DROP, BLADE_RADIUS, TableSawStation},
    shop::{
        ActiveStation, Block, Part, ShopAssets, block_transform, inches, place_parts,
        pointer_on_plane, set_material_on, snap_to_sixteenth,
    },
};

/// Where the station's coordinates start in the shop: the blade's fence-side face, at the
/// table top.
pub const ORIGIN: Vec3 = Vec3::new(0.0, inches(34.0), 0.0);

pub const HELP: &str = "Left/Right: move the fence 1/16\"   Shift+Left/Right: 1\"   \
                        Or drag the fence\n\
                        Hold Space: push the board   R: new board\n\
                        Click a cut piece or press C: carry it to the miter saw   \
                        Tab: go to the miter saw";

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
        app.init_resource::<TableSawStation>()
            .add_systems(Startup, spawn_saw)
            .add_systems(
                Update,
                (
                    (move_fence_with_keys, push_board, new_board, carry_with_key)
                        .run_if(resource_equals(ActiveStation::TableSaw)),
                    (draw_fence, draw_board),
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

fn spawn_saw(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    shop: Res<ShopAssets>,
    station: Res<TableSawStation>,
) {
    let cube = shop.cube.clone();
    let cast_iron = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.62, 0.65),
        metallic: 0.8,
        perceptual_roughness: 0.45,
        ..default()
    });
    let cabinet = materials.add(Color::srgb(0.2, 0.3, 0.45));
    let outfeed = materials.add(Color::srgb(0.32, 0.32, 0.34));
    let wood = shop.wood.clone();

    let mut spawn_box = |material: &Handle<StandardMaterial>, min: Vec3, max: Vec3| {
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(material.clone()),
            block_transform(Block { min, max }, ORIGIN),
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
        Vec3::new(inches(-10.0), -ORIGIN.y, inches(-10.0)),
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
        Vec3::new(inches(-9.0), -ORIGIN.y, OUTFEED_END),
        Vec3::new(inches(19.0), inches(-0.75), OUTFEED_END + inches(0.75)),
    );

    let kerf = station.saw.blade.kerf.as_meters_f32();
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(BLADE_RADIUS, kerf))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.75, 0.75, 0.78),
            metallic: 1.0,
            perceptual_roughness: 0.3,
            ..default()
        })),
        // A cylinder's axis starts along Y; turn it to X so the blade spins toward the operator.
        Transform::from_translation(ORIGIN + Vec3::new(-kerf / 2.0, -BLADE_DROP, 0.0))
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

    commands.spawn((
        Mesh3d(cube.clone()),
        MeshMaterial3d(wood.clone()),
        Transform::default(),
        Visibility::Hidden,
        Pickable::IGNORE,
        Part::Uncut,
        AtTableSaw,
    ));
    for part in [Part::Piece, Part::Offcut] {
        commands
            .spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(wood.clone()),
                Transform::default(),
                Visibility::Hidden,
                part,
                AtTableSaw,
            ))
            .observe(highlight_carryable)
            .observe(set_material_on::<PointerOut>(wood.clone()))
            .observe(carry_clicked);
    }
}

fn move_fence_with_keys(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<TableSawStation>) {
    let step = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        FENCE_BIG_STEP
    } else {
        FENCE_STEP
    };
    let fence = station.saw.fence;
    if keys.just_pressed(KeyCode::ArrowRight) {
        station.set_fence(fence + step);
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        station.set_fence(fence - step);
    }
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

fn push_board(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut station: ResMut<TableSawStation>,
) {
    if keys.pressed(KeyCode::Space) && !station.is_through() {
        station.push(FEED_SPEED * time.delta_secs());
    }
}

fn new_board(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<TableSawStation>) {
    if keys.just_pressed(KeyCode::KeyR) {
        station.new_board();
    }
}

/// Carries a piece off the outfeed side to the miter saw and goes there. Returns false if
/// that piece isn't there to take.
fn carry(
    part: Part,
    table_saw: &mut TableSawStation,
    miter_saw: &mut MiterSawStation,
    active: &mut ActiveStation,
) -> bool {
    let Some(board) = table_saw.take(part) else {
        return false;
    };
    miter_saw.load(board);
    *active = ActiveStation::MiterSaw;
    true
}

/// C carries the piece cut against the fence, or the offcut once that piece is gone.
fn carry_with_key(
    keys: Res<ButtonInput<KeyCode>>,
    mut table_saw: ResMut<TableSawStation>,
    mut miter_saw: ResMut<MiterSawStation>,
    mut active: ResMut<ActiveStation>,
) {
    if keys.just_pressed(KeyCode::KeyC)
        && !carry(Part::Piece, &mut table_saw, &mut miter_saw, &mut active)
    {
        carry(Part::Offcut, &mut table_saw, &mut miter_saw, &mut active);
    }
}

fn carry_clicked(
    click: On<PointerClick>,
    parts: Query<&Part>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut table_saw: ResMut<TableSawStation>,
    mut miter_saw: ResMut<MiterSawStation>,
    mut active: ResMut<ActiveStation>,
) {
    if click.button == PointerButton::Primary
        && let Ok(part) = parts.get(click.entity)
        && carry(*part, &mut table_saw, &mut miter_saw, &mut active)
        && let Ok(mut material) = materials.get_mut(click.entity)
    {
        material.0 = shop.wood.clone();
    }
}

/// Lights up a cut piece under the pointer when it can be carried off.
fn highlight_carryable(
    over: On<PointerOver>,
    station: Res<TableSawStation>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    if station.is_through()
        && let Ok(mut material) = materials.get_mut(over.entity)
    {
        material.0 = shop.wood_highlight.clone();
    }
}

fn draw_fence(station: Res<TableSawStation>, mut fence: Single<&mut Transform, With<Fence>>) {
    if station.is_changed() {
        let x = station.saw.fence.as_meters_f32() + FENCE_WIDTH / 2.0;
        fence.translation = ORIGIN + Vec3::new(x, FENCE_HEIGHT / 2.0, FENCE_Z);
    }
}

fn draw_board(
    station: Res<TableSawStation>,
    mut parts: Query<(&Part, &mut Transform, &mut Visibility), With<AtTableSaw>>,
) {
    if station.is_changed() {
        place_parts(&station.board_blocks(), ORIGIN, &mut parts);
    }
}

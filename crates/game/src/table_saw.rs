//! The table saw station: the saw, its fence, the board, and the controls.

use std::f32::consts::FRAC_PI_2;

use bevy::{picking::pointer::PointerButton, prelude::*};
use woodsim::Length;

use crate::{
    board_mesh::{Grain, board_mesh},
    chop::MiterSawStation,
    feed::{BLADE_DROP, BLADE_RADIUS, TableSawStation},
    shop::{
        Block, Part, ShopAssets, ShopView, block_transform, inches, place_parts, pointer_on_plane,
        set_material_on, snap_to_sixteenth, station_part,
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

pub const HELP: &str = "Left/Right: move the fence 1/16\"   Shift+Left/Right: 1\"   \
                        Or drag the fence\n\
                        Hold Space: push the board   R: new board   Esc: back to the shop\n\
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
                        .run_if(resource_equals(ShopView::TableSaw)),
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
    // Machined cast iron: satin steel that picks up the room's reflections.
    let cast_iron = materials.add(StandardMaterial {
        base_color: Color::srgb(0.46, 0.47, 0.49),
        metallic: 0.9,
        perceptual_roughness: 0.32,
        ..default()
    });
    // Powder-coated cabinet paint with a little sheen.
    let cabinet = materials.add(StandardMaterial {
        base_color: Color::srgb(0.13, 0.27, 0.22),
        perceptual_roughness: 0.5,
        clearcoat: 0.4,
        clearcoat_perceptual_roughness: 0.3,
        ..default()
    });
    let dark_metal = materials.add(StandardMaterial {
        base_color: Color::srgb(0.07, 0.07, 0.08),
        metallic: 0.6,
        perceptual_roughness: 0.4,
        ..default()
    });
    let safety_red = materials.add(StandardMaterial {
        base_color: Color::srgb(0.78, 0.12, 0.06),
        perceptual_roughness: 0.35,
        clearcoat: 0.6,
        ..default()
    });
    let tape_yellow = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.78, 0.18),
        perceptual_roughness: 0.4,
        ..default()
    });
    let wood = shop.wood.clone();

    let spawn_box =
        |commands: &mut Commands, material: &Handle<StandardMaterial>, min: Vec3, max: Vec3| {
            let entity = commands.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(material.clone()),
                block_transform(Block { min, max }, ORIGIN),
            ));
            station_part(entity, ShopView::TableSaw);
        };
    spawn_box(
        &mut commands,
        &cast_iron,
        Vec3::new(TABLE_LEFT, inches(-1.5), TABLE_BACK),
        Vec3::new(TABLE_RIGHT, 0.0, TABLE_FRONT),
    );
    // Miter slots: shallow channels either side of the blade, drawn as dark strips.
    for x in [-6.5, 5.5] {
        spawn_box(
            &mut commands,
            &dark_metal,
            Vec3::new(inches(x), inches(-0.2), TABLE_BACK),
            Vec3::new(inches(x + 0.75), inches(0.02), TABLE_FRONT),
        );
    }
    // The throat plate around the blade.
    spawn_box(
        &mut commands,
        &safety_red,
        Vec3::new(inches(-1.75), inches(-0.2), inches(-7.0)),
        Vec3::new(inches(1.75), inches(0.03), inches(7.0)),
    );
    // Front rail, which the fence rides on, with a measuring tape along its top.
    spawn_box(
        &mut commands,
        &dark_metal,
        Vec3::new(TABLE_LEFT, inches(-2.5), TABLE_FRONT),
        Vec3::new(TABLE_RIGHT, 0.0, TABLE_FRONT + inches(2.0)),
    );
    spawn_box(
        &mut commands,
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
        &safety_red,
        Vec3::new(inches(-12.0), inches(-5.0), TABLE_FRONT + inches(1.5)),
        Vec3::new(inches(-8.5), inches(-2.5), TABLE_FRONT + inches(3.0)),
    );
    // The cabinet on a dark plinth, with a dust port out the back.
    spawn_box(
        &mut commands,
        &cabinet,
        Vec3::new(inches(-10.0), inches(3.0) - ORIGIN.y, inches(-10.0)),
        Vec3::new(inches(10.0), inches(-1.5), inches(10.0)),
    );
    spawn_box(
        &mut commands,
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
        &cabinet,
        Vec3::new(inches(-9.0), -ORIGIN.y, OUTFEED_END),
        Vec3::new(inches(19.0), inches(-0.75), OUTFEED_END + inches(0.75)),
    );

    let kerf = station.saw.blade.kerf.as_meters_f32();
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(BLADE_RADIUS, kerf))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.82, 0.82, 0.85),
            metallic: 1.0,
            perceptual_roughness: 0.22,
            ..default()
        })),
        // A cylinder's axis starts along Y; turn it to X so the blade spins toward the operator.
        Transform::from_translation(ORIGIN + Vec3::new(-kerf / 2.0, -BLADE_DROP, 0.0))
            .with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
        Pickable::IGNORE,
    ));
    // The riving knife stands in the kerf just behind the blade, so pieces pass either side.
    commands.spawn((
        Mesh3d(cube.clone()),
        MeshMaterial3d(dark_metal.clone()),
        block_transform(
            Block {
                min: Vec3::new(-kerf * 0.8, 0.0, inches(-6.5)),
                max: Vec3::new(-kerf * 0.2, inches(2.25), inches(-3.0)),
            },
            ORIGIN,
        ),
        Pickable::IGNORE,
    ));

    let fence_color = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.2, 0.08),
        perceptual_roughness: 0.35,
        clearcoat: 0.6,
        ..default()
    });
    let fence_hover = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.45, 0.25),
        emissive: LinearRgba::new(1.0, 0.35, 0.1, 1.0) * 40.0,
        perceptual_roughness: 0.35,
        ..default()
    });
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

    // Each part gets its own mesh, reshaped as the board is cut.
    let mut part_mesh = || Mesh3d(meshes.add(board_mesh(Vec3::splat(0.01), Vec3::ZERO, Grain::Z)));
    commands.spawn((
        part_mesh(),
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
                part_mesh(),
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

/// Carries a piece off the outfeed side to the miter saw. Up close, the camera follows it; in
/// the overview, it stays put. Returns false if that piece isn't there to take.
fn carry(
    part: Part,
    table_saw: &mut TableSawStation,
    miter_saw: &mut MiterSawStation,
    view: &mut ShopView,
) -> bool {
    let Some(board) = table_saw.take(part) else {
        return false;
    };
    miter_saw.load(board);
    if *view != ShopView::Overview {
        *view = ShopView::MiterSaw;
    }
    true
}

/// C carries the piece cut against the fence, or the offcut once that piece is gone.
fn carry_with_key(
    keys: Res<ButtonInput<KeyCode>>,
    mut table_saw: ResMut<TableSawStation>,
    mut miter_saw: ResMut<MiterSawStation>,
    mut view: ResMut<ShopView>,
) {
    if keys.just_pressed(KeyCode::KeyC)
        && !carry(Part::Piece, &mut table_saw, &mut miter_saw, &mut view)
    {
        carry(Part::Offcut, &mut table_saw, &mut miter_saw, &mut view);
    }
}

fn carry_clicked(
    click: On<PointerClick>,
    parts: Query<&Part>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut table_saw: ResMut<TableSawStation>,
    mut miter_saw: ResMut<MiterSawStation>,
    mut view: ResMut<ShopView>,
) {
    if click.button == PointerButton::Primary
        && let Ok(part) = parts.get(click.entity)
        && carry(*part, &mut table_saw, &mut miter_saw, &mut view)
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
    mut meshes: ResMut<Assets<Mesh>>,
    mut parts: Query<(&Part, &Mesh3d, &mut Transform, &mut Visibility), With<AtTableSaw>>,
) {
    if station.is_changed() {
        let blocks = station.board_blocks();
        place_parts(&blocks, ORIGIN, Grain::Z, &mut meshes, &mut parts);
    }
}

//! The garage around the saws: walls that cut away on the camera's side like a dollhouse, the
//! light coming in, and the things a real shop collects. None of it affects the simulation.
//!
//! Sunlight only gets in through the window because an invisible shell (the full walls and a
//! roof) casts shadows the camera never sees: it's on render layer 1, which the sun sees and
//! the camera doesn't.

use bevy::{
    camera::visibility::RenderLayers,
    ecs::system::EntityCommands,
    light::{
        CascadeShadowConfigBuilder, DirectionalLightShadowMap, FogVolume, NotShadowCaster,
        VolumetricLight,
    },
    math::Affine2,
    prelude::*,
};

use crate::{
    board_mesh::{Grain, board_mesh},
    shop::{SHOP_HALF_SIZE, ShopAssets, ShopView},
    textures::PEGBOARD_TILE,
};

const WALL_HEIGHT: f32 = 2.4;
const WALL_THICKNESS: f32 = 0.12;
/// Walls on the camera's side drop to this height so the camera can see in.
const CUTAWAY_HEIGHT: f32 = 0.35;
/// The window in the back wall, in world x and y.
const WINDOW_X: (f32, f32) = (0.1, 1.5);
const WINDOW_Y: (f32, f32) = (1.0, 2.05);
/// Which way sunlight travels: in through the back window, down onto the floor by the saws.
const SUN_DIRECTION: Vec3 = Vec3::new(-0.25, -0.62, 0.74);
const SUN_LUX: f32 = 14_000.0;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.05, 0.047, 0.043)))
            .insert_resource(DirectionalLightShadowMap { size: 4096 })
            .add_systems(
                Startup,
                (spawn_walls, spawn_lights, spawn_furnishings, spawn_motes),
            )
            .add_systems(
                Update,
                (cut_away_walls, hide_overhead_in_overview, drift_motes),
            );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WallSide {
    Back,
    Front,
    Left,
    Right,
}

impl WallSide {
    fn outward(self) -> Vec3 {
        match self {
            WallSide::Back => Vec3::NEG_Z,
            WallSide::Front => Vec3::Z,
            WallSide::Left => Vec3::NEG_X,
            WallSide::Right => Vec3::X,
        }
    }
}

/// One version of a wall: full height, or cut away for when it's between the camera and the shop.
#[derive(Component)]
struct Wall {
    side: WallSide,
    cut_away: bool,
}

/// Something hung on a wall; it's hidden while that wall is cut away.
#[derive(Component)]
struct OnWall(WallSide);

/// Something hanging from the ceiling. It's hidden in the overview, where it would block the
/// view from above; its light still shines.
#[derive(Component)]
struct Overhead;

/// A fleck of sawdust hanging in the air. It sparkles where the sun catches it.
#[derive(Component)]
struct Mote {
    home: Vec3,
    phase: f32,
}

/// Spawns a box filling `min`..`max` in shop coordinates.
fn spawn_box<'a>(
    commands: &'a mut Commands,
    cube: &Handle<Mesh>,
    material: &Handle<StandardMaterial>,
    min: Vec3,
    max: Vec3,
) -> EntityCommands<'a> {
    commands.spawn((
        Mesh3d(cube.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_translation((min + max) / 2.0).with_scale(max - min),
        Pickable::IGNORE,
    ))
}

/// The walls' footprints at a given height, with the back wall split around the window when
/// `window` is set.
fn wall_boxes(height: f32, outset: f32, window: bool) -> Vec<(WallSide, Vec3, Vec3)> {
    let inner = SHOP_HALF_SIZE - outset;
    let outer = SHOP_HALF_SIZE + WALL_THICKNESS + outset;
    let mut boxes = vec![
        (
            WallSide::Front,
            Vec3::new(-outer, 0.0, inner),
            Vec3::new(outer, height, outer),
        ),
        (
            WallSide::Left,
            Vec3::new(-outer, 0.0, -inner),
            Vec3::new(-inner, height, inner),
        ),
        (
            WallSide::Right,
            Vec3::new(inner, 0.0, -inner),
            Vec3::new(outer, height, inner),
        ),
    ];
    if window {
        let (left, right) = WINDOW_X;
        let (sill, head) = WINDOW_Y;
        boxes.extend([
            (
                WallSide::Back,
                Vec3::new(-outer, 0.0, -outer),
                Vec3::new(left, height, -inner),
            ),
            (
                WallSide::Back,
                Vec3::new(right, 0.0, -outer),
                Vec3::new(outer, height, -inner),
            ),
            (
                WallSide::Back,
                Vec3::new(left, 0.0, -outer),
                Vec3::new(right, sill, -inner),
            ),
            (
                WallSide::Back,
                Vec3::new(left, head, -outer),
                Vec3::new(right, height, -inner),
            ),
        ]);
    } else {
        boxes.push((
            WallSide::Back,
            Vec3::new(-outer, 0.0, -outer),
            Vec3::new(outer, height, -inner),
        ));
    }
    boxes
}

fn spawn_walls(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    shop: Res<ShopAssets>,
) {
    let paint = materials.add(StandardMaterial {
        base_color: Color::srgb(0.82, 0.79, 0.74),
        perceptual_roughness: 0.9,
        ..default()
    });
    // The visible walls: each side full height and cut away, one shown at a time.
    for (cut_away, height, window) in [(false, WALL_HEIGHT, true), (true, CUTAWAY_HEIGHT, false)] {
        for (side, min, max) in wall_boxes(height, 0.0, window) {
            spawn_box(&mut commands, &shop.cube, &paint, min, max)
                .insert((Wall { side, cut_away }, NotShadowCaster));
        }
    }

    // The shadow shell, a hair larger than the walls so it doesn't shadow their faces.
    let shell = RenderLayers::layer(1);
    let mut shadow_box = |min: Vec3, max: Vec3| {
        spawn_box(&mut commands, &shop.cube, &paint, min, max).insert(shell.clone());
    };
    for (_, min, max) in wall_boxes(WALL_HEIGHT, 0.03, true) {
        shadow_box(min, max);
    }
    let roof = SHOP_HALF_SIZE + WALL_THICKNESS + 0.03;
    shadow_box(
        Vec3::new(-roof, WALL_HEIGHT, -roof),
        Vec3::new(roof, WALL_HEIGHT + 0.1, roof),
    );

    // Dark, unlit ground around the garage, so the lit shop stands out like a diorama.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(40.0, 40.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.045, 0.042, 0.038),
            unlit: true,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.005, 0.0),
        Pickable::IGNORE,
    ));
}

/// Shows each wall's full or cut-away version depending on which side of the shop the camera
/// is on, and hides whatever hangs on a cut-away wall.
fn cut_away_walls(
    camera: Single<&GlobalTransform, With<Camera3d>>,
    mut walls: Query<(&Wall, &mut Visibility)>,
    mut fixtures: Query<(&OnWall, &mut Visibility), Without<Wall>>,
) {
    let eye = camera.translation();
    let toward_camera = Vec3::new(eye.x, 0.0, eye.z).normalize_or_zero();
    let near = |side: WallSide| side.outward().dot(toward_camera) > 0.35;
    let shown = |visible: bool| {
        if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        }
    };
    for (wall, mut visibility) in &mut walls {
        visibility.set_if_neq(shown(near(wall.side) == wall.cut_away));
    }
    for (on_wall, mut visibility) in &mut fixtures {
        visibility.set_if_neq(shown(!near(on_wall.0)));
    }
}

fn hide_overhead_in_overview(
    view: Res<ShopView>,
    mut overhead: Query<&mut Visibility, With<Overhead>>,
) {
    if view.is_changed() {
        let shown = if *view == ShopView::Overview {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        for mut visibility in &mut overhead {
            *visibility = shown;
        }
    }
}

fn spawn_lights(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    shop: Res<ShopAssets>,
) {
    // Late-afternoon sun, slanting in through the back window into the dusty air.
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.86, 0.68),
            illuminance: SUN_LUX,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default().looking_to(SUN_DIRECTION.normalize(), Vec3::Y),
        CascadeShadowConfigBuilder {
            num_cascades: 2,
            first_cascade_far_bound: 6.0,
            maximum_distance: 20.0,
            ..default()
        }
        .build(),
        VolumetricLight,
        // The sun sees the shadow shell; the camera doesn't.
        RenderLayers::from_layers(&[0, 1]),
    ));
    commands.spawn((
        FogVolume {
            density_factor: 0.4,
            scattering: 0.6,
            absorption: 0.02,
            light_tint: Color::srgb(1.0, 0.92, 0.8),
            ..default()
        },
        Transform::from_xyz(0.0, WALL_HEIGHT / 2.0, 0.0).with_scale(Vec3::new(
            2.0 * SHOP_HALF_SIZE,
            WALL_HEIGHT,
            2.0 * SHOP_HALF_SIZE,
        )),
    ));

    // Two LED shop lights hanging over the saws.
    let housing = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.86, 0.87),
        metallic: 0.6,
        perceptual_roughness: 0.4,
        ..default()
    });
    let diffuser = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::new(1.0, 0.97, 0.92, 1.0) * 2500.0,
        ..default()
    });
    let chain = meshes.add(Cylinder::new(0.004, WALL_HEIGHT - 2.25));
    for (center, along_x) in [
        (Vec3::new(0.25, 2.25, -0.2), false),
        (Vec3::new(-1.6, 2.25, -0.05), true),
    ] {
        let size = if along_x {
            Vec3::new(1.2, 0.05, 0.16)
        } else {
            Vec3::new(0.16, 0.05, 1.2)
        };
        spawn_box(
            &mut commands,
            &shop.cube,
            &housing,
            center - size / 2.0,
            center + size / 2.0,
        )
        .insert((NotShadowCaster, Overhead));
        let lens = size * Vec3::new(0.85, 0.3, 0.85);
        let lens_center = center - Vec3::Y * 0.03;
        spawn_box(
            &mut commands,
            &shop.cube,
            &diffuser,
            lens_center - lens / 2.0,
            lens_center + lens / 2.0,
        )
        .insert((NotShadowCaster, Overhead));
        for end in [-0.45, 0.45] {
            let offset = if along_x {
                Vec3::X * end
            } else {
                Vec3::Z * end
            };
            commands.spawn((
                Mesh3d(chain.clone()),
                MeshMaterial3d(housing.clone()),
                Transform::from_translation(center + offset + Vec3::Y * (WALL_HEIGHT - 2.25) / 2.0),
                NotShadowCaster,
                Overhead,
                Pickable::IGNORE,
            ));
        }
        commands.spawn((
            SpotLight {
                color: Color::srgb(1.0, 0.92, 0.82),
                intensity: 7_000.0,
                range: 8.0,
                inner_angle: 0.5,
                outer_angle: 1.15,
                shadow_maps_enabled: true,
                ..default()
            },
            Transform::from_translation(center - Vec3::Y * 0.07).looking_to(Vec3::NEG_Y, Vec3::X),
        ));
    }
}

fn spawn_furnishings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    shop: Res<ShopAssets>,
) {
    let cube = shop.cube.clone();
    let mut material = |color: Color, metallic: f32, roughness: f32| {
        materials.add(StandardMaterial {
            base_color: color,
            metallic,
            perceptual_roughness: roughness,
            ..default()
        })
    };
    let steel = material(Color::srgb(0.08, 0.08, 0.09), 0.7, 0.45);
    let chrome = material(Color::srgb(0.85, 0.86, 0.88), 1.0, 0.18);
    let rubber = material(Color::srgb(0.1, 0.1, 0.11), 0.0, 0.9);
    let window_trim = material(Color::srgb(0.95, 0.94, 0.9), 0.0, 0.5);
    let handle_wood = material(Color::srgb(0.55, 0.3, 0.14), 0.0, 0.5);
    let red = material(Color::srgb(0.72, 0.07, 0.05), 0.0, 0.3);
    let yellow = material(Color::srgb(0.95, 0.72, 0.08), 0.0, 0.35);
    let sawdust = materials.add(StandardMaterial {
        base_color: Color::srgba(0.86, 0.76, 0.58, 0.4),
        perceptual_roughness: 1.0,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let glass = materials.add(StandardMaterial {
        base_color: Color::srgba(0.75, 0.85, 0.95, 0.12),
        perceptual_roughness: 0.05,
        reflectance: 0.6,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let pegboard = materials.add(StandardMaterial {
        base_color_texture: Some(shop.pegboard.clone()),
        perceptual_roughness: 0.85,
        uv_transform: Affine2::from_scale(Vec2::new(1.9, 1.0) / PEGBOARD_TILE),
        ..default()
    });

    // The window: trim around the opening, a sill, crossed muntins, and a pane of glass.
    let (left, right) = WINDOW_X;
    let (sill, head) = WINDOW_Y;
    let face = -SHOP_HALF_SIZE;
    for (min, max) in [
        (
            Vec3::new(left - 0.06, sill - 0.05, face),
            Vec3::new(right + 0.06, sill, face + 0.06),
        ),
        (
            Vec3::new(left - 0.06, head, face),
            Vec3::new(right + 0.06, head + 0.06, face + 0.03),
        ),
        (
            Vec3::new(left - 0.06, sill, face),
            Vec3::new(left, head, face + 0.03),
        ),
        (
            Vec3::new(right, sill, face),
            Vec3::new(right + 0.06, head, face + 0.03),
        ),
        (
            Vec3::new((left + right) / 2.0 - 0.02, sill, face - 0.06),
            Vec3::new((left + right) / 2.0 + 0.02, head, face - 0.03),
        ),
        (
            Vec3::new(left, (sill + head) / 2.0 - 0.02, face - 0.06),
            Vec3::new(right, (sill + head) / 2.0 + 0.02, face - 0.03),
        ),
    ] {
        spawn_box(&mut commands, &cube, &window_trim, min, max).insert(OnWall(WallSide::Back));
    }
    spawn_box(
        &mut commands,
        &cube,
        &glass,
        Vec3::new(left, sill, face - 0.05),
        Vec3::new(right, head, face - 0.045),
    )
    .insert((OnWall(WallSide::Back), NotShadowCaster));

    // Pegboard behind the miter saw, with the hand tools a shop starts with.
    let board_min = Vec3::new(-2.55, 1.05, face);
    let board_max = Vec3::new(-0.65, 2.05, face + 0.006);
    spawn_box(&mut commands, &cube, &pegboard, board_min, board_max).insert(OnWall(WallSide::Back));
    let on_board = face + 0.006;
    let mut tool = |material: &Handle<StandardMaterial>, min: Vec3, max: Vec3| {
        spawn_box(
            &mut commands,
            &cube,
            material,
            min + Vec3::Z * on_board,
            max + Vec3::Z * on_board,
        )
        .insert(OnWall(WallSide::Back));
    };
    // A handsaw: a steel blade tapering to the toe, and a wooden handle.
    tool(
        &chrome,
        Vec3::new(-2.42, 1.62, 0.0),
        Vec3::new(-1.98, 1.74, 0.003),
    );
    tool(
        &chrome,
        Vec3::new(-2.42, 1.58, 0.0),
        Vec3::new(-2.1, 1.62, 0.003),
    );
    tool(
        &handle_wood,
        Vec3::new(-1.98, 1.58, 0.0),
        Vec3::new(-1.86, 1.76, 0.03),
    );
    // A hammer.
    tool(
        &handle_wood,
        Vec3::new(-1.74, 1.45, 0.0),
        Vec3::new(-1.71, 1.77, 0.025),
    );
    tool(
        &steel,
        Vec3::new(-1.79, 1.77, 0.0),
        Vec3::new(-1.66, 1.81, 0.035),
    );
    // A framing square.
    tool(
        &chrome,
        Vec3::new(-1.55, 1.25, 0.0),
        Vec3::new(-1.51, 1.86, 0.004),
    );
    tool(
        &chrome,
        Vec3::new(-1.55, 1.25, 0.0),
        Vec3::new(-1.15, 1.29, 0.004),
    );
    // Three F-clamps.
    for i in 0..3 {
        let x = -1.32 + 0.12 * i as f32;
        tool(
            &chrome,
            Vec3::new(x, 1.4, 0.0),
            Vec3::new(x + 0.015, 1.9, 0.01),
        );
        tool(
            &red,
            Vec3::new(x - 0.01, 1.84, 0.0),
            Vec3::new(x + 0.07, 1.88, 0.03),
        );
        tool(
            &red,
            Vec3::new(x - 0.01, 1.52, 0.0),
            Vec3::new(x + 0.07, 1.57, 0.03),
        );
    }
    // Chisels, and a tape measure.
    for i in 0..4 {
        let x = -0.98 + 0.05 * i as f32;
        tool(
            &handle_wood,
            Vec3::new(x, 1.72, 0.0),
            Vec3::new(x + 0.025, 1.84, 0.025),
        );
        tool(
            &chrome,
            Vec3::new(x + 0.004, 1.6, 0.0),
            Vec3::new(x + 0.021, 1.72, 0.005),
        );
    }
    tool(
        &yellow,
        Vec3::new(-0.98, 1.3, 0.0),
        Vec3::new(-0.9, 1.38, 0.04),
    );

    // A lumber rack on the left wall: steel standards with arms, stacked with boards.
    let wall = -SHOP_HALF_SIZE;
    let arm_reach = 0.36;
    for z in [-1.8, -0.75, 0.3] {
        spawn_box(
            &mut commands,
            &cube,
            &steel,
            Vec3::new(wall, 1.1, z - 0.025),
            Vec3::new(wall + 0.04, 2.25, z + 0.025),
        )
        .insert(OnWall(WallSide::Left));
        for y in [1.25, 1.6, 1.95] {
            spawn_box(
                &mut commands,
                &cube,
                &steel,
                Vec3::new(wall, y - 0.025, z - 0.02),
                Vec3::new(wall + arm_reach, y, z + 0.02),
            )
            .insert(OnWall(WallSide::Left));
        }
    }
    // (width, thickness) of boards on each arm level, bottom to top, in inches.
    let stacks: [&[(f32, f32)]; 3] = [
        &[
            (7.25, 0.75),
            (7.25, 0.75),
            (5.5, 0.75),
            (3.5, 1.5),
            (3.5, 1.5),
        ],
        &[(9.25, 0.75), (5.5, 0.75), (5.5, 0.75), (3.5, 0.75)],
        &[(11.25, 0.75), (7.25, 0.75), (2.5, 0.75)],
    ];
    for ((level, stack), arm_top) in stacks.iter().enumerate().zip([1.25, 1.6, 1.95]) {
        let mut x = wall + 0.05;
        for (i, &(width, thickness)) in stack.iter().enumerate() {
            let (width, thickness) = (width * 0.0254, thickness * 0.0254);
            let length = if (level + i) % 3 == 0 { 2.44 } else { 1.83 };
            let z0 = -0.75 - length / 2.0 + 0.1 * (i % 2) as f32;
            let size = Vec3::new(width, thickness, length);
            let shift = Vec3::new(0.13 * (level * 5 + i) as f32, 0.0, 0.41 * i as f32);
            commands.spawn((
                Mesh3d(meshes.add(board_mesh(size, shift, Grain::Z))),
                MeshMaterial3d(shop.wood.clone()),
                Transform::from_xyz(x, arm_top, z0),
                OnWall(WallSide::Left),
                Pickable::IGNORE,
            ));
            x += width + 0.008;
            if x > wall + arm_reach - 0.06 {
                break;
            }
        }
    }

    // A rolling tool chest in the back corner: red drawers, chrome pulls, black casters.
    let chest_min = Vec3::new(2.25, 0.08, -2.95);
    let chest_max = Vec3::new(2.95, 1.0, -2.45);
    let chest_paint = materials.add(StandardMaterial {
        base_color: Color::srgb(0.66, 0.05, 0.04),
        perceptual_roughness: 0.25,
        clearcoat: 0.8,
        clearcoat_perceptual_roughness: 0.15,
        ..default()
    });
    spawn_box(&mut commands, &cube, &chest_paint, chest_min, chest_max);
    // A seam and a pull at the top of each drawer.
    for top in [0.97, 0.84, 0.71, 0.58, 0.38] {
        spawn_box(
            &mut commands,
            &cube,
            &steel,
            Vec3::new(chest_min.x + 0.02, top, chest_max.z),
            Vec3::new(chest_max.x - 0.02, top + 0.008, chest_max.z + 0.004),
        );
        spawn_box(
            &mut commands,
            &cube,
            &chrome,
            Vec3::new(chest_min.x + 0.08, top - 0.03, chest_max.z),
            Vec3::new(chest_max.x - 0.08, top - 0.015, chest_max.z + 0.025),
        );
    }
    let caster = meshes.add(Cylinder::new(0.035, 0.03));
    for (x, z) in [(2.31, -2.89), (2.89, -2.89), (2.31, -2.51), (2.89, -2.51)] {
        commands.spawn((
            Mesh3d(caster.clone()),
            MeshMaterial3d(rubber.clone()),
            Transform::from_xyz(x, 0.04, z)
                .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
            Pickable::IGNORE,
        ));
    }

    // A shop vac by the table saw, ready for the sawdust.
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.2, 0.42))),
        MeshMaterial3d(yellow.clone()),
        Transform::from_xyz(1.35, 0.25, -1.5),
        Pickable::IGNORE,
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.21, 0.1))),
        MeshMaterial3d(steel.clone()),
        Transform::from_xyz(1.35, 0.51, -1.5),
        Pickable::IGNORE,
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.22, 0.04))),
        MeshMaterial3d(rubber.clone()),
        Transform::from_xyz(1.35, 0.03, -1.5),
        Pickable::IGNORE,
    ));

    // Anti-fatigue mats where the operator stands at each saw.
    for (min, max) in [
        (Vec3::new(-0.5, 0.0, 0.5), Vec3::new(0.65, 0.012, 1.1)),
        (Vec3::new(-2.3, 0.0, 0.42), Vec3::new(-0.9, 0.012, 0.95)),
    ] {
        spawn_box(&mut commands, &cube, &rubber, min, max);
    }

    // Sawdust that's drifted down around the table saw: soft, overlapping drifts that are
    // densest in the middle.
    let fleck = meshes.add(Cylinder::new(1.0, 1.0));
    let drifts = [
        (0.1, -0.95, 0.2),
        (0.4, -1.05, 0.12),
        (-0.15, -0.75, 0.1),
        (0.55, -0.6, 0.08),
        (0.32, 0.38, 0.1),
        (0.05, -1.3, 0.15),
    ];
    for (i, (x, z, radius)) in drifts.into_iter().enumerate() {
        for (layer, scale) in [1.0, 0.65, 0.4].into_iter().enumerate() {
            let turn = (i * 3 + layer) as f32 * 1.3;
            let nudge = Vec3::new(turn.sin(), 0.0, turn.cos()) * radius * 0.2;
            commands.spawn((
                Mesh3d(fleck.clone()),
                MeshMaterial3d(sawdust.clone()),
                Transform::from_translation(Vec3::new(x, 0.001 + 0.0005 * layer as f32, z) + nudge)
                    .with_rotation(Quat::from_rotation_y(turn))
                    .with_scale(Vec3::new(radius * scale, 0.001, radius * scale * 0.6)),
                NotShadowCaster,
                Pickable::IGNORE,
            ));
        }
    }

    // A scrap bin by the table saw, with offcuts sticking out of it.
    let bin = Vec3::new(1.3, 0.0, 0.05);
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.2, 0.6))),
        MeshMaterial3d(material_steel_blue(&mut materials)),
        Transform::from_translation(bin + Vec3::Y * 0.3),
        Pickable::IGNORE,
    ));
    for (i, (lean, turn, length, width)) in [
        (0.25, 0.3, 0.55, 0.06),
        (-0.2, 1.4, 0.48, 0.09),
        (0.12, 2.6, 0.62, 0.04),
        (-0.3, 4.0, 0.4, 0.14),
    ]
    .into_iter()
    .enumerate()
    {
        let size = Vec3::new(width, 0.019, length);
        commands.spawn((
            Mesh3d(meshes.add(board_mesh(
                size,
                Vec3::new(0.2 * i as f32, 0.0, 0.3),
                Grain::Z,
            ))),
            MeshMaterial3d(shop.wood.clone()),
            Transform::from_translation(bin + Vec3::new(0.0, 0.18, 0.0))
                .with_rotation(
                    Quat::from_rotation_y(turn)
                        * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2 + lean),
                )
                .with_scale(Vec3::ONE),
            Pickable::IGNORE,
        ));
    }

    // A pair of sawhorses out on the open floor, holding a 2x10.
    let horse_z = 1.5;
    for center_x in [1.3, 2.4] {
        let beam = Vec3::new(0.04, 0.09, 0.9);
        commands.spawn((
            Mesh3d(meshes.add(board_mesh(beam, Vec3::new(center_x, 0.0, 0.0), Grain::Z))),
            MeshMaterial3d(shop.wood.clone()),
            Transform::from_xyz(center_x - 0.02, 0.62, horse_z - 0.45),
            Pickable::IGNORE,
        ));
        // Splayed legs: each is a board standing on end, tipped in toward the beam.
        let leg = meshes.add(board_mesh(
            Vec3::new(0.035, 0.09, 0.66),
            Vec3::new(center_x, 0.0, 0.3),
            Grain::Z,
        ));
        for end in [horse_z - 0.4, horse_z + 0.31] {
            for side in [-1.0_f32, 1.0] {
                let foot = Vec3::new(center_x + side * 0.17 - 0.0175, 0.0, end + 0.09);
                commands.spawn((
                    Mesh3d(leg.clone()),
                    MeshMaterial3d(shop.wood.clone()),
                    Transform::from_translation(foot).with_rotation(
                        Quat::from_rotation_z(side * 0.26)
                            * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
                    ),
                    Pickable::IGNORE,
                ));
            }
        }
    }
    let plank = Vec3::new(1.6, 0.038, 0.235);
    commands.spawn((
        Mesh3d(meshes.add(board_mesh(plank, Vec3::new(0.9, 0.0, 0.07), Grain::X))),
        MeshMaterial3d(shop.wood.clone()),
        Transform::from_xyz(1.08, 0.71, horse_z - 0.2),
        Pickable::IGNORE,
    ));
}

fn material_steel_blue(materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.24, 0.34),
        perceptual_roughness: 0.6,
        ..default()
    })
}

fn spawn_motes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Sphere::new(0.003).mesh().ico(1).expect("a valid sphere"));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.95, 0.85),
        perceptual_roughness: 1.0,
        ..default()
    });
    // Scatter them through the sunbeam, from the window down to the floor.
    let window = Vec3::new((WINDOW_X.0 + WINDOW_X.1) / 2.0, 1.5, -SHOP_HALF_SIZE);
    let down_beam = SUN_DIRECTION.normalize();
    for i in 0u32..400 {
        // A cheap repeatable scatter, so the dust looks the same every run.
        let r = |k: u32| {
            let h = (i * 7919 + k * 104_729) as f32;
            (h.sin() * 43_758.547).fract().abs()
        };
        let along = r(1) * 2.6;
        let spread = Vec3::new(r(2) - 0.5, r(3) - 0.5, r(4) - 0.5) * Vec3::new(1.6, 1.2, 0.6);
        let home = window + down_beam * along + spread;
        if home.y < 0.05 {
            continue;
        }
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(home),
            Mote {
                home,
                phase: r(5) * 100.0,
            },
            NotShadowCaster,
            Pickable::IGNORE,
        ));
    }
}

/// Dust drifts lazily on the air.
fn drift_motes(time: Res<Time>, mut motes: Query<(&Mote, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (mote, mut transform) in &mut motes {
        let p = mote.phase;
        transform.translation = mote.home
            + Vec3::new(
                (t * 0.13 + p).sin() * 0.06,
                (t * 0.09 + p * 1.7).sin() * 0.05,
                (t * 0.11 + p * 2.3).cos() * 0.06,
            );
    }
}

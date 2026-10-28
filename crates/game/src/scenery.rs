//! The garage around the saws: walls that cut away on the camera's side like a dollhouse, the
//! sunny yard outside, and the things a real shop collects. None of it affects the simulation.
//!
//! The look is bright and toylike, and cheap to draw: one sun with soft shadows, a cool
//! ambient light so shade stays colorful instead of dark, and plain painted materials.

use bevy::{
    ecs::system::EntityCommands,
    light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, NotShadowCaster},
    math::Affine2,
    picking::pointer::PointerButton,
    prelude::*,
};
use woodsim::Length;

use crate::{
    hand::{Gesture, Hand, Workpiece, from_rack},
    meshes::{Grain, board_mesh, rounded_box},
    shop::{SHOP_HALF_SIZE, ShopAssets, ShopView, set_material_on},
    textures::{self, GRASS_TILE, PEGBOARD_TILE},
};

/// The sky, which also tints the far-off lawn.
pub const SKY: Color = Color::srgb(0.66, 0.84, 0.96);

const WALL_HEIGHT: f32 = 2.4;
const WALL_THICKNESS: f32 = 0.12;
/// Walls are painted two colors, split at this height.
const WAINSCOT_HEIGHT: f32 = 1.0;
/// Walls on the camera's side drop to this height so the camera can see in.
const CUTAWAY_HEIGHT: f32 = 0.35;
/// The dark band along the top of every wall, like the cut edge of a dollhouse.
const CAP_THICKNESS: f32 = 0.025;
/// The window in the back wall, in world x and y.
const WINDOW_X: (f32, f32) = (0.1, 1.5);
const WINDOW_Y: (f32, f32) = (1.0, 2.05);
/// Which way sunlight travels: steeply down from the right of the camera's usual view, so
/// shadows fall where you can see them.
const SUN_DIRECTION: Vec3 = Vec3::new(-0.6, -0.72, -0.3);
const SUN_LUX: f32 = 1_900.0;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(SKY))
            .insert_resource(GlobalAmbientLight {
                // Sky-blue fill, so shade reads cool and colorful against the warm sun.
                color: Color::srgb(0.8, 0.87, 1.0),
                brightness: 75.0,
                ..default()
            })
            .insert_resource(DirectionalLightShadowMap { size: 2048 })
            .add_systems(
                Startup,
                (spawn_walls, spawn_lights, spawn_yard, spawn_furnishings),
            )
            .add_systems(Update, (cut_away_walls, hide_overhead_in_overview));
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
/// view from above.
#[derive(Component)]
struct Overhead;

/// Spawns a rounded box filling `min`..`max` in shop coordinates.
fn spawn_box<'a>(
    commands: &'a mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<StandardMaterial>,
    min: Vec3,
    max: Vec3,
) -> EntityCommands<'a> {
    commands.spawn((
        Mesh3d(meshes.add(rounded_box(max - min))),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(min),
        Pickable::IGNORE,
    ))
}

/// A plain painted material.
fn paint(materials: &mut Assets<StandardMaterial>, color: Color) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.6,
        ..default()
    })
}

/// The walls' footprints at a given height, with the back wall split around the window when
/// `window` is set.
fn wall_boxes(height: f32, window: bool) -> Vec<(WallSide, Vec3, Vec3)> {
    let inner = SHOP_HALF_SIZE;
    let outer = SHOP_HALF_SIZE + WALL_THICKNESS;
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
) {
    let lower = paint(&mut materials, Color::srgb(0.24, 0.6, 0.62));
    let upper = paint(&mut materials, Color::srgb(0.98, 0.94, 0.85));
    let trim = paint(&mut materials, Color::srgb(1.0, 0.98, 0.94));
    let cap = paint(&mut materials, Color::srgb(0.16, 0.3, 0.34));
    // Each side has a full-height and a cut-away version, and one is shown at a time. Walls
    // cast no shadows: the sun lights the whole shop, whichever walls are standing.
    for (cut_away, height, window) in [(false, WALL_HEIGHT, true), (true, CUTAWAY_HEIGHT, false)] {
        for (side, min, max) in wall_boxes(height, window) {
            let wall = || (Wall { side, cut_away }, NotShadowCaster);
            let split = WAINSCOT_HEIGHT.clamp(min.y, max.y);
            for (material, bottom, top) in [(&lower, min.y, split), (&upper, split, max.y)] {
                if top > bottom {
                    spawn_box(
                        &mut commands,
                        &mut meshes,
                        material,
                        min.with_y(bottom),
                        max.with_y(top),
                    )
                    .insert(wall());
                }
            }
            // A chair rail where the colors meet, standing a little proud of the wall.
            if min.y < WAINSCOT_HEIGHT && max.y > WAINSCOT_HEIGHT + 0.03 {
                let proud = -side.outward() * 0.015;
                spawn_box(
                    &mut commands,
                    &mut meshes,
                    &trim,
                    (min + proud.min(Vec3::ZERO)).with_y(WAINSCOT_HEIGHT - 0.02),
                    (max + proud.max(Vec3::ZERO)).with_y(WAINSCOT_HEIGHT + 0.03),
                )
                .insert(wall());
            }
            // Only the tops of the walls get a cap, not the pieces under and over the window.
            if max.y >= height {
                spawn_box(
                    &mut commands,
                    &mut meshes,
                    &cap,
                    min.with_y(height),
                    max.with_y(height + CAP_THICKNESS),
                )
                .insert(wall());
            }
        }
    }
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
) {
    // A warm midday sun, high enough to light the whole shop.
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.95, 0.86),
            illuminance: SUN_LUX,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default().looking_to(SUN_DIRECTION.normalize(), Vec3::Y),
        CascadeShadowConfigBuilder {
            num_cascades: 2,
            first_cascade_far_bound: 8.0,
            maximum_distance: 25.0,
            ..default()
        }
        .build(),
    ));

    // Two LED shop lights hanging over the saws. They only show up close; the sun does the
    // lighting.
    let housing = paint(&mut materials, Color::srgb(0.92, 0.93, 0.95));
    let diffuser = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::new(1.0, 0.97, 0.92, 1.0) * 300.0,
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
            &mut meshes,
            &housing,
            center - size / 2.0,
            center + size / 2.0,
        )
        .insert((NotShadowCaster, Overhead));
        let lens = size * Vec3::new(0.85, 0.3, 0.85);
        let lens_center = center - Vec3::Y * 0.03;
        spawn_box(
            &mut commands,
            &mut meshes,
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
    }
}

/// The lawn around the garage, a driveway out front, and a few round trees and bushes.
fn spawn_yard(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    const LAWN_SIZE: f32 = 120.0;
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(LAWN_SIZE, LAWN_SIZE))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(images.add(textures::grass())),
            perceptual_roughness: 0.9,
            // The plane's UVs span the whole lawn; repeat the grass every GRASS_TILE.
            uv_transform: Affine2::from_scale(Vec2::splat(LAWN_SIZE / GRASS_TILE)),
            ..default()
        })),
        Transform::from_xyz(0.0, -0.01, 0.0),
        Pickable::IGNORE,
    ));
    let driveway = paint(&mut materials, Color::srgb(0.84, 0.82, 0.78));
    let outer = SHOP_HALF_SIZE + WALL_THICKNESS;
    spawn_box(
        &mut commands,
        &mut meshes,
        &driveway,
        Vec3::new(-2.2, -0.04, outer),
        Vec3::new(2.2, 0.02, outer + 6.0),
    );

    let trunk = paint(&mut materials, Color::srgb(0.55, 0.36, 0.22));
    let leaves = [
        paint(&mut materials, Color::srgb(0.36, 0.7, 0.3)),
        paint(&mut materials, Color::srgb(0.45, 0.78, 0.32)),
        paint(&mut materials, Color::srgb(0.3, 0.62, 0.32)),
    ];
    let ball = meshes.add(Sphere::new(1.0).mesh().ico(3).expect("a valid sphere"));
    let post = meshes.add(Cylinder::new(1.0, 1.0));
    // Each tree: where it stands, and how big it is.
    for (i, (x, z, scale)) in [
        (-5.2, -4.6, 1.2),
        (4.9, -4.9, 1.0),
        (-5.8, 2.2, 0.9),
        (5.6, 3.4, 1.1),
        (-3.6, 6.5, 0.8),
    ]
    .into_iter()
    .enumerate()
    {
        let base = Vec3::new(x, 0.0, z);
        commands.spawn((
            Mesh3d(post.clone()),
            MeshMaterial3d(trunk.clone()),
            Transform::from_translation(base + Vec3::Y * 0.6 * scale)
                .with_scale(Vec3::new(0.12, 1.2, 0.12) * scale),
            Pickable::IGNORE,
        ));
        // A cluster of overlapping balls makes a soft, round crown.
        for (j, (offset, radius)) in [
            (Vec3::new(0.0, 1.9, 0.0), 0.95),
            (Vec3::new(0.55, 1.55, 0.25), 0.7),
            (Vec3::new(-0.5, 1.6, -0.2), 0.72),
            (Vec3::new(0.05, 2.5, -0.1), 0.6),
        ]
        .into_iter()
        .enumerate()
        {
            commands.spawn((
                Mesh3d(ball.clone()),
                MeshMaterial3d(leaves[(i + j) % leaves.len()].clone()),
                Transform::from_translation(base + offset * scale)
                    .with_scale(Vec3::splat(radius * scale)),
                Pickable::IGNORE,
            ));
        }
    }
    // Bushes along the back of the garage.
    for (i, x) in [-2.4_f32, -1.5, 2.0, 2.8].into_iter().enumerate() {
        let radius = 0.42 + 0.08 * (i % 2) as f32;
        commands.spawn((
            Mesh3d(ball.clone()),
            MeshMaterial3d(leaves[i % leaves.len()].clone()),
            Transform::from_xyz(x, radius * 0.6, -outer - 0.55).with_scale(Vec3::new(
                radius,
                radius * 0.85,
                radius,
            )),
            Pickable::IGNORE,
        ));
    }
}

fn spawn_furnishings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    shop: Res<ShopAssets>,
) {
    let charcoal = paint(&mut materials, Color::srgb(0.22, 0.24, 0.3));
    let steel = paint(&mut materials, Color::srgb(0.84, 0.86, 0.9));
    let rubber = paint(&mut materials, Color::srgb(0.2, 0.21, 0.25));
    let white = paint(&mut materials, Color::srgb(1.0, 0.98, 0.94));
    let handle_wood = paint(&mut materials, Color::srgb(0.78, 0.46, 0.22));
    let red = paint(&mut materials, Color::srgb(0.92, 0.25, 0.22));
    let yellow = paint(&mut materials, Color::srgb(1.0, 0.8, 0.2));
    let blue = paint(&mut materials, Color::srgb(0.25, 0.52, 0.92));
    let purple = paint(&mut materials, Color::srgb(0.55, 0.38, 0.8));
    let green = paint(&mut materials, Color::srgb(0.38, 0.72, 0.4));
    let terracotta = paint(&mut materials, Color::srgb(0.88, 0.5, 0.32));
    let leaf = paint(&mut materials, Color::srgb(0.36, 0.72, 0.34));
    let mat_color = paint(&mut materials, Color::srgb(0.27, 0.3, 0.38));
    let sawdust = materials.add(StandardMaterial {
        base_color: Color::srgba(0.98, 0.84, 0.6, 0.55),
        perceptual_roughness: 1.0,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let glass = materials.add(StandardMaterial {
        base_color: Color::srgba(0.75, 0.9, 1.0, 0.25),
        perceptual_roughness: 0.1,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let pegboard = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.74, 0.5),
        base_color_texture: Some(shop.pegboard.clone()),
        perceptual_roughness: 0.8,
        // Rounded boxes' UVs are in meters.
        uv_transform: Affine2::from_scale(Vec2::splat(1.0 / PEGBOARD_TILE)),
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
        spawn_box(&mut commands, &mut meshes, &white, min, max).insert(OnWall(WallSide::Back));
    }
    spawn_box(
        &mut commands,
        &mut meshes,
        &glass,
        Vec3::new(left, sill, face - 0.05),
        Vec3::new(right, head, face - 0.045),
    )
    .insert((OnWall(WallSide::Back), NotShadowCaster));

    // A clock on the back wall, at ten past ten.
    let clock_center = Vec3::new(2.15, 1.75, face);
    let disc = meshes.add(Cylinder::new(1.0, 1.0));
    for (material, radius, depth) in [(&charcoal, 0.17, 0.03), (&white, 0.145, 0.04)] {
        commands.spawn((
            Mesh3d(disc.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(clock_center + Vec3::Z * depth / 2.0)
                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
                .with_scale(Vec3::new(radius, depth, radius)),
            OnWall(WallSide::Back),
            Pickable::IGNORE,
        ));
    }
    // Angles counterclockwise from twelve: the minute hand on the 2, the hour hand past the 10.
    for (angle, length) in [(-1.05_f32, 0.11), (0.96, 0.07)] {
        let mut hand = rounded_box(Vec3::new(0.012, length, 0.006));
        hand.translate_by(Vec3::new(-0.006, 0.0, 0.0));
        commands.spawn((
            Mesh3d(meshes.add(hand)),
            MeshMaterial3d(charcoal.clone()),
            Transform::from_translation(clock_center + Vec3::Z * 0.04)
                .with_rotation(Quat::from_rotation_z(angle)),
            OnWall(WallSide::Back),
            Pickable::IGNORE,
        ));
    }

    // Pegboard behind the miter saw, with the hand tools a shop starts with.
    let board_min = Vec3::new(-2.55, 1.15, face);
    let board_max = Vec3::new(-0.65, 2.05, face + 0.012);
    spawn_box(&mut commands, &mut meshes, &pegboard, board_min, board_max)
        .insert(OnWall(WallSide::Back));
    let on_board = face + 0.012;
    let mut tool = |material: &Handle<StandardMaterial>, min: Vec3, max: Vec3| {
        spawn_box(
            &mut commands,
            &mut meshes,
            material,
            min + Vec3::Z * on_board,
            max + Vec3::Z * on_board,
        )
        .insert(OnWall(WallSide::Back));
    };
    // A handsaw: a steel blade tapering to the toe, and a wooden handle.
    tool(
        &steel,
        Vec3::new(-2.42, 1.62, 0.0),
        Vec3::new(-1.98, 1.74, 0.006),
    );
    tool(
        &steel,
        Vec3::new(-2.42, 1.58, 0.0),
        Vec3::new(-2.1, 1.62, 0.006),
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
        &charcoal,
        Vec3::new(-1.79, 1.77, 0.0),
        Vec3::new(-1.66, 1.81, 0.035),
    );
    // A framing square.
    tool(
        &steel,
        Vec3::new(-1.55, 1.25, 0.0),
        Vec3::new(-1.51, 1.86, 0.006),
    );
    tool(
        &steel,
        Vec3::new(-1.55, 1.25, 0.0),
        Vec3::new(-1.15, 1.29, 0.006),
    );
    // Three F-clamps.
    for i in 0..3 {
        let x = -1.32 + 0.12 * i as f32;
        tool(
            &steel,
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
            &steel,
            Vec3::new(x + 0.004, 1.6, 0.0),
            Vec3::new(x + 0.021, 1.72, 0.006),
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
            &mut meshes,
            &charcoal,
            Vec3::new(wall, 1.1, z - 0.025),
            Vec3::new(wall + 0.04, 2.25, z + 0.025),
        )
        .insert(OnWall(WallSide::Left));
        for y in [1.25, 1.6, 1.95] {
            spawn_box(
                &mut commands,
                &mut meshes,
                &charcoal,
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
            let inches = |n: f32| Length::from_inch_fraction((n * 4.0).round() as i64, 4);
            commands
                .spawn((
                    Mesh3d(meshes.add(board_mesh(size, shift, Grain::Z))),
                    MeshMaterial3d(shop.wood.clone()),
                    Transform::from_xyz(x, arm_top, z0),
                    OnWall(WallSide::Left),
                    RackBoard {
                        width: inches(width / 0.0254),
                        thickness: inches(thickness / 0.0254),
                    },
                ))
                .observe(highlight_rack_board)
                .observe(set_material_on::<PointerOut>(shop.wood.clone()))
                .observe(take_from_rack);
            x += width + 0.008;
            if x > wall + arm_reach - 0.06 {
                break;
            }
        }
    }

    // A rolling tool chest in the back corner: purple drawers, pale pulls, casters.
    let chest_min = Vec3::new(2.25, 0.08, -2.95);
    let chest_max = Vec3::new(2.95, 1.0, -2.45);
    spawn_box(&mut commands, &mut meshes, &purple, chest_min, chest_max);
    // A seam and a pull at the top of each drawer.
    for top in [0.97, 0.84, 0.71, 0.58, 0.38] {
        spawn_box(
            &mut commands,
            &mut meshes,
            &charcoal,
            Vec3::new(chest_min.x + 0.02, top, chest_max.z - 0.004),
            Vec3::new(chest_max.x - 0.02, top + 0.008, chest_max.z + 0.004),
        );
        spawn_box(
            &mut commands,
            &mut meshes,
            &white,
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
    for (material, radius, height, y) in [
        (&blue, 0.2, 0.42, 0.25),
        (&charcoal, 0.21, 0.1, 0.51),
        (&rubber, 0.22, 0.04, 0.03),
    ] {
        commands.spawn((
            Mesh3d(meshes.add(Cylinder::new(radius, height))),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(1.35, y, -1.5),
            Pickable::IGNORE,
        ));
    }

    // A potted plant in the front corner, for a bit of life.
    let pot = Vec3::new(-2.55, 0.0, 2.55);
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.17, 0.32))),
        MeshMaterial3d(terracotta),
        Transform::from_translation(pot + Vec3::Y * 0.16),
        Pickable::IGNORE,
    ));
    let ball = meshes.add(Sphere::new(1.0).mesh().ico(3).expect("a valid sphere"));
    for (offset, radius) in [
        (Vec3::new(0.0, 0.52, 0.0), 0.2),
        (Vec3::new(0.12, 0.44, 0.06), 0.15),
        (Vec3::new(-0.1, 0.46, -0.08), 0.15),
        (Vec3::new(0.02, 0.68, -0.02), 0.13),
    ] {
        commands.spawn((
            Mesh3d(ball.clone()),
            MeshMaterial3d(leaf.clone()),
            Transform::from_translation(pot + offset).with_scale(Vec3::splat(radius)),
            Pickable::IGNORE,
        ));
    }

    // Anti-fatigue mats where the operator stands at each saw.
    for (min, max) in [
        (Vec3::new(-0.5, 0.0, 0.5), Vec3::new(0.65, 0.015, 1.1)),
        (Vec3::new(-2.3, 0.0, 0.42), Vec3::new(-0.9, 0.015, 0.95)),
    ] {
        spawn_box(&mut commands, &mut meshes, &mat_color, min, max);
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
    let green_hover = paint(&mut materials, Color::srgb(0.5, 0.86, 0.52));
    commands
        .spawn((
            Mesh3d(meshes.add(Cylinder::new(0.2, 0.6))),
            MeshMaterial3d(green.clone()),
            Transform::from_translation(bin + Vec3::Y * 0.3),
            ScrapBin { hover: green_hover },
        ))
        .observe(highlight_bin)
        .observe(set_material_on::<PointerOut>(green))
        .observe(throw_away);
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
            Transform::from_translation(bin + Vec3::new(0.0, 0.18, 0.0)).with_rotation(
                Quat::from_rotation_y(turn)
                    * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2 + lean),
            ),
            Pickable::IGNORE,
        ));
    }
}

/// A board on the lumber rack: click it with empty hands to take a new board that size.
#[derive(Component)]
struct RackBoard {
    width: Length,
    thickness: Length,
}

/// The scrap bin: click it holding a board to throw the board away.
#[derive(Component)]
struct ScrapBin {
    hover: Handle<StandardMaterial>,
}

/// Lights up a board on the rack under the pointer when a click would take one.
fn highlight_rack_board(
    over: On<PointerOver>,
    hand: Res<Hand>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    if hand.0.is_none()
        && let Ok(mut material) = materials.get_mut(over.entity)
    {
        material.0 = shop.wood_highlight.clone();
    }
}

fn take_from_rack(
    click: On<PointerClick>,
    boards: Query<&RackBoard>,
    gesture: Res<Gesture>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut hand: ResMut<Hand>,
) {
    if click.button != PointerButton::Primary || gesture.dragged() || hand.0.is_some() {
        return;
    }
    if let Ok(board) = boards.get(click.entity) {
        hand.0 = Some(Workpiece::Board(from_rack(board.width, board.thickness)));
        if let Ok(mut material) = materials.get_mut(click.entity) {
            material.0 = shop.wood.clone();
        }
    }
}

/// Lights up the scrap bin under the pointer when there's a board in hand to throw in it.
fn highlight_bin(
    over: On<PointerOver>,
    hand: Res<Hand>,
    mut bins: Query<(&ScrapBin, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    if hand.0.is_some()
        && let Ok((bin, mut material)) = bins.get_mut(over.entity)
    {
        material.0 = bin.hover.clone();
    }
}

fn throw_away(click: On<PointerClick>, gesture: Res<Gesture>, mut hand: ResMut<Hand>) {
    if click.button == PointerButton::Primary && !gesture.dragged() {
        hand.0 = None;
    }
}

//! The workbench station: a bench to put parts together on, and the assembly on it.

use bevy::{picking::pointer::PointerButton, prelude::*};
use woodsim::{Fastener, Surface};

use crate::{
    hand::{Gesture, Hand},
    join::{BenchStation, bounds, member_pose, surface_at},
    meshes::{Grain, board_mesh, lumber_mesh},
    shop::{
        Block, Placed, Shape, ShopAssets, ShopView, inches, rounded_block, set_material_on,
        station_part,
    },
};

/// Where the station's coordinates start in the shop: the middle of the bench top. It stands in
/// the front corner, out from the table saw.
pub const ORIGIN: Vec3 = Vec3::new(1.85, inches(34.0), 1.45);

/// The space the bench takes up, for outlining it in the overview.
pub const BOUNDS: Block = Block {
    min: Vec3::new(-HALF_LENGTH, -ORIGIN.y, -HALF_DEPTH),
    max: Vec3::new(HALF_LENGTH, inches(10.0), HALF_DEPTH),
};

pub const HELP: &str = "Holding a part: click the bench to start, or where it goes on the others\n\
                        Click the last part that went on: pick it back up\n\
                        1: glue   2: brads   3: screws\n\
                        R: turn the last part another way   C: line it up at another corner\n\
                        Tab: go to the table saw   Esc: back to the shop";

const HALF_LENGTH: f32 = inches(30.0);
const HALF_DEPTH: f32 = inches(15.0);
const TOP_THICKNESS: f32 = inches(1.75);
/// Where the assembly's middle sits on the bench.
const ASSEMBLY_MIDDLE: Vec3 = Vec3::new(0.0, 0.0, inches(-2.0));

pub struct BenchPlugin;

impl Plugin for BenchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BenchStation>()
            .add_systems(Startup, spawn_bench)
            .add_systems(
                Update,
                (
                    work_with_keys.run_if(resource_equals(ShopView::Bench)),
                    draw_assembly,
                )
                    .chain(),
            );
    }
}

/// A part of the assembly, by its place in it.
#[derive(Component)]
struct MemberPart(usize);

fn spawn_bench(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    shop: Res<ShopAssets>,
) {
    let legs = materials.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.52, 0.92),
        perceptual_roughness: 0.55,
        ..default()
    });
    // A thick maple top on sturdy blue legs, with a shelf below.
    let top = Block {
        min: Vec3::new(-HALF_LENGTH, -TOP_THICKNESS, -HALF_DEPTH),
        max: Vec3::new(HALF_LENGTH, 0.0, HALF_DEPTH),
    };
    let entity = commands.spawn((
        Mesh3d(meshes.add(board_mesh(top.max - top.min, Vec3::ZERO, Grain::X))),
        MeshMaterial3d(shop.maple.clone()),
        Transform::from_translation(ORIGIN + top.min),
    ));
    station_part(entity, ShopView::Bench);
    for x in [-HALF_LENGTH + inches(2.0), HALF_LENGTH - inches(5.0)] {
        for z in [-HALF_DEPTH + inches(2.0), HALF_DEPTH - inches(5.0)] {
            let entity = commands.spawn((
                rounded_block(
                    &mut meshes,
                    Block {
                        min: Vec3::new(x, -ORIGIN.y, z),
                        max: Vec3::new(x + inches(3.0), -TOP_THICKNESS, z + inches(3.0)),
                    },
                    ORIGIN,
                ),
                MeshMaterial3d(legs.clone()),
            ));
            station_part(entity, ShopView::Bench);
        }
    }
    let shelf = Block {
        min: Vec3::new(
            -HALF_LENGTH + inches(2.0),
            inches(-27.0),
            -HALF_DEPTH + inches(2.0),
        ),
        max: Vec3::new(
            HALF_LENGTH - inches(2.0),
            inches(-26.0),
            HALF_DEPTH - inches(2.0),
        ),
    };
    let entity = commands.spawn((
        Mesh3d(meshes.add(board_mesh(shelf.max - shelf.min, Vec3::ZERO, Grain::X))),
        MeshMaterial3d(shop.maple.clone()),
        Transform::from_translation(ORIGIN + shelf.min),
    ));
    station_part(entity, ShopView::Bench);
}

/// Number keys choose the fastener; R and C put the last part on another way.
fn work_with_keys(keys: Res<ButtonInput<KeyCode>>, mut bench: ResMut<BenchStation>) {
    for (key, fastener) in [
        (KeyCode::Digit1, Fastener::Glue),
        (KeyCode::Digit2, Fastener::Brads),
        (KeyCode::Digit3, Fastener::Screws),
    ] {
        if keys.just_pressed(key) {
            bench.fastener = fastener;
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        bench.turn_last();
    }
    if keys.just_pressed(KeyCode::KeyC) {
        bench.slide_last();
    }
}

/// Where the assembly's origin is on the bench, so its middle sits in the middle of the bench
/// and its bottom on the top.
fn assembly_origin(bench: &BenchStation) -> Vec3 {
    let Some(assembly) = &bench.assembly else {
        return ORIGIN;
    };
    let (near, far) = bounds(assembly);
    let middle = (near + far) / 2.0;
    ORIGIN + ASSEMBLY_MIDDLE - Vec3::new(middle.x, near.y, middle.z)
}

/// Keeps one part on the bench for each member of the assembly, where the assembly puts it.
fn draw_assembly(
    mut commands: Commands,
    bench: Res<BenchStation>,
    shop: Res<ShopAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut parts: Query<(Entity, &MemberPart, &Mesh3d, &mut Transform, &mut Shape)>,
) {
    if !bench.is_changed() {
        return;
    }
    let members = bench
        .assembly
        .as_ref()
        .map_or(&[][..], |assembly| assembly.members());
    let origin = assembly_origin(&bench);
    let placed = |index: usize| {
        let member = &members[index];
        let (rotation, corner) = member_pose(member);
        let placed = Placed {
            board: member.board.clone(),
            corner: origin + corner,
            rotation,
            grain: Vec3::new(0.13, 0.0, 0.41) * index as f32,
        };
        (
            Transform::from_translation(placed.corner).with_rotation(placed.rotation),
            placed,
        )
    };
    let mut drawn = 0;
    for (entity, part, mesh, mut transform, mut shape) in &mut parts {
        if part.0 >= members.len() {
            commands.entity(entity).despawn();
            continue;
        }
        let (pose, placed) = placed(part.0);
        *transform = pose;
        shape.update(&placed, mesh, &mut meshes);
        drawn += 1;
    }
    for index in drawn..members.len() {
        let (pose, placed) = placed(index);
        commands
            .spawn((
                Mesh3d(meshes.add(lumber_mesh(&placed.board, placed.grain))),
                MeshMaterial3d(shop.wood.clone()),
                pose,
                MemberPart(index),
                Shape::built(&placed),
            ))
            .observe(highlight_member)
            .observe(set_material_on::<PointerOut>(shop.wood.clone()))
            .observe(member_clicked);
    }
}

/// The surface of the part `entity` shows under a click, from where the click hit it.
fn clicked_surface(
    board: &woodsim::Board,
    transform: &GlobalTransform,
    click: &PointerClick,
) -> Option<Surface> {
    let (point, normal) = (click.hit.position?, click.hit.normal?);
    let inverse = transform.affine().inverse();
    let local_point = inverse.transform_point3(point);
    let local_normal = inverse.transform_vector3(normal);
    surface_at(board, local_point, local_normal)
}

/// Holding a part, a click on the assembly puts it there, against the surface clicked; with
/// empty hands, a click on the last part that went on picks it back up.
fn member_clicked(
    click: On<PointerClick>,
    parts: Query<(&MemberPart, &GlobalTransform)>,
    gesture: Res<Gesture>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut hand: ResMut<Hand>,
    mut bench: ResMut<BenchStation>,
) {
    if click.button != PointerButton::Primary || gesture.dragged() {
        return;
    }
    let Ok((part, transform)) = parts.get(click.entity) else {
        return;
    };
    let Some(target) = bench
        .assembly
        .as_ref()
        .and_then(|assembly| assembly.members().get(part.0))
        .map(|member| member.board.clone())
    else {
        return;
    };
    let handled = match hand.0.take() {
        Some(board) => match clicked_surface(&target, transform, &click) {
            Some(surface) => match bench.put(board, Some((part.0, surface))) {
                Ok(()) => true,
                Err(board) => {
                    hand.0 = Some(board);
                    false
                }
            },
            None => {
                hand.0 = Some(board);
                false
            }
        },
        None if bench.is_last(part.0) => {
            hand.0 = bench.take_last();
            true
        }
        None => false,
    };
    if handled && let Ok(mut material) = materials.get_mut(click.entity) {
        material.0 = shop.wood.clone();
    }
}

/// Lights up a part of the assembly under the pointer when a click would do something: put the
/// part in hand against it, or pick it back up.
fn highlight_member(
    over: On<PointerOver>,
    parts: Query<&MemberPart>,
    hand: Res<Hand>,
    bench: Res<BenchStation>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    let Ok(part) = parts.get(over.entity) else {
        return;
    };
    if (hand.0.is_some() || bench.is_last(part.0))
        && let Ok(mut material) = materials.get_mut(over.entity)
    {
        material.0 = shop.wood_highlight.clone();
    }
}

/// The bench's reading: the fastener chosen and how many parts are together.
pub fn reading(bench: &BenchStation) -> String {
    let parts = bench
        .assembly
        .as_ref()
        .map_or(0, |assembly| assembly.members().len());
    let parts = match parts {
        1 => "1 part".to_string(),
        n => format!("{n} parts"),
    };
    format!(
        "Fastener: {}   Together: {parts}",
        crate::join::fastened(bench.fastener)
    )
}

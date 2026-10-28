//! The workbench station: a bench to put parts together on, the assembly on it, the part in
//! hand, and the pile's parts listed to bring over.

use bevy::{picking::pointer::PointerButton, prelude::*};
use woodsim::{Fastener, Surface};

use crate::{
    chop::MiterSawStation,
    join::{BenchStation, bounds, member_pose, surface_at},
    meshes::{Grain, board_mesh, lumber_mesh, rounded_box},
    shop::{
        Block, Placed, Shape, ShopAssets, ShopView, describe_board, inches, rounded_block,
        set_material_on, station_part,
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

pub const HELP: &str = "Click a part in the list: bring it to the bench\n\
                        Click the side of the part in hand, then where it goes on the others\n\
                        1: glue   2: brads   3: screws\n\
                        R: turn the last part another way   C: line it up at another corner\n\
                        U: take the last part back off   S: set everything aside on the pile\n\
                        Tab: go to the table saw   Esc: back to the shop";

const HALF_LENGTH: f32 = inches(30.0);
const HALF_DEPTH: f32 = inches(15.0);
const TOP_THICKNESS: f32 = inches(1.75);
/// Where the assembly's middle sits on the bench, and where the part in hand waits: lying along
/// the front edge, at the left.
const ASSEMBLY_MIDDLE: Vec3 = Vec3::new(inches(4.0), 0.0, inches(-2.0));
const HELD_CORNER: Vec3 = Vec3::new(inches(-22.0), 0.0, inches(8.0));

pub struct BenchPlugin;

impl Plugin for BenchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BenchStation>()
            .add_systems(Startup, (spawn_bench, spawn_pile_list))
            .add_systems(
                Update,
                (
                    work_with_keys.run_if(resource_equals(ShopView::Bench)),
                    (draw_assembly, draw_held, draw_pile_list),
                )
                    .chain(),
            );
    }
}

/// A part of the assembly, by its place in it.
#[derive(Component)]
struct MemberPart(usize);

/// The part in hand.
#[derive(Component)]
struct HeldPart;

/// The list of the pile's parts, shown at the bench.
#[derive(Component)]
struct PileList;

/// A part on the pile in the list, by its place on the pile.
#[derive(Component)]
struct PileRow(usize);

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

    // The part in hand, drawn once there is one.
    commands
        .spawn((
            Mesh3d(meshes.add(rounded_box(Vec3::splat(0.01)))),
            MeshMaterial3d(shop.wood.clone()),
            Transform::default(),
            Visibility::Hidden,
            HeldPart,
            Shape::default(),
        ))
        .observe(highlight_held)
        .observe(set_material_on::<PointerOut>(shop.wood.clone()))
        .observe(held_clicked);
}

/// The panel listing the pile's parts, in the cut list's place. It shows at the bench only.
fn spawn_pile_list(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(16),
            right: px(16),
            // A set width, so long lines wrap inside the panel rather than spill out of it.
            width: vw(36),
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(14), px(8)),
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(12)),
            row_gap: px(4),
            display: Display::None,
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 0.98, 0.94, 0.94)),
        BorderColor::all(Color::srgba(0.17, 0.2, 0.27, 0.18)),
        PileList,
    ));
}

/// Shows the pile's parts at the bench, each one a line to click to bring it over.
fn draw_pile_list(
    mut commands: Commands,
    view: Res<ShopView>,
    miter_saw: Res<MiterSawStation>,
    mut list: Single<(Entity, &mut Node), With<PileList>>,
    rows: Query<Entity, With<PileRow>>,
) {
    if !(view.is_changed() || miter_saw.is_changed()) {
        return;
    }
    let (list_entity, node) = &mut *list;
    node.display = if *view == ShopView::Bench {
        Display::Flex
    } else {
        Display::None
    };
    for row in &rows {
        commands.entity(row).despawn();
    }
    let text = Color::srgb(0.17, 0.2, 0.27);
    let heading = if miter_saw.parts.is_empty() {
        "The pile is empty".to_string()
    } else {
        "On the pile (click one to bring it over):".to_string()
    };
    commands.spawn((
        Text::new(heading),
        TextFont {
            font_size: FontSize::Px(17.0),
            ..default()
        },
        TextColor(text),
        PileRow(usize::MAX),
        ChildOf(*list_entity),
        Pickable::IGNORE,
    ));
    for (index, part) in miter_saw.parts.iter().enumerate() {
        commands
            .spawn((
                Text::new(format!("{}. {}", index + 1, describe_board(part))),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(Color::srgb(0.88, 0.38, 0.1)),
                PileRow(index),
                ChildOf(*list_entity),
            ))
            .observe(pile_row_clicked);
    }
}

fn pile_row_clicked(
    click: On<PointerClick>,
    rows: Query<&PileRow>,
    mut miter_saw: ResMut<MiterSawStation>,
    mut bench: ResMut<BenchStation>,
) {
    let Ok(row) = rows.get(click.entity) else {
        return;
    };
    if click.button != PointerButton::Primary {
        return;
    }
    if let Some(board) = miter_saw.remove_from_pile(row.0) {
        miter_saw.parts.extend(bench.bring(board));
    }
}

/// Number keys choose the fastener; R and C put the last part on another way; U takes it back
/// off; S sets everything aside.
fn work_with_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut bench: ResMut<BenchStation>,
    mut miter_saw: ResMut<MiterSawStation>,
) {
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
    if keys.just_pressed(KeyCode::KeyU) {
        miter_saw.parts.extend(bench.undo());
    }
    if keys.just_pressed(KeyCode::KeyS) {
        let boards = bench.clear();
        miter_saw.parts.extend(boards);
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

/// Lays the part in hand along the front of the bench, at the left.
fn draw_held(
    bench: Res<BenchStation>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut held: Single<(&Mesh3d, &mut Transform, &mut Visibility, &mut Shape), With<HeldPart>>,
) {
    if !bench.is_changed() {
        return;
    }
    let (mesh, transform, visibility, shape) = &mut *held;
    let Some(board) = &bench.held else {
        **visibility = Visibility::Hidden;
        return;
    };
    // Lying flat with its length along the bench, like the assembly's first part.
    let placed = Placed {
        board: board.clone(),
        corner: ORIGIN + HELD_CORNER,
        rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        grain: Vec3::new(0.29, 0.0, 0.07),
    };
    **transform = Transform::from_translation(placed.corner).with_rotation(placed.rotation);
    shape.update(&placed, mesh, &mut meshes);
    **visibility = Visibility::Inherited;
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

fn held_clicked(
    click: On<PointerClick>,
    transforms: Query<&GlobalTransform>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut bench: ResMut<BenchStation>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Some(board) = bench.held.clone() else {
        return;
    };
    let Ok(transform) = transforms.get(click.entity) else {
        return;
    };
    if let Some(surface) = clicked_surface(&board, transform, &click) {
        bench.pick(surface);
        if let Ok(mut material) = materials.get_mut(click.entity) {
            material.0 = shop.wood.clone();
        }
    }
}

fn member_clicked(
    click: On<PointerClick>,
    parts: Query<(&MemberPart, &GlobalTransform)>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut bench: ResMut<BenchStation>,
) {
    if click.button != PointerButton::Primary || bench.picked.is_none() {
        return;
    }
    let Ok((part, transform)) = parts.get(click.entity) else {
        return;
    };
    let Some(board) = bench
        .assembly
        .as_ref()
        .and_then(|assembly| assembly.members().get(part.0))
        .map(|member| member.board.clone())
    else {
        return;
    };
    if let Some(surface) = clicked_surface(&board, transform, &click)
        && bench.join((part.0, surface))
        && let Ok(mut material) = materials.get_mut(click.entity)
    {
        material.0 = shop.wood.clone();
    }
}

/// Lights up the part in hand under the pointer, to pick its side.
fn highlight_held(
    over: On<PointerOver>,
    bench: Res<BenchStation>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    if bench.held.is_some()
        && let Ok(mut material) = materials.get_mut(over.entity)
    {
        material.0 = shop.wood_highlight.clone();
    }
}

/// Lights up a part of the assembly under the pointer once there's a side picked to put
/// against it.
fn highlight_member(
    over: On<PointerOver>,
    bench: Res<BenchStation>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    if bench.picked.is_some()
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

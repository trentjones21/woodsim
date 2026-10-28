//! The workbench station: a bench to put parts together on. Parts lie loose on it until
//! they're fastened together. The surface under the pointer lights up, and clicking picks it:
//! first the surface that moves, then the one it goes against.

use std::f32::consts::PI;

use bevy::{
    asset::RenderAssetUsages,
    input::{
        gestures::PinchGesture,
        mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    },
    light::NotShadowCaster,
    mesh::Indices,
    picking::{backend::HitData, pointer::PointerButton},
    prelude::*,
    render::render_resource::PrimitiveTopology,
};
use woodsim::{Board, Fastener, Rotation, Surface, outline};

use crate::{
    hand::{Gesture, Hand},
    join::{BenchStation, HALF_TOP, Spot, assembly_point, fastened, surface_at},
    meshes::{Grain, board_mesh, lumber_mesh},
    shop::{
        Block, Placed, Shape, ShopAssets, ShopCamera, ShopView, inches, rounded_block, station_part,
    },
};

/// Where the station's coordinates start in the shop: the middle of the bench top. It stands in
/// the front corner, out from the table saw.
pub const ORIGIN: Vec3 = Vec3::new(1.85, inches(34.0), 1.45);

/// The space the bench takes up, for outlining it in the overview.
pub const BOUNDS: Block = Block {
    min: Vec3::new(-HALF_TOP.x, -ORIGIN.y, -HALF_TOP.y),
    max: Vec3::new(HALF_TOP.x, inches(10.0), HALF_TOP.y),
};

pub const HELP: &str = "Click a surface of a part, then the surface it goes against\n\
                        Q/E: turn the part under the pointer, or around the joint   \
                        F: flip it\n\
                        Arrows: slide it across the joint (Shift: 1/16\")   Enter: fasten it\n\
                        1: glue   2: brads   3: screws   U: take the last joint apart\n\
                        Backspace: let go   Space: pick up a loose part\n\
                        Drag: look around   Scroll: zoom   Tab: go to the table saw   \
                        Esc: back to the shop";

const TOP_THICKNESS: f32 = inches(1.75);

/// How far the sheet that lights up a surface stands off it, so it shows over the wood, and
/// how far in from the surface's edges it stops, so it never pokes out past them.
const SHEET_LIFT: f32 = 0.0003;
const SHEET_INSET: f32 = 0.0004;

pub struct BenchPlugin;

impl Plugin for BenchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BenchStation>()
            .init_resource::<BenchView>()
            .init_resource::<Pointing>()
            .add_systems(Startup, (spawn_bench, spawn_marks))
            .add_systems(
                Update,
                (
                    look_from_the_front,
                    (work_with_keys, look_around).run_if(resource_equals(ShopView::Bench)),
                    draw_bench,
                    mark_surfaces,
                )
                    .chain(),
            );
    }
}

/// Where the camera looks at the bench from: around it (from in front, toward the right) and
/// down at it, and how far away.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct BenchView {
    yaw: f32,
    pitch: f32,
    distance: f32,
}

/// The point on the bench top the camera looks at, from its middle.
const LOOK_AT: Vec3 = Vec3::new(-0.05, 0.0, -0.06);
const LEAST_PITCH: f32 = 10.0 * PI / 180.0;
const MOST_PITCH: f32 = 85.0 * PI / 180.0;
const NEAREST: f32 = 0.35;
const FARTHEST: f32 = 2.0;
/// How far the view turns per pixel dragged, in radians.
const TURN_PER_PIXEL: f32 = 0.006;
/// Each line of scroll zooms in by this fraction. Trackpads scroll in pixels instead.
const ZOOM_PER_LINE: f32 = 0.1;
const PIXELS_PER_LINE: f32 = 20.0;

impl Default for BenchView {
    /// In front of the bench, looking down at it.
    fn default() -> BenchView {
        BenchView {
            yaw: 0.0,
            pitch: 0.62f32.atan2(0.74),
            distance: 0.62f32.hypot(0.74),
        }
    }
}

impl BenchView {
    pub fn pose(&self) -> Transform {
        let toward_camera = Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        );
        let focus = ORIGIN + LOOK_AT;
        Transform::from_translation(focus + toward_camera * self.distance)
            .looking_at(focus, Vec3::Y)
    }
}

/// A part on the bench, drawn in its place in the list of parts shown.
#[derive(Component)]
struct BenchPart {
    slot: usize,
    board: Board,
    group: usize,
    member: usize,
}

/// The part on the bench under the pointer, if any, and the surface of it, until it moves.
#[derive(Resource, Default)]
struct Pointing {
    part: Option<Entity>,
    surface: Option<Surface>,
}

/// A sheet that lights up a surface: the one under the pointer, or the one picked.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Pointed,
    Picked,
}

/// The board and surface a mark's sheet was last built for.
#[derive(Component, Default)]
struct MarkShape(Option<(Board, Surface)>);

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
        min: Vec3::new(-HALF_TOP.x, -TOP_THICKNESS, -HALF_TOP.y),
        max: Vec3::new(HALF_TOP.x, 0.0, HALF_TOP.y),
    };
    let entity = commands.spawn((
        Mesh3d(meshes.add(board_mesh(top.max - top.min, Vec3::ZERO, Grain::X))),
        MeshMaterial3d(shop.maple.clone()),
        Transform::from_translation(ORIGIN + top.min),
    ));
    station_part(entity, ShopView::Bench);
    for x in [-HALF_TOP.x + inches(2.0), HALF_TOP.x - inches(5.0)] {
        for z in [-HALF_TOP.y + inches(2.0), HALF_TOP.y - inches(5.0)] {
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
            -HALF_TOP.x + inches(2.0),
            inches(-27.0),
            -HALF_TOP.y + inches(2.0),
        ),
        max: Vec3::new(
            HALF_TOP.x - inches(2.0),
            inches(-26.0),
            HALF_TOP.y - inches(2.0),
        ),
    };
    let entity = commands.spawn((
        Mesh3d(meshes.add(board_mesh(shelf.max - shelf.min, Vec3::ZERO, Grain::X))),
        MeshMaterial3d(shop.maple.clone()),
        Transform::from_translation(ORIGIN + shelf.min),
    ));
    station_part(entity, ShopView::Bench);
}

/// How the part that moves looks: its wood, warmed so it stands out from the rest without
/// hiding the grain.
#[derive(Resource)]
struct MovingWood(Handle<StandardMaterial>);

/// The two sheets that light up surfaces: pale yellow under the pointer, blue where picked, to
/// stand out against the orange of the part that moves.
fn spawn_marks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    shop: Res<ShopAssets>,
) {
    let wood = materials.get(&shop.wood).cloned().unwrap_or_default();
    commands.insert_resource(MovingWood(materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.68, 0.38),
        emissive: LinearRgba::new(0.9, 0.4, 0.05, 1.0) * 1.5,
        ..wood
    })));
    for (mark, color) in [
        (Mark::Pointed, Color::srgb(1.0, 0.9, 0.35)),
        (Mark::Picked, Color::srgb(0.15, 0.5, 1.0)),
    ] {
        // A stand-in until there's a surface to lay it over; it's hidden until then.
        commands.spawn((
            Mesh3d(meshes.add(Rectangle::new(0.01, 0.01))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                unlit: true,
                ..default()
            })),
            Transform::default(),
            Visibility::Hidden,
            mark,
            MarkShape::default(),
            NotShadowCaster,
            Pickable::IGNORE,
        ));
    }
}

/// Comes to the bench looking from the front, however the view was turned last time.
fn look_from_the_front(shop_view: Res<ShopView>, mut view: ResMut<BenchView>) {
    if shop_view.is_changed() && *shop_view == ShopView::Bench {
        *view = BenchView::default();
    }
}

/// Dragging turns the view around the bench and tips it up or down; scrolling or pinching
/// zooms.
fn look_around(
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut pinches: MessageReader<PinchGesture>,
    gesture: Res<Gesture>,
    mut view: ResMut<BenchView>,
) {
    if mouse.pressed(MouseButton::Left) && gesture.dragged() && motion.delta != Vec2::ZERO {
        view.yaw -= motion.delta.x * TURN_PER_PIXEL;
        view.pitch = (view.pitch + motion.delta.y * TURN_PER_PIXEL).clamp(LEAST_PITCH, MOST_PITCH);
    }
    let lines = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / PIXELS_PER_LINE,
    };
    let pinch: f32 = pinches.read().map(|pinch| pinch.0).sum();
    if lines != 0.0 || pinch != 0.0 {
        let factor = (1.0 - ZOOM_PER_LINE).powf(lines) * (1.0 - pinch);
        view.distance = (view.distance * factor).clamp(NEAREST, FARTHEST);
    }
}

/// Which way each arrow key (left, right, up, down) slides a part across a joint whose
/// directions are `directions`, seen by a camera whose right and up are `right` and `up`: along
/// which direction, and whether toward its most side.
fn arrow_slides(directions: [Vec3; 2], right: Vec3, up: Vec3) -> [(usize, bool); 4] {
    let screen = directions.map(|d| Vec2::new(d.dot(right), d.dot(up)));
    // The direction that runs more nearly across the screen goes with left and right.
    let (across, upward) =
        if screen[0].x.abs() + screen[1].y.abs() >= screen[1].x.abs() + screen[0].y.abs() {
            (0, 1)
        } else {
            (1, 0)
        };
    let rightward = screen[across].x > 0.0;
    let upward_most = screen[upward].y > 0.0;
    [
        (across, !rightward),
        (across, rightward),
        (upward, upward_most),
        (upward, !upward_most),
    ]
}

fn work_with_keys(
    keys: Res<ButtonInput<KeyCode>>,
    camera: Single<&GlobalTransform, With<ShopCamera>>,
    pointing: Res<Pointing>,
    parts: Query<&BenchPart>,
    mut bench: ResMut<BenchStation>,
    mut hand: ResMut<Hand>,
) {
    for (key, fastener) in [
        (KeyCode::Digit1, Fastener::Glue),
        (KeyCode::Digit2, Fastener::Brads),
        (KeyCode::Digit3, Fastener::Screws),
    ] {
        if keys.just_pressed(key) && bench.fastener != fastener {
            bench.fastener = fastener;
        }
    }
    // Q and E turn a part a quarter turn, seen from above: the part under the pointer, or else
    // the picked one, or the part being lined up, around the joint. F flips it over.
    let under_pointer = pointing
        .part
        .and_then(|entity| parts.get(entity).ok())
        .map(|part| part.group);
    for (key, quarters) in [(KeyCode::KeyQ, 1), (KeyCode::KeyE, -1), (KeyCode::KeyF, 2)] {
        if !keys.just_pressed(key) {
            continue;
        }
        if bench.lining_up.is_some() {
            bench.turn_around_joint(quarters);
        } else if let Some(group) = under_pointer.or(bench.picked.map(|picked| picked.group)) {
            let rotation = match key {
                KeyCode::KeyF => Rotation::quarter_turns(0, 2),
                _ => Rotation::quarter_turns(2, quarters),
            };
            bench.turn_group(group, rotation);
        }
    }
    if let Some(directions) = bench.slide_directions() {
        let fine = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let arrows = [
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
        ];
        let slides = arrow_slides(directions, camera.right().into(), camera.up().into());
        for (key, (axis, toward_most)) in arrows.into_iter().zip(slides) {
            if keys.just_pressed(key) {
                bench.slide(axis, toward_most, fine);
            }
        }
    }
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]) {
        bench.glue();
    }
    if keys.any_just_pressed([KeyCode::Backspace, KeyCode::Delete]) {
        bench.cancel();
    }
    if keys.just_pressed(KeyCode::KeyU) {
        bench.take_apart();
    }
    if keys.just_pressed(KeyCode::Space)
        && hand.0.is_none()
        && let Some(board) = bench.take_picked()
    {
        hand.0 = Some(board);
    }
}

/// A part drawn on the bench, with what `draw_bench` keeps up to date on it.
type DrawnPart = (
    Entity,
    &'static mut BenchPart,
    &'static Mesh3d,
    &'static mut Transform,
    &'static mut Shape,
    &'static mut MeshMaterial3d<StandardMaterial>,
);

/// Keeps one part drawn on the bench for each part shown: loose, put together, or being lined
/// up where it would go, lit up if it's the part that moves.
fn draw_bench(
    mut commands: Commands,
    bench: Res<BenchStation>,
    shop: Res<ShopAssets>,
    moving_wood: Res<MovingWood>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut pointing: ResMut<Pointing>,
    mut parts: Query<DrawnPart>,
) {
    if !bench.is_changed() {
        return;
    }
    // Parts move; the surface under the pointer may not be the same one now.
    pointing.surface = None;
    let shown = bench.shown();
    let placed = |slot: usize| {
        let part = &shown[slot];
        Placed {
            board: part.board.clone(),
            corner: ORIGIN + part.corner,
            rotation: part.rotation,
            grain: Vec3::new(0.13, 0.0, 0.41) * slot as f32,
        }
    };
    let material = |moving: bool| {
        if moving {
            moving_wood.0.clone()
        } else {
            shop.wood.clone()
        }
    };
    let mut drawn = 0;
    for (entity, mut part, mesh, mut transform, mut shape, mut wood) in &mut parts {
        let Some(showing) = shown.get(part.slot) else {
            commands.entity(entity).despawn();
            continue;
        };
        let placed = placed(part.slot);
        *transform = Transform::from_translation(placed.corner).with_rotation(placed.rotation);
        shape.update(&placed, mesh, &mut meshes);
        wood.0 = material(showing.moving);
        part.board = showing.board.clone();
        part.group = showing.group;
        part.member = showing.member;
        drawn += 1;
    }
    for (slot, showing) in shown.iter().enumerate().skip(drawn) {
        let placed = placed(slot);
        commands
            .spawn((
                Mesh3d(meshes.add(lumber_mesh(&placed.board, placed.grain))),
                MeshMaterial3d(material(showing.moving)),
                Transform::from_translation(placed.corner).with_rotation(placed.rotation),
                BenchPart {
                    slot,
                    board: showing.board.clone(),
                    group: showing.group,
                    member: showing.member,
                },
                Shape::built(&placed),
            ))
            .observe(point_at_part::<PointerMove>)
            .observe(point_at_part::<PointerOver>)
            .observe(point_away)
            .observe(part_clicked);
    }
}

/// The surface of `part` under a pointer that hit it at `hit`, and where, in the part's mesh
/// axes.
fn surface_under(
    part: &BenchPart,
    transform: &GlobalTransform,
    hit: &HitData,
) -> Option<(Surface, Vec3)> {
    let (point, normal) = (hit.position?, hit.normal?);
    let inverse = transform.affine().inverse();
    let local = inverse.transform_point3(point);
    let surface = surface_at(&part.board, local, inverse.transform_vector3(normal))?;
    Some((surface, local))
}

/// A pointer event that says where it hit.
trait Hits {
    fn hit(&self) -> &HitData;
}

impl Hits for PointerMove {
    fn hit(&self) -> &HitData {
        &self.hit
    }
}

impl Hits for PointerOver {
    fn hit(&self) -> &HitData {
        &self.hit
    }
}

/// Notes the part under the pointer and the surface of it, as the pointer moves over it or
/// it moves under the pointer.
fn point_at_part<E: EntityEvent + Hits>(
    event: On<E>,
    parts: Query<(&BenchPart, &GlobalTransform)>,
    mut pointing: ResMut<Pointing>,
) {
    let entity = event.event_target();
    let Ok((part, transform)) = parts.get(entity) else {
        return;
    };
    let surface = surface_under(part, transform, event.hit()).map(|(surface, _)| surface);
    if pointing.part != Some(entity) || pointing.surface != surface {
        *pointing = Pointing {
            part: Some(entity),
            surface,
        };
    }
}

fn point_away(out: On<PointerOut>, mut pointing: ResMut<Pointing>) {
    if pointing.part == Some(out.entity) {
        *pointing = Pointing::default();
    }
}

/// Holding a part, a click on the bench's parts sets it down near there; with empty hands, a
/// click picks the surface under it.
fn part_clicked(
    click: On<PointerClick>,
    parts: Query<(&BenchPart, &GlobalTransform)>,
    gesture: Res<Gesture>,
    mut hand: ResMut<Hand>,
    mut bench: ResMut<BenchStation>,
) {
    if click.button != PointerButton::Primary || gesture.dragged() {
        return;
    }
    let Ok((part, transform)) = parts.get(click.entity) else {
        return;
    };
    if let Some(board) = hand.0.take() {
        let near = click.hit.position.map(|point| (point - ORIGIN).xz());
        bench.set_down(board, near);
        return;
    }
    let Some((surface, local)) = surface_under(part, transform, &click.hit) else {
        bench.missed();
        return;
    };
    let Some(member) = bench
        .groups
        .get(part.group)
        .and_then(|group| group.assembly.members().get(part.member))
    else {
        return;
    };
    let at = assembly_point(member, local);
    bench.pick(
        Spot {
            group: part.group,
            member: part.member,
            surface,
        },
        at,
    );
}

/// Lays the sheets over the surface under the pointer, when a click there would pick it, and
/// the surface picked.
fn mark_surfaces(
    pointing: Res<Pointing>,
    bench: Res<BenchStation>,
    hand: Res<Hand>,
    view: Res<ShopView>,
    parts: Query<(Entity, &BenchPart, &Transform), Without<Mark>>,
    mut marks: Query<(
        &Mark,
        &Mesh3d,
        &mut Transform,
        &mut Visibility,
        &mut MarkShape,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (mark, mesh, mut transform, mut visibility, mut shape) in &mut marks {
        let spot = match mark {
            Mark::Pointed => pointing
                .part
                .and_then(|entity| parts.get(entity).ok())
                .zip(pointing.surface)
                .map(|((_, part, _), surface)| Spot {
                    group: part.group,
                    member: part.member,
                    surface,
                })
                .filter(|_| hand.0.is_none() && *view == ShopView::Bench)
                .filter(|&spot| Some(spot) != bench.picked),
            Mark::Picked => bench.picked,
        };
        let found = spot.and_then(|spot| {
            parts
                .iter()
                .find(|(_, part, _)| part.group == spot.group && part.member == spot.member)
                .map(|(_, part, placed)| (spot.surface, part, placed))
        });
        let Some((surface, part, placed)) = found else {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
            continue;
        };
        if *transform != *placed {
            *transform = *placed;
        }
        let built = shape
            .0
            .as_ref()
            .is_some_and(|(board, built)| *board == part.board && *built == surface);
        if !built && let Some(mut sheet_mesh) = meshes.get_mut(&mesh.0) {
            *sheet_mesh = sheet(&outline(&part.board, surface));
            shape.0 = Some((part.board.clone(), surface));
        }
        if *visibility != Visibility::Inherited {
            *visibility = Visibility::Inherited;
        }
    }
}

/// A thin sheet over a surface, given as `woodsim::outline` gives it, in the board's mesh axes
/// (`w`, `t`, `l` along x, y, z), standing just off the surface so it shows over the wood.
fn sheet(polygons: &[Vec<[woodsim::Length; 3]>]) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for polygon in polygons {
        let points: Vec<Vec3> = polygon
            .iter()
            .map(|&[l, w, t]| Vec3::new(w.as_meters_f32(), t.as_meters_f32(), l.as_meters_f32()))
            .collect();
        if points.len() < 3 {
            continue;
        }
        // The outline goes counterclockwise seen from outside, so this faces out, and the
        // inside is to the left of each edge.
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize_or_zero();
        let n = points.len();
        let first = positions.len() as u32;
        for (i, &point) in points.iter().enumerate() {
            let inward = |edge: Vec3| normal.cross(edge).normalize_or_zero();
            let before = inward(point - points[(i + n - 1) % n]);
            let after = inward(points[(i + 1) % n] - point);
            let inset = (before + after) * SHEET_INSET / (1.0 + before.dot(after)).max(0.1);
            positions.push((point + inset + normal * SHEET_LIFT).to_array());
            normals.push(normal.to_array());
        }
        for k in 1..points.len() as u32 - 1 {
            indices.extend([first, first + k, first + k + 1]);
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_indices(Indices::U32(indices))
}

/// The bench's reading: the fastener chosen and how many parts are on it.
pub fn reading(bench: &BenchStation) -> String {
    let parts: usize = bench
        .groups
        .iter()
        .map(|group| group.assembly.members().len())
        .sum();
    let parts = match parts {
        1 => "1 part".to_string(),
        n => format!("{n} parts"),
    };
    format!(
        "Fastener: {}   On the bench: {parts}",
        fastened(bench.fastener)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_slide_the_way_the_joint_runs_on_screen() {
        // Seen from the front, a joint across the bench top: its first direction runs to the
        // right, its second away from the operator, which looks like up on screen.
        let (right, up) = (Vec3::X, Vec3::new(0.0, 0.6, -0.8));
        let slides = arrow_slides([Vec3::X, Vec3::NEG_Z], right, up);
        assert_eq!(slides, [(0, false), (0, true), (1, true), (1, false)]);
        // Seen from the right side, the second direction now runs to the right on screen.
        let right = Vec3::NEG_Z;
        let slides = arrow_slides([Vec3::NEG_X, Vec3::NEG_Z], right, Vec3::Y);
        assert_eq!(slides[1], (1, true));
        assert_eq!(slides[2], (0, false));
    }

    #[test]
    fn a_sheet_covers_a_surface_just_off_it() {
        let board = Board::new(
            woodsim::Length::from_inches(24),
            woodsim::Length::from_inches(4),
            woodsim::Length::from_inch_fraction(3, 4),
        );
        let mesh = sheet(&outline(&board, Surface::Top));
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("expected positions");
        };
        assert_eq!(positions.len(), 4);
        for position in positions {
            assert!((position[1] - inches(0.75) - SHEET_LIFT).abs() < 1e-6);
            // In from every edge: across the width (x) and along the length (z).
            let inside = |at: f32, size: f32| (at.min(size - at) - SHEET_INSET).abs() < 1e-6;
            assert!(inside(position[0], inches(4.0)), "{position:?}");
            assert!(inside(position[2], inches(24.0)), "{position:?}");
        }
    }
}

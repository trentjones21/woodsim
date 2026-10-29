//! The workbench station: a bench to put parts together on. Parts lie loose on it until
//! they're fastened together. The surface under the pointer lights up, and clicking picks it:
//! first the surface that moves, then the one it goes against.

use std::{
    f32::consts::PI,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use bevy::{
    asset::RenderAssetUsages,
    input::{
        gestures::PinchGesture,
        mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    },
    light::NotShadowCaster,
    math::DVec3,
    mesh::Indices,
    picking::{backend::HitData, pointer::PointerButton},
    prelude::*,
    render::render_resource::PrimitiveTopology,
};
use plans::Plan;
use woodsim::{Board, Rotation, Surface, exposed};

use crate::{
    controls::{Action, Actions, Arrow},
    hand::{Gesture, Hand},
    hud::OnHud,
    join::{BenchStation, HALF_TOP, Spot, assembly_point, surface_at},
    meshes::{Grain, board_mesh, lumber_mesh},
    shoot::Studio,
    shop::{
        Block, Placed, Shape, ShopAssets, ShopCamera, ShopView, inches, rounded_block, station_part,
    },
    studio,
};

/// Where the station's coordinates start in the shop: the middle of the bench top. It stands in
/// the front corner, out from the table saw.
pub const ORIGIN: Vec3 = Vec3::new(1.85, inches(34.0), 1.45);

/// The space the bench takes up, for outlining it in the overview.
pub const BOUNDS: Block = Block {
    min: Vec3::new(-HALF_TOP.x, -ORIGIN.y, -HALF_TOP.y),
    max: Vec3::new(HALF_TOP.x, inches(10.0), HALF_TOP.y),
};

const TOP_THICKNESS: f32 = inches(1.75);

/// Where plans are saved, from where the game runs.
const PLANS: &str = "plans";

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
                    (work_at_bench, look_around).run_if(resource_equals(ShopView::Bench)),
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

/// Comes to the bench looking from the front, however the view was turned last time, and looks
/// from there again when asked.
fn look_from_the_front(
    shop_view: Res<ShopView>,
    actions: Res<Actions>,
    mut view: ResMut<BenchView>,
) {
    let asked = *shop_view == ShopView::Bench && actions.just(Action::ResetView);
    if shop_view.is_changed() && *shop_view == ShopView::Bench || asked {
        *view = BenchView::default();
    }
}

/// Dragging, with either button, turns the view around the bench and tips it up or down;
/// scrolling or pinching zooms.
fn look_around(
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut pinches: MessageReader<PinchGesture>,
    gesture: Res<Gesture>,
    on_hud: Res<OnHud>,
    mut view: ResMut<BenchView>,
) {
    // A drag that starts on a button isn't one to look around with.
    if mouse.any_pressed([MouseButton::Left, MouseButton::Right])
        && !on_hud.pressed()
        && gesture.dragged()
        && motion.delta != Vec2::ZERO
    {
        view.yaw -= motion.delta.x * TURN_PER_PIXEL;
        view.pitch = (view.pitch + motion.delta.y * TURN_PER_PIXEL).clamp(LEAST_PITCH, MOST_PITCH);
    }
    // Scrolling over the HUD scrolls the inventory, not the view.
    let lines = match scroll.unit {
        _ if on_hud.over() => 0.0,
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

/// Does what's asked at the bench: picks the fastener; turns or flips the part under the
/// pointer, or else the picked one (or the only one), or the part being lined up around the
/// joint; slides it across the joint; fastens it or lets go; takes the last joint apart; picks
/// a part up; draws up plans; or takes a glue-up into the studio.
#[allow(clippy::too_many_arguments)]
fn work_at_bench(
    actions: Res<Actions>,
    camera: Single<(&GlobalTransform, &Camera), With<ShopCamera>>,
    pointing: Res<Pointing>,
    parts: Query<&BenchPart>,
    mut bench: ResMut<BenchStation>,
    mut hand: ResMut<Hand>,
    mut studio: ResMut<Studio>,
    mut view: ResMut<ShopView>,
) {
    let (camera, picture) = *camera;
    let under_pointer = pointing
        .part
        .and_then(|entity| parts.get(entity).ok())
        .map(|part| part.group);
    let slides = bench
        .slide_directions()
        .map(|directions| arrow_slides(directions, camera.right().into(), camera.up().into()));
    for action in actions.started() {
        match action {
            Action::Use(fastener) if bench.fastener != fastener => bench.fastener = fastener,
            Action::TurnLeft | Action::TurnRight | Action::Flip => {
                let quarters = match action {
                    Action::TurnLeft => 1,
                    Action::TurnRight => -1,
                    _ => 2,
                };
                if bench.lining_up.is_some() {
                    bench.turn_around_joint(quarters);
                } else if let Some(group) = under_pointer.or(bench.turnable()) {
                    let rotation = match action {
                        Action::Flip => Rotation::quarter_turns(0, 2),
                        _ => Rotation::quarter_turns(2, quarters),
                    };
                    bench.turn_group(group, rotation);
                }
            }
            Action::Slide(arrow, fine) => {
                if let Some(slides) = slides {
                    let index = match arrow {
                        Arrow::Left => 0,
                        Arrow::Right => 1,
                        Arrow::Up => 2,
                        Arrow::Down => 3,
                    };
                    let (axis, toward_most) = slides[index];
                    bench.slide(axis, toward_most, fine);
                }
            }
            Action::Fasten => {
                bench.glue();
            }
            Action::LetGo => bench.cancel(),
            Action::TakeApart => {
                bench.take_apart();
            }
            Action::PickUp => {
                if hand.0.is_none()
                    && let Some(work) = bench.take_picked()
                {
                    hand.0 = Some(work);
                }
            }
            Action::Plans => draw_up_plans(&mut bench, under_pointer),
            Action::Studio => {
                if let Some(piece) = bench.to_plan(under_pointer) {
                    studio::open(&mut studio, &piece, picture);
                    *view = ShopView::Studio;
                }
            }
            _ => {}
        }
    }
}

/// Draws up plans for the glue-up under the pointer (or the picked one, or the last put
/// together), saves them, and opens them to print.
fn draw_up_plans(bench: &mut BenchStation, under_pointer: Option<usize>) {
    let Some(assembly) = bench.to_plan(under_pointer) else {
        return;
    };
    let page = Plan::of(assembly).page();
    let note = match save_plans(&page, Path::new(PLANS)) {
        Ok(path) => match open_in_browser(&path) {
            Ok(()) => format!("Drew up plans in {}, opened to print.", path.display()),
            Err(error) => format!(
                "Drew up plans in {}, but couldn't open them: {error}.",
                path.display()
            ),
        },
        Err(error) => format!("Couldn't save the plans: {error}."),
    };
    bench.note(note);
}

/// Saves `page` as the next unused `plan-N.html` in `folder`, making the folder if it isn't
/// there.
fn save_plans(page: &str, folder: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let path = (1..)
        .map(|n| folder.join(format!("plan-{n}.html")))
        .find(|path| !path.exists())
        .expect("a free name");
    fs::write(&path, page)?;
    Ok(path)
}

/// Opens a saved page in the browser, to read or print.
fn open_in_browser(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", ""]);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = Command::new("xdg-open");
    command.arg(path).spawn().map(|_| ())
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
    if let Some(work) = hand.0.take() {
        let near = click.hit.position.map(|point| (point - ORIGIN).xz());
        bench.set_down(work, near);
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
            *sheet_mesh = sheet(&exposed(&part.board, surface));
            shape.0 = Some((part.board.clone(), surface));
        }
        if *visibility != Visibility::Inherited {
            *visibility = Visibility::Inherited;
        }
    }
}

/// A thin sheet over a surface, given as `woodsim::outline` gives it, in the board's mesh axes
/// (`w`, `t`, `l` along x, y, z), standing just off the surface so it shows over the wood. It
/// stops short of the surface's own edges, but not of edges where one of its polygons meets
/// another, and each polygon takes in the corners of the others its edges run through, so a
/// surface cut into pieces around a slot is covered with no gaps between them, not even the
/// pinholes the GPU leaves along an edge one triangle has a corner partway along.
pub(crate) fn sheet(polygons: &[Vec<[woodsim::Length; 3]>]) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    // Corners in nanometers, to find where polygons meet before they're rounded to meters.
    let corners: Vec<Vec<DVec3>> = polygons
        .iter()
        .map(|polygon| {
            polygon
                .iter()
                .map(|p| DVec3::from_array(p.map(|x| x.as_nm() as f64)))
                .collect()
        })
        .collect();
    let meters = |p: DVec3| Vec3::new(p.y as f32, p.z as f32, p.x as f32) * 1e-9;
    for (index, polygon) in corners.iter().enumerate() {
        if polygon.len() < 3 {
            continue;
        }
        // The outline goes counterclockwise seen from outside, so this faces out, and the
        // inside is to the left of each edge.
        let normal = (meters(polygon[1]) - meters(polygon[0]))
            .cross(meters(polygon[2]) - meters(polygon[0]))
            .normalize_or_zero();
        let around = with_corners_along(&corners, index);
        let points: Vec<Vec3> = around.iter().map(|&p| meters(p)).collect();
        let n = points.len();
        // In from an edge of the surface, or not at all from one another polygon runs along.
        let inward = |from: usize| {
            let to = (from + 1) % n;
            if meets_another(&corners, index, around[from], around[to]) {
                return Vec3::ZERO;
            }
            normal.cross(points[to] - points[from]).normalize_or_zero()
        };
        let first = positions.len() as u32;
        for (i, &point) in points.iter().enumerate() {
            let (before, after) = (inward((i + n - 1) % n), inward(i));
            let inset = (before + after) * SHEET_INSET / (1.0 + before.dot(after)).max(0.1);
            positions.push((point + inset + normal * SHEET_LIFT).to_array());
            normals.push(normal.to_array());
        }
        for k in 1..n as u32 - 1 {
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

/// Corners nearer than this to a line, in nanometers, are on it, as rounding leaves them.
const ON_LINE: f64 = 4.0;

/// The corners of `corners[polygon]`, with every other polygon's corner that's partway along one
/// of its edges put in there, in order along it.
fn with_corners_along(corners: &[Vec<DVec3>], polygon: usize) -> Vec<DVec3> {
    let own = &corners[polygon];
    let mut around = Vec::new();
    for (k, &a) in own.iter().enumerate() {
        let b = own[(k + 1) % own.len()];
        around.push(a);
        let length = a.distance(b);
        if length < ON_LINE {
            continue;
        }
        let along = (b - a) / length;
        let mut between: Vec<(f64, DVec3)> = corners
            .iter()
            .enumerate()
            .filter(|&(other, _)| other != polygon)
            .flat_map(|(_, others)| others.iter().copied())
            .filter_map(|p| {
                let s = (p - a).dot(along);
                let off = (p - a).cross(along).length();
                (off < ON_LINE && s > ON_LINE && s < length - ON_LINE).then_some((s, p))
            })
            .collect();
        between.sort_by(|x, y| x.0.total_cmp(&y.0));
        between.dedup_by(|x, y| (x.0 - y.0).abs() < ON_LINE);
        around.extend(between.into_iter().map(|(_, p)| p));
    }
    around
}

/// Whether the edge from `a` to `b` of `corners[polygon]` runs along an edge of another of the
/// polygons for some of its length: inside the surface they cover together, rather than one of
/// its edges.
fn meets_another(corners: &[Vec<DVec3>], polygon: usize, a: DVec3, b: DVec3) -> bool {
    let length = a.distance(b);
    if length < ON_LINE {
        return false;
    }
    let along = (b - a) / length;
    let off = |p: DVec3| (p - a).cross(along).length();
    corners.iter().enumerate().any(|(other, around)| {
        other != polygon
            && (0..around.len()).any(|k| {
                let (c, d) = (around[k], around[(k + 1) % around.len()]);
                let (s, t) = ((c - a).dot(along), (d - a).dot(along));
                off(c) < ON_LINE
                    && off(d) < ON_LINE
                    && s.max(t).min(length) - s.min(t).max(0.0) > ON_LINE
            })
    })
}

/// How many parts and joints are on the bench: "3 parts, 2 joints".
pub fn reading(bench: &BenchStation) -> String {
    let parts: usize = bench
        .groups
        .iter()
        .map(|group| group.assembly.members().len())
        .sum();
    let joints: usize = bench
        .groups
        .iter()
        .map(|group| group.assembly.joints().len())
        .sum();
    let parts = match parts {
        0 => return "nothing yet".to_string(),
        1 => "1 part".to_string(),
        n => format!("{n} parts"),
    };
    let joints = match joints {
        0 => String::new(),
        1 => ", 1 joint".to_string(),
        n => format!(", {n} joints"),
    };
    format!("{parts}{joints}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_are_saved_to_the_next_free_page() {
        let folder = std::env::temp_dir().join(format!("woodshop-plans-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let first = save_plans("one", &folder).unwrap();
        let second = save_plans("two", &folder).unwrap();
        assert_eq!(first.file_name().unwrap(), "plan-1.html");
        assert_eq!(second.file_name().unwrap(), "plan-2.html");
        assert_eq!(fs::read_to_string(&first).unwrap(), "one");
        fs::remove_dir_all(&folder).unwrap();
    }

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

    /// A 1x6 jointed on its beveled edge, the last pass stopped partway: its top face is cut
    /// into a few polygons around the step the cutterhead's curve left.
    const HALF_JOINTED: &str = include_str!("../../woodsim/tests/pasted/half-jointed.txt");

    /// How much of the plane a sheet's triangles cover, in square meters.
    fn covered(mesh: &Mesh) -> f32 {
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("expected positions");
        };
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("expected indices");
        };
        indices
            .chunks(3)
            .map(|k| {
                let [a, b, c] = [k[0], k[1], k[2]].map(|i| Vec3::from_array(positions[i as usize]));
                (b - a).cross(c - a).length() / 2.0
            })
            .sum()
    }

    #[test]
    fn a_sheet_over_a_surface_cut_into_pieces_leaves_no_gaps_between_them() {
        let woodsim::Wood::GlueUp(glue_up) = woodsim::Recipe::read(HALF_JOINTED).unwrap().wood
        else {
            panic!("a glue-up of one");
        };
        let board = &glue_up.members()[0].board;
        let face = exposed(board, Surface::Top);
        assert!(face.len() > 1, "the step cuts the face into pieces");
        let area: f32 = face
            .iter()
            .map(|polygon| {
                let points: Vec<Vec3> = polygon
                    .iter()
                    .map(|p| Vec3::from_array(p.map(|x| x.as_meters_f32())))
                    .collect();
                (1..points.len() - 1)
                    .map(|k| {
                        (points[k] - points[0])
                            .cross(points[k + 1] - points[0])
                            .length()
                            / 2.0
                    })
                    .sum::<f32>()
            })
            .sum();
        // It's in from the face's edges all the way around, and nowhere else: what it doesn't
        // cover is no more than a strip that wide around the outside.
        let around = 2.0 * (board.length() + board.width()).as_meters_f32();
        let uncovered = area - covered(&sheet(&face));
        assert!(uncovered > 0.0, "{uncovered}");
        assert!(
            uncovered < 1.05 * around * SHEET_INSET,
            "{uncovered} m² uncovered"
        );
    }

    #[test]
    fn a_sheet_covers_a_surface_just_off_it() {
        let board = Board::new(
            woodsim::Length::from_inches(24),
            woodsim::Length::from_inches(4),
            woodsim::Length::from_inch_fraction(3, 4),
        );
        let mesh = sheet(&exposed(&board, Surface::Top));
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

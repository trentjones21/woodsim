//! The shop around the stations: shared layout helpers, the camera, and moving between the
//! overview and the stations.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::{
    camera::Exposure,
    core_pipeline::tonemapping::Tonemapping,
    ecs::system::{EntityCommands, SystemParam},
    input::{
        gestures::PinchGesture,
        mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    },
    math::Affine2,
    pbr::{DistanceFog, FogFalloff},
    picking::pointer::PointerButton,
    prelude::*,
    render::view::{ColorGrading, ColorGradingGlobal},
};
use woodsim::{Board, Length, Orientation, Piece};

use crate::{
    bench::{self, BenchView},
    chop::MiterSawStation,
    controls::{Action, Actions, Arrow},
    dress::JointerStation,
    feed::TableSawStation,
    hand::{DRAG_THRESHOLD, Gesture, Hand, set_down},
    hud::OnHud,
    join::BenchStation,
    jointer,
    meshes::{lumber_mesh, rounded_box},
    miter_saw, planer,
    scenery::SKY,
    shoot::Studio,
    studio, table_saw,
    textures::{self, FLOOR_TILE, MAPLE, PINE},
    thickness::PlanerStation,
};

/// Converts inches to meters, for laying out the scene.
pub const fn inches(n: f32) -> f32 {
    n * 0.0254
}

/// A box to draw, as opposite corners in a station's own coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub min: Vec3,
    pub max: Vec3,
}

/// The parts of a board a saw shows: the board in one piece (slots and all), or both sides of a
/// cut that went through it.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    Whole,
    Piece,
    Offcut,
}

/// A board on a saw, and where it sits, in the saw's own frame: `across` its table toward the
/// fence, `up`, and `along` it, as x, y, and z. `corner` is where the board's `l = w = t = 0`
/// corner is, and `rotation` turns the board's own axes (`w`, `t`, `l` along x, y, z) to lie the
/// way it does. `grain` is where its corner sits in the board it was cut from, so the grain of
/// the pieces lines up and stays put on the wood as they move.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub board: Board,
    pub corner: Vec3,
    pub rotation: Quat,
    pub grain: Vec3,
}

impl Placed {
    /// `board` lying as `lie` says, with the near corner of the space it takes up on the saw
    /// at `near`.
    pub fn lying(board: &Board, lie: Orientation, near: Vec3) -> Placed {
        let (rotation, corner) = lay(board, lie);
        Placed {
            board: board.clone(),
            corner: near + corner,
            rotation,
            grain: Vec3::ZERO,
        }
    }

    /// A piece cut from this board, where it was on it.
    pub fn piece(&self, piece: &Piece) -> Placed {
        let offset = Vec3::new(
            piece.w.as_meters_f32(),
            piece.t.as_meters_f32(),
            piece.l.as_meters_f32(),
        );
        Placed {
            board: piece.board.clone(),
            corner: self.corner + self.rotation * offset,
            rotation: self.rotation,
            grain: self.grain + offset,
        }
    }

    /// The space the board takes up on the saw, as its near and far corners.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let size = board_size(&self.board);
        let corners = (0..8).map(|k| {
            let corner = Vec3::new((k & 1) as f32, ((k >> 1) & 1) as f32, (k >> 2) as f32);
            self.corner + self.rotation * (corner * size)
        });
        corners.fold((Vec3::MAX, Vec3::MIN), |(low, high), corner| {
            (low.min(corner), high.max(corner))
        })
    }
}

/// A board's size along its own axes: `w`, `t`, `l` as x, y, z.
fn board_size(board: &Board) -> Vec3 {
    Vec3::new(
        board.width().as_meters_f32(),
        board.thickness().as_meters_f32(),
        board.length().as_meters_f32(),
    )
}

/// The rotation that lays `board` on a saw as `lie` says, and where its corner ends up from the
/// near corner of the space its wood takes up, which woodsim measures the saw's settings from:
/// `lie`'s own turn, from the board's axes onto the saw's, with the mesh's axes (`w`, `t`, `l`)
/// and the station's (across, up, along) in place of woodsim's.
fn lay(board: &Board, lie: Orientation) -> (Quat, Vec3) {
    let [l, w, t] = lie
        .rotation()
        .columns()
        .map(|[along, across, up]| Vec3::new(across as f32, up as f32, along as f32));
    let rotation = Quat::from_mat3(&Mat3::from_cols(w, t, l));
    let near = board
        .corners()
        .into_iter()
        .map(|[l, w, t]| {
            rotation * Vec3::new(w.as_meters_f32(), t.as_meters_f32(), l.as_meters_f32())
        })
        .fold(Vec3::MAX, Vec3::min);
    (rotation, -near)
}

/// What to draw on a saw: a board in one piece, or both sides of a cut that went through it.
#[derive(Debug, Default, PartialEq)]
pub struct SawBoards {
    pub whole: Vec<Placed>,
    pub piece: Vec<Placed>,
    pub offcut: Vec<Placed>,
}

impl SawBoards {
    /// The boards of `part`: one for a board, one for each board of a glue-up, or none.
    pub fn get(&self, part: Part) -> &[Placed] {
        match part {
            Part::Whole => &self.whole,
            Part::Piece => &self.piece,
            Part::Offcut => &self.offcut,
        }
    }
}

/// An entity drawing one board on a saw, with what `place_parts` keeps up to date on it.
pub type SawPart = (
    &'static Part,
    &'static Slot,
    &'static Mesh3d,
    &'static mut Transform,
    &'static mut Visibility,
    &'static mut Shape,
);

/// Which of a part's boards an entity on a saw draws: a board, or one board of a glue-up.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Slot(pub usize);

/// What the camera is showing: the whole shop, or one station up close. A station's keyboard
/// controls only work while it's up close.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShopView {
    #[default]
    Overview,
    TableSaw,
    MiterSaw,
    Jointer,
    Planer,
    Bench,
    /// The photo studio, where a part from the bench is set up and rendered.
    Studio,
}

/// The floor runs this far from the middle of the shop in each direction, in meters.
pub const SHOP_HALF_SIZE: f32 = 3.0;

/// How bright the camera expects the scene to be. Lower is brighter.
const EXPOSURE_EV100: f32 = 7.0;

/// How steeply the overview looks down at the floor.
const OVERVIEW_PITCH: f32 = 46.0 * PI / 180.0;
const MIN_DISTANCE: f32 = 1.5;
const MAX_DISTANCE: f32 = 10.0;
/// How far the overview's focus may wander from the middle of the shop.
const PAN_LIMIT: f32 = 2.5;
/// Keyboard panning speed, as a fraction of the camera's distance per second.
const PAN_SPEED: f32 = 0.6;
/// Dragging the floor pans this far per pixel, as a fraction of the camera's distance.
const DRAG_PAN: f32 = 0.0012;
/// Each line of scroll zooms in by this fraction. Trackpads scroll in pixels instead.
const ZOOM_PER_LINE: f32 = 0.1;
const PIXELS_PER_LINE: f32 = 20.0;

/// Where the overview camera is aimed and how it's oriented.
#[derive(Resource)]
struct OverviewCamera {
    /// The point on the floor the camera looks at.
    focus: Vec3,
    /// The camera's direction around the focus, measured from +z toward +x.
    yaw: f32,
    distance: f32,
}

impl Default for OverviewCamera {
    fn default() -> OverviewCamera {
        OverviewCamera {
            // Between the saws and the workbench.
            focus: Vec3::new(0.0, 0.5, 0.1),
            yaw: 30.0 * PI / 180.0,
            distance: 8.0,
        }
    }
}

impl OverviewCamera {
    fn pose(&self) -> Transform {
        let toward_camera = Vec3::new(
            self.yaw.sin() * OVERVIEW_PITCH.cos(),
            OVERVIEW_PITCH.sin(),
            self.yaw.cos() * OVERVIEW_PITCH.cos(),
        );
        Transform::from_translation(self.focus + toward_camera * self.distance)
            .looking_at(self.focus, Vec3::Y)
    }

    /// Directions across the floor: away from the camera, and to its right.
    fn ground_axes(&self) -> (Vec3, Vec3) {
        let forward = Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos());
        let right = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        (forward, right)
    }

    /// Slides the focus across the floor; it keeps its height.
    fn pan(&mut self, by: Vec3) {
        self.focus.x = (self.focus.x + by.x).clamp(-PAN_LIMIT, PAN_LIMIT);
        self.focus.z = (self.focus.z + by.z).clamp(-PAN_LIMIT, PAN_LIMIT);
    }

    fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }
}

/// How far the view at a saw is turned from where you'd stand to work there: around the work
/// and up over it, in radians, and how much nearer or further, as a factor. Coming to a saw puts
/// it back where you'd stand, as asking to does.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct SawView {
    turn: f32,
    tip: f32,
    zoom: f32,
}

impl Default for SawView {
    fn default() -> SawView {
        SawView {
            turn: 0.0,
            tip: 0.0,
            zoom: 1.0,
        }
    }
}

/// How far down a saw's view looks, at least and at most, and how near and far it comes.
const SAW_LEAST_PITCH: f32 = 8.0 * PI / 180.0;
const SAW_MOST_PITCH: f32 = 85.0 * PI / 180.0;
const SAW_NEAREST: f32 = 0.3;
const SAW_FARTHEST: f32 = 2.5;
/// How far a saw's view turns per pixel dragged, in radians, as the bench's does.
const SAW_TURN_PER_PIXEL: f32 = 0.006;

impl SawView {
    /// Turned around the work by `yaw` and up over it by `pitch`, in radians, from looking
    /// from `toward` it, as far as it turns up or down.
    fn turned(&mut self, toward: Vec3, yaw: f32, pitch: f32) {
        let (_, standing, _) = around(toward);
        self.turn += yaw;
        self.tip = (standing + self.tip + pitch).clamp(SAW_LEAST_PITCH, SAW_MOST_PITCH) - standing;
    }

    /// Nearer by `factor`, from `distance` away, as near or far as it comes.
    fn zoomed(&mut self, distance: f32, factor: f32) {
        self.zoom = (distance * self.zoom * factor).clamp(SAW_NEAREST, SAW_FARTHEST) / distance;
    }

    /// Where to look from at `target`, turned as it's been from looking from `toward` it (the
    /// way, and how far): around it, up over it, and nearer or further.
    fn from(&self, target: Vec3, toward: Vec3) -> Transform {
        let (yaw, pitch, distance) = around(toward);
        let (yaw, pitch) = (
            yaw + self.turn,
            (pitch + self.tip).clamp(SAW_LEAST_PITCH, SAW_MOST_PITCH),
        );
        let distance = (distance * self.zoom).clamp(SAW_NEAREST, SAW_FARTHEST);
        let toward = Vec3::new(
            yaw.sin() * pitch.cos(),
            pitch.sin(),
            yaw.cos() * pitch.cos(),
        );
        Transform::from_translation(target + toward * distance).looking_at(target, Vec3::Y)
    }
}

/// `toward`, a way to look from and how far, as how far it's turned around from +z toward +x,
/// how far up it looks from, and how far it is.
fn around(toward: Vec3) -> (f32, f32, f32) {
    let distance = toward.length();
    (
        toward.x.atan2(toward.z),
        (toward.y / distance).asin(),
        distance,
    )
}

/// The station under the pointer, and how many of its parts are, so moving from one part to
/// the next doesn't flicker.
#[derive(Resource, Default)]
pub struct HoveredStation {
    pub station: Option<ShopView>,
    parts: u32,
}

#[derive(Component)]
pub struct ShopCamera;

/// Textures and materials more than one part of the shop uses.
#[derive(Resource)]
pub struct ShopAssets {
    /// Pine, for boards. Use it on meshes from `board_mesh` or `lumber_mesh`, which lay the
    /// grain along the board.
    pub wood: Handle<StandardMaterial>,
    /// How a piece of wood looks under the pointer when it can be picked up.
    pub wood_highlight: Handle<StandardMaterial>,
    /// Maple, for bench tops.
    pub maple: Handle<StandardMaterial>,
    pub floor: Handle<StandardMaterial>,
    pub pegboard: Handle<Image>,
}

impl FromWorld for ShopAssets {
    fn from_world(world: &mut World) -> ShopAssets {
        let mut images = world.resource_mut::<Assets<Image>>();
        let pine = images.add(textures::wood(&PINE));
        let maple = images.add(textures::wood(&MAPLE));
        let floor = images.add(textures::floor());
        let pegboard = images.add(textures::pegboard());

        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let wood = |tint: Color, texture: &Handle<Image>| StandardMaterial {
            base_color: tint,
            base_color_texture: Some(texture.clone()),
            perceptual_roughness: 0.7,
            reflectance: 0.3,
            ..default()
        };
        ShopAssets {
            wood: materials.add(wood(Color::WHITE, &pine)),
            wood_highlight: materials.add(StandardMaterial {
                emissive: LinearRgba::new(0.9, 0.6, 0.25, 1.0) * 60.0,
                ..wood(Color::srgb(1.0, 0.95, 0.85), &pine)
            }),
            maple: materials.add(wood(Color::WHITE, &maple)),
            floor: materials.add(StandardMaterial {
                base_color_texture: Some(floor),
                perceptual_roughness: 0.8,
                // The floor's UVs span the whole shop; repeat the pattern every FLOOR_TILE.
                uv_transform: Affine2::from_scale(Vec2::splat(2.0 * SHOP_HALF_SIZE / FLOOR_TILE)),
                ..default()
            }),
            pegboard,
        }
    }
}

pub struct ShopPlugin;

impl Plugin for ShopPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShopView>()
            .init_resource::<ShopAssets>()
            .init_resource::<OverviewCamera>()
            .init_resource::<SawView>()
            .init_resource::<HoveredStation>()
            .add_systems(Startup, spawn_shop)
            .add_systems(
                Update,
                (
                    switch_view,
                    move_overview.run_if(resource_equals(ShopView::Overview)),
                    look_around_saw,
                    follow_view,
                    outline_hovered_station,
                )
                    .chain(),
            );
    }
}

/// The floor and the camera. The walls, lights, and furnishings are in `scenery`.
fn spawn_shop(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    shop: Res<ShopAssets>,
    overview: Res<OverviewCamera>,
) {
    commands
        .spawn((
            Mesh3d(
                meshes.add(
                    Plane3d::default()
                        .mesh()
                        .size(2.0 * SHOP_HALF_SIZE, 2.0 * SHOP_HALF_SIZE),
                ),
            ),
            MeshMaterial3d(shop.floor.clone()),
        ))
        .observe(drag_floor);

    commands.spawn((
        Camera3d::default(),
        overview.pose(),
        ShopCamera,
        Exposure {
            ev100: EXPOSURE_EV100,
        },
        Tonemapping::TonyMcMapface,
        ColorGrading {
            global: ColorGradingGlobal {
                post_saturation: 1.1,
                ..default()
            },
            ..default()
        },
        // The lawn fades into the sky far off, so the ground never ends at a hard edge.
        DistanceFog {
            color: SKY,
            falloff: FogFalloff::Linear {
                start: 20.0,
                end: 45.0,
            },
            ..default()
        },
    ));
}

/// Makes `entity` part of `station`'s body: in the overview, pointing at it outlines the
/// station, and clicking it, from anywhere, goes to that station.
pub fn station_part(mut entity: EntityCommands, station: ShopView) {
    entity
        .observe(
            move |_: On<PointerOver>, mut hovered: ResMut<HoveredStation>| {
                if hovered.station == Some(station) {
                    hovered.parts += 1;
                } else {
                    *hovered = HoveredStation {
                        station: Some(station),
                        parts: 1,
                    };
                }
            },
        )
        .observe(
            move |_: On<PointerOut>, mut hovered: ResMut<HoveredStation>| {
                if hovered.station == Some(station) {
                    hovered.parts = hovered.parts.saturating_sub(1);
                    if hovered.parts == 0 {
                        hovered.station = None;
                    }
                }
            },
        )
        .observe(move |click: On<PointerClick>, mut stations: Stations| {
            if click.button != PointerButton::Primary || stations.gesture.dragged() {
                return;
            }
            // Holding a board, a click on a station sets it down there.
            if stations.hand.0.is_some() {
                stations.set_down(station, click.hit.position);
            }
            if *stations.view != station {
                *stations.view = station;
            }
        });
}

/// Everything a click on a station can change: where the camera is, what's in hand, and the
/// stations a board can be set down at.
#[derive(SystemParam)]
pub struct Stations<'w> {
    pub view: ResMut<'w, ShopView>,
    pub gesture: Res<'w, Gesture>,
    pub hand: ResMut<'w, Hand>,
    pub table_saw: ResMut<'w, TableSawStation>,
    pub miter_saw: ResMut<'w, MiterSawStation>,
    pub jointer: ResMut<'w, JointerStation>,
    pub planer: ResMut<'w, PlanerStation>,
    pub bench: ResMut<'w, BenchStation>,
}

impl Stations<'_> {
    /// Sets the board in hand down at `station`, near `near` if it's somewhere in the shop
    /// the station has room, as `hand::set_down` does.
    pub fn set_down(&mut self, station: ShopView, near: Option<Vec3>) -> bool {
        set_down(
            station,
            near,
            &mut self.hand,
            (
                &mut self.table_saw,
                &mut self.miter_saw,
                &mut self.jointer,
                &mut self.planer,
            ),
            &mut self.bench,
        )
    }
}

/// Esc backs out to the overview; Tab goes around the stations: the table saw, the miter saw,
/// the jointer, the planer, the workbench.
fn switch_view(actions: Res<Actions>, mut view: ResMut<ShopView>) {
    for action in actions.started() {
        let next = match action {
            // The studio is entered from the bench, and backs out to it.
            Action::Back if *view == ShopView::Studio => ShopView::Bench,
            Action::Back => ShopView::Overview,
            Action::GoTo(station) => station,
            Action::NextStation => match *view {
                ShopView::TableSaw => ShopView::MiterSaw,
                ShopView::MiterSaw => ShopView::Jointer,
                ShopView::Jointer => ShopView::Planer,
                ShopView::Planer => ShopView::Bench,
                ShopView::Overview | ShopView::Bench | ShopView::Studio => ShopView::TableSaw,
            },
            _ => continue,
        };
        if *view != next {
            *view = next;
        }
    }
}

/// Pans with the keyboard, rotates in quarter turns, and zooms with the scroll wheel, a pinch,
/// or the zoom buttons.
fn move_overview(
    actions: Res<Actions>,
    scroll: Res<AccumulatedMouseScroll>,
    on_hud: Res<OnHud>,
    mut pinches: MessageReader<PinchGesture>,
    time: Res<Time>,
    mut overview: ResMut<OverviewCamera>,
) {
    let (forward, right) = overview.ground_axes();
    let mut direction = Vec3::ZERO;
    for (arrow, toward) in [
        (Arrow::Up, forward),
        (Arrow::Down, -forward),
        (Arrow::Right, right),
        (Arrow::Left, -right),
    ] {
        if actions.held(Action::Pan(arrow)) {
            direction += toward;
        }
    }
    if direction != Vec3::ZERO {
        let step = PAN_SPEED * overview.distance * time.delta_secs();
        overview.pan(direction.normalize() * step);
    }

    if actions.just(Action::RotateLeft) {
        overview.yaw += FRAC_PI_2;
    }
    if actions.just(Action::RotateRight) {
        overview.yaw -= FRAC_PI_2;
    }

    // Scrolling over the HUD scrolls the inventory, not the view.
    let mut lines = match scroll.unit {
        _ if on_hud.over() => 0.0,
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / PIXELS_PER_LINE,
    };
    // A click on a zoom button goes as far as a few lines of scrolling.
    if actions.just(Action::ZoomIn) {
        lines += 3.0;
    }
    if actions.just(Action::ZoomOut) {
        lines -= 3.0;
    }
    let pinch: f32 = pinches.read().map(|pinch| pinch.0).sum();
    if lines != 0.0 || pinch != 0.0 {
        overview.zoom((1.0 - ZOOM_PER_LINE).powf(lines) * (1.0 - pinch));
    }
}

/// Dragging the floor in the overview slides the shop under the pointer.
fn drag_floor(drag: On<PointerDrag>, view: Res<ShopView>, mut overview: ResMut<OverviewCamera>) {
    if *view != ShopView::Overview {
        return;
    }
    let (forward, right) = overview.ground_axes();
    // The floor is seen at an angle, so a pixel of vertical drag covers more of it.
    let along = forward * drag.delta.y / OVERVIEW_PITCH.sin() - right * drag.delta.x;
    let scale = DRAG_PAN * overview.distance;
    overview.pan(along * scale);
}

/// Where the operator stands at each saw (in front, a little to the side, looking down), and
/// the work they're looking at, as where the station starts, the eye from there, and what it
/// looks at. At the bench, the view turns; see `BenchView`.
fn standing_at(station: ShopView, using_gauge: bool) -> Option<(Vec3, Vec3, Vec3)> {
    let (origin, eye, target) = match station {
        ShopView::Overview => return None,
        // The miter gauge's head and a board held across the table stand between a low eye and
        // the blade, so with the gauge in use the view rises to look down over them.
        ShopView::TableSaw if using_gauge => (
            table_saw::ORIGIN,
            Vec3::new(-0.1, 1.3, 1.0),
            Vec3::new(-0.02, 0.0, 0.12),
        ),
        ShopView::TableSaw => (
            table_saw::ORIGIN,
            Vec3::new(-0.3, 0.79, 1.35),
            Vec3::new(0.1, 0.0, -0.05),
        ),
        // A little to the left, as a right hand on the handle has it, so the blade's open face
        // shows rather than its edge.
        ShopView::MiterSaw => (
            miter_saw::ORIGIN,
            Vec3::new(-0.3, 0.7, 1.15),
            Vec3::new(0.0, 0.12, 0.0),
        ),
        // Off the infeed end, on the open side, looking down along the fence at the gap the
        // cutterhead turns in.
        // In front of the infeed wing and off to the side, looking down at the head where the
        // board goes in.
        ShopView::Planer => (
            planer::ORIGIN,
            Vec3::new(-0.7, 0.8, 1.2),
            Vec3::new(0.0, 0.06, 0.12),
        ),
        ShopView::Jointer => (
            jointer::ORIGIN,
            Vec3::new(-0.42, 0.72, 0.9),
            Vec3::new(0.06, 0.0, -0.08),
        ),
        ShopView::Bench | ShopView::Studio => return None,
    };
    Some((origin, eye, target))
}

/// Where the camera looks from at a saw: where the operator stands, turned around the work,
/// up or down, and nearer or further as `view` says.
fn station_pose(station: ShopView, using_gauge: bool, view: &SawView) -> Option<Transform> {
    let (origin, eye, target) = standing_at(station, using_gauge)?;
    Some(view.from(origin + target, eye - target))
}

/// At a saw, dragging the shop around it, or right-dragging anywhere, turns the view around the
/// work and tips it up or down; scrolling or pinching zooms; and the view comes back to where
/// you'd stand when asked, and on coming to the saw. A left drag that starts on a board, a
/// fence, or a stop moves that instead.
#[allow(clippy::too_many_arguments)]
fn look_around_saw(
    view: Res<ShopView>,
    actions: Res<Actions>,
    table: Res<TableSawStation>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut pinches: MessageReader<PinchGesture>,
    (gesture, on_hud): (Res<Gesture>, Res<OnHud>),
    mut moved: Local<f32>,
    mut saw_view: ResMut<SawView>,
) {
    if view.is_changed() || actions.just(Action::ResetView) {
        *saw_view = SawView::default();
    }
    let Some((_, eye, target)) = standing_at(*view, table.using_gauge()) else {
        return;
    };
    let toward = eye - target;
    // How far the pointer's moved since a button went down, to tell a drag from a click.
    let buttons = [MouseButton::Left, MouseButton::Right];
    if mouse.any_just_pressed(buttons) {
        *moved = 0.0;
    }
    if mouse.any_pressed(buttons) {
        *moved += motion.delta.length();
    }
    let turning =
        mouse.pressed(MouseButton::Right) || mouse.pressed(MouseButton::Left) && !gesture.grabbed();
    if turning && !on_hud.pressed() && *moved > DRAG_THRESHOLD && motion.delta != Vec2::ZERO {
        saw_view.turned(
            toward,
            -motion.delta.x * SAW_TURN_PER_PIXEL,
            motion.delta.y * SAW_TURN_PER_PIXEL,
        );
    }
    // Scrolling over the HUD scrolls the inventory, not the view.
    let lines = match scroll.unit {
        _ if on_hud.over() => 0.0,
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / PIXELS_PER_LINE,
    };
    let pinch: f32 = pinches.read().map(|pinch| pinch.0).sum();
    if lines != 0.0 || pinch != 0.0 {
        saw_view.zoomed(
            toward.length(),
            (1.0 - ZOOM_PER_LINE).powf(lines) * (1.0 - pinch),
        );
    }
}

/// Eases the camera toward the overview or the station being worked at.
fn follow_view(
    view: Res<ShopView>,
    overview: Res<OverviewCamera>,
    (bench_view, saw_view): (Res<BenchView>, Res<SawView>),
    table: Res<TableSawStation>,
    studio: Res<Studio>,
    time: Res<Time>,
    mut camera: Single<&mut Transform, With<ShopCamera>>,
) {
    let goal = match *view {
        ShopView::Bench => bench_view.pose(),
        ShopView::Studio => studio::camera_pose(&studio).unwrap_or_else(|| bench_view.pose()),
        _ => station_pose(*view, table.using_gauge(), &saw_view).unwrap_or_else(|| overview.pose()),
    };
    // The studio is far from the shop: going there or back is a cut, not a flight. In it, the
    // camera follows the setup exactly, so what's seen is what renders.
    let far = camera.translation.distance(goal.translation) > 10.0;
    if far || *view == ShopView::Studio {
        **camera = goal;
        return;
    }
    let t = 1.0 - (-6.0 * time.delta_secs()).exp();
    camera.translation = camera.translation.lerp(goal.translation, t);
    camera.rotation = camera.rotation.slerp(goal.rotation, t);
}

/// In the overview, outlines the station under the pointer to show it can be clicked.
fn outline_hovered_station(
    view: Res<ShopView>,
    hovered: Res<HoveredStation>,
    planer_station: Res<PlanerStation>,
    mut gizmos: Gizmos,
) {
    if *view != ShopView::Overview {
        return;
    }
    let outline = match hovered.station {
        Some(ShopView::TableSaw) => block_transform(table_saw::BOUNDS, table_saw::ORIGIN),
        Some(ShopView::MiterSaw) => block_transform(miter_saw::BOUNDS, miter_saw::ORIGIN),
        Some(ShopView::Jointer) => block_transform(jointer::BOUNDS, jointer::ORIGIN),
        Some(ShopView::Planer) => {
            block_transform(planer::bounds(&planer_station.machine), planer::ORIGIN)
        }
        Some(ShopView::Bench) => block_transform(bench::BOUNDS, bench::ORIGIN),
        _ => return,
    };
    gizmos.cube(outline, Color::srgb(1.0, 0.85, 0.3));
}

/// The board and grain a mesh was last built from, so it's only rebuilt when they change and
/// not every frame the blade moves.
#[derive(Component, Default)]
pub struct Shape(Option<(Board, Vec3)>);

impl Shape {
    /// The board the mesh was built from.
    pub fn board(&self) -> Option<&Board> {
        self.0.as_ref().map(|(board, _)| board)
    }

    /// The shape of a mesh just built for `placed`.
    pub fn built(placed: &Placed) -> Shape {
        Shape(Some((placed.board.clone(), placed.grain)))
    }

    /// Rebuilds `mesh` for `placed` unless it was built for it already.
    pub fn update(&mut self, placed: &Placed, mesh: &Mesh3d, meshes: &mut Assets<Mesh>) {
        let shape = Shape::built(placed);
        if self.0 != shape.0
            && let Some(mut mesh) = meshes.get_mut(&mesh.0)
        {
            *mesh = lumber_mesh(&placed.board, placed.grain);
            *self = shape;
        }
    }
}

/// Places each board of each part on a saw, rebuilding its mesh to match the board and hiding
/// the entities with no board to draw. Each board needs its own mesh. `frame` turns the saw's own
/// frame (across, up, along) into the station's coordinates, which start at `origin`. Returns
/// the boards no entity draws yet, with where each goes, for the station to spawn.
pub fn place_parts<'a>(
    boards: &SawBoards,
    origin: Vec3,
    frame: Quat,
    meshes: &mut Assets<Mesh>,
    parts: impl IntoIterator<
        Item = (
            &'a Part,
            &'a Slot,
            &'a Mesh3d,
            Mut<'a, Transform>,
            Mut<'a, Visibility>,
            Mut<'a, Shape>,
        ),
    >,
) -> Vec<(Part, Slot, Transform, Placed)> {
    let pose = |placed: &Placed| {
        Transform::from_translation(origin + frame * placed.corner)
            .with_rotation(frame * placed.rotation)
    };
    let mut drawn = Vec::new();
    for (part, slot, mesh, mut transform, mut visibility, mut shape) in parts {
        match boards.get(*part).get(slot.0) {
            Some(placed) => {
                *transform = pose(placed);
                shape.update(placed, mesh, meshes);
                *visibility = Visibility::Inherited;
                drawn.push((*part, *slot));
            }
            None => *visibility = Visibility::Hidden,
        }
    }
    [Part::Whole, Part::Piece, Part::Offcut]
        .into_iter()
        .flat_map(|part| {
            boards
                .get(part)
                .iter()
                .enumerate()
                .map(move |(k, placed)| (part, Slot(k), placed))
        })
        .filter(|(part, slot, _)| !drawn.contains(&(*part, *slot)))
        .map(|(part, slot, placed)| (part, slot, pose(placed), placed.clone()))
        .collect()
}

/// A rounded box filling `block`, given in the coordinates of a station at `origin`.
pub fn rounded_block(meshes: &mut Assets<Mesh>, block: Block, origin: Vec3) -> (Mesh3d, Transform) {
    (
        Mesh3d(meshes.add(rounded_box(block.max - block.min))),
        Transform::from_translation(origin + block.min),
    )
}

/// Places a unit cube so it fills `block`, given in the coordinates of a station at `origin`.
pub fn block_transform(block: Block, origin: Vec3) -> Transform {
    Transform::from_translation(origin + (block.min + block.max) / 2.0)
        .with_scale(block.max - block.min)
}

/// Rounds a position in meters to the nearest 1/16", the finest mark on a fence or stop scale.
pub fn snap_to_sixteenth(meters: f32) -> Length {
    snap(meters, 16)
}

/// Rounds a position in meters to the nearest `1/fraction` of an inch.
pub fn snap(meters: f32, fraction: i64) -> Length {
    let steps = (meters / inches(1.0) * fraction as f32).round() as i64;
    Length::from_inch_fraction(steps, fraction)
}

/// Where the pointer meets a level plane `height` meters above the floor.
pub fn pointer_on_plane(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    position: Vec2,
    height: f32,
) -> Option<Vec3> {
    let ray = camera.viewport_to_world(camera_transform, position).ok()?;
    ray.plane_intersection_point(Vec3::Y * height, InfinitePlane3d::new(Vec3::Y))
}

/// Returns an observer that swaps the entity's material when event `E` happens to it.
pub fn set_material_on<E: EntityEvent>(
    material: Handle<StandardMaterial>,
) -> impl Fn(On<E>, Query<&mut MeshMaterial3d<StandardMaterial>>) {
    move |event, mut materials| {
        if let Ok(mut current) = materials.get_mut(event.event_target()) {
            current.0 = material.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saws_view_turns_around_the_work_and_comes_back_to_where_you_stand() {
        for (station, gauge) in [
            (ShopView::TableSaw, false),
            (ShopView::TableSaw, true),
            (ShopView::MiterSaw, false),
            (ShopView::Jointer, false),
            (ShopView::Planer, false),
        ] {
            let (origin, eye, target) = standing_at(station, gauge).unwrap();
            let (work, toward) = (origin + target, eye - target);
            let standing = station_pose(station, gauge, &SawView::default()).unwrap();
            assert!(
                standing.translation.distance(origin + eye) < 1e-5,
                "{station:?}"
            );

            // Turned halfway around, it looks at the same work from as far away, from behind.
            let mut view = SawView::default();
            view.turned(toward, PI, 0.0);
            let behind = station_pose(station, gauge, &view).unwrap();
            let seen = work - behind.translation;
            assert!((seen.length() - toward.length()).abs() < 1e-5);
            assert!(behind.forward().dot(seen.normalize()) > 1.0 - 1e-5);
            assert!(seen.z * toward.z > 0.0 && seen.x * toward.x >= 0.0);

            // Tipped up as far as it goes, and no further: a little back down comes right
            // down, not from wherever the drag went past.
            let pitch = |view: &SawView| {
                let pose = station_pose(station, gauge, view).unwrap();
                ((pose.translation - work).y / toward.length()).asin()
            };
            view.turned(toward, 0.0, 10.0);
            assert!((pitch(&view) - SAW_MOST_PITCH).abs() < 1e-5);
            view.turned(toward, 0.0, -0.1);
            assert!((pitch(&view) - (SAW_MOST_PITCH - 0.1)).abs() < 1e-5);

            // Zoomed in as near as it comes, and back out from there.
            view.zoomed(toward.length(), 0.01);
            let near = station_pose(station, gauge, &view).unwrap();
            assert!((near.translation.distance(work) - SAW_NEAREST).abs() < 1e-5);
            view.zoomed(toward.length(), 2.0);
            let out = station_pose(station, gauge, &view).unwrap();
            assert!((out.translation.distance(work) - 2.0 * SAW_NEAREST).abs() < 1e-5);
        }
    }

    #[test]
    fn every_way_a_board_lies_it_fills_its_space_from_the_near_corner_as_woodsim_says() {
        let board = Board::new(
            Length::from_inches(24),
            Length::from_inches(3),
            Length::from_inch_fraction(3, 4),
        );
        let flat = Orientation::default();
        for mut lie in [
            flat,
            flat.tipped(),
            flat.flipped(),
            flat.flipped().tipped(),
            flat.stood(),
            flat.stood().flipped(),
        ] {
            for _ in 0..4 {
                let placed = Placed::lying(&board, lie, Vec3::ZERO);
                let (near, far) = placed.bounds();
                assert!(near.length() < 1e-6, "{lie:?} starts at {near}");
                // Across, up, and along, it reaches as far as the axis woodsim lays that way,
                // and the board's own corner is at the far side of any it lays backward.
                for ((axis, backward), size, corner) in [
                    (lie.across(), far.x, placed.corner.x),
                    (lie.up(), far.y, placed.corner.y),
                    (lie.along(), far.z, placed.corner.z),
                ] {
                    assert!(
                        (size - axis.of(&board).as_meters_f32()).abs() < 1e-6,
                        "{lie:?}"
                    );
                    let expected = if backward { size } else { 0.0 };
                    assert!((corner - expected).abs() < 1e-6, "{lie:?}");
                }
                lie = lie.turned_left();
            }
        }
    }

    #[test]
    fn overview_pans_relative_to_where_the_camera_faces() {
        let mut overview = OverviewCamera {
            focus: Vec3::ZERO,
            yaw: FRAC_PI_2,
            distance: 4.0,
        };
        // Facing -x from the +x side: "forward" is -x and "right" is -z.
        let (forward, right) = overview.ground_axes();
        assert!(forward.abs_diff_eq(Vec3::NEG_X, 1e-6));
        assert!(right.abs_diff_eq(Vec3::NEG_Z, 1e-6));

        overview.pan(forward);
        assert!(overview.focus.abs_diff_eq(Vec3::NEG_X, 1e-6));
    }

    #[test]
    fn overview_stays_over_the_shop_and_within_zoom_limits() {
        let mut overview = OverviewCamera::default();
        overview.pan(Vec3::new(100.0, 5.0, -100.0));
        assert_eq!(
            overview.focus,
            Vec3::new(PAN_LIMIT, OverviewCamera::default().focus.y, -PAN_LIMIT)
        );

        overview.zoom(0.0);
        assert_eq!(overview.distance, MIN_DISTANCE);
        overview.zoom(100.0);
        assert_eq!(overview.distance, MAX_DISTANCE);
    }

    #[test]
    fn overview_camera_looks_down_at_its_focus() {
        let overview = OverviewCamera::default();
        let pose = overview.pose();

        assert!((pose.translation.distance(overview.focus) - overview.distance).abs() < 1e-5);
        let looking = pose.forward().as_vec3();
        let to_focus = (overview.focus - pose.translation).normalize();
        assert!(looking.abs_diff_eq(to_focus, 1e-5));
        assert!(pose.translation.y > overview.focus.y);
    }
}

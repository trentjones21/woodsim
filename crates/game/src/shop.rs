//! The shop around the stations: shared layout helpers, the camera, the readout, and moving
//! between the overview and the stations.

use std::{
    f32::consts::{FRAC_PI_2, PI},
    time::Duration,
};

use bevy::{
    camera::Exposure,
    core_pipeline::tonemapping::Tonemapping,
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    ecs::system::EntityCommands,
    input::{
        gestures::PinchGesture,
        mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    },
    math::Affine2,
    pbr::{DistanceFog, FogFalloff},
    picking::pointer::PointerButton,
    prelude::*,
    render::view::{ColorGrading, ColorGradingGlobal},
    time::common_conditions::on_real_timer,
};
use woodsim::{Axis, Board, Cut, Kerf, Length, Orientation, Piece};

use crate::{
    chop::MiterSawStation,
    feed::TableSawStation,
    meshes::{lumber_mesh, rounded_box},
    miter_saw,
    scenery::SKY,
    table_saw,
    textures::{self, FLOOR_TILE, MAPLE, PINE},
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
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
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
        let offset = Vec3::new(piece.w.as_meters_f32(), 0.0, piece.l.as_meters_f32());
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
/// near corner of the space it takes up. Matches `woodsim::Orientation`: any flip comes first,
/// about the saw's along axis, then the quarter turns about the up axis.
fn lay(board: &Board, lie: Orientation) -> (Quat, Vec3) {
    let flip = if lie.is_flipped() {
        Quat::from_rotation_z(PI)
    } else {
        Quat::IDENTITY
    };
    let rotation = Quat::from_rotation_y(lie.turns() as f32 * FRAC_PI_2) * flip;
    let placed = Placed {
        board: board.clone(),
        corner: Vec3::ZERO,
        rotation,
        grain: Vec3::ZERO,
    };
    (rotation, -placed.bounds().0)
}

/// What to draw on a saw: a board in one piece, or both sides of a cut that went through it.
#[derive(Debug, Default, PartialEq)]
pub struct SawBoards {
    pub whole: Option<Placed>,
    pub piece: Option<Placed>,
    pub offcut: Option<Placed>,
}

impl SawBoards {
    pub fn get(&self, part: Part) -> Option<&Placed> {
        match part {
            Part::Whole => self.whole.as_ref(),
            Part::Piece => self.piece.as_ref(),
            Part::Offcut => self.offcut.as_ref(),
        }
    }
}

/// What the camera is showing: the whole shop, or one station up close. A station's keyboard
/// controls only work while it's up close.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShopView {
    #[default]
    Overview,
    TableSaw,
    MiterSaw,
}

pub const OVERVIEW_HELP: &str = "W/A/S/D or arrows: pan   Drag the floor: pan   \
                                 Scroll or pinch: zoom   Q/E: rotate\n\
                                 Click a saw to work at it   Tab: go to the table saw";

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
            // Between the two saws.
            focus: Vec3::new(-0.4, 0.5, -0.4),
            yaw: 30.0 * PI / 180.0,
            distance: 7.6,
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

/// The station under the pointer, and how many of its parts are, so moving from one part to
/// the next doesn't flicker.
#[derive(Resource, Default)]
struct HoveredStation {
    station: Option<ShopView>,
    parts: u32,
}

#[derive(Component)]
struct ShopCamera;

#[derive(Component)]
enum ReadoutLine {
    Reading,
    Status,
    Help,
    CutList,
    /// Frame rate and frame time, so performance can be checked while playing.
    Fps,
}

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
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .init_resource::<ShopView>()
            .init_resource::<ShopAssets>()
            .init_resource::<OverviewCamera>()
            .init_resource::<HoveredStation>()
            .add_systems(Startup, (spawn_shop, spawn_readout))
            .add_systems(
                Update,
                (
                    switch_view,
                    move_overview.run_if(resource_equals(ShopView::Overview)),
                    follow_view,
                    outline_hovered_station,
                    update_readout,
                )
                    .chain(),
            )
            // Often enough to follow, slow enough to read.
            .add_systems(
                Update,
                update_fps.run_if(on_real_timer(Duration::from_millis(250))),
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
        .observe(move |click: On<PointerClick>, mut view: ResMut<ShopView>| {
            if click.button == PointerButton::Primary && *view != station {
                *view = station;
            }
        });
}

/// Esc backs out to the overview; Tab goes from one saw to the other.
fn switch_view(keys: Res<ButtonInput<KeyCode>>, mut view: ResMut<ShopView>) {
    if keys.just_pressed(KeyCode::Escape) && *view != ShopView::Overview {
        *view = ShopView::Overview;
    }
    if keys.just_pressed(KeyCode::Tab) {
        *view = match *view {
            ShopView::TableSaw => ShopView::MiterSaw,
            ShopView::Overview | ShopView::MiterSaw => ShopView::TableSaw,
        };
    }
}

/// Pans with the keyboard, rotates in quarter turns, and zooms with the scroll wheel or a pinch.
fn move_overview(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    mut pinches: MessageReader<PinchGesture>,
    time: Res<Time>,
    mut overview: ResMut<OverviewCamera>,
) {
    let (forward, right) = overview.ground_axes();
    let mut direction = Vec3::ZERO;
    for (pan_keys, toward) in [
        ([KeyCode::KeyW, KeyCode::ArrowUp], forward),
        ([KeyCode::KeyS, KeyCode::ArrowDown], -forward),
        ([KeyCode::KeyD, KeyCode::ArrowRight], right),
        ([KeyCode::KeyA, KeyCode::ArrowLeft], -right),
    ] {
        if keys.any_pressed(pan_keys) {
            direction += toward;
        }
    }
    if direction != Vec3::ZERO {
        let step = PAN_SPEED * overview.distance * time.delta_secs();
        overview.pan(direction.normalize() * step);
    }

    if keys.just_pressed(KeyCode::KeyQ) {
        overview.yaw += FRAC_PI_2;
    }
    if keys.just_pressed(KeyCode::KeyE) {
        overview.yaw -= FRAC_PI_2;
    }

    let lines = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / PIXELS_PER_LINE,
    };
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

/// Where the operator stands at each station (in front, a little to the side, looking down),
/// and how far away the work they're looking at is.
fn station_pose(station: ShopView) -> Option<Transform> {
    let (origin, eye, target) = match station {
        ShopView::Overview => return None,
        ShopView::TableSaw => (
            table_saw::ORIGIN,
            Vec3::new(-0.3, 0.79, 1.35),
            Vec3::new(0.1, 0.0, -0.05),
        ),
        ShopView::MiterSaw => (
            miter_saw::ORIGIN,
            Vec3::new(0.0, 0.7, 1.2),
            Vec3::new(0.0, 0.12, 0.0),
        ),
    };
    Some(Transform::from_translation(origin + eye).looking_at(origin + target, Vec3::Y))
}

/// Eases the camera toward the overview or the station being worked at.
fn follow_view(
    view: Res<ShopView>,
    overview: Res<OverviewCamera>,
    time: Res<Time>,
    mut camera: Single<&mut Transform, With<ShopCamera>>,
) {
    let goal = station_pose(*view).unwrap_or_else(|| overview.pose());
    let t = 1.0 - (-6.0 * time.delta_secs()).exp();
    camera.translation = camera.translation.lerp(goal.translation, t);
    camera.rotation = camera.rotation.slerp(goal.rotation, t);
}

/// In the overview, outlines the station under the pointer to show it can be clicked.
fn outline_hovered_station(view: Res<ShopView>, hovered: Res<HoveredStation>, mut gizmos: Gizmos) {
    if *view != ShopView::Overview {
        return;
    }
    let outline = match hovered.station {
        Some(ShopView::TableSaw) => block_transform(table_saw::BOUNDS, table_saw::ORIGIN),
        Some(ShopView::MiterSaw) => block_transform(miter_saw::BOUNDS, miter_saw::ORIGIN),
        _ => return,
    };
    gizmos.cube(outline, Color::srgb(1.0, 0.85, 0.3));
}

fn spawn_readout(mut commands: Commands) {
    let text = Color::srgb(0.17, 0.2, 0.27);
    let panel = Node {
        position_type: PositionType::Absolute,
        padding: UiRect::axes(px(14), px(8)),
        border: UiRect::all(px(2)),
        border_radius: BorderRadius::all(px(12)),
        ..default()
    };
    let lines = [
        (
            ReadoutLine::Reading,
            34.0,
            Color::srgb(0.88, 0.38, 0.1),
            Node {
                top: px(16),
                left: px(16),
                ..panel.clone()
            },
        ),
        // Narrow enough to wrap before it runs into the cut list.
        (
            ReadoutLine::Status,
            18.0,
            text,
            Node {
                top: px(86),
                left: px(16),
                max_width: percent(55),
                ..panel.clone()
            },
        ),
        (
            ReadoutLine::Help,
            15.0,
            text.with_alpha(0.75),
            Node {
                bottom: px(16),
                left: px(16),
                ..panel.clone()
            },
        ),
        (
            ReadoutLine::CutList,
            17.0,
            text,
            Node {
                top: px(16),
                right: px(16),
                ..panel.clone()
            },
        ),
        (
            ReadoutLine::Fps,
            15.0,
            text.with_alpha(0.75),
            Node {
                bottom: px(16),
                right: px(16),
                ..panel
            },
        ),
    ];
    for (line, size, color, node) in lines {
        commands.spawn((
            Text::default(),
            TextFont {
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(color),
            node,
            BackgroundColor(Color::srgba(1.0, 0.98, 0.94, 0.94)),
            BorderColor::all(Color::srgba(0.17, 0.2, 0.27, 0.18)),
            line,
        ));
    }
}

fn update_readout(
    view: Res<ShopView>,
    hovered: Res<HoveredStation>,
    table_saw: Res<TableSawStation>,
    miter_saw: Res<MiterSawStation>,
    mut lines: Query<(&ReadoutLine, &mut Text)>,
) {
    if !(view.is_changed()
        || hovered.is_changed()
        || table_saw.is_changed()
        || miter_saw.is_changed())
    {
        return;
    }
    let (reading, status, help) = match *view {
        ShopView::Overview => (
            format!(
                "Fence: {}   Length: {}",
                table_saw.saw.fence, miter_saw.stop
            ),
            match hovered.station {
                Some(ShopView::TableSaw) => "Click to work at the table saw.",
                Some(ShopView::MiterSaw) => "Click to work at the miter saw.",
                _ => "Click a saw to work at it.",
            }
            .to_string(),
            OVERVIEW_HELP,
        ),
        ShopView::TableSaw => (
            format!(
                "Fence: {}   Blade: {}",
                table_saw.saw.fence, table_saw.saw.height
            ),
            table_saw.status(),
            table_saw::HELP,
        ),
        ShopView::MiterSaw => (
            format!("Length: {}", miter_saw.stop),
            miter_saw.status(),
            miter_saw::HELP,
        ),
    };
    for (line, mut text) in &mut lines {
        text.0 = match line {
            ReadoutLine::Reading => reading.clone(),
            ReadoutLine::Status => status.clone(),
            ReadoutLine::Help => help.to_string(),
            ReadoutLine::CutList => miter_saw.cut_list(),
            ReadoutLine::Fps => continue,
        };
    }
}

/// Shows the frame rate, the average frame time, and the slowest frame of the last couple of
/// seconds, which is where hitches show up.
fn update_fps(diagnostics: Res<DiagnosticsStore>, mut lines: Query<(&ReadoutLine, &mut Text)>) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|fps| fps.smoothed());
    let frame_time = diagnostics.get(&FrameTimeDiagnosticsPlugin::FRAME_TIME);
    let average = frame_time.and_then(|time| time.smoothed());
    let worst = frame_time.and_then(|time| time.values().copied().reduce(f64::max));
    let (Some(fps), Some(average), Some(worst)) = (fps, average, worst) else {
        return;
    };
    for (line, mut text) in &mut lines {
        if let ReadoutLine::Fps = line {
            text.0 = format!("{fps:.0} fps   {average:.1} ms   worst {worst:.1} ms");
        }
    }
}

/// Describes a finished cut, e.g. "Ripped a 3/4" x 6" x 24" piece, plus a ... offcut.", and
/// any strips a slot already in the board set loose.
pub fn describe_cut(verb: &str, cut: &Cut) -> String {
    let piece = &cut.piece.board;
    let mut description = match &cut.offcut {
        Some(offcut) => format!(
            "{verb} a {piece} piece, plus a {} offcut. The blade turned {} to sawdust.",
            offcut.board, cut.waste
        ),
        None => format!(
            "{verb} a {piece} piece, and the blade turned the last {} to sawdust.",
            cut.waste
        ),
    };
    let loose: Vec<String> = cut
        .loose
        .iter()
        .map(|piece| format!("a {}", piece.board))
        .collect();
    if !loose.is_empty() {
        description.push_str(&format!(
            " The slot already in the board split off {}.",
            loose.join(" and ")
        ));
    }
    description
}

/// Describes a board and the slots sawn into it, e.g. "3/4" x 7-1/4" x 24", with a stopped cut
/// 11-9/16" long on top and 13" on the bottom".
pub fn describe_board(board: &Board) -> String {
    let kerfs: Vec<String> = board
        .kerfs()
        .iter()
        .map(|kerf| describe_kerf(board, kerf))
        .collect();
    if kerfs.is_empty() {
        board.to_string()
    } else {
        format!("{board}, with {}", kerfs.join(" and "))
    }
}

/// Describes a slot sawn into `board` from below, the way a table saw cuts: a stopped cut if it
/// goes through, otherwise a groove (or a rabbet, along an edge). Lengths are rounded to 1/32",
/// as you'd measure them.
pub fn describe_kerf(board: &Board, kerf: &Kerf) -> String {
    let thirty_second = Length::from_inch_fraction(1, 32);
    let span = |t: Length| {
        kerf.span_at(t).map_or(Length::ZERO, |(start, end)| {
            (end.min(kerf.along().of(board)) - start.max(Length::ZERO))
                .max(Length::ZERO)
                .rounded_to(thirty_second)
        })
    };
    let thickness = board.thickness();
    if kerf.depth(thickness) >= thickness {
        // A slot through the board's whole length would have cut it apart, so this one stops.
        let (top, bottom) = (span(thickness), span(Length::ZERO));
        return format!("a stopped cut {top} long on top and {bottom} on the bottom");
    }
    let depth = kerf.depth(thickness).rounded_to(thirty_second);
    let at_edge = kerf.near <= Length::ZERO || kerf.far >= kerf.across.of(board);
    let kind = match (kerf.across, at_edge) {
        (Axis::Width, false) => "groove",
        (Axis::Width, true) => "rabbet",
        (Axis::Length, false) => "dado",
        (Axis::Length, true) => "rabbet across the end",
    };
    // How far it runs on the face the blade came through.
    let face = if kerf.from_below() {
        Length::ZERO
    } else {
        thickness
    };
    let reach = kerf.along().of(board);
    let all_the_way = kerf
        .span_at(face)
        .is_some_and(|(start, end)| start <= Length::ZERO && end >= reach);
    match (all_the_way, kerf.across) {
        (true, Axis::Width) => format!("a {depth}-deep {kind} the full length"),
        (true, Axis::Length) => format!("a {depth}-deep {kind} all the way across"),
        (false, _) => format!("a {depth}-deep {kind} {} long", span(face)),
    }
}

/// Places each part of a board on a saw, rebuilding its mesh to match the board and hiding the
/// parts that aren't there. Each part needs its own mesh. `frame` turns the saw's own frame
/// (across, up, along) into the station's coordinates, which start at `origin`.
pub fn place_parts<'a>(
    boards: &SawBoards,
    origin: Vec3,
    frame: Quat,
    meshes: &mut Assets<Mesh>,
    parts: impl IntoIterator<
        Item = (
            &'a Part,
            &'a Mesh3d,
            Mut<'a, Transform>,
            Mut<'a, Visibility>,
        ),
    >,
) {
    for (part, mesh, mut transform, mut visibility) in parts {
        match boards.get(*part) {
            Some(placed) => {
                *transform = Transform::from_translation(origin + frame * placed.corner)
                    .with_rotation(frame * placed.rotation);
                if let Some(mut mesh) = meshes.get_mut(&mesh.0) {
                    *mesh = lumber_mesh(&placed.board, placed.grain);
                }
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
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

//! The shop around the stations: shared layout helpers, the camera, the readout, and moving
//! between the overview and the stations.

use std::{
    f32::consts::{FRAC_PI_2, PI},
    time::Duration,
};

use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    camera::{Exposure, Hdr},
    core_pipeline::tonemapping::Tonemapping,
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    ecs::system::EntityCommands,
    input::{
        gestures::PinchGesture,
        mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    },
    light::{GeneratedEnvironmentMapLight, ShadowFilteringMethod, VolumetricFog},
    math::Affine2,
    pbr::ScreenSpaceAmbientOcclusion,
    picking::pointer::PointerButton,
    post_process::{
        bloom::Bloom,
        dof::{DepthOfField, DepthOfFieldMode},
        effect_stack::Vignette,
    },
    prelude::*,
    render::view::{ColorGrading, ColorGradingGlobal},
    time::common_conditions::on_real_timer,
};
use woodsim::{Cut, Length};

use crate::{
    board_mesh::{Grain, board_mesh},
    chop::MiterSawStation,
    feed::TableSawStation,
    miter_saw, table_saw,
    textures::{self, CONCRETE_TILE, MAPLE, PINE},
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

/// The parts of a board a saw shows: what the blade hasn't reached, and both sides of the cut.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Uncut,
    Piece,
    Offcut,
}

/// Where to draw each part of the board on a saw.
#[derive(Debug, Default, PartialEq)]
pub struct BoardBlocks {
    pub uncut: Option<Block>,
    pub piece: Option<Block>,
    pub offcut: Option<Block>,
    /// The corner the whole board's grain is measured from. It moves with the board, so the
    /// grain stays put on the wood as the board slides and splits.
    pub grain_origin: Vec3,
}

impl BoardBlocks {
    pub fn get(&self, part: Part) -> Option<Block> {
        match part {
            Part::Uncut => self.uncut,
            Part::Piece => self.piece,
            Part::Offcut => self.offcut,
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
/// Depth of field, as an f-stop. The overview's shallow focus makes the shop look like a
/// miniature; up close, only the far background softens.
const OVERVIEW_APERTURE: f32 = 0.4;
const STATION_APERTURE: f32 = 2.0;

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

/// Meshes, textures, and materials more than one part of the shop uses.
#[derive(Resource)]
pub struct ShopAssets {
    /// A 1 m cube, scaled to fill each box in the scene.
    pub cube: Handle<Mesh>,
    /// Pine, for boards. Use it on meshes from `board_mesh` so the grain runs the right way.
    pub wood: Handle<StandardMaterial>,
    /// How a piece of wood looks under the pointer when it can be picked up.
    pub wood_highlight: Handle<StandardMaterial>,
    /// Maple, for bench tops.
    pub maple: Handle<StandardMaterial>,
    pub concrete: Handle<StandardMaterial>,
    pub pegboard: Handle<Image>,
    /// The blurry room the camera reflects; see `textures::room_cubemap`.
    pub room: Handle<Image>,
}

impl FromWorld for ShopAssets {
    fn from_world(world: &mut World) -> ShopAssets {
        let cube = world
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(1.0, 1.0, 1.0));
        let mut images = world.resource_mut::<Assets<Image>>();
        let (pine, pine_normals) = textures::wood(&PINE);
        let (pine, pine_normals) = (images.add(pine), images.add(pine_normals));
        let (maple, maple_normals) = textures::wood(&MAPLE);
        let (maple, maple_normals) = (images.add(maple), images.add(maple_normals));
        let (concrete, concrete_normals) = textures::concrete();
        let (concrete, concrete_normals) = (images.add(concrete), images.add(concrete_normals));
        let pegboard = images.add(textures::pegboard());
        let room = images.add(textures::room_cubemap());

        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let wood =
            |tint: Color, texture: &Handle<Image>, normals: &Handle<Image>| StandardMaterial {
                base_color: tint,
                base_color_texture: Some(texture.clone()),
                normal_map_texture: Some(normals.clone()),
                perceptual_roughness: 0.62,
                reflectance: 0.35,
                ..default()
            };
        ShopAssets {
            cube,
            wood: materials.add(wood(Color::WHITE, &pine, &pine_normals)),
            wood_highlight: materials.add(StandardMaterial {
                emissive: LinearRgba::new(0.9, 0.6, 0.25, 1.0) * 60.0,
                ..wood(Color::srgb(1.0, 0.95, 0.85), &pine, &pine_normals)
            }),
            maple: materials.add(StandardMaterial {
                perceptual_roughness: 0.45,
                ..wood(Color::WHITE, &maple, &maple_normals)
            }),
            concrete: materials.add(StandardMaterial {
                base_color_texture: Some(concrete),
                normal_map_texture: Some(concrete_normals),
                perceptual_roughness: 0.78,
                reflectance: 0.3,
                // The floor's UVs span the whole shop; repeat a slab every CONCRETE_TILE.
                uv_transform: Affine2::from_scale(Vec2::splat(
                    2.0 * SHOP_HALF_SIZE / CONCRETE_TILE,
                )),
                ..default()
            }),
            pegboard,
            room,
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

/// The concrete floor and the camera. The walls, lights, and furnishings are in `scenery`.
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
            MeshMaterial3d(shop.concrete.clone()),
        ))
        .observe(drag_floor);

    let mut camera = commands.spawn((
        Camera3d::default(),
        overview.pose(),
        ShopCamera,
        // Render in HDR so bright sun patches and lamps can bloom, then tone-map like film.
        Hdr,
        Exposure {
            ev100: EXPOSURE_EV100,
        },
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        ColorGrading {
            global: ColorGradingGlobal {
                temperature: 0.0,
                post_saturation: 1.05,
                ..default()
            },
            ..default()
        },
    ));
    camera.insert((
        // Ambient occlusion darkens creases and contact points; temporal anti-aliasing smooths
        // it, the soft shadows, and the light shafts. Both need multisampling off.
        Msaa::Off,
        ScreenSpaceAmbientOcclusion::default(),
        TemporalAntiAliasing::default(),
        ShadowFilteringMethod::Temporal,
        DepthOfField {
            mode: DepthOfFieldMode::Bokeh,
            focal_distance: overview.distance,
            aperture_f_stops: OVERVIEW_APERTURE,
            ..default()
        },
        VolumetricFog {
            ambient_intensity: 0.0,
            step_count: 64,
            jitter: 0.5,
            ..default()
        },
        GeneratedEnvironmentMapLight {
            environment_map: shop.room.clone(),
            intensity: 40.0,
            ..default()
        },
        // A soft darkening toward the corners, like a camera lens, to draw the eye inward.
        Vignette {
            intensity: 0.45,
            radius: 0.85,
            smoothness: 3.0,
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
fn station_pose(station: ShopView) -> Option<(Transform, f32)> {
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
    let pose = Transform::from_translation(origin + eye).looking_at(origin + target, Vec3::Y);
    Some((pose, eye.distance(target)))
}

/// Eases the camera toward the overview or the station being worked at, keeping what it's
/// looking at in focus.
fn follow_view(
    view: Res<ShopView>,
    overview: Res<OverviewCamera>,
    time: Res<Time>,
    camera: Single<(&mut Transform, &mut DepthOfField), With<ShopCamera>>,
) {
    let (goal, focus, aperture) = match station_pose(*view) {
        Some((pose, focus)) => (pose, focus, STATION_APERTURE),
        None => (overview.pose(), overview.distance, OVERVIEW_APERTURE),
    };
    let t = 1.0 - (-6.0 * time.delta_secs()).exp();
    let (mut transform, mut depth_of_field) = camera.into_inner();
    transform.translation = transform.translation.lerp(goal.translation, t);
    transform.rotation = transform.rotation.slerp(goal.rotation, t);
    depth_of_field.focal_distance += (focus - depth_of_field.focal_distance) * t;
    depth_of_field.aperture_f_stops += (aperture - depth_of_field.aperture_f_stops) * t;
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
    let text = Color::srgb(0.95, 0.92, 0.86);
    let panel = Node {
        position_type: PositionType::Absolute,
        padding: UiRect::axes(px(14), px(8)),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(10)),
        ..default()
    };
    let lines = [
        (
            ReadoutLine::Reading,
            34.0,
            Color::srgb(1.0, 0.78, 0.42),
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
            BackgroundColor(Color::srgba(0.07, 0.06, 0.05, 0.72)),
            BorderColor::all(Color::srgba(1.0, 0.9, 0.75, 0.14)),
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
            format!("Fence: {}", table_saw.saw.fence),
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

/// Describes a finished cut, e.g. "Ripped a 3/4" x 6" x 24" piece, plus a ... offcut."
pub fn describe_cut(verb: &str, cut: &Cut) -> String {
    match &cut.offcut {
        Some(offcut) => format!(
            "{verb} a {} piece, plus a {offcut} offcut. The blade turned {} to sawdust.",
            cut.piece, cut.waste
        ),
        None => format!(
            "{verb} a {} piece, and the blade turned the last {} to sawdust.",
            cut.piece, cut.waste
        ),
    }
}

/// Places each part of a board on a saw, reshaping its mesh to the part's size and hiding the
/// parts that aren't there. Each part needs its own mesh.
pub fn place_parts<'a>(
    blocks: &BoardBlocks,
    origin: Vec3,
    grain: Grain,
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
        match blocks.get(*part) {
            Some(block) => {
                *transform = Transform::from_translation(origin + block.min);
                if let Some(mut mesh) = meshes.get_mut(&mesh.0) {
                    let size = block.max - block.min;
                    *mesh = board_mesh(size, block.min - blocks.grain_origin, grain);
                }
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

/// Places a unit cube so it fills `block`, given in the coordinates of a station at `origin`.
pub fn block_transform(block: Block, origin: Vec3) -> Transform {
    Transform::from_translation(origin + (block.min + block.max) / 2.0)
        .with_scale(block.max - block.min)
}

/// Rounds a position in meters to the nearest 1/16", the finest mark on a fence or stop scale.
pub fn snap_to_sixteenth(meters: f32) -> Length {
    let sixteenths = (meters / inches(1.0) * 16.0).round() as i64;
    Length::from_inch_fraction(sixteenths, 16)
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

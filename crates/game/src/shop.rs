//! The shop around the stations: shared layout helpers, the camera, the readout, and moving
//! between stations.

use bevy::prelude::*;
use woodsim::{Cut, Length};

use crate::{chop::MiterSawStation, feed::TableSawStation, miter_saw, table_saw};

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

/// Which station the player is working at.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActiveStation {
    #[default]
    TableSaw,
    MiterSaw,
}

#[derive(Component)]
struct ShopCamera;

#[derive(Component)]
enum ReadoutLine {
    Reading,
    Status,
    Help,
}

pub struct ShopPlugin;

impl Plugin for ShopPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveStation>()
            .add_systems(Startup, (spawn_shop, spawn_readout))
            .add_systems(Update, (switch_station, follow_station, update_readout));
    }
}

/// The garage around the saws: a floor, a light, and the camera.
fn spawn_shop(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(6.0, 6.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.55, 0.55, 0.52))),
        Pickable::IGNORE,
    ));
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(1.0, 4.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Camera3d::default(),
        camera_pose(ActiveStation::default()),
        ShopCamera,
    ));
}

/// Where the operator stands at each station: in front, a little to the side, looking down.
fn camera_pose(station: ActiveStation) -> Transform {
    let (origin, eye, target) = match station {
        ActiveStation::TableSaw => (
            table_saw::ORIGIN,
            Vec3::new(-0.3, 0.79, 1.35),
            Vec3::new(0.1, 0.0, -0.05),
        ),
        ActiveStation::MiterSaw => (
            miter_saw::ORIGIN,
            Vec3::new(0.0, 0.7, 1.2),
            Vec3::new(0.0, 0.12, 0.0),
        ),
    };
    Transform::from_translation(origin + eye).looking_at(origin + target, Vec3::Y)
}

fn switch_station(keys: Res<ButtonInput<KeyCode>>, mut active: ResMut<ActiveStation>) {
    if keys.just_pressed(KeyCode::Tab) {
        *active = match *active {
            ActiveStation::TableSaw => ActiveStation::MiterSaw,
            ActiveStation::MiterSaw => ActiveStation::TableSaw,
        };
    }
}

/// Eases the camera over to the active station.
fn follow_station(
    active: Res<ActiveStation>,
    time: Res<Time>,
    mut camera: Single<&mut Transform, With<ShopCamera>>,
) {
    let goal = camera_pose(*active);
    let t = 1.0 - (-6.0 * time.delta_secs()).exp();
    camera.translation = camera.translation.lerp(goal.translation, t);
    camera.rotation = camera.rotation.slerp(goal.rotation, t);
}

fn spawn_readout(mut commands: Commands) {
    let lines = [
        (ReadoutLine::Reading, 36.0, Val::Px(12.0), Val::Auto),
        (ReadoutLine::Status, 18.0, Val::Px(76.0), Val::Auto),
        (ReadoutLine::Help, 16.0, Val::Auto, Val::Px(12.0)),
    ];
    for (line, size, top, bottom) in lines {
        commands.spawn((
            Text::default(),
            TextFont {
                font_size: FontSize::Px(size),
                ..default()
            },
            Node {
                position_type: PositionType::Absolute,
                top,
                bottom,
                left: px(12),
                padding: UiRect::axes(px(10), px(6)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            line,
        ));
    }
}

fn update_readout(
    active: Res<ActiveStation>,
    table_saw: Res<TableSawStation>,
    miter_saw: Res<MiterSawStation>,
    mut lines: Query<(&ReadoutLine, &mut Text)>,
) {
    if !(active.is_changed() || table_saw.is_changed() || miter_saw.is_changed()) {
        return;
    }
    let (reading, status, help) = match *active {
        ActiveStation::TableSaw => (
            format!("Fence: {}", table_saw.saw.fence),
            table_saw.status(),
            table_saw::HELP,
        ),
        ActiveStation::MiterSaw => (
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
        };
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

/// Places each part of a board on a saw, hiding the parts that aren't there.
pub fn place_parts<'a>(
    blocks: &BoardBlocks,
    origin: Vec3,
    parts: impl IntoIterator<Item = (&'a Part, Mut<'a, Transform>, Mut<'a, Visibility>)>,
) {
    for (part, mut transform, mut visibility) in parts {
        match blocks.get(*part) {
            Some(block) => {
                *transform = block_transform(block, origin);
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

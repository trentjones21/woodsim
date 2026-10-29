//! The photo studio: a piece of wood on a turntable in front of a seamless backdrop, a camera
//! on it, and three soft boxes around it (`Studio`), set up for a render that `trace` takes its
//! time over and that's saved as a picture. Plain Rust; the Bevy side is in `studio.rs`.
//!
//! The camera, the backdrop, and the lights stand still and the piece turns, as on a real
//! product shoot, so lights placed around the camera ("45° to the left") stay where they're
//! said to be. The studio's frame has `y` up and the camera looking along `-z` from the `+z`
//! side, with the piece on the floor (`y` = 0) turning about its middle.

use std::{
    f32::consts::PI,
    fs, io,
    path::{Path, PathBuf},
};

use bevy::{
    asset::RenderAssetUsages,
    math::Affine3A,
    mesh::{Indices, VertexAttributeValues},
    prelude::*,
    render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat},
};
use woodsim::{Assembly, Board, Source};

use crate::{
    controls::Step,
    gpu::{Gpu, GpuRender},
    grain::{Log, mix},
    meshes::lumber_mesh,
    trace::{Camera, Environment, Lamp, Material, Picture, Render, Scene, Triangle, Wood},
};

/// Where renders are saved: `renders/render-1.png`, `renders/render-2.png`, ...
pub const FOLDER: &str = "renders";

/// How much of the picture the piece fills when it's framed, each way.
const FILL: f32 = 0.78;
/// The lens's f-number: stopped down enough that the whole piece is sharp, with only what's
/// far behind it a little soft.
const F_NUMBER: f32 = 8.0;
/// Lenses, as focal lengths on a 35 mm camera (whose picture is 24 mm high).
const LENSES: [u32; 8] = [24, 35, 50, 70, 85, 105, 135, 200];
const SENSOR_HALF_HEIGHT: f32 = 12.0;
/// How far the camera may come in to, or go out from, the piece's middle, in meters.
const NEAREST: f32 = 0.1;
const FARTHEST: f32 = 12.0;
/// How low and how high the camera may look down at the piece.
const LEAST_PITCH: f32 = 2.0 * PI / 180.0;
const MOST_PITCH: f32 = 85.0 * PI / 180.0;
/// How bright the picture comes out: light is multiplied by this before it's developed.
const EXPOSURE: f32 = 1.0;

/// Each light's name, and its soft box: half its width and height as a share of how far it is
/// from the piece, and how bright it is at full power.
const LIGHTS: [(&str, f32, f32, f32); 3] = [
    ("Key light", 0.32, 0.32, 9.0),
    ("Fill light", 0.45, 0.45, 0.9),
    ("Rim light", 0.07, 0.35, 16.0),
];

/// One light: how far around from the camera it is (in degrees, to the right is positive, so
/// 180 is behind the piece), how far up (0 level with it, 90 overhead), and how bright, in
/// percent of full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Light {
    pub around: i32,
    pub height: i32,
    pub power: u32,
}

/// What the backdrop is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backdrop {
    #[default]
    White,
    Gray,
    Black,
    /// Black and glossy, so the piece is reflected in it.
    Acrylic,
}

impl Backdrop {
    const ALL: [Backdrop; 4] = [
        Backdrop::White,
        Backdrop::Gray,
        Backdrop::Black,
        Backdrop::Acrylic,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Backdrop::White => "White",
            Backdrop::Gray => "Gray",
            Backdrop::Black => "Black",
            Backdrop::Acrylic => "Gloss black",
        }
    }

    /// What it's made of, for the render, and its color, for the view while setting up.
    fn material(self) -> Material {
        match self {
            Backdrop::White => Material::Matte(Vec3::new(0.8, 0.79, 0.77)),
            Backdrop::Gray => Material::Matte(Vec3::splat(0.22)),
            Backdrop::Black => Material::Matte(Vec3::splat(0.025)),
            Backdrop::Acrylic => Material::Finished {
                color: Vec3::splat(0.005),
                roughness: 0.06,
            },
        }
    }

    pub fn color(self) -> Color {
        match self.material() {
            Material::Matte(color) | Material::Finished { color, .. } => {
                Color::linear_rgb(color.x, color.y, color.z)
            }
            Material::Wood(_) => Color::WHITE,
        }
    }

    pub fn glossy(self) -> bool {
        self == Backdrop::Acrylic
    }

    /// The next backdrop, or the one before, going around.
    pub fn next(self, forward: bool) -> Backdrop {
        cycle(&Backdrop::ALL, self, forward)
    }
}

/// How long a render takes over itself: how wide it's drawn, and how many samples each pixel
/// gets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quality {
    Draft,
    #[default]
    Good,
    Best,
}

impl Quality {
    const ALL: [Quality; 3] = [Quality::Draft, Quality::Good, Quality::Best];

    pub fn name(self) -> &'static str {
        match self {
            Quality::Draft => "Draft",
            Quality::Good => "Good",
            Quality::Best => "Best",
        }
    }

    pub fn width(self) -> u32 {
        match self {
            Quality::Draft => 1280,
            Quality::Good => 1920,
            Quality::Best => 2560,
        }
    }

    /// The next quality, or the one before, going around.
    pub fn next(self, forward: bool) -> Quality {
        cycle(&Quality::ALL, self, forward)
    }

    /// About how long a render takes on `engine`, on a good computer.
    fn takes(self, engine: Engine) -> &'static str {
        match (engine, self) {
            (Engine::Gpu, Quality::Draft) => "a second or two",
            (Engine::Gpu, Quality::Good) => "about 15 seconds",
            (Engine::Gpu, Quality::Best) => "under a minute",
            (Engine::Cpu, Quality::Draft) => "a few seconds",
            (Engine::Cpu, Quality::Good) => "about a minute",
            (Engine::Cpu, Quality::Best) => "a few minutes",
        }
    }

    pub fn samples(self) -> u32 {
        match self {
            Quality::Draft => 64,
            Quality::Good => 256,
            Quality::Best => 512,
        }
    }
}

/// Which renderer draws the picture: the GPU, four or five times faster, or the CPU, the
/// reference the GPU's is tested against. They draw the same picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Engine {
    #[default]
    Gpu,
    Cpu,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Engine::Gpu => "GPU",
            Engine::Cpu => "CPU",
        }
    }
}

/// How the studio's set up, kept from one piece to the next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Setup {
    pub lights: [Light; 3],
    /// The light being moved.
    pub picked: usize,
    pub backdrop: Backdrop,
    pub quality: Quality,
    /// The lens's focal length, in millimeters on a 35 mm camera.
    pub lens: u32,
    /// How far the camera looks down at the piece, in radians.
    pub pitch: f32,
    pub engine: Engine,
}

impl Setup {
    /// The tangent of half the angle the camera sees from the middle of its picture to its
    /// top.
    pub fn tangent(&self) -> f32 {
        SENSOR_HALF_HEIGHT / self.lens as f32
    }
}

impl Default for Setup {
    /// The classic three lights: a key up and to one side, a dimmer fill low on the other, and
    /// a rim behind to pick out the edges.
    fn default() -> Setup {
        Setup {
            lights: [
                Light {
                    around: -45,
                    height: 40,
                    power: 100,
                },
                Light {
                    around: 60,
                    height: 15,
                    power: 100,
                },
                Light {
                    around: 160,
                    height: 30,
                    power: 100,
                },
            ],
            picked: 0,
            backdrop: Backdrop::White,
            quality: Quality::Good,
            lens: 70,
            pitch: 24.0 * PI / 180.0,
            engine: Engine::Gpu,
        }
    }
}

/// A board of the piece, as it sits in the piece's own frame (its middle at the origin): the
/// rotation that turns its mesh (`w`, `t`, `l` along x, y, z) into place, and where its corner
/// is; its mesh's triangles; and where it sat in its stock board, and the log that came from.
#[derive(Clone)]
struct Member {
    board: Board,
    rotation: Quat,
    corner: Vec3,
    triangles: Vec<[Vec3; 3]>,
    normals: Vec<Vec3>,
    on_stock: Vec3,
    log: Log,
}

/// A piece in the studio: its boards, how it's turned on the turntable and whether it's
/// flipped over, and how far the camera is from it.
#[derive(Clone)]
pub struct Shoot {
    members: Vec<Member>,
    /// Every corner of its wood, in its own frame, to see how it sits as it turns.
    points: Vec<Vec3>,
    pub turn: f32,
    flipped: bool,
    distance: f32,
}

/// A board placed in the studio: the rotation that turns its mesh into place, and where its
/// corner is.
pub struct Posed<'a> {
    pub board: &'a Board,
    pub rotation: Quat,
    pub corner: Vec3,
}

impl Shoot {
    /// `piece` in the studio as it lay on the bench (its `x` along the bench, `y` away from
    /// the operator, `z` up), turned a little so its front and side both show.
    pub fn new(piece: &Assembly) -> Shoot {
        let onto = |v: [f64; 3]| Vec3::new(v[0] as f32, v[2] as f32, -v[1] as f32);
        let meters = |v: [woodsim::Length; 3]| v.map(|length| length.as_nm() as f64 / 1e9);
        let mut stocks: Vec<&woodsim::Origin> = Vec::new();
        let mut members: Vec<Member> = piece
            .members()
            .iter()
            .map(|member| {
                let [l, w, t] = member.axes().map(onto);
                let rotation = Quat::from_mat3(&Mat3::from_cols(w, t, l));
                let corner = onto(meters(member.at));
                let stock = member.board.origin().stock();
                let index = match stocks.iter().position(|seen| seen.same(stock)) {
                    Some(index) => index,
                    None => {
                        stocks.push(stock);
                        stocks.len() - 1
                    }
                };
                let (width, thickness) = match stock.source() {
                    Source::Stock {
                        width, thickness, ..
                    } => (width.as_meters_f32(), thickness.as_meters_f32()),
                    Source::Sawn { .. } => (0.1, 0.02),
                };
                let [sl, sw, st] = member
                    .board
                    .origin()
                    .at_on_stock()
                    .map(|l| l.as_meters_f32());
                let mesh = lumber_mesh(&member.board, Vec3::ZERO);
                let (triangles, normals) = triangles_of(&mesh);
                Member {
                    board: member.board.clone(),
                    rotation,
                    corner,
                    triangles,
                    normals,
                    on_stock: Vec3::new(sl, sw, st),
                    log: Log::for_stock(width, thickness, mix(index as u32 + 1) ^ 0x3c6e_f372),
                }
            })
            .collect();
        // Centered on its middle.
        let corners = |members: &[Member]| -> Vec<Vec3> {
            members
                .iter()
                .flat_map(|member| {
                    member
                        .triangles
                        .iter()
                        .flatten()
                        .map(move |&corner| member.rotation * corner + member.corner)
                })
                .collect()
        };
        let (min, max) = bounds(&corners(&members));
        let middle = (min + max) / 2.0;
        for member in &mut members {
            member.corner -= middle;
        }
        let mut points = corners(&members);
        points.sort_by(|a, b| {
            a.to_array()
                .partial_cmp(&b.to_array())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        points.dedup();
        Shoot {
            members,
            points,
            turn: -30.0 * PI / 180.0,
            flipped: false,
            distance: 1.0,
        }
    }

    /// How the piece is turned and lifted to sit on the floor.
    fn pose(&self) -> (Quat, Vec3) {
        let flip = if self.flipped {
            Quat::from_rotation_x(PI)
        } else {
            Quat::IDENTITY
        };
        let rotation = Quat::from_rotation_y(self.turn) * flip;
        let lowest = self
            .points
            .iter()
            .map(|&p| (rotation * p).y)
            .fold(f32::MAX, f32::min);
        (rotation, Vec3::new(0.0, -lowest.min(f32::MAX / 2.0), 0.0))
    }

    /// Every board, where it is in the studio.
    pub fn posed(&self) -> Vec<Posed<'_>> {
        let (rotation, lift) = self.pose();
        self.members
            .iter()
            .map(|member| Posed {
                board: &member.board,
                rotation: rotation * member.rotation,
                corner: rotation * member.corner + lift,
            })
            .collect()
    }

    /// The box the piece fills in the studio.
    fn bounds(&self) -> (Vec3, Vec3) {
        let (rotation, lift) = self.pose();
        let points: Vec<Vec3> = self.points.iter().map(|&p| rotation * p + lift).collect();
        bounds(&points)
    }

    /// How tall the piece stands, and the point the camera looks at: its middle.
    pub fn target(&self) -> Vec3 {
        let (min, max) = self.bounds();
        Vec3::new(0.0, (min.y + max.y) / 2.0, 0.0)
    }

    /// How far the piece reaches from its middle, at most, however it's turned.
    pub fn radius(&self) -> f32 {
        self.points.iter().map(|p| p.length()).fold(0.0, f32::max)
    }

    /// Turns the piece on the turntable by `angle` radians, counterclockwise seen from above.
    pub fn turn_by(&mut self, angle: f32) {
        self.turn = (self.turn + angle).rem_euclid(2.0 * PI);
    }

    pub fn flip(&mut self) {
        self.flipped = !self.flipped;
    }

    /// Brings the camera in by `factor` (less than one) or takes it back.
    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(NEAREST, FARTHEST);
    }

    /// Moves the camera back just far enough that the piece fills `FILL` of a picture `aspect`
    /// times as wide as it's high, seen as `setup` says.
    pub fn frame(&mut self, setup: &Setup, aspect: f32) {
        let target = self.target();
        let (back, right, up) = camera_axes(setup.pitch);
        let tangent = SENSOR_HALF_HEIGHT / setup.lens as f32 * FILL;
        let (rotation, lift) = self.pose();
        self.distance = self
            .points
            .iter()
            .map(|&p| {
                let q = rotation * p + lift - target;
                (q.dot(up).abs() / tangent).max(q.dot(right).abs() / (tangent * aspect))
                    + q.dot(back)
            })
            .fold(NEAREST, f32::max)
            .min(FARTHEST);
    }

    /// Where the camera is, looking at `target`.
    pub fn camera_position(&self, setup: &Setup) -> Vec3 {
        let (back, _, _) = camera_axes(setup.pitch);
        self.target() + back * self.distance
    }

    /// The soft boxes, as the renderer has them.
    pub fn lamps(&self, setup: &Setup) -> Vec<Lamp> {
        let target = self.target();
        let distance = (self.radius() * 4.0).max(1.5);
        setup
            .lights
            .iter()
            .zip(LIGHTS)
            .map(|(light, (_, half_width, half_height, brightness))| {
                let (around, up) = (
                    (light.around as f32).to_radians(),
                    (light.height as f32).to_radians(),
                );
                let toward = Vec3::new(around.sin() * up.cos(), up.sin(), around.cos() * up.cos());
                let across = Vec3::Y.cross(toward).try_normalize().unwrap_or(Vec3::X);
                let upward = across.cross(toward);
                Lamp {
                    center: target + toward * distance,
                    u: across * (half_width * distance),
                    v: upward * (half_height * distance),
                    radiance: Vec3::splat(brightness * light.power as f32 / 100.0),
                }
            })
            .collect()
    }

    /// The backdrop: a floor sweeping up in a curve into a wall behind the piece, far enough
    /// back to fall into shadow.
    pub fn backdrop(&self) -> Vec<Triangle> {
        let depth = self.radius() * 2.0 + 0.6;
        let (radius, height, half_width, front) = (0.8, 5.0, 8.0, 12.0);
        let mut rows: Vec<(f32, f32, Vec3)> = vec![(front, 0.0, Vec3::Y)];
        const STEPS: usize = 32;
        for step in 0..=STEPS {
            let angle = step as f32 / STEPS as f32 * PI / 2.0;
            rows.push((
                -depth - radius * angle.sin(),
                radius - radius * angle.cos(),
                Vec3::new(0.0, angle.cos(), angle.sin()),
            ));
        }
        rows.push((-depth - radius, height, Vec3::Z));
        let mut triangles = Vec::new();
        for pair in rows.windows(2) {
            let [(z0, y0, n0), (z1, y1, n1)] = [pair[0], pair[1]];
            let a = Vec3::new(-half_width, y0, z0);
            let b = Vec3::new(half_width, y0, z0);
            let c = Vec3::new(half_width, y1, z1);
            let d = Vec3::new(-half_width, y1, z1);
            triangles.push(Triangle {
                corners: [a, b, c],
                normals: [n0, n0, n1],
                material: 0,
            });
            triangles.push(Triangle {
                corners: [a, c, d],
                normals: [n0, n1, n1],
                material: 0,
            });
        }
        triangles
    }

    /// Everything the renderer needs to draw the piece.
    pub fn scene(&self, setup: &Setup) -> Scene {
        let mut triangles = self.backdrop();
        let mut materials = vec![setup.backdrop.material()];
        for (member, posed) in self.members.iter().zip(self.posed()) {
            let material = materials.len() as u32;
            let to_mesh =
                Affine3A::from_rotation_translation(posed.rotation, posed.corner).inverse();
            // The mesh's x, y, z are the board's w, t, l; its stock has them as l, w, t.
            let to_stock = Affine3A::from_mat3_translation(
                Mat3::from_cols(Vec3::Y, Vec3::Z, Vec3::X),
                member.on_stock,
            ) * to_mesh;
            materials.push(Material::Wood(Wood {
                to_stock,
                along: posed.rotation * Vec3::Z,
                log: member.log,
            }));
            for (corners, normal) in member.triangles.iter().zip(&member.normals) {
                let normal = posed.rotation * *normal;
                triangles.push(Triangle {
                    corners: corners.map(|corner| posed.rotation * corner + posed.corner),
                    normals: [normal; 3],
                    material,
                });
            }
        }
        let position = self.camera_position(setup);
        let forward = (self.target() - position).normalize();
        let right = forward.cross(Vec3::Y).normalize();
        let lens = setup.lens as f32;
        Scene::new(
            triangles,
            materials,
            self.lamps(setup),
            Environment {
                zenith: Vec3::new(0.17, 0.18, 0.19),
                horizon: Vec3::splat(0.08),
                ground: Vec3::splat(0.03),
                panels: 0.7,
            },
            Camera {
                position,
                forward,
                right,
                up: right.cross(forward),
                tan_half_height: SENSOR_HALF_HEIGHT / lens,
                aperture: lens / 1000.0 / (2.0 * F_NUMBER),
                focus: self.distance,
            },
            EXPOSURE,
        )
    }
}

/// From the piece toward the camera, and the camera's right and up, looking `pitch` down.
fn camera_axes(pitch: f32) -> (Vec3, Vec3, Vec3) {
    let back = Vec3::new(0.0, pitch.sin(), pitch.cos());
    (back, Vec3::X, back.cross(Vec3::NEG_X).normalize())
}

fn bounds(points: &[Vec3]) -> (Vec3, Vec3) {
    points.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(min, max), &p| (min.min(p), max.max(p)),
    )
}

/// A mesh's triangles, and the normal each faces.
fn triangles_of(mesh: &Mesh) -> (Vec<[Vec3; 3]>, Vec<Vec3>) {
    let (
        Some(VertexAttributeValues::Float32x3(positions)),
        Some(VertexAttributeValues::Float32x3(normals)),
        Some(Indices::U32(indices)),
    ) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
        mesh.indices(),
    )
    else {
        return (Vec::new(), Vec::new());
    };
    indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|corners| {
            let at = |k: usize| Vec3::from_array(positions[corners[k] as usize]);
            (
                [at(0), at(1), at(2)],
                Vec3::from_array(normals[corners[0] as usize]),
            )
        })
        .unzip()
}

/// A render under way, on the GPU or the CPU.
enum Renderer {
    Gpu(GpuRender),
    Cpu(Render),
}

impl Renderer {
    fn engine(&self) -> Engine {
        match self {
            Renderer::Gpu(_) => Engine::Gpu,
            Renderer::Cpu(_) => Engine::Cpu,
        }
    }

    fn size(&self) -> (u32, u32) {
        match self {
            Renderer::Gpu(render) => render.size(),
            Renderer::Cpu(render) => render.size(),
        }
    }

    fn progress(&self) -> f32 {
        match self {
            Renderer::Gpu(render) => render.progress(),
            Renderer::Cpu(render) => render.progress(),
        }
    }

    fn samples(&self) -> u32 {
        match self {
            Renderer::Gpu(render) => render.samples(),
            Renderer::Cpu(render) => render.samples(),
        }
    }

    fn is_done(&self) -> bool {
        match self {
            Renderer::Gpu(render) => render.is_done(),
            Renderer::Cpu(render) => render.is_done(),
        }
    }

    fn elapsed(&self) -> std::time::Duration {
        match self {
            Renderer::Gpu(render) => render.elapsed(),
            Renderer::Cpu(render) => render.elapsed(),
        }
    }

    fn picture_since(&self, version: u64) -> Option<Picture> {
        match self {
            Renderer::Gpu(render) => render.picture_since(version),
            Renderer::Cpu(render) => render.picture_since(version),
        }
    }

    fn finish(self) -> Picture {
        match self {
            Renderer::Gpu(render) => render.finish(),
            Renderer::Cpu(render) => render.finish(),
        }
    }

    fn stop_and_finish(self) -> Picture {
        match self {
            Renderer::Gpu(render) => render.stop_and_finish(),
            Renderer::Cpu(render) => render.stop_and_finish(),
        }
    }
}

/// What the studio's doing: being set up, rendering, or showing a render it's saved.
#[derive(Default)]
enum Job {
    #[default]
    SettingUp,
    Rendering(Renderer),
    Done {
        picture: Picture,
        size: (u32, u32),
        saved: Result<PathBuf, String>,
    },
}

/// The photo studio: how it's set up, the piece in it, and the render under way or done.
#[derive(Resource, Default)]
pub struct Studio {
    pub setup: Setup,
    shoot: Option<Shoot>,
    job: Job,
    /// How many pieces have been brought in, so the set's rebuilt for each.
    opened: u64,
}

impl Studio {
    /// Brings `piece` into the studio, framed in a picture `aspect` times as wide as it's high.
    pub fn open(&mut self, piece: &Assembly, aspect: f32) {
        let mut shoot = Shoot::new(piece);
        shoot.frame(&self.setup, aspect);
        self.shoot = Some(shoot);
        self.job = Job::SettingUp;
        self.opened += 1;
    }

    /// Frames the piece again for a picture `aspect` times as wide as it's high.
    pub fn reframe(&mut self, aspect: f32) {
        if let Some((shoot, setup)) = self.adjust() {
            shoot.frame(setup, aspect);
        }
    }

    /// How many pieces have been brought in.
    pub fn opened(&self) -> u64 {
        self.opened
    }

    /// Takes the piece out, stopping any render.
    pub fn close(&mut self) {
        self.shoot = None;
        self.job = Job::SettingUp;
    }

    pub fn shoot(&self) -> Option<&Shoot> {
        self.shoot.as_ref()
    }

    /// The piece, to turn it or move the camera, and the setup, to move a light, unless a
    /// render's under way; a render that's done is put away, back to setting up.
    pub fn adjust(&mut self) -> Option<(&mut Shoot, &mut Setup)> {
        match self.job {
            Job::Rendering(_) => return None,
            Job::Done { .. } => self.job = Job::SettingUp,
            Job::SettingUp => {}
        }
        Some((self.shoot.as_mut()?, &mut self.setup))
    }

    pub fn rendering(&self) -> bool {
        matches!(self.job, Job::Rendering(_))
    }

    /// Whether a render's done and showing.
    pub fn done(&self) -> bool {
        matches!(self.job, Job::Done { .. })
    }

    /// Starts rendering the piece in a picture `aspect` times as wide as it's high, on `gpu`
    /// if the setup says to and there is one, or else on the CPU.
    pub fn render(&mut self, aspect: f32, gpu: Option<Gpu>) {
        let Some(shoot) = &self.shoot else {
            return;
        };
        let quality = self.setup.quality;
        let width = quality.width();
        let height = ((width as f32 / aspect).round() as u32).max(1);
        let scene = shoot.scene(&self.setup);
        let samples = quality.samples();
        self.job = Job::Rendering(match gpu.filter(|_| self.setup.engine == Engine::Gpu) {
            Some(gpu) => Renderer::Gpu(GpuRender::start(gpu, &scene, width, height, samples)),
            None => Renderer::Cpu(Render::start(scene, width, height, samples)),
        });
    }

    /// Renders on the CPU from now on, or on the GPU.
    pub fn switch_engine(&mut self) {
        if let Some((_, setup)) = self.adjust() {
            setup.engine = match setup.engine {
                Engine::Gpu => Engine::Cpu,
                Engine::Cpu => Engine::Gpu,
            };
        }
    }

    /// Stops the render under way, keeping nothing.
    pub fn stop(&mut self) {
        self.job = Job::SettingUp;
    }

    /// Whether the render under way has come far enough to finish early.
    pub fn can_finish_now(&self) -> bool {
        matches!(&self.job, Job::Rendering(render) if render.samples() > 0)
    }

    /// Finishes the render under way where it's got to, and saves it in `folder`.
    pub fn finish_now(&mut self, folder: &Path) -> Option<Result<PathBuf, String>> {
        if !self.can_finish_now() {
            return None;
        }
        let Job::Rendering(render) = std::mem::take(&mut self.job) else {
            return None;
        };
        let size = render.size();
        Some(self.saved(render.stop_and_finish(), size, folder))
    }

    /// Saves the render once it's done, in `folder`, and says where.
    pub fn poll(&mut self, folder: &Path) -> Option<Result<PathBuf, String>> {
        let Job::Rendering(render) = &self.job else {
            return None;
        };
        if !render.is_done() {
            return None;
        }
        let Job::Rendering(render) = std::mem::take(&mut self.job) else {
            return None;
        };
        let size = render.size();
        Some(self.saved(render.finish(), size, folder))
    }

    fn saved(
        &mut self,
        picture: Picture,
        size: (u32, u32),
        folder: &Path,
    ) -> Result<PathBuf, String> {
        let saved = if picture.pixels.len() == size.0 as usize * size.1 as usize * 4 {
            save(&picture.pixels, size, folder).map_err(|error| error.to_string())
        } else {
            Err("the render didn't come out".to_string())
        };
        self.job = Job::Done {
            picture,
            size,
            saved: saved.clone(),
        };
        saved
    }

    /// The render as it's come out so far, or as it was saved, if it's newer than `version`,
    /// and its size.
    pub fn picture_since(&self, version: u64) -> Option<(Picture, (u32, u32))> {
        match &self.job {
            Job::SettingUp => None,
            Job::Rendering(render) => render.picture_since(version).map(|p| (p, render.size())),
            Job::Done { picture, size, .. } => {
                (picture.version > version).then(|| (picture.clone(), *size))
            }
        }
    }

    /// The light being moved: its name and how it's set.
    pub fn picked(&self) -> (&'static str, Light) {
        let index = self.setup.picked.min(LIGHTS.len() - 1);
        (LIGHTS[index].0, self.setup.lights[index])
    }

    pub fn light_name(index: usize) -> &'static str {
        LIGHTS[index.min(LIGHTS.len() - 1)].0
    }

    /// Picks the `index`th light to move.
    pub fn pick(&mut self, index: usize) {
        if let Some((_, setup)) = self.adjust() {
            setup.picked = index.min(LIGHTS.len() - 1);
        }
    }

    /// Swings the picked light around the piece: 5° a step, or 45°.
    pub fn swing(&mut self, step: Step) {
        if let Some((_, setup)) = self.adjust() {
            let light = &mut setup.lights[setup.picked];
            let by = if step.big { 45 } else { 5 };
            let around = light.around + if step.more { by } else { -by };
            light.around = (around + 180).rem_euclid(360) - 180;
        }
    }

    /// Raises or lowers the picked light: 5° a step, or 15°.
    pub fn raise(&mut self, step: Step) {
        if let Some((_, setup)) = self.adjust() {
            let light = &mut setup.lights[setup.picked];
            let by = if step.big { 15 } else { 5 };
            light.height = (light.height + if step.more { by } else { -by }).clamp(0, 90);
        }
    }

    /// Brightens or dims the picked light: 10% a step, or 50%, off at the least.
    pub fn brighten(&mut self, step: Step) {
        if let Some((_, setup)) = self.adjust() {
            let light = &mut setup.lights[setup.picked];
            let by = if step.big { 50 } else { 10 };
            light.power = if step.more {
                (light.power + by).min(300)
            } else {
                light.power.saturating_sub(by)
            };
        }
    }

    /// A longer lens or a shorter one, the camera moving back or in to keep the piece framed
    /// the same: 5 mm a step, or to the next of the usual lenses.
    pub fn change_lens(&mut self, step: Step) {
        if let Some((shoot, setup)) = self.adjust() {
            let lens = setup.lens;
            let next = if step.big && step.more {
                LENSES.iter().copied().find(|&l| l > lens).unwrap_or(lens)
            } else if step.big {
                LENSES
                    .iter()
                    .rev()
                    .copied()
                    .find(|&l| l < lens)
                    .unwrap_or(lens)
            } else if step.more {
                (lens + 5).min(LENSES[LENSES.len() - 1])
            } else {
                lens.saturating_sub(5).max(LENSES[0])
            };
            shoot.zoom(next as f32 / lens as f32);
            setup.lens = next;
        }
    }

    /// The next backdrop, or the one before.
    pub fn change_backdrop(&mut self, step: Step) {
        if let Some((_, setup)) = self.adjust() {
            setup.backdrop = setup.backdrop.next(step.more);
        }
    }

    /// The next quality, finer and slower, or the one before, going around.
    pub fn change_quality(&mut self, step: Step) {
        if let Some((_, setup)) = self.adjust() {
            setup.quality = setup.quality.next(step.more);
        }
    }

    /// Tips the camera up to look down at the piece more steeply, or down to look more level.
    pub fn tip(&mut self, by: f32) {
        if let Some((_, setup)) = self.adjust() {
            setup.pitch = (setup.pitch + by).clamp(LEAST_PITCH, MOST_PITCH);
        }
    }

    /// Where the picked light is, in words: "45° to the left, 40° up".
    pub fn where_light(light: Light) -> (String, String) {
        let around = match light.around {
            0 => "by the camera".to_string(),
            180 | -180 => "behind".to_string(),
            a if a < 0 => format!("{}° left", -a),
            a => format!("{a}° right"),
        };
        let height = match light.height {
            0 => "level".to_string(),
            90 => "overhead".to_string(),
            h => format!("{h}° up"),
        };
        (around, height)
    }

    pub fn power_reading(light: Light) -> String {
        match light.power {
            0 => "off".to_string(),
            p => format!("{p}%"),
        }
    }

    /// The line on what's going on.
    pub fn status(&self) -> String {
        match &self.job {
            Job::SettingUp if self.shoot.is_none() => {
                "Nothing in the studio. Bring a part from the workbench.".to_string()
            }
            Job::SettingUp => {
                let (name, _) = self.picked();
                let (quality, engine) = (self.setup.quality, self.setup.engine);
                format!(
                    "Drag to turn the piece and look from higher or lower; scroll to frame it. \
                     Move the {} or pick another light, then render: {} quality takes {} on \
                     the {}.",
                    name.to_lowercase(),
                    quality.name().to_lowercase(),
                    quality.takes(engine),
                    engine.name(),
                )
            }
            Job::Rendering(render) => {
                let (width, height) = render.size();
                let progress = render.progress();
                let left = if progress > 0.02 {
                    let seconds = render.elapsed().as_secs_f32() * (1.0 - progress) / progress;
                    format!(", about {} to go", describe_time(seconds))
                } else {
                    String::new()
                };
                format!(
                    "Rendering {width} × {height} on the {}: {:.0}% ({} of {} samples){left}. \
                     Finish now to save it as it is, or stop to change the setup.",
                    render.engine().name(),
                    progress * 100.0,
                    render.samples(),
                    self.setup.quality.samples(),
                )
            }
            Job::Done {
                saved: Ok(path), ..
            } => format!(
                "Saved the render as {}. Change anything to set up the next.",
                path.display()
            ),
            Job::Done {
                saved: Err(error), ..
            } => format!("Couldn't save the render: {error}."),
        }
    }
}

/// The item after `now` in `all`, or before it, going around.
fn cycle<T: Copy + PartialEq>(all: &[T], now: T, forward: bool) -> T {
    let index = all.iter().position(|&item| item == now).unwrap_or(0);
    let next = if forward {
        (index + 1) % all.len()
    } else {
        (index + all.len() - 1) % all.len()
    };
    all[next]
}

/// A time to go, roughly: "40 s", "3 min".
fn describe_time(seconds: f32) -> String {
    if seconds < 90.0 {
        format!("{} s", (seconds / 5.0).ceil() as u32 * 5)
    } else {
        format!("{} min", (seconds / 60.0).round() as u32)
    }
}

/// Saves `pixels` (four bytes each, row by row from the top), `size` wide and high, as the
/// next free `render-N.png` in `folder`.
pub fn save(pixels: &[u8], size: (u32, u32), folder: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let path = (1..)
        .map(|n| folder.join(format!("render-{n}.png")))
        .find(|path| !path.exists())
        .expect("a free name");
    let image = Image::new(
        Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels.to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD,
    );
    image
        .try_into_dynamic()
        .map_err(io::Error::other)?
        .to_rgb8()
        .save(&path)
        .map_err(io::Error::other)?;
    Ok(path)
}

/// The backdrop's triangles as a mesh, to show while setting up.
pub fn backdrop_mesh(triangles: &[Triangle]) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    for triangle in triangles {
        let [a, b, c] = triangle.corners;
        // Wound to face the way its normals do.
        let order = if (b - a).cross(c - a).dot(triangle.normals[0]) >= 0.0 {
            [0, 1, 2]
        } else {
            [0, 2, 1]
        };
        for k in order {
            positions.push(triangle.corners[k].to_array());
            normals.push(triangle.normals[k].to_array());
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
}

#[cfg(test)]
mod tests {
    use woodsim::{Length, Orientation, Pass};

    use super::*;

    fn one_by_four() -> Board {
        Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        )
    }

    fn piece() -> Assembly {
        Assembly::of(one_by_four())
    }

    #[test]
    fn the_piece_sits_on_the_floor_however_it_turns() {
        let mut shoot = Shoot::new(&piece());
        for _ in 0..8 {
            shoot.turn_by(0.7);
            let (min, max) = shoot.bounds();
            assert!(min.y.abs() < 1e-5, "{min}");
            // It turns about its middle.
            assert!((min.x + max.x).abs() < 0.01 && (min.z + max.z).abs() < 0.01);
            shoot.flip();
            let (min, _) = shoot.bounds();
            assert!(min.y.abs() < 1e-5, "{min}");
        }
    }

    #[test]
    fn the_piece_is_framed_to_fill_the_picture() {
        let setup = Setup::default();
        let mut shoot = Shoot::new(&piece());
        shoot.frame(&setup, 16.0 / 9.0);
        let scene_camera = shoot.camera_position(&setup);
        let target = shoot.target();
        let forward = (target - scene_camera).normalize();
        let right = forward.cross(Vec3::Y).normalize();
        let up = right.cross(forward);
        let tangent = SENSOR_HALF_HEIGHT / setup.lens as f32;
        let (rotation, lift) = shoot.pose();
        let mut most: f32 = 0.0;
        for &p in &shoot.points {
            let q = rotation * p + lift - scene_camera;
            let depth = q.dot(forward);
            let across = q.dot(right) / depth / (tangent * 16.0 / 9.0);
            let high = q.dot(up) / depth / tangent;
            assert!(across.abs() <= FILL + 1e-3 && high.abs() <= FILL + 1e-3);
            most = most.max(across.abs()).max(high.abs());
        }
        // It's as big as it can be: it touches the edge of the frame one way.
        assert!((most - FILL).abs() < 1e-3, "{most}");
    }

    #[test]
    fn a_longer_lens_backs_the_camera_off_to_keep_the_framing() {
        let mut studio = Studio::default();
        studio.open(&piece(), 1.5);
        let before = studio.shoot().unwrap().distance;
        studio.change_lens(Step {
            more: true,
            big: true,
        });
        assert_eq!(studio.setup.lens, 85);
        let after = studio.shoot().unwrap().distance;
        assert!((after / before - 85.0 / 70.0).abs() < 1e-4);
        studio.change_lens(Step {
            more: false,
            big: false,
        });
        assert_eq!(studio.setup.lens, 80);
    }

    #[test]
    fn lights_are_where_they_say_they_are() {
        let studio_setup = Setup::default();
        let shoot = Shoot::new(&piece());
        let lamps = shoot.lamps(&studio_setup);
        let target = shoot.target();
        // The key is up and to the camera's left, in front of the piece.
        let key = lamps[0].center - target;
        assert!(key.x < 0.0 && key.y > 0.0 && key.z > 0.0, "{key}");
        // The rim is behind the piece, to the right.
        let rim = lamps[2].center - target;
        assert!(rim.x > 0.0 && rim.z < 0.0, "{rim}");
        // Each faces the piece.
        for lamp in &lamps {
            assert!(lamp.u.cross(lamp.v).dot(target - lamp.center) > 0.0);
        }
        assert_eq!(
            Studio::where_light(studio_setup.lights[0]),
            ("45° left".to_string(), "40° up".to_string())
        );
    }

    #[test]
    fn a_light_swings_all_the_way_around() {
        let mut studio = Studio::default();
        studio.open(&piece(), 1.5);
        let big = Step {
            more: true,
            big: true,
        };
        for _ in 0..8 {
            studio.swing(big);
        }
        assert_eq!(studio.picked().1.around, -45);
        studio.pick(2);
        assert_eq!(studio.picked().0, "Rim light");
        for _ in 0..30 {
            studio.raise(big);
            studio.brighten(big);
        }
        assert_eq!(studio.picked().1.height, 90);
        assert_eq!(studio.picked().1.power, 300);
    }

    #[test]
    fn the_scene_has_the_wood_where_the_piece_is() {
        let setup = Setup::default();
        let mut shoot = Shoot::new(&piece());
        shoot.turn = 0.0;
        shoot.frame(&setup, 1.5);
        let scene_triangles = shoot.backdrop().len();
        let (min, max) = shoot.bounds();
        let mut wood = 0;
        for member in &shoot.members {
            wood += member.triangles.len();
        }
        assert!(wood >= 12, "a board has at least two triangles a side");
        // A board's corner in its stock is where it sat: new lumber, so at the origin.
        assert_eq!(shoot.members[0].on_stock, Vec3::ZERO);
        // Unturned, a 1x4 lies along the studio's x, flat.
        let size = max - min;
        assert!(
            (size - Vec3::new(0.6096, 0.01905, 0.0889))
                .abs()
                .max_element()
                < 1e-4,
            "{size}"
        );
        assert!(scene_triangles > 10);
        let _ = shoot.scene(&setup);
    }

    #[test]
    fn a_cut_piece_knows_where_it_sat_in_its_stock() {
        let saw = woodsim::MiterSaw::twelve_inch();
        let Ok(Pass::Cut(cut)) = saw.crosscut(
            &one_by_four(),
            Orientation::default(),
            Length::from_inches(10),
        ) else {
            panic!("expected a cut");
        };
        let offcut = cut.offcut.unwrap().board;
        let shoot = Shoot::new(&Assembly::of(offcut));
        // The offcut came from the end of the board nearer l = 0.
        let on_stock = shoot.members[0].on_stock;
        assert!(
            on_stock.y.abs() < 1e-6 && on_stock.z.abs() < 1e-6,
            "{on_stock}"
        );
        let piece = Shoot::new(&Assembly::of(cut.piece.board));
        assert!(piece.members[0].on_stock.x > on_stock.x);
    }

    #[test]
    fn a_render_is_saved_as_the_next_free_picture() {
        let folder = std::env::temp_dir().join(format!("woodshop-renders-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let pixels = vec![200u8; 4 * 6 * 4];
        let first = save(&pixels, (6, 4), &folder).unwrap();
        let second = save(&pixels, (6, 4), &folder).unwrap();
        assert!(first.ends_with("render-1.png") && second.ends_with("render-2.png"));
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_status_says_how_far_a_render_has_come() {
        let mut studio = Studio::default();
        assert!(studio.status().starts_with("Nothing"));
        studio.open(&piece(), 1.5);
        assert!(studio.status().contains("key light"));
        studio.setup.quality = Quality::Draft;
        studio.render(1.5, None);
        assert!(studio.rendering());
        assert!(
            studio
                .status()
                .starts_with("Rendering 1280 × 853 on the CPU"),
            "{}",
            studio.status()
        );
        // While it renders, the setup can't change.
        studio.pick(1);
        assert_eq!(studio.setup.picked, 0);
        studio.stop();
        assert!(!studio.rendering());
        // It renders on the GPU unless told otherwise.
        assert!(
            studio.status().ends_with("on the GPU."),
            "{}",
            studio.status()
        );
        studio.switch_engine();
        assert_eq!(studio.setup.engine, Engine::Cpu);
        assert!(
            studio.status().ends_with("a few seconds on the CPU."),
            "{}",
            studio.status()
        );
        assert_eq!(describe_time(42.0), "45 s");
        assert_eq!(describe_time(200.0), "3 min");
    }

    /// The pieces the studio is checked on, made the way you'd make them.
    mod pieces {
        use woodsim::{Angle, Blade, Fastener, Mating, MiterSaw, Surface, TableSaw, surfaces};

        use super::*;

        fn table_saw(fence: Length, tilt: i32) -> TableSaw {
            TableSaw {
                blade: Blade {
                    diameter: Length::from_inches(10),
                    kerf: Blade::FULL_KERF,
                },
                fence,
                height: Length::from_inches(2),
                miter_gauge: None,
                gauge_angle: Angle::ZERO,
                tilt: Angle::from_hundredths(tilt),
            }
        }

        fn miter(board: &Board, far: bool) -> Surface {
            surfaces(board)
                .into_iter()
                .find(|surface| {
                    matches!(surface, Surface::Side(edge)
                        if edge.out.w != 0 && (edge.out.l > 0) == far)
                })
                .expect("a miter")
        }

        /// Puts `piece`'s `surface` against `onto`, face up, flush at the least stop.
        fn add(assembly: &mut Assembly, onto: (usize, Surface), piece: &Board, surface: Surface) {
            let other = Assembly::of(piece.clone());
            let turn = (0..4)
                .find(|&turn| {
                    let mating = Mating {
                        turn,
                        slide: [Length::ZERO; 2],
                    };
                    let motion = assembly.mating(onto, &other, (0, surface), mating).unwrap();
                    motion.rotation.apply([0.0, 0.0, 1.0])[2] > 0.999
                })
                .expect("a way that keeps it face up");
            let stops = assembly.stops(onto, &other, (0, surface), turn).unwrap();
            let mating = Mating {
                turn,
                slide: [stops[0][0], stops[1][0]],
            };
            assert!(assembly.attach(onto, &other, (0, surface), mating, Fastener::Glue));
        }

        /// A picture frame: a 1x4 ripped to a 1-1/2" strip, and four pieces mitered off it,
        /// each 10" long point to point, glued up.
        pub fn frame() -> Assembly {
            let stock = Board::new(
                Length::from_inches(48),
                Length::from_inch_fraction(7, 2),
                Length::from_inch_fraction(3, 4),
            );
            let Ok(Pass::Cut(rip)) =
                table_saw(Length::from_inch_fraction(3, 2), 0).rip(&stock, Orientation::default())
            else {
                panic!("expected the 1x4 ripped");
            };
            let saw = |hundredths| MiterSaw {
                miter: Angle::from_hundredths(hundredths),
                ..MiterSaw::twelve_inch()
            };
            let mut strip = rip.piece.board;
            let mut pieces = Vec::new();
            for _ in 0..4 {
                let Ok(Pass::Cut(trim)) =
                    saw(-4500).crosscut(&strip, Orientation::default(), Length::from_inches(2))
                else {
                    panic!("expected the end trimmed");
                };
                let Ok(Pass::Cut(cut)) = saw(4500).crosscut(
                    &trim.offcut.unwrap().board,
                    Orientation::default(),
                    Length::from_inches(10),
                ) else {
                    panic!("expected a frame piece");
                };
                pieces.push(cut.piece.board);
                strip = cut.offcut.unwrap().board;
            }
            let mut frame = Assembly::of(pieces[0].clone());
            for (last, piece) in pieces.iter().enumerate().skip(1).map(|(k, p)| (k - 1, p)) {
                let onto = (last, miter(&frame.members()[last].board, true));
                add(&mut frame, onto, piece, miter(piece, false));
            }
            frame
        }

        /// A 1x6, 18" long, with a 45° bevel ripped along one edge.
        pub fn beveled() -> Assembly {
            let stock = Board::new(
                Length::from_inches(18),
                Length::from_inch_fraction(11, 2),
                Length::from_inch_fraction(3, 4),
            );
            let Ok(Pass::Cut(rip)) =
                table_saw(Length::from_inches(4), 4500).rip(&stock, Orientation::default())
            else {
                panic!("expected the 1x6 beveled");
            };
            Assembly::of(rip.piece.board)
        }
    }

    /// Renders real pieces into `renders/` at the top of the workspace, to look at:
    /// `cargo test -p game --release -- --ignored studio_renders`. Set `WOODSHOP_QUALITY` to
    /// `draft` or `best` for less or more time over them, and `WOODSHOP_ENGINE` to `cpu` to
    /// render on the CPU.
    #[test]
    #[ignore]
    fn studio_renders() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../renders");
        let quality = match std::env::var("WOODSHOP_QUALITY").as_deref() {
            Ok("draft") => Quality::Draft,
            Ok("best") => Quality::Best,
            _ => Quality::Good,
        };
        let engine = match std::env::var("WOODSHOP_ENGINE").as_deref() {
            Ok("cpu") => Engine::Cpu,
            _ => Engine::Gpu,
        };
        let shots = [
            ("mitered frame", pieces::frame(), Backdrop::White),
            ("beveled board", pieces::beveled(), Backdrop::Gray),
            ("mitered frame", pieces::frame(), Backdrop::Acrylic),
        ];
        for (name, piece, backdrop) in shots {
            let mut studio = Studio::default();
            studio.setup.quality = quality;
            studio.setup.backdrop = backdrop;
            studio.setup.engine = engine;
            studio.open(&piece, 1.5);
            let started = std::time::Instant::now();
            studio.render(1.5, Some(crate::gpu::for_tests()));
            let saved = loop {
                if let Some(saved) = studio.poll(&folder) {
                    break saved;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            };
            println!(
                "{name} on {}, on the {}: {:?} in {:.1} s",
                backdrop.name(),
                engine.name(),
                saved,
                started.elapsed().as_secs_f32()
            );
        }
    }
}

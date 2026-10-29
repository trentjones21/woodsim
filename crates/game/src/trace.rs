//! The studio's renderer: a path tracer that runs on the CPU, in background threads, so it
//! costs nothing until a render starts. Light bounces around the scene as it would in a real
//! studio: soft boxes and a dim room light the wood, the floor bounces light back up into it,
//! and shadows fall soft and dark where the wood meets the floor.
//!
//! The scene is triangles in a bounding volume hierarchy (`Bvh`), lit by rectangular lamps
//! (`Lamp`) that only light things (the camera doesn't see them directly, though a glossy
//! finish reflects them) and by the room around it (`Environment`). Each pixel takes many
//! samples (`Render`), each a path from the camera, lit at every bounce by a lamp sampled
//! directly and weighed against finding it by bouncing (multiple importance sampling). Samples
//! follow an Owen-scrambled Sobol sequence, so they spread evenly and the noise clears fast.

use std::{
    f32::consts::{FRAC_1_PI, PI},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use bevy::math::{Affine3A, Vec2, Vec3};

use crate::grain::{self, Log, PINE, mix};

/// How many times a path may bounce. Past the third, paths that carry little light are cut
/// short at random (and those that go on count for more), so this is rarely reached.
pub(crate) const MAX_BOUNCES: u32 = 10;
/// How far a bounced ray starts off the surface it left, in meters, so it doesn't hit it again.
pub(crate) const OFFSET: f32 = 2e-5;
/// The most light one sample can bring after bouncing, so a rare bright path doesn't leave a
/// lone white pixel.
pub(crate) const BRIGHTEST_BOUNCE: f32 = 12.0;
/// Pixels are rendered in squares this many across.
const TILE: u32 = 32;
/// How much of an end grain's color a finish leaves: end grain soaks it up and reads darker.
pub(crate) const END_GRAIN: f32 = 0.62;
/// How rough a satin finish is (perceptually, 0 to 1), on faces and on end grain.
pub(crate) const SATIN: f32 = 0.3;
pub(crate) const SATIN_ON_END_GRAIN: f32 = 0.55;
/// A clear finish warms the wood under it.
pub(crate) const FINISH_TINT: Vec3 = Vec3::new(1.0, 0.94, 0.82);
/// How much light a finish reflects looking straight at it (a refractive index of 1.5).
pub(crate) const FINISH_REFLECTANCE: f32 = 0.04;

/// A ray: where it starts and which way it goes (a unit vector).
#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

/// A triangle to render: its corners, the normal at each (the same for a flat face, varying
/// across a curved one), and which material it's made of.
#[derive(Clone, Copy, Debug)]
pub struct Triangle {
    pub corners: [Vec3; 3],
    pub normals: [Vec3; 3],
    pub material: u32,
}

/// What a surface is made of.
#[derive(Clone, Debug)]
pub enum Material {
    /// Matte, like paper: this color, scattering light evenly.
    Matte(Vec3),
    /// A color under a clear finish this rough (0 to 1), like painted acrylic.
    Finished { color: Vec3, roughness: f32 },
    /// Wood under a satin finish.
    Wood(Wood),
}

/// Where a board's wood is: what turns a point in the scene into a point in its stock board
/// (along its length, across its width, up through its thickness, in meters), which way its
/// length runs in the scene, and the log the stock was cut from.
#[derive(Clone, Debug)]
pub struct Wood {
    pub to_stock: Affine3A,
    pub along: Vec3,
    pub log: Log,
}

/// A rectangular lamp, like a soft box: its middle, half its extent each way, and how bright
/// its face is. It lights the side `u × v` faces.
#[derive(Clone, Copy, Debug)]
pub struct Lamp {
    pub center: Vec3,
    pub u: Vec3,
    pub v: Vec3,
    pub radiance: Vec3,
}

impl Lamp {
    fn normal(&self) -> Vec3 {
        self.u.cross(self.v).normalize()
    }

    fn area(&self) -> f32 {
        4.0 * self.u.cross(self.v).length()
    }

    /// Where `ray` meets its face from in front, if it does before `t_max`: how far, and the
    /// cosine between the ray and its normal.
    fn hit(&self, ray: &Ray, t_max: f32) -> Option<(f32, f32)> {
        let normal = self.normal();
        let facing = -ray.dir.dot(normal);
        if facing <= 1e-6 {
            return None;
        }
        let t = (ray.origin - self.center).dot(normal) / facing;
        if t <= 0.0 || t >= t_max {
            return None;
        }
        let offset = ray.origin + ray.dir * t - self.center;
        let inside = |axis: Vec3| offset.dot(axis).abs() <= axis.length_squared();
        (inside(self.u) && inside(self.v)).then_some((t, facing))
    }
}

/// The room around the set, lighting it from every direction it isn't hidden from: a soft
/// glow overhead, darker toward the floor, with a long bright panel to each side for a finish
/// to catch.
#[derive(Clone, Copy, Debug)]
pub struct Environment {
    /// How bright straight up, at the horizon, and below.
    pub zenith: Vec3,
    pub horizon: Vec3,
    pub ground: Vec3,
    /// How bright the panels to each side are, 0 for none.
    pub panels: f32,
}

impl Environment {
    pub fn radiance(&self, dir: Vec3) -> Vec3 {
        let up = dir.y;
        let mut color = if up >= 0.0 {
            self.horizon.lerp(self.zenith, up.sqrt())
        } else {
            self.horizon.lerp(self.ground, (-up * 4.0).min(1.0))
        };
        // Two tall panels to the sides, a little behind the set, and a broad one overhead.
        let side = dir.x.abs();
        let panel = smoothstep(0.8, 0.86, side) * smoothstep(0.85, 0.6, dir.y.abs()) * {
            let back = -dir.z;
            smoothstep(-0.55, -0.45, back) * smoothstep(0.55, 0.45, back)
        };
        let overhead = smoothstep(0.9, 0.95, up);
        color += Vec3::splat(self.panels * panel + self.panels * 0.6 * overhead);
        color
    }
}

/// A camera: where it is, which ways its view runs, how wide it sees (the tangent of half its
/// view's height), and its lens's aperture radius and the distance it's focused at, in meters.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub position: Vec3,
    pub forward: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub tan_half_height: f32,
    pub aperture: f32,
    pub focus: f32,
}

impl Camera {
    /// The ray through `at` on the picture (-1 to 1 across and up, its width `aspect` times
    /// its height), from the point `lens` (0 to 1 each way) on the lens.
    fn ray(&self, at: Vec2, aspect: f32, lens: Vec2) -> Ray {
        let toward = (self.forward
            + self.right * (at.x * self.tan_half_height * aspect)
            + self.up * (at.y * self.tan_half_height))
            .normalize();
        let focused = self.position + toward * (self.focus / toward.dot(self.forward));
        let disc = concentric(lens) * self.aperture;
        let origin = self.position + self.right * disc.x + self.up * disc.y;
        Ray {
            origin,
            dir: (focused - origin).normalize(),
        }
    }
}

/// Everything a render needs: what's there, how it's lit, the camera, and how bright the
/// picture comes out (light is multiplied by `exposure` before it's turned into colors).
pub struct Scene {
    pub(crate) bvh: Bvh,
    pub(crate) materials: Vec<Material>,
    pub(crate) lamps: Vec<Lamp>,
    pub(crate) environment: Environment,
    pub(crate) camera: Camera,
    pub(crate) exposure: f32,
}

impl Scene {
    pub fn new(
        triangles: Vec<Triangle>,
        materials: Vec<Material>,
        lamps: Vec<Lamp>,
        environment: Environment,
        camera: Camera,
        exposure: f32,
    ) -> Scene {
        Scene {
            bvh: Bvh::build(triangles),
            materials,
            lamps: lamps
                .into_iter()
                .filter(|lamp| lamp.radiance.max_element() > 0.0)
                .collect(),
            environment,
            camera,
            exposure,
        }
    }

    /// How much light comes back along one path from the camera: the `index`th sample of the
    /// pixel whose own random numbers start from `seed`.
    fn sample(&self, mut ray: Ray, seed: u32, index: u32) -> Vec3 {
        let mut rng = Rng(mix(seed ^ mix(index.wrapping_add(0x5bd1_e995))));
        let mut light = Vec3::ZERO;
        let mut throughput = Vec3::ONE;
        // How likely the last bounce was to go the way it went, for weighing a lamp it found
        // against finding it directly. The camera doesn't see lamps.
        let mut last_pdf: Option<f32> = None;
        for bounce in 0..MAX_BOUNCES {
            let found = self.bvh.intersect(&ray, f32::INFINITY);
            let t_surface = found.map_or(f32::INFINITY, |hit| hit.t);
            if let Some(pdf) = last_pdf
                && let Some((lamp, t, cosine)) = self
                    .lamps
                    .iter()
                    .filter_map(|lamp| lamp.hit(&ray, t_surface).map(|(t, c)| (lamp, t, c)))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
            {
                let lamp_pdf = t * t / (cosine * lamp.area()) / self.lamps.len() as f32;
                light += clamped(
                    throughput * lamp.radiance * power_heuristic(pdf, lamp_pdf),
                    bounce,
                );
                break;
            }
            let Some(hit) = found else {
                light += clamped(throughput * self.environment.radiance(ray.dir), bounce);
                break;
            };
            let surface = self.surface(&hit, &ray);
            let wo = -ray.dir;
            let [u1, u2, u3, u4] =
                sobol4(index, mix(seed ^ (bounce + 1).wrapping_mul(0x68e3_1da4)));
            // Light from a lamp, straight to here.
            if !self.lamps.is_empty() {
                let count = self.lamps.len();
                let lamp = &self.lamps[(rng.next() * count as f32) as usize % count];
                let point = lamp.center + lamp.u * (2.0 * u3 - 1.0) + lamp.v * (2.0 * u4 - 1.0);
                let to = point - surface.position;
                let distance = to.length();
                let wi = to / distance;
                let cosine = -wi.dot(lamp.normal());
                if cosine > 0.0 && wi.dot(surface.geometric) > 0.0 {
                    let (f, pdf) = surface.eval(wo, wi);
                    if f.max_element() > 0.0
                        && !self
                            .bvh
                            .occluded(&surface.leaving(wi), distance * (1.0 - 1e-4))
                    {
                        let lamp_pdf = distance * distance / (cosine * lamp.area()) / count as f32;
                        light += clamped(
                            throughput * f * lamp.radiance * power_heuristic(lamp_pdf, pdf)
                                / lamp_pdf,
                            bounce,
                        );
                    }
                }
            }
            // On to wherever the light bounces.
            let Some((wi, weight, pdf)) = surface.sample(wo, Vec2::new(u1, u2), rng.next()) else {
                break;
            };
            throughput *= weight;
            last_pdf = Some(pdf);
            ray = surface.leaving(wi);
            if bounce >= 3 {
                let survive = throughput.max_element().min(0.95);
                if rng.next() >= survive {
                    break;
                }
                throughput /= survive;
            }
        }
        light
    }

    /// What's at a hit: where it is, which way the surface faces (toward the ray), and how it
    /// reflects light.
    fn surface(&self, hit: &Hit, ray: &Ray) -> Surface {
        let triangle = &self.bvh.shading[hit.triangle as usize];
        let position = ray.origin + ray.dir * hit.t;
        let [n0, n1, n2] = triangle.normals;
        let mut geometric = triangle.geometric;
        let mut normal = (n0 * (1.0 - hit.u - hit.v) + n1 * hit.u + n2 * hit.v).normalize();
        if geometric.dot(ray.dir) > 0.0 {
            geometric = -geometric;
        }
        if normal.dot(geometric) < 0.0 {
            normal = -normal;
        }
        let (color, coat) = match &self.materials[triangle.material as usize] {
            Material::Matte(color) => (*color, None),
            Material::Finished { color, roughness } => (*color, Some(*roughness)),
            Material::Wood(wood) => {
                let at = wood.to_stock.transform_point3(position);
                let end = smoothstep(0.55, 0.85, normal.dot(wood.along).abs());
                let color = grain::wood(&PINE, &wood.log, at)
                    * FINISH_TINT
                    * (1.0 + (END_GRAIN - 1.0) * end);
                (color, Some(SATIN + (SATIN_ON_END_GRAIN - SATIN) * end))
            }
        };
        Surface {
            position,
            geometric,
            frame: Frame::around(normal),
            color,
            alpha: coat.map(|roughness| (roughness * roughness).max(1e-4)),
        }
    }
}

/// A point on a surface and how it reflects light: diffusely in `color`, under a glossy
/// finish whose microfacets are `alpha` rough, if it has one.
struct Surface {
    position: Vec3,
    geometric: Vec3,
    frame: Frame,
    color: Vec3,
    alpha: Option<f32>,
}

impl Surface {
    /// A ray leaving the surface toward `dir`.
    fn leaving(&self, dir: Vec3) -> Ray {
        let side = if dir.dot(self.geometric) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        Ray {
            origin: self.position + self.geometric * (OFFSET * side),
            dir,
        }
    }

    /// How likely a bounce is to come off the finish rather than the color under it, seen
    /// from `cos_o` off the normal.
    fn glossy_share(&self, cos_o: f32) -> f32 {
        match self.alpha {
            None => 0.0,
            Some(_) => {
                let fresnel = schlick(cos_o);
                let diffuse = (1.0 - fresnel) * luminance(self.color);
                (fresnel / (fresnel + diffuse)).clamp(0.15, 0.9)
            }
        }
    }

    /// The light reflected toward `wo` from `wi` (times the cosine at `wi`), and how likely
    /// `sample` was to pick `wi`.
    fn eval(&self, wo: Vec3, wi: Vec3) -> (Vec3, f32) {
        let (o, i) = (self.frame.to_local(wo), self.frame.to_local(wi));
        if o.z <= 0.0 || i.z <= 0.0 {
            return (Vec3::ZERO, 0.0);
        }
        let diffuse_pdf = i.z * FRAC_1_PI;
        let Some(alpha) = self.alpha else {
            return (self.color * FRAC_1_PI * i.z, diffuse_pdf);
        };
        let a2 = alpha * alpha;
        let h = (o + i).normalize();
        let d = ggx(h.z, a2);
        let g1_o = smith_g1(o.z, a2);
        let fresnel = schlick(o.dot(h));
        let specular = d * g1_o * smith_g1(i.z, a2) * fresnel / (4.0 * o.z);
        let diffuse =
            self.color * FRAC_1_PI * (1.0 - schlick(o.z)) * (1.0 - schlick(i.z)) * i.z * 1.05;
        let share = self.glossy_share(o.z);
        let pdf = share * d * g1_o / (4.0 * o.z) + (1.0 - share) * diffuse_pdf;
        (diffuse + Vec3::splat(specular), pdf)
    }

    /// Picks the way the light bounces toward `wo`, from two random numbers for the direction
    /// and one to choose between the finish and the color under it: the direction it came
    /// from, what it's weighted by, and how likely that direction was.
    fn sample(&self, wo: Vec3, u: Vec2, choose: f32) -> Option<(Vec3, Vec3, f32)> {
        let o = self.frame.to_local(wo);
        if o.z <= 0.0 {
            return None;
        }
        let i = match self.alpha {
            Some(alpha) if choose < self.glossy_share(o.z) => {
                let h = sample_visible_normal(o, alpha, u);
                let i = h * (2.0 * o.dot(h)) - o;
                if i.z <= 0.0 {
                    return None;
                }
                i
            }
            _ => cosine_hemisphere(u),
        };
        let wi = self.frame.to_world(i);
        if wi.dot(self.geometric) <= 0.0 {
            return None;
        }
        let (f, pdf) = self.eval(wo, wi);
        (pdf > 0.0).then(|| (wi, f / pdf, pdf))
    }
}

/// The light a sample brings, held to `BRIGHTEST_BOUNCE` once it's bounced.
fn clamped(light: Vec3, bounce: u32) -> Vec3 {
    let most = light.max_element();
    if bounce == 0 || most <= BRIGHTEST_BOUNCE {
        light
    } else {
        light * (BRIGHTEST_BOUNCE / most)
    }
}

fn power_heuristic(a: f32, b: f32) -> f32 {
    let (a2, b2) = (a * a, b * b);
    if a2 + b2 > 0.0 { a2 / (a2 + b2) } else { 0.0 }
}

fn schlick(cosine: f32) -> f32 {
    let m = (1.0 - cosine).clamp(0.0, 1.0);
    let m2 = m * m;
    FINISH_REFLECTANCE + (1.0 - FINISH_REFLECTANCE) * m2 * m2 * m
}

/// How much of a rough surface's microfacets face `cos_h` off its normal.
fn ggx(cos_h: f32, a2: f32) -> f32 {
    let k = cos_h * cos_h * (a2 - 1.0) + 1.0;
    a2 / (PI * k * k)
}

/// How much of a rough surface is seen, not hidden by its own bumps, from `cos` off its normal.
fn smith_g1(cos: f32, a2: f32) -> f32 {
    2.0 * cos / (cos + (a2 + (1.0 - a2) * cos * cos).sqrt())
}

/// A microfacet normal seen from `o`, as likely as it's visible from there (Heitz 2018).
fn sample_visible_normal(o: Vec3, alpha: f32, u: Vec2) -> Vec3 {
    let v = Vec3::new(alpha * o.x, alpha * o.y, o.z).normalize();
    let along = v.x * v.x + v.y * v.y;
    let t1 = if along > 0.0 {
        Vec3::new(-v.y, v.x, 0.0) / along.sqrt()
    } else {
        Vec3::X
    };
    let t2 = v.cross(t1);
    let r = u.x.sqrt();
    let phi = 2.0 * PI * u.y;
    let p1 = r * phi.cos();
    let s = 0.5 * (1.0 + v.z);
    let p2 = (1.0 - s) * (1.0 - p1 * p1).max(0.0).sqrt() + s * r * phi.sin();
    let n = t1 * p1 + t2 * p2 + v * (1.0 - p1 * p1 - p2 * p2).max(0.0).sqrt();
    Vec3::new(alpha * n.x, alpha * n.y, n.z.max(0.0)).normalize()
}

fn cosine_hemisphere(u: Vec2) -> Vec3 {
    let d = concentric(u);
    Vec3::new(d.x, d.y, (1.0 - d.length_squared()).max(0.0).sqrt())
}

/// A point on the unit disc, spread evenly, from a point on the unit square (Shirley's
/// concentric map, which keeps neighbors neighbors).
fn concentric(u: Vec2) -> Vec2 {
    let p = u * 2.0 - Vec2::ONE;
    if p == Vec2::ZERO {
        return Vec2::ZERO;
    }
    let (r, theta) = if p.x.abs() > p.y.abs() {
        (p.x, PI / 4.0 * (p.y / p.x))
    } else {
        (p.y, PI / 2.0 - PI / 4.0 * (p.x / p.y))
    };
    Vec2::new(theta.cos(), theta.sin()) * r
}

fn luminance(color: Vec3) -> f32 {
    color.dot(Vec3::new(0.2126, 0.7152, 0.0722))
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Three directions at right angles, the third a surface's normal.
struct Frame {
    tangent: Vec3,
    bitangent: Vec3,
    normal: Vec3,
}

impl Frame {
    /// Around `normal`, without a seam (Duff et al. 2017).
    fn around(normal: Vec3) -> Frame {
        let sign = 1.0f32.copysign(normal.z);
        let a = -1.0 / (sign + normal.z);
        let b = normal.x * normal.y * a;
        Frame {
            tangent: Vec3::new(
                1.0 + sign * normal.x * normal.x * a,
                sign * b,
                -sign * normal.x,
            ),
            bitangent: Vec3::new(b, sign + normal.y * normal.y * a, -normal.y),
            normal,
        }
    }

    fn to_local(&self, v: Vec3) -> Vec3 {
        Vec3::new(
            v.dot(self.tangent),
            v.dot(self.bitangent),
            v.dot(self.normal),
        )
    }

    fn to_world(&self, v: Vec3) -> Vec3 {
        self.tangent * v.x + self.bitangent * v.y + self.normal * v.z
    }
}

/// Random numbers for what the Sobol samples don't cover: a permuted congruential generator,
/// small and fast.
struct Rng(u32);

impl Rng {
    /// A number from 0 up to 1.
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
        let word = ((self.0 >> ((self.0 >> 28) + 4)) ^ self.0).wrapping_mul(277_803_737);
        ((word >> 22) ^ word) as f32 * (1.0 / 4_294_967_296.0)
    }
}

/// The direction numbers of the first four dimensions of the Sobol sequence, from their
/// primitive polynomials (degree `s`, middle coefficients `a`) and first numbers `m`, as Joe
/// and Kuo list them; the first dimension is the van der Corput sequence.
pub(crate) const SOBOL: [[u32; 32]; 4] = [
    directions(0, 0, [0; 3]),
    directions(1, 0, [1, 0, 0]),
    directions(2, 1, [1, 3, 0]),
    directions(3, 1, [1, 3, 1]),
];

const fn directions(s: usize, a: u32, m: [u32; 3]) -> [u32; 32] {
    let mut v = [0u32; 32];
    let mut i = 0;
    while i < 32 {
        v[i] = if s == 0 {
            1 << (31 - i)
        } else if i < s {
            m[i] << (31 - i)
        } else {
            let mut x = v[i - s] ^ (v[i - s] >> s);
            let mut k = 1;
            while k < s {
                if (a >> (s - 1 - k)) & 1 == 1 {
                    x ^= v[i - k];
                }
                k += 1;
            }
            x
        };
        i += 1;
    }
    v
}

fn sobol(index: u32, dimension: usize) -> u32 {
    let mut x = 0;
    let mut bits = index;
    let mut k = 0;
    while bits != 0 {
        if bits & 1 == 1 {
            x ^= SOBOL[dimension][k];
        }
        bits >>= 1;
        k += 1;
    }
    x
}

/// Scrambles `x`'s bits as Owen scrambling does, each bit flipped depending on those above it,
/// differently for each `seed` (Burley 2020, from Laine and Karras).
fn owen(x: u32, seed: u32) -> u32 {
    let mut x = x.reverse_bits();
    x = x.wrapping_add(seed);
    x ^= x.wrapping_mul(0x6c50_b47c);
    x ^= x.wrapping_mul(0xb82f_1e52);
    x ^= x.wrapping_mul(0xc7af_e638);
    x ^= x.wrapping_mul(0x8d22_f6e6);
    x.reverse_bits()
}

/// The `index`th point of a four-dimensional Sobol sequence, shuffled and scrambled by `seed`,
/// each coordinate from 0 up to 1.
fn sobol4(index: u32, seed: u32) -> [f32; 4] {
    let shuffled = owen(index, seed);
    [0, 1, 2, 3].map(|dimension| {
        let x = owen(
            sobol(shuffled, dimension),
            mix(seed ^ (dimension as u32 + 1).wrapping_mul(0x9e37_79b9)),
        );
        (x >> 8) as f32 * (1.0 / 16_777_216.0)
    })
}

/// Where a ray met a triangle: how far along it, the triangle (in the hierarchy's order), and
/// where on it, as weights of its second and third corners.
#[derive(Clone, Copy, Debug)]
struct Hit {
    t: f32,
    triangle: u32,
    u: f32,
    v: f32,
}

/// A triangle as the ray test wants it: a corner and the edges from it.
#[derive(Clone, Copy)]
pub(crate) struct Packed {
    pub(crate) corner: Vec3,
    pub(crate) edge1: Vec3,
    pub(crate) edge2: Vec3,
}

/// What shading needs of a triangle.
#[derive(Clone, Copy)]
pub(crate) struct Shading {
    pub(crate) geometric: Vec3,
    pub(crate) normals: [Vec3; 3],
    pub(crate) material: u32,
}

/// A box in the hierarchy: what it bounds, and either its first child (the second follows it)
/// or, if `count` isn't zero, the first of its triangles.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Node {
    pub(crate) min: Vec3,
    pub(crate) max: Vec3,
    pub(crate) first: u32,
    pub(crate) count: u32,
}

/// A bounding volume hierarchy: boxes in boxes around the triangles, so a ray only tests the
/// few triangles near its path.
pub(crate) struct Bvh {
    pub(crate) nodes: Vec<Node>,
    pub(crate) packed: Vec<Packed>,
    pub(crate) shading: Vec<Shading>,
}

/// How many triangles a box holds before it's split, and how many ways splits are tried along
/// each axis.
const LEAF: usize = 4;
const BINS: usize = 12;

impl Bvh {
    fn build(triangles: Vec<Triangle>) -> Bvh {
        let triangles: Vec<Triangle> = triangles
            .into_iter()
            .filter(|t| {
                (t.corners[1] - t.corners[0]).cross(t.corners[2] - t.corners[0]) != Vec3::ZERO
            })
            .collect();
        let centers: Vec<Vec3> = triangles
            .iter()
            .map(|t| (t.corners[0] + t.corners[1] + t.corners[2]) / 3.0)
            .collect();
        let bounds: Vec<(Vec3, Vec3)> = triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.corners;
                (a.min(b).min(c), a.max(b).max(c))
            })
            .collect();
        let mut order: Vec<u32> = (0..triangles.len() as u32).collect();
        let mut nodes = vec![Node {
            min: Vec3::ZERO,
            max: Vec3::ZERO,
            first: 0,
            count: triangles.len() as u32,
        }];
        let mut pending = vec![0usize];
        while let Some(index) = pending.pop() {
            let (first, count) = (nodes[index].first as usize, nodes[index].count as usize);
            let items = &mut order[first..first + count];
            let (min, max) = items.iter().fold(
                (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
                |(min, max), &i| (min.min(bounds[i as usize].0), max.max(bounds[i as usize].1)),
            );
            nodes[index].min = min;
            nodes[index].max = max;
            if count <= LEAF {
                continue;
            }
            let Some(split) = best_split(items, &centers, &bounds) else {
                continue;
            };
            let (axis, at) = split;
            let mut left = 0;
            for k in 0..items.len() {
                if centers[items[k] as usize][axis] < at {
                    items.swap(k, left);
                    left += 1;
                }
            }
            if left == 0 || left == count {
                continue;
            }
            let child = nodes.len();
            nodes.push(Node {
                min: Vec3::ZERO,
                max: Vec3::ZERO,
                first: first as u32,
                count: left as u32,
            });
            nodes.push(Node {
                min: Vec3::ZERO,
                max: Vec3::ZERO,
                first: (first + left) as u32,
                count: (count - left) as u32,
            });
            nodes[index].first = child as u32;
            nodes[index].count = 0;
            pending.extend([child, child + 1]);
        }
        let packed = order
            .iter()
            .map(|&i| {
                let [a, b, c] = triangles[i as usize].corners;
                Packed {
                    corner: a,
                    edge1: b - a,
                    edge2: c - a,
                }
            })
            .collect();
        let shading = order
            .iter()
            .map(|&i| {
                let t = &triangles[i as usize];
                let [a, b, c] = t.corners;
                let mut geometric = (b - a).cross(c - a).normalize();
                if geometric.dot(t.normals[0] + t.normals[1] + t.normals[2]) < 0.0 {
                    geometric = -geometric;
                }
                Shading {
                    geometric,
                    normals: t.normals,
                    material: t.material,
                }
            })
            .collect();
        Bvh {
            nodes,
            packed,
            shading,
        }
    }

    /// The nearest triangle `ray` meets before `t_max`.
    fn intersect(&self, ray: &Ray, t_max: f32) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        let mut nearest = t_max;
        self.walk(
            ray,
            |first, count, nearest_so_far| {
                for k in first..first + count {
                    if let Some((t, u, v)) = self.packed[k].hit(ray, *nearest_so_far) {
                        *nearest_so_far = t;
                        best = Some(Hit {
                            t,
                            triangle: k as u32,
                            u,
                            v,
                        });
                    }
                }
                false
            },
            &mut nearest,
        );
        best
    }

    /// Whether anything is in the way of `ray` before `t_max`.
    fn occluded(&self, ray: &Ray, t_max: f32) -> bool {
        let mut limit = t_max;
        let mut blocked = false;
        self.walk(
            ray,
            |first, count, limit| {
                blocked = (first..first + count).any(|k| self.packed[k].hit(ray, *limit).is_some());
                blocked
            },
            &mut limit,
        );
        blocked
    }

    /// Goes through the boxes `ray` passes through before `limit`, nearest first, handing
    /// each leaf's triangles to `visit`, which may bring `limit` in, or return true to stop.
    fn walk(
        &self,
        ray: &Ray,
        mut visit: impl FnMut(usize, usize, &mut f32) -> bool,
        limit: &mut f32,
    ) {
        if self.packed.is_empty() {
            return;
        }
        let inverse = Vec3::ONE / ray.dir;
        let mut stack = [0u32; 64];
        let mut depth = 1;
        while depth > 0 {
            depth -= 1;
            let node = &self.nodes[stack[depth] as usize];
            if node.count > 0 {
                if visit(node.first as usize, node.count as usize, limit) {
                    return;
                }
                continue;
            }
            let (a, b) = (node.first as usize, node.first as usize + 1);
            let near_a = self.nodes[a].entered(ray.origin, inverse, *limit);
            let near_b = self.nodes[b].entered(ray.origin, inverse, *limit);
            match (near_a, near_b) {
                (Some(ta), Some(tb)) => {
                    let (first, second) = if ta <= tb { (a, b) } else { (b, a) };
                    stack[depth] = second as u32;
                    stack[depth + 1] = first as u32;
                    depth += 2;
                }
                (Some(_), None) => {
                    stack[depth] = a as u32;
                    depth += 1;
                }
                (None, Some(_)) => {
                    stack[depth] = b as u32;
                    depth += 1;
                }
                (None, None) => {}
            }
        }
    }
}

impl Node {
    /// How far along a ray it enters the box, if it does before `limit`.
    fn entered(&self, origin: Vec3, inverse: Vec3, limit: f32) -> Option<f32> {
        let t0 = (self.min - origin) * inverse;
        let t1 = (self.max - origin) * inverse;
        let near = t0.min(t1).max_element().max(0.0);
        let far = t0.max(t1).min_element().min(limit);
        (near <= far).then_some(near)
    }
}

impl Packed {
    /// Where `ray` crosses the triangle, if it does before `limit`: how far, and the weights
    /// of its second and third corners there (Möller and Trumbore).
    fn hit(&self, ray: &Ray, limit: f32) -> Option<(f32, f32, f32)> {
        let p = ray.dir.cross(self.edge2);
        let det = self.edge1.dot(p);
        if det.abs() < 1e-14 {
            return None;
        }
        let inverse = 1.0 / det;
        let s = ray.origin - self.corner;
        let u = s.dot(p) * inverse;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = s.cross(self.edge1);
        let v = ray.dir.dot(q) * inverse;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let t = self.edge2.dot(q) * inverse;
        (t > 0.0 && t < limit).then_some((t, u, v))
    }
}

/// The best place to split `items` in two, by the surface area heuristic: the axis and where
/// along it, or none if no split is cheaper than testing them all.
fn best_split(items: &[u32], centers: &[Vec3], bounds: &[(Vec3, Vec3)]) -> Option<(usize, f32)> {
    let (low, high) = items.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(low, high), &i| (low.min(centers[i as usize]), high.max(centers[i as usize])),
    );
    let area = |min: Vec3, max: Vec3| {
        let d = (max - min).max(Vec3::ZERO);
        d.x * d.y + d.y * d.z + d.z * d.x
    };
    let mut best: Option<(f32, usize, f32)> = None;
    for axis in 0..3 {
        let extent = high[axis] - low[axis];
        if extent <= 0.0 {
            continue;
        }
        let mut bins = [(0usize, Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)); BINS];
        for &i in items {
            let bin = (((centers[i as usize][axis] - low[axis]) / extent * BINS as f32) as usize)
                .min(BINS - 1);
            let (count, min, max) = &mut bins[bin];
            *count += 1;
            *min = min.min(bounds[i as usize].0);
            *max = max.max(bounds[i as usize].1);
        }
        for split in 1..BINS {
            let side = |range: std::ops::Range<usize>| {
                bins[range].iter().fold(
                    (0, Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
                    |(n, lo, hi), &(count, min, max)| (n + count, lo.min(min), hi.max(max)),
                )
            };
            let (n_left, left_min, left_max) = side(0..split);
            let (n_right, right_min, right_max) = side(split..BINS);
            if n_left == 0 || n_right == 0 {
                continue;
            }
            let cost = n_left as f32 * area(left_min, left_max)
                + n_right as f32 * area(right_min, right_max);
            if best.is_none_or(|(least, _, _)| cost < least) {
                best = Some((cost, axis, low[axis] + extent * split as f32 / BINS as f32));
            }
        }
    }
    let (all_min, all_max) = items.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(min, max), &i| (min.min(bounds[i as usize].0), max.max(bounds[i as usize].1)),
    );
    let (cost, axis, at) = best?;
    // Testing a box costs about as much as a triangle.
    (cost < (items.len() as f32 - 1.0) * area(all_min, all_max)).then_some((axis, at))
}

/// Turns light into a color on screen: a filmic curve that rolls bright light off gently
/// without shifting hues (Khronos PBR Neutral), then sRGB.
pub fn develop(light: Vec3) -> Vec3 {
    const START: f32 = 0.8 - 0.04;
    const DESATURATION: f32 = 0.15;
    let least = light.min_element();
    let offset = if least < 0.08 {
        least - 6.25 * least * least
    } else {
        0.04
    };
    let color = light - Vec3::splat(offset);
    let peak = color.max_element();
    let color = if peak < START {
        color
    } else {
        let d = 1.0 - START;
        let new_peak = 1.0 - d * d / (peak + d - START);
        let color = color * (new_peak / peak);
        let g = 1.0 - 1.0 / (DESATURATION * (peak - new_peak) + 1.0);
        color.lerp(Vec3::splat(new_peak), g)
    };
    color.max(Vec3::ZERO).map(|c| {
        if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
    })
}

/// A rendered picture's pixels, four bytes each (red, green, blue, opaque), row by row from
/// the top, and how many times it's been developed.
#[derive(Clone, Default)]
pub struct Picture {
    pub pixels: Vec<u8>,
    pub version: u64,
}

/// A square of pixels and the light their samples have brought so far.
struct Tile {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    light: Vec<Vec3>,
    samples: u32,
}

/// What the render's threads share.
struct Shared {
    scene: Scene,
    width: u32,
    height: u32,
    /// Which samples each pass takes, from and up to: a single sample first, so the picture
    /// appears quickly, then more each pass.
    passes: Vec<(u32, u32)>,
    tiles: Vec<Mutex<Tile>>,
    /// The next tile to render, counting through every pass, and how many are done.
    next: AtomicUsize,
    done: AtomicUsize,
    samples_done: AtomicU64,
    stop: AtomicBool,
    picture: Mutex<Picture>,
    developing: Mutex<()>,
}

/// A render under way, in background threads. Dropping it stops them.
pub struct Render {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
    started: Instant,
}

impl Render {
    /// Starts rendering `scene` `width` by `height` pixels with `samples` samples each, on
    /// every core but two, which the game keeps.
    pub fn start(scene: Scene, width: u32, height: u32, samples: u32) -> Render {
        let threads =
            thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(2).max(1));
        Render::with_threads(scene, width, height, samples, threads)
    }

    fn with_threads(scene: Scene, width: u32, height: u32, samples: u32, threads: usize) -> Render {
        let mut passes = Vec::new();
        let mut from = 0;
        while from < samples {
            let step = from.clamp(1, 16);
            passes.push((from, (from + step).min(samples)));
            from += step;
        }
        let mut tiles = Vec::new();
        for y in (0..height).step_by(TILE as usize) {
            for x in (0..width).step_by(TILE as usize) {
                let (w, h) = (TILE.min(width - x), TILE.min(height - y));
                tiles.push(Mutex::new(Tile {
                    x,
                    y,
                    width: w,
                    height: h,
                    light: vec![Vec3::ZERO; (w * h) as usize],
                    samples: 0,
                }));
            }
        }
        let shared = Arc::new(Shared {
            scene,
            width,
            height,
            passes,
            tiles,
            next: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            samples_done: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            picture: Mutex::new(Picture::default()),
            developing: Mutex::new(()),
        });
        let workers = (0..threads)
            .map(|_| {
                let shared = shared.clone();
                thread::spawn(move || shared.work())
            })
            .collect();
        Render {
            shared,
            workers,
            started: Instant::now(),
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.shared.width, self.shared.height)
    }

    /// How far along it is, from 0 to 1.
    pub fn progress(&self) -> f32 {
        let shared = &self.shared;
        let total = shared.width as u64
            * shared.height as u64
            * shared.passes.last().map_or(0, |p| p.1) as u64;
        if total == 0 {
            return 1.0;
        }
        shared.samples_done.load(Ordering::Relaxed) as f32 / total as f32
    }

    /// How many samples each pixel has had, at least.
    pub fn samples(&self) -> u32 {
        let shared = &self.shared;
        let done = shared.done.load(Ordering::Relaxed);
        let passes = done / shared.tiles.len().max(1);
        match passes {
            0 => 0,
            n => shared.passes[(n - 1).min(shared.passes.len() - 1)].1,
        }
    }

    pub fn is_done(&self) -> bool {
        self.shared.done.load(Ordering::Acquire)
            >= self.shared.passes.len() * self.shared.tiles.len()
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// The picture as it's come out so far, if it's been developed again since `version`.
    pub fn picture_since(&self, version: u64) -> Option<Picture> {
        let picture = self.shared.picture.lock().ok()?;
        (picture.version > version).then(|| picture.clone())
    }

    /// Stops where it's got to, and gives the picture as it is.
    pub fn stop_and_finish(self) -> Picture {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.finish()
    }

    /// Waits for it to finish, and gives the finished picture.
    pub fn finish(mut self) -> Picture {
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
        self.shared.develop();
        self.shared
            .picture
            .lock()
            .map(|p| p.clone())
            .unwrap_or_default()
    }
}

impl Drop for Render {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

impl Shared {
    /// Renders tiles until every pass is done or the render is stopped.
    fn work(&self) {
        let tiles = self.tiles.len();
        let jobs = tiles * self.passes.len();
        while !self.stop.load(Ordering::Relaxed) {
            let job = self.next.fetch_add(1, Ordering::Relaxed);
            if job >= jobs {
                return;
            }
            let (from, to) = self.passes[job / tiles];
            self.render_tile(job % tiles, from, to);
            let done = self.done.fetch_add(1, Ordering::AcqRel) + 1;
            if done == jobs {
                self.develop();
            } else if done.is_multiple_of(tiles) {
                // Another thread developing the picture now can show this pass too.
                if let Ok(_developing) = self.developing.try_lock() {
                    self.develop_now();
                }
            }
        }
    }

    fn render_tile(&self, index: usize, from: u32, to: u32) {
        let (x0, y0, width, height) = {
            let Ok(tile) = self.tiles[index].lock() else {
                return;
            };
            (tile.x, tile.y, tile.width, tile.height)
        };
        let mut light = vec![Vec3::ZERO; (width * height) as usize];
        let aspect = self.width as f32 / self.height as f32;
        let camera = &self.scene.camera;
        for y in 0..height {
            if self.stop.load(Ordering::Relaxed) {
                return;
            }
            for x in 0..width {
                let (px, py) = (x0 + x, y0 + y);
                let seed =
                    mix(px.wrapping_mul(0x8da6_b343) ^ py.wrapping_mul(0xd816_3841) ^ 0x2c1b_3c6d);
                let mut sum = Vec3::ZERO;
                for index in from..to {
                    let [jx, jy, lx, ly] = sobol4(index, seed);
                    let at = Vec2::new(
                        (px as f32 + 0.5 + tent(jx)) / self.width as f32 * 2.0 - 1.0,
                        1.0 - (py as f32 + 0.5 + tent(jy)) / self.height as f32 * 2.0,
                    );
                    let ray = camera.ray(at, aspect, Vec2::new(lx, ly));
                    let sample = self.scene.sample(ray, seed, index);
                    if sample.is_finite() {
                        sum += sample;
                    }
                }
                light[(y * width + x) as usize] = sum;
            }
        }
        if let Ok(mut tile) = self.tiles[index].lock() {
            for (total, more) in tile.light.iter_mut().zip(light) {
                *total += more;
            }
            tile.samples += to - from;
        }
        self.samples_done
            .fetch_add((width * height * (to - from)) as u64, Ordering::Relaxed);
    }

    /// Develops the picture from every tile as it is now.
    fn develop(&self) {
        if let Ok(_developing) = self.developing.lock() {
            self.develop_now();
        }
    }

    fn develop_now(&self) {
        let (width, height) = (self.width as usize, self.height as usize);
        let mut pixels = vec![0u8; width * height * 4];
        let half = Vec2::new(width as f32, height as f32) / 2.0;
        for tile in &self.tiles {
            let Ok(tile) = tile.lock() else {
                continue;
            };
            if tile.samples == 0 {
                continue;
            }
            let scale = self.scene.exposure / tile.samples as f32;
            for y in 0..tile.height {
                for x in 0..tile.width {
                    let (px, py) = ((tile.x + x) as usize, (tile.y + y) as usize);
                    // A touch of vignetting, as a lens has.
                    let off = (Vec2::new(px as f32, py as f32) - half) / half.length();
                    let vignette = 1.0 - 0.35 * off.length_squared().powf(1.5);
                    let light = tile.light[(y * tile.width + x) as usize] * scale * vignette;
                    let color = develop(light);
                    // A little noise breaks up banding in smooth gradients.
                    let dither = (mix((py * width + px) as u32) >> 24) as f32 / 255.0 - 0.5;
                    let at = (py * width + px) * 4;
                    for channel in 0..3 {
                        pixels[at + channel] =
                            (color[channel] * 255.0 + dither).round().clamp(0.0, 255.0) as u8;
                    }
                    pixels[at + 3] = 255;
                }
            }
        }
        if let Ok(mut picture) = self.picture.lock() {
            picture.pixels = pixels;
            picture.version += 1;
        }
    }
}

/// A random offset from a pixel's middle, from -1 to 1 and likelier near 0, so neighboring
/// pixels blend a little at edges, as a camera's do.
fn tent(u: f32) -> f32 {
    if u < 0.5 {
        (2.0 * u).sqrt() - 1.0
    } else {
        1.0 - (2.0 - 2.0 * u).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square on the floor (`y` = 0) `half` across each way from the middle, facing up.
    fn floor(half: f32, material: u32) -> Vec<Triangle> {
        let corners = [
            Vec3::new(-half, 0.0, -half),
            Vec3::new(half, 0.0, -half),
            Vec3::new(half, 0.0, half),
            Vec3::new(-half, 0.0, half),
        ];
        [[0, 2, 1], [0, 3, 2]]
            .map(|[a, b, c]| Triangle {
                corners: [corners[a], corners[b], corners[c]],
                normals: [Vec3::Y; 3],
                material,
            })
            .to_vec()
    }

    fn looking_down(height: f32) -> Camera {
        Camera {
            position: Vec3::new(0.0, height, 0.0),
            forward: Vec3::NEG_Y,
            right: Vec3::X,
            up: Vec3::NEG_Z,
            tan_half_height: 0.01,
            aperture: 0.0,
            focus: height,
        }
    }

    fn dark() -> Environment {
        Environment {
            zenith: Vec3::ZERO,
            horizon: Vec3::ZERO,
            ground: Vec3::ZERO,
            panels: 0.0,
        }
    }

    /// The average light the camera sees straight ahead, over `samples` samples.
    fn seen(scene: &Scene, samples: u32) -> Vec3 {
        let mut sum = Vec3::ZERO;
        for index in 0..samples {
            let ray = scene.camera.ray(Vec2::ZERO, 1.0, Vec2::splat(0.5));
            sum += scene.sample(ray, 12345, index);
        }
        sum / samples as f32
    }

    #[test]
    fn a_matte_floor_under_an_even_sky_reflects_its_color() {
        // Lit evenly from the whole sky above, a matte surface's radiance is its albedo
        // times the sky's.
        let sky = Environment {
            zenith: Vec3::ONE,
            horizon: Vec3::ONE,
            ground: Vec3::ONE,
            panels: 0.0,
        };
        let scene = Scene::new(
            floor(1000.0, 0),
            vec![Material::Matte(Vec3::splat(0.5))],
            Vec::new(),
            sky,
            looking_down(1.0),
            1.0,
        );
        let light = seen(&scene, 4096);
        assert!(
            (light - Vec3::splat(0.5)).abs().max_element() < 0.01,
            "{light}"
        );
    }

    #[test]
    fn a_lamp_overhead_lights_the_floor_as_its_size_and_distance_say() {
        // A small lamp of radiance L and area A, d above the floor and facing it, gives it an
        // irradiance of about L A / d²; a matte floor of albedo ρ then reflects ρ E / π.
        let (radiance, half, height) = (100.0, 0.05, 2.0);
        let lamp = Lamp {
            center: Vec3::new(0.0, height, 0.0),
            u: Vec3::new(half, 0.0, 0.0),
            v: Vec3::new(0.0, 0.0, half),
            radiance: Vec3::splat(radiance),
        };
        assert!(lamp.normal().y < 0.0, "it faces down");
        let mut camera = looking_down(1.0);
        camera.position = Vec3::new(0.0, 1.0, 0.3);
        let scene = Scene::new(
            floor(1000.0, 0),
            vec![Material::Matte(Vec3::splat(0.5))],
            vec![lamp],
            dark(),
            camera,
            1.0,
        );
        let at = Vec3::new(0.0, 0.0, 0.3);
        let cos = height / (at - lamp.center).length();
        let distance2 = (at - lamp.center).length_squared();
        let expected = 0.5 * radiance * lamp.area() * cos * cos / distance2 / PI;
        let light = seen(&scene, 2048);
        assert!(
            (light.x / expected - 1.0).abs() < 0.03,
            "{light} vs {expected}"
        );
    }

    #[test]
    fn a_finish_reflects_a_lamp_whether_found_by_bouncing_or_directly() {
        // A lamp seen in a nearly mirror-like finish: what comes back is the lamp, dimmed by
        // the finish's reflectance, however the two ways of finding it are weighed.
        let lamp = Lamp {
            center: Vec3::new(0.0, 3.0, 0.0),
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 0.0, 1.0),
            radiance: Vec3::splat(10.0),
        };
        let scene = Scene::new(
            floor(1000.0, 0),
            vec![Material::Finished {
                color: Vec3::ZERO,
                roughness: 0.05,
            }],
            vec![lamp],
            dark(),
            looking_down(1.0),
            1.0,
        );
        let light = seen(&scene, 2048);
        assert!(
            (light.x / (10.0 * FINISH_REFLECTANCE) - 1.0).abs() < 0.05,
            "{light}"
        );
    }

    #[test]
    fn the_hierarchy_finds_what_testing_every_triangle_finds() {
        let mut triangles = Vec::new();
        let mut rng = Rng(7);
        for _ in 0..500 {
            let a = Vec3::new(rng.next(), rng.next(), rng.next()) * 2.0 - Vec3::ONE;
            let b = a + (Vec3::new(rng.next(), rng.next(), rng.next()) - Vec3::splat(0.5)) * 0.3;
            let c = a + (Vec3::new(rng.next(), rng.next(), rng.next()) - Vec3::splat(0.5)) * 0.3;
            triangles.push(Triangle {
                corners: [a, b, c],
                normals: [Vec3::Y; 3],
                material: 0,
            });
        }
        let bvh = Bvh::build(triangles.clone());
        for _ in 0..2000 {
            let origin = (Vec3::new(rng.next(), rng.next(), rng.next()) * 2.0 - Vec3::ONE) * 3.0;
            let target = Vec3::new(rng.next(), rng.next(), rng.next()) - Vec3::splat(0.5);
            let ray = Ray {
                origin,
                dir: (target - origin).normalize(),
            };
            let brute = triangles
                .iter()
                .filter_map(|t| {
                    let packed = Packed {
                        corner: t.corners[0],
                        edge1: t.corners[1] - t.corners[0],
                        edge2: t.corners[2] - t.corners[0],
                    };
                    packed.hit(&ray, f32::INFINITY).map(|(t, _, _)| t)
                })
                .fold(f32::INFINITY, f32::min);
            let found = bvh
                .intersect(&ray, f32::INFINITY)
                .map_or(f32::INFINITY, |hit| hit.t);
            assert_eq!(found, brute);
            assert_eq!(bvh.occluded(&ray, 100.0), brute < 100.0);
        }
    }

    #[test]
    fn sobol_points_fall_evenly_into_every_stratum() {
        for dimension in 0..4 {
            let mut values: Vec<u32> = (0..16).map(|i| sobol(i, dimension) >> 28).collect();
            values.sort();
            assert_eq!(values, (0..16).collect::<Vec<_>>(), "dimension {dimension}");
        }
        // The first two dimensions together put one point in each of 16 cells of any shape
        // of 16 equal rectangles.
        for (columns, rows) in [(16, 1), (8, 2), (4, 4), (2, 8), (1, 16)] {
            let mut cells: Vec<(u32, u32)> = (0..16)
                .map(|i| {
                    let point = sobol4(i, 99);
                    (
                        (point[0] * columns as f32) as u32,
                        (point[1] * rows as f32) as u32,
                    )
                })
                .collect();
            cells.sort();
            cells.dedup();
            assert_eq!(cells.len(), 16, "{columns} x {rows}");
        }
    }

    #[test]
    fn develop_keeps_black_black_and_rolls_off_bright_light() {
        assert_eq!(develop(Vec3::ZERO), Vec3::ZERO);
        let mut last = 0.0;
        for k in 1..100 {
            let value = develop(Vec3::splat(k as f32 * 0.1)).x;
            assert!(value >= last && value <= 1.0, "{k}: {value}");
            last = value;
        }
        assert!(last > 0.97);
    }

    #[test]
    fn a_render_fills_every_pixel_and_finishes() {
        let sky = Environment {
            zenith: Vec3::splat(0.5),
            horizon: Vec3::splat(0.5),
            ground: Vec3::splat(0.5),
            panels: 0.0,
        };
        let mut camera = looking_down(1.0);
        camera.tan_half_height = 0.5;
        let scene = Scene::new(
            floor(0.3, 0),
            vec![Material::Matte(Vec3::splat(0.8))],
            Vec::new(),
            sky,
            camera,
            1.0,
        );
        let render = Render::with_threads(scene, 70, 40, 8, 3);
        let picture = render.finish();
        assert_eq!(picture.pixels.len(), 70 * 40 * 4);
        assert!(picture.pixels.chunks(4).all(|pixel| pixel[3] == 255));
        // The floor fills the middle; the sky shows around it.
        let middle = &picture.pixels[(20 * 70 + 35) * 4..][..3];
        assert!(middle.iter().all(|&c| c > 100), "{middle:?}");
    }
}

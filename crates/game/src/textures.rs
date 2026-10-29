//! Textures made in code: wood grain, the shop floor, pegboard, and grass. There are no image
//! files; everything here is generated at startup.
//!
//! Every texture tiles seamlessly, so surfaces of any size can repeat it. They're painted
//! rather than photographic: soft color and pattern, no bumps.

use std::f32::consts::TAU;

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::meshes::BladeShape;

/// One repeat of a wood texture covers this much board, in meters: along the grain, and across.
pub const WOOD_TILE_ALONG: f32 = 0.8;
pub const WOOD_TILE_ACROSS: f32 = 0.2;
/// One repeat of the floor covers this much, in meters: two tiles each way.
pub const FLOOR_TILE: f32 = 1.0;
/// One repeat of pegboard: four holes each way, an inch apart.
pub const PEGBOARD_TILE: f32 = 4.0 * 0.0254;
/// One repeat of the lawn, in meters.
pub const GRASS_TILE: f32 = 4.0;

/// How a kind of wood looks.
pub struct Species {
    /// The light, fast-growing part of each growth ring (sRGB).
    pub earlywood: [f32; 3],
    /// The dense, darker band at the end of each ring (sRGB).
    pub latewood: [f32; 3],
    /// Growth rings across one tile; more rings means tighter grain.
    pub rings: i32,
    /// How far the grain wanders from straight, in rings.
    pub waviness: f32,
    pub seed: u32,
}

pub const PINE: Species = Species {
    earlywood: [0.99, 0.84, 0.6],
    latewood: [0.92, 0.64, 0.38],
    rings: 7,
    waviness: 2.6,
    seed: 7,
};

pub const MAPLE: Species = Species {
    earlywood: [0.99, 0.89, 0.72],
    latewood: [0.93, 0.77, 0.56],
    rings: 16,
    waviness: 1.8,
    seed: 31,
};

/// Wood grain. U runs along the grain.
pub fn wood(species: &Species) -> Image {
    const WIDTH: u32 = 1024;
    const HEIGHT: u32 = 256;
    let seed = species.seed;
    let mut color = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let (u, v) = (x as f32 / WIDTH as f32, y as f32 / HEIGHT as f32);
            // The grain wanders slowly along the board and a little across it.
            let warp = (fbm(u, v, 3, 2, 4, seed) - 0.5) * species.waviness
                + (fbm(u, v, 12, 6, 2, seed + 10) - 0.5) * 0.25;
            let ring = v * species.rings as f32 + warp;
            let t = ring - ring.floor();
            // Each ring has its own share of dense latewood, so the bands vary in width. It
            // darkens in fairly quickly, then the next ring starts abruptly.
            let index = (ring.floor() as i32).rem_euclid(species.rings);
            let start = 0.5 + 0.3 * hash(index, 0, seed + 40);
            let latewood = smoothstep(start, start + 0.14, t) * 0.85;
            // Faint fibers along the grain, and broad patches of lighter and darker wood.
            let fiber = tiled_noise(u * 16.0, v * 320.0, 16, 320, seed + 20);
            let patch = (fbm(u, v, 2, 1, 3, seed + 30) - 0.5) * 0.1;
            let shade = 1.0 + patch + (fiber - 0.5) * 0.05;
            for channel in 0..3 {
                let base = lerp(
                    species.earlywood[channel],
                    species.latewood[channel],
                    latewood,
                );
                color.push(to_byte(base * shade));
            }
            color.push(255);
        }
    }
    texture(WIDTH, HEIGHT, color, TextureFormat::Rgba8UnormSrgb)
}

/// The shop floor: big square tiles in two warm grays, checkered, with pale grout.
pub fn floor() -> Image {
    const SIZE: u32 = 512;
    const TILES: u32 = 2;
    let tile = SIZE / TILES;
    let mut color = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (u, v) = (x as f32 / SIZE as f32, y as f32 / SIZE as f32);
            let checker = (x / tile + y / tile) % 2;
            let base = if checker == 0 {
                [0.8, 0.77, 0.72]
            } else {
                [0.73, 0.7, 0.65]
            };
            // A gentle mottle, so the tiles don't look like flat paint.
            let shade = 1.0 + (fbm(u, v, 4, 4, 4, 101) - 0.5) * 0.06;
            // Grout runs along each tile's edge.
            let edge = (x % tile)
                .min(tile - 1 - x % tile)
                .min(y % tile)
                .min(tile - 1 - y % tile);
            let grout = 1.0 - smoothstep(1.0, 2.5, edge as f32);
            for (channel, base) in base.into_iter().enumerate() {
                let grout_color = [0.9, 0.88, 0.84][channel];
                color.push(to_byte(lerp(base * shade, grout_color, grout)));
            }
            color.push(255);
        }
    }
    texture(SIZE, SIZE, color, TextureFormat::Rgba8UnormSrgb)
}

/// Pegboard with a hole every inch. It's pale, so a material's color paints it.
pub fn pegboard() -> Image {
    const SIZE: u32 = 256;
    let mut color = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (u, v) = (x as f32 / SIZE as f32, y as f32 / SIZE as f32);
            // Distance to the nearest hole center, in quarter-tiles (one inch each).
            let (cu, cv) = ((u * 4.0).fract() - 0.5, (v * 4.0).fract() - 0.5);
            let hole = 1.0 - smoothstep(0.1, 0.13, (cu * cu + cv * cv).sqrt());
            let grain = (fbm(u, v, 8, 8, 3, 404) - 0.5) * 0.06;
            for _ in 0..3 {
                color.push(to_byte(lerp(0.95 * (1.0 + grain), 0.2, hole)));
            }
            color.push(255);
        }
    }
    texture(SIZE, SIZE, color, TextureFormat::Rgba8UnormSrgb)
}

/// A mown lawn: bright green with soft patches of lighter and darker grass.
pub fn grass() -> Image {
    const SIZE: u32 = 256;
    let mut color = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (u, v) = (x as f32 / SIZE as f32, y as f32 / SIZE as f32);
            let patch = fbm(u, v, 3, 3, 4, 505) - 0.5;
            let blades = tiled_noise(u * 128.0, v * 128.0, 128, 128, 606) - 0.5;
            let shade = 1.0 + patch * 0.16 + blades * 0.06;
            // Lighter patches lean yellow, darker ones blue, like real grass in the sun.
            for (channel, base) in [0.47, 0.74, 0.32].into_iter().enumerate() {
                let tint = [0.06, 0.0, -0.05][channel] * patch;
                color.push(to_byte(base * shade + tint));
            }
            color.push(255);
        }
    }
    texture(SIZE, SIZE, color, TextureFormat::Rgba8UnormSrgb)
}

/// A saw blade's face, square on, filling the square around it (`meshes::saw_blade` lays it on
/// flat): bright steel, dark carbide on the front of each tooth, four expansion slots cut in
/// from the rim, a mark printed near the rim, and a red label around the arbor nut with an
/// arrow the way the blade turns. It's opaque everywhere: the mesh's outline cuts the teeth.
pub fn saw_blade(shape: &BladeShape) -> Image {
    const SIZE: u32 = 512;
    let mut color = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let pixel = 2.0 * shape.radius / SIZE as f32;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (u, v) = (
                (x as f32 + 0.5) / SIZE as f32,
                (y as f32 + 0.5) / SIZE as f32,
            );
            let (up, out) = (
                (0.5 - v) * 2.0 * shape.radius,
                (u - 0.5) * 2.0 * shape.radius,
            );
            let paint = blade_paint(shape, up.hypot(out), out.atan2(up), pixel);
            color.extend(paint.map(to_byte));
            color.push(255);
        }
    }
    blade_texture(SIZE, color)
}

/// The same blade spinning too fast for its teeth to be seen: each point shows the blade as it
/// passed there over the last turn, more of what passed most recently, so the label's arrow and
/// the printed mark smear into arcs trailing behind the way it turns, and the teeth into a
/// see-through ring as solid as they are wide. Over that, white streaks whirl around it, as
/// a cartoon draws speed: turned slowly, they and the arcs show it spinning.
pub fn spinning_blade(shape: &BladeShape) -> Image {
    // The blade around its arbor, ring by ring, enough steps around to see each tooth.
    const AROUND: usize = 1024;
    const RINGS: usize = 160;
    // A third is the average all the way around, and the rest fades out behind each point in
    // about an eighth of a turn.
    const EVEN: f32 = 0.35;
    let fade = (-8.0 / AROUND as f32).exp();
    let step = TAU / AROUND as f32;
    let ring_width = shape.radius / RINGS as f32;
    let mut polar = vec![[0.0f32; 4]; AROUND * RINGS];
    for ring in 0..RINGS {
        let radius = (ring as f32 + 0.5) * ring_width;
        // The blade where it is, with how much of it is there, and its color weighed by that.
        let sharp: Vec<[f32; 4]> = (0..AROUND)
            .map(|i| {
                let samples = 4;
                let mut sum = [0.0; 4];
                for sample in 0..samples {
                    let angle = (i as f32 + (sample as f32 + 0.5) / samples as f32) * step;
                    if radius <= shape.rim(angle) {
                        let paint = blade_paint(shape, radius, angle, ring_width);
                        for channel in 0..3 {
                            sum[channel] += paint[channel] / samples as f32;
                        }
                        sum[3] += 1.0 / samples as f32;
                    }
                }
                sum
            })
            .collect();
        let mut even = [0.0; 4];
        for value in &sharp {
            for channel in 0..4 {
                even[channel] += value[channel] / AROUND as f32;
            }
        }
        // What passed each point came from further around: run back against the way it turns,
        // twice around so the trail coming into the start has built up.
        let mut trail = [0.0; 4];
        for k in (0..2 * AROUND).rev() {
            let i = k % AROUND;
            for channel in 0..4 {
                trail[channel] = trail[channel] * fade + sharp[i][channel] * (1.0 - fade);
            }
            if k < AROUND {
                for channel in 0..4 {
                    polar[ring * AROUND + i][channel] =
                        even[channel] * EVEN + trail[channel] * (1.0 - EVEN);
                }
            }
        }
        // A little darker than the blade standing still, as its slots and teeth smear over it,
        // and the streaks on that, with their colors weighed by how much blade is there.
        for i in 0..AROUND {
            let streak = speed_streaks(shape, radius, (i as f32 + 0.5) * step, ring_width);
            let value = &mut polar[ring * AROUND + i];
            for channel in 0..3 {
                value[channel] = lerp(value[channel] * 0.85, STREAK * value[3], streak);
            }
        }
    }

    const SIZE: u32 = 256;
    let mut color = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    // Where nothing is, the teeth's color, so filtering at the edge doesn't darken them.
    let edge = polar[(RINGS - 1) * AROUND];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (u, v) = (
                (x as f32 + 0.5) / SIZE as f32,
                (y as f32 + 0.5) / SIZE as f32,
            );
            let (up, out) = (
                (0.5 - v) * 2.0 * shape.radius,
                (u - 0.5) * 2.0 * shape.radius,
            );
            // Between the four nearest points of the polar grid.
            let ring = (up.hypot(out) / ring_width - 0.5).max(0.0);
            let around = out.atan2(up).rem_euclid(TAU) / step - 0.5;
            let (r0, a0) = (ring.floor() as usize, around.floor());
            let (fr, fa) = (ring - r0 as f32, around - a0);
            let mut sum = [0.0; 4];
            for (dr, wr) in [(0, 1.0 - fr), (1, fr)] {
                for (da, wa) in [(0, 1.0 - fa), (1, fa)] {
                    let value = if r0 + dr < RINGS {
                        let i = (a0 as i64 + da).rem_euclid(AROUND as i64) as usize;
                        polar[(r0 + dr) * AROUND + i]
                    } else {
                        [0.0; 4]
                    };
                    for channel in 0..4 {
                        sum[channel] += value[channel] * wr * wa;
                    }
                }
            }
            let alpha = sum[3];
            for channel in 0..3 {
                let straight = if alpha > 1e-3 {
                    sum[channel] / alpha
                } else {
                    edge[channel] / edge[3].max(1e-3)
                };
                color.push(to_byte(straight));
            }
            color.push(to_byte(alpha));
        }
    }
    blade_texture(SIZE, color)
}

/// The color of the streaks drawn around a spinning blade, in sRGB.
const STREAK: f32 = 0.99;

/// How much of a pixel `radius` out from a spinning blade's arbor at `angle`, soft over
/// `pixel`, the speed streaks drawn around it cover: arcs at a few distances out, each thin at
/// its tail and thick at its head, which leads the way the blade turns.
fn speed_streaks(shape: &BladeShape, radius: f32, angle: f32, pixel: f32) -> f32 {
    let r = shape.radius;
    // Where each starts, how far out it is, how far around it goes, and how wide its head is.
    const STREAKS: [(f32, f32, f32, f32); 4] = [
        (0.3, 0.87, 1.8, 0.05),
        (3.4, 0.85, 1.5, 0.045),
        (1.9, 0.66, 1.6, 0.045),
        (5.0, 0.53, 1.3, 0.04),
    ];
    STREAKS
        .iter()
        .map(|&(start, out, length, head)| {
            let along = (angle - start).rem_euclid(TAU) / length;
            if along >= 1.0 {
                return 0.0;
            }
            // Widening from its tail to its head, then rounded off.
            let half = head * r / 2.0 * along.sqrt() * (1.0 - along.powi(8)).sqrt();
            smoothstep(-0.5, 0.5, (half - (radius - out * r).abs()) / pixel)
        })
        .fold(0.0, f32::max)
}

/// What a saw blade's face looks like `radius` out from its arbor at `angle` (from up toward
/// +z, the way it turns), with edges soft over `pixel`, in sRGB.
fn blade_paint(shape: &BladeShape, radius: f32, angle: f32, pixel: f32) -> [f32; 3] {
    const STEEL: [f32; 3] = [0.8, 0.82, 0.86];
    const SLOT: [f32; 3] = [0.16, 0.17, 0.21];
    const CARBIDE: [f32; 3] = [0.3, 0.32, 0.38];
    const LABEL: [f32; 3] = [0.92, 0.3, 0.22];
    const PRINT: [f32; 3] = [0.98, 0.97, 0.94];
    const FLANGE: [f32; 3] = [0.28, 0.3, 0.35];
    const NUT: [f32; 3] = [0.56, 0.58, 0.63];
    let inch = 0.0254;
    let r = shape.radius;
    // How much of a pixel is inside, from how far inside it is (negative outside).
    let inside = |distance: f32| smoothstep(-0.5, 0.5, distance / pixel);
    let mix = |color: [f32; 3], over: [f32; 3], amount: f32| {
        [0, 1, 2].map(|channel| lerp(color[channel], over[channel], amount))
    };
    // How far around from `start` the point is, going the way the blade turns.
    let past = |start: f32| (angle - start).rem_euclid(TAU);
    // Faint rings, as the plate was ground.
    let ground = 1.0 + 0.012 * (radius / r * 50.0).sin();
    let mut color = STEEL.map(|channel| channel * ground);

    // Four expansion slots, in from the gullets, each ending in a round hole.
    let slot_end = 0.72 * r;
    for k in 0..4 {
        let slot = 0.4 + k as f32 * TAU / 4.0;
        let (across, along) = ((angle - slot).sin() * radius, (angle - slot).cos() * radius);
        let in_slot = inside(0.03 * inch - across.abs()) * inside(along - slot_end);
        let hole = inside(0.09 * inch - Vec2::new(across, along - slot_end).length());
        color = mix(color, SLOT, in_slot.max(hole));
    }

    // A mark printed near the rim, an arc a sixth of the way around with a point at its head.
    color = mix(
        color,
        PRINT,
        arrow(radius - 0.82 * r, past(2.2) * radius, 0.025 * r, 0.9 * r),
    );

    // Carbide brazed onto the front of each tooth, down from its tip.
    let phase = shape.phase(angle);
    let tip = inside((phase - 0.82) * shape.pitch() * radius) * inside(radius - (r - 0.16 * inch));
    color = mix(color, CARBIDE, tip);

    // The label, with an arrow around it the way the blade turns.
    color = mix(color, LABEL, inside(0.4 * r - radius));
    color = mix(
        color,
        PRINT,
        arrow(radius - 0.27 * r, past(0.6) * radius, 0.06 * r, 0.6 * r),
    );

    // The flange, and the nut that holds the blade on, hexagonal.
    color = mix(color, FLANGE, inside(0.15 * r - radius));
    let (sin, cos) = angle.sin_cos();
    let hex = (0..3)
        .map(|k| {
            let side = k as f32 * TAU / 6.0;
            (radius * (cos * side.cos() + sin * side.sin())).abs()
        })
        .fold(0.0, f32::max);
    color = mix(color, NUT, inside(0.075 * r - hex));
    color
}

/// How much of a pixel an arrow along an arc covers: `across` is how far out the pixel is from
/// the arrow's middle line, `along` how far around it from the arrow's tail, both in meters.
/// It's `width` wide and `length` long, its head as long as it is wide.
fn arrow(across: f32, along: f32, width: f32, length: f32) -> f32 {
    let head = 2.0 * width;
    let shaft = length - head;
    let half = if along < shaft {
        width / 2.0
    } else {
        // The head narrows from twice the shaft's width to a point.
        width * (1.0 - (along - shaft) / head)
    };
    let soft = width / 6.0;
    let inside_across = smoothstep(-soft, soft, half - across.abs());
    let inside_along = smoothstep(-soft, soft, along.min(length - along));
    inside_across * inside_along
}

/// A blade's texture, mipmapped and clamped at its edges.
fn blade_texture(size: u32, pixels: Vec<u8>) -> Image {
    let mut image = texture(size, size, pixels, TextureFormat::Rgba8UnormSrgb);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    image
}

/// A repeating, mipmapped texture, so it stays crisp up close and doesn't shimmer far away.
fn texture(width: u32, height: u32, pixels: Vec<u8>, format: TextureFormat) -> Image {
    let (data, levels) = with_mipmaps(width, height, pixels);
    let mut image = Image::new_fill(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    image
}

/// Appends each smaller mip level to an RGBA8 image, halving until 1x1, by averaging 2x2
/// blocks. Returns the data and the number of levels.
pub fn with_mipmaps(width: u32, height: u32, pixels: Vec<u8>) -> (Vec<u8>, u32) {
    let mut data = pixels;
    let (mut w, mut h) = (width as usize, height as usize);
    let mut start = 0;
    let mut levels = 1;
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = Vec::with_capacity(nw * nh * 4);
        for y in 0..nh {
            for x in 0..nw {
                for channel in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (2 * x + dx).min(w - 1);
                        let sy = (2 * y + dy).min(h - 1);
                        sum += data[start + (sy * w + sx) * 4 + channel] as u32;
                    }
                    next.push((sum / 4) as u8);
                }
            }
        }
        start = data.len();
        data.extend_from_slice(&next);
        (w, h) = (nw, nh);
        levels += 1;
    }
    (data, levels)
}

/// A repeatable pseudo-random value in [0, 1) for a lattice point.
fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

/// Smooth value noise that repeats every `period_x` by `period_y` lattice cells.
fn tiled_noise(x: f32, y: f32, period_x: i32, period_y: i32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ix, iy) = (x0 as i32, y0 as i32);
    let corner = |dx: i32, dy: i32| {
        hash(
            (ix + dx).rem_euclid(period_x),
            (iy + dy).rem_euclid(period_y),
            seed,
        )
    };
    lerp(
        lerp(corner(0, 0), corner(1, 0), sx),
        lerp(corner(0, 1), corner(1, 1), sx),
        sy,
    )
}

/// Layers of tiling noise over the unit square, each twice as fine as the last, starting from
/// `cells_x` by `cells_y` lattice cells. Returns roughly [0, 1].
fn fbm(u: f32, v: f32, cells_x: i32, cells_y: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amplitude, mut total) = (0.0, 0.5, 0.0);
    for octave in 0..octaves {
        let (px, py) = (cells_x << octave, cells_y << octave);
        sum += amplitude * tiled_noise(u * px as f32, v * py as f32, px, py, seed + octave);
        total += amplitude;
        amplitude *= 0.5;
    }
    sum / total
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn to_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_tiles_seamlessly() {
        for y in [0.0, 0.3, 0.77] {
            let left = fbm(0.0, y, 3, 2, 4, 9);
            let right = fbm(1.0, y, 3, 2, 4, 9);
            assert!((left - right).abs() < 1e-5, "{left} vs {right}");
        }
    }

    #[test]
    fn mipmaps_run_down_to_one_pixel() {
        let (data, levels) = with_mipmaps(8, 2, vec![200; 8 * 2 * 4]);
        // 8x2, 4x1, 2x1, 1x1.
        assert_eq!(levels, 4);
        assert_eq!(data.len(), (16 + 4 + 2 + 1) * 4);
        assert!(data.iter().all(|&byte| byte == 200));
    }
}

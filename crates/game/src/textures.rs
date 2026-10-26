//! Textures made in code: wood grain, concrete, and pegboard, plus the soft room light that
//! metal and paint reflect. There are no image files; everything here is generated at startup.
//!
//! Every texture tiles seamlessly, so surfaces of any size can repeat it.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{
        Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
    },
};

/// One repeat of a wood texture covers this much board, in meters: along the grain, and across.
pub const WOOD_TILE_ALONG: f32 = 0.8;
pub const WOOD_TILE_ACROSS: f32 = 0.2;
/// One repeat of the concrete covers this much floor, in meters, with a control joint at its edge.
pub const CONCRETE_TILE: f32 = 2.0;
/// One repeat of pegboard: four holes each way, an inch apart.
pub const PEGBOARD_TILE: f32 = 4.0 * 0.0254;

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
    earlywood: [0.93, 0.83, 0.66],
    latewood: [0.79, 0.60, 0.40],
    rings: 7,
    waviness: 2.6,
    seed: 7,
};

pub const MAPLE: Species = Species {
    earlywood: [0.91, 0.81, 0.66],
    latewood: [0.84, 0.70, 0.52],
    rings: 16,
    waviness: 1.8,
    seed: 31,
};

/// A wood color texture and its matching normal map. U runs along the grain.
pub fn wood(species: &Species) -> (Image, Image) {
    const WIDTH: u32 = 1024;
    const HEIGHT: u32 = 256;
    let seed = species.seed;
    let height_at = |u: f32, v: f32| -> (f32, f32) {
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
        // Fine fibers run along the grain: long in u, very thin in v.
        let fiber = tiled_noise(u * 16.0, v * 320.0, 16, 320, seed + 20);
        (latewood, fiber)
    };

    let mut color = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    let mut heights = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let (u, v) = (x as f32 / WIDTH as f32, y as f32 / HEIGHT as f32);
            let (latewood, fiber) = height_at(u, v);
            // Broad patches of lighter and darker wood, like real boards.
            let patch = (fbm(u, v, 2, 1, 3, seed + 30) - 0.5) * 0.14;
            let shade = 1.0 + patch + (fiber - 0.5) * 0.08;
            for channel in 0..3 {
                let base = lerp(
                    species.earlywood[channel],
                    species.latewood[channel],
                    latewood,
                );
                color.push(to_byte(base * shade));
            }
            color.push(255);
            heights.push(latewood * 0.7 + fiber * 0.3);
        }
    }
    let normals = normal_map(&heights, WIDTH, HEIGHT, 2.5);
    (
        texture(WIDTH, HEIGHT, color, TextureFormat::Rgba8UnormSrgb),
        texture(WIDTH, HEIGHT, normals, TextureFormat::Rgba8Unorm),
    )
}

/// A troweled concrete slab: mottled gray with fine aggregate and a saw-cut control joint
/// along two edges, so repeating it makes a grid of slabs.
pub fn concrete() -> (Image, Image) {
    const SIZE: u32 = 1024;
    let mut color = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let mut heights = Vec::with_capacity((SIZE * SIZE) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (u, v) = (x as f32 / SIZE as f32, y as f32 / SIZE as f32);
            let mottle = fbm(u, v, 4, 4, 5, 101) - 0.5;
            let stain = smoothstep(0.62, 0.8, fbm(u, v, 3, 3, 4, 202)) * 0.08;
            let aggregate = tiled_noise(u * 400.0, v * 400.0, 400, 400, 303) - 0.5;
            // A narrow joint where the slab edges meet.
            let edge = (x.min(SIZE - x)).min(y.min(SIZE - y)) as f32;
            let joint = 1.0 - smoothstep(1.0, 3.0, edge);
            let shade = 1.0 + mottle * 0.16 + aggregate * 0.06 - stain - joint * 0.45;
            for channel in [0.60, 0.59, 0.56] {
                color.push(to_byte(channel * shade));
            }
            color.push(255);
            heights.push(mottle * 0.3 + aggregate * 0.2 - joint);
        }
    }
    let normals = normal_map(&heights, SIZE, SIZE, 1.5);
    (
        texture(SIZE, SIZE, color, TextureFormat::Rgba8UnormSrgb),
        texture(SIZE, SIZE, normals, TextureFormat::Rgba8Unorm),
    )
}

/// Brown hardboard with a hole every inch.
pub fn pegboard() -> Image {
    const SIZE: u32 = 256;
    let mut color = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (u, v) = (x as f32 / SIZE as f32, y as f32 / SIZE as f32);
            // Distance to the nearest hole center, in quarter-tiles (one inch each).
            let (cu, cv) = ((u * 4.0).fract() - 0.5, (v * 4.0).fract() - 0.5);
            let hole = 1.0 - smoothstep(0.1, 0.13, (cu * cu + cv * cv).sqrt());
            let grain = (fbm(u, v, 8, 8, 3, 404) - 0.5) * 0.1;
            for channel in [0.56, 0.41, 0.27] {
                color.push(to_byte(lerp(channel * (1.0 + grain), 0.06, hole)));
            }
            color.push(255);
        }
    }
    texture(SIZE, SIZE, color, TextureFormat::Rgba8UnormSrgb)
}

/// A blurry stand-in for the room around everything: a bright ceiling of shop lights, warm
/// walls, a darker floor, and a window of daylight. Reflections and soft fill light come from
/// this once Bevy filters it into an environment map.
pub fn room_cubemap() -> Image {
    const SIZE: u32 = 64;
    let window = Vec3::new(0.2, 0.35, -1.0).normalize();
    let mut data = Vec::with_capacity((SIZE * SIZE * 6 * 8) as usize);
    for face in 0..6 {
        for y in 0..SIZE {
            for x in 0..SIZE {
                let s = 2.0 * (x as f32 + 0.5) / SIZE as f32 - 1.0;
                let t = 2.0 * (y as f32 + 0.5) / SIZE as f32 - 1.0;
                let direction = match face {
                    0 => Vec3::new(1.0, -t, -s),
                    1 => Vec3::new(-1.0, -t, s),
                    2 => Vec3::new(s, 1.0, t),
                    3 => Vec3::new(s, -1.0, -t),
                    4 => Vec3::new(s, -t, 1.0),
                    _ => Vec3::new(-s, -t, -1.0),
                }
                .normalize();
                let up = direction.y;
                let floor = Vec3::new(0.30, 0.30, 0.29);
                let walls = Vec3::new(0.70, 0.69, 0.66);
                let ceiling = Vec3::new(1.0, 0.99, 0.96);
                let mut radiance = if up < 0.0 {
                    floor.lerp(walls, smoothstep(-0.6, 0.0, up))
                } else {
                    walls.lerp(ceiling, smoothstep(0.1, 0.9, up))
                };
                let toward_window = direction.dot(window).max(0.0);
                radiance += Vec3::new(2.4, 2.6, 2.9) * smoothstep(0.88, 0.97, toward_window);
                for value in [radiance.x, radiance.y, radiance.z, 1.0] {
                    data.extend_from_slice(&half_float(value).to_le_bytes());
                }
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
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
fn with_mipmaps(width: u32, height: u32, pixels: Vec<u8>) -> (Vec<u8>, u32) {
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

/// Turns a tiling height field into a tangent-space normal map. Bumps lean the surface away
/// from higher neighbors; `strength` sets how steep they look.
fn normal_map(heights: &[f32], width: u32, height: u32, strength: f32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let at = |x: usize, y: usize| heights[(y % h) * w + (x % w)];
    let mut pixels = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let du = at(x + 1, y) - at(x + w - 1, y);
            let dv = at(x, y + 1) - at(x, y + h - 1);
            let normal = Vec3::new(-du * strength, -dv * strength, 1.0).normalize();
            for value in [normal.x, normal.y, normal.z] {
                pixels.push(((value * 0.5 + 0.5) * 255.0).round() as u8);
            }
            pixels.push(255);
        }
    }
    pixels
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

/// The bits of `value` as a 16-bit float. Only handles the positive, finite values a lighting
/// texture needs; tiny values flush to zero.
fn half_float(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = (bits >> 13) & 0x3ff;
    match exponent {
        ..=0 => sign,
        31.. => sign | 0x7c00,
        _ => sign | ((exponent as u16) << 10) | mantissa as u16,
    }
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

    #[test]
    fn half_floats_match_known_values() {
        assert_eq!(half_float(1.0), 0x3c00);
        assert_eq!(half_float(2.5), 0x4100);
        assert_eq!(half_float(0.0), 0);
    }
}

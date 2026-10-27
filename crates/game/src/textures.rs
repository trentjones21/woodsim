//! Textures made in code: wood grain, the shop floor, pegboard, and grass. There are no image
//! files; everything here is generated at startup.
//!
//! Every texture tiles seamlessly, so surfaces of any size can repeat it. They're painted
//! rather than photographic: soft color and pattern, no bumps.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

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

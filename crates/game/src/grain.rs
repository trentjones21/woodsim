//! Wood for the studio's renders, worked out in three dimensions rather than painted on: a
//! board is a piece of a log, and its grain is the log's growth rings, so wherever a ray meets
//! the wood its color comes from how far that point is from the pith. A face cuts the rings at
//! a shallow angle and shows them as long stripes and arches, and an end cuts straight across
//! them and shows the rings themselves.

use bevy::math::{Vec2, Vec3};

/// A kind of wood, in linear color, as it looks under a clear finish.
pub struct Species {
    /// The light, fast-growing part of each ring.
    pub earlywood: Vec3,
    /// The dense, darker band at the end of each ring.
    pub latewood: Vec3,
    /// How far apart the rings are, in meters.
    pub ring: f32,
    /// How far the rings wander from round, as a share of a ring.
    pub waviness: f32,
}

/// Construction pine: pale straw, with wide rings and a distinct band of latewood in each.
pub const PINE: Species = Species {
    earlywood: Vec3::new(0.74, 0.5, 0.26),
    latewood: Vec3::new(0.42, 0.2, 0.07),
    ring: 0.0036,
    waviness: 1.6,
};

/// Where a stock board was cut from its log: where the pith runs across the board's width and
/// thickness at its `l = 0` end, how far it drifts per meter along the board (no log is sawn
/// quite parallel to its pith, and the drift is what draws arches on a face), how this tree's
/// wood is colored compared to the species' usual, and a seed for the wobbles that make each
/// board its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Log {
    pub pith: Vec2,
    pub drift: Vec2,
    pub tone: Vec3,
    pub seed: u32,
}

impl Log {
    /// The log a stock board `width` wide and `thickness` thick (in meters) came from, chosen
    /// by `seed`: flat-sawn, its pith a little below the bottom face somewhere across it, so the
    /// face shows arches and the ends show the curve of the rings.
    pub fn for_stock(width: f32, thickness: f32, seed: u32) -> Log {
        let random = |k: u32| unit(mix(seed ^ k.wrapping_mul(0x9e37_79b9)));
        let below = 0.02 + 0.07 * random(1);
        let across = width * (0.15 + 0.7 * random(2));
        let pith = if random(3) < 0.75 {
            Vec2::new(across, -below)
        } else {
            // Now and then the board came from the other side of the pith.
            Vec2::new(across, thickness + below)
        };
        let drift = Vec2::new(random(4) - 0.5, random(5) - 0.5) * Vec2::new(0.14, 0.1);
        // Some trees are paler or redder than others.
        let light = 0.92 + 0.14 * random(6);
        let warm = 0.06 * (random(7) - 0.5);
        Log {
            pith,
            drift,
            tone: Vec3::new(light * (1.0 + warm), light, light * (1.0 - 2.0 * warm)),
            seed: mix(seed),
        }
    }
}

/// The color of the wood at `at`: along the stock board's length, across its width, and up
/// through its thickness (`l`, `w`, `t`, in meters from its corner).
pub fn wood(species: &Species, log: &Log, at: Vec3) -> Vec3 {
    let (l, w, t) = (at.x, at.y, at.z);
    let seed = log.seed;
    let q = Vec2::new(w, t) - (log.pith + log.drift * l);
    let radius = q.length();
    // The rings wobble along the log, slowly and then more quickly, and a little around it;
    // and they're unevenly spaced, as good years and bad ones come and go.
    let wobble = (fbm(Vec3::new(q.x * 12.0, q.y * 12.0, l * 0.8), 3, seed)
        + 0.5 * noise(Vec3::new(q.x * 30.0, q.y * 30.0, l * 4.0), seed + 7))
        * species.ring
        * species.waviness;
    let ripple = noise(Vec3::new(q.x * 90.0, q.y * 90.0, l * 9.0), seed + 1) * species.ring * 0.15;
    let years = noise(Vec3::new(radius * 70.0, 0.5, 0.5), seed + 8) * species.ring * 2.5;
    let ring = (radius + wobble + ripple + years) / species.ring;
    // Each ring has its own share of latewood: the wood darkens over the last part of the
    // ring, then the next starts abruptly.
    let index = ring.floor() as i32;
    let into = ring - ring.floor();
    let start = 0.52 + 0.34 * unit(mix(index as u32 ^ seed.wrapping_add(2)));
    let strength = 1.0 - 0.75 * unit(mix(index as u32 ^ seed.wrapping_add(3))).powi(2);
    let latewood =
        smoothstep(start, start + 0.13, into) * (1.0 - smoothstep(0.985, 1.0, into)) * strength;
    // The earlywood darkens a little toward the latewood too.
    let early = species.earlywood * (1.0 - 0.1 * smoothstep(0.1, start, into));
    let color = early.lerp(species.latewood, latewood);
    // Fine fibers along the grain (and specks across the ends), and broad patches of lighter
    // and darker wood.
    let fiber = noise(Vec3::new(w * 2200.0, t * 2200.0, l * 24.0), seed + 4);
    let pore = noise(Vec3::new(w * 5200.0, t * 5200.0, l * 300.0), seed + 6);
    let patch = fbm(Vec3::new(l * 1.4, w * 5.0, t * 5.0), 2, seed + 5);
    color * log.tone * (1.0 + 0.07 * fiber + 0.04 * pore + 0.12 * patch)
}

/// Gradient noise in three dimensions, about -1 to 1, smooth and different for each `seed`.
pub fn noise(p: Vec3, seed: u32) -> f32 {
    let cell = p.floor();
    let f = p - cell;
    let (x, y, z) = (cell.x as i32, cell.y as i32, cell.z as i32);
    let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (u, v, w) = (fade(f.x), fade(f.y), fade(f.z));
    let corner = |dx: i32, dy: i32, dz: i32| {
        let h = mix(seed
            ^ (x + dx).wrapping_mul(0x27d4_eb2d) as u32
            ^ (y + dy).wrapping_mul(0x1656_67b1) as u32
            ^ (z + dz).wrapping_mul(0x5bd1_e995) as u32);
        gradient(h, f - Vec3::new(dx as f32, dy as f32, dz as f32))
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x0 = lerp(
        lerp(corner(0, 0, 0), corner(1, 0, 0), u),
        lerp(corner(0, 1, 0), corner(1, 1, 0), u),
        v,
    );
    let x1 = lerp(
        lerp(corner(0, 0, 1), corner(1, 0, 1), u),
        lerp(corner(0, 1, 1), corner(1, 1, 1), u),
        v,
    );
    lerp(x0, x1, w)
}

/// Noise in `octaves` layers, each twice as fine and half as strong as the last.
pub fn fbm(p: Vec3, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amplitude, mut scale) = (0.0, 0.5, 1.0);
    for octave in 0..octaves {
        sum += noise(p * scale, seed.wrapping_add(octave * 7919)) * amplitude;
        amplitude *= 0.5;
        scale *= 2.0;
    }
    sum
}

/// One of the twelve directions to a cube's edges, dotted with `d`.
fn gradient(hash: u32, d: Vec3) -> f32 {
    match hash % 12 {
        0 => d.x + d.y,
        1 => -d.x + d.y,
        2 => d.x - d.y,
        3 => -d.x - d.y,
        4 => d.x + d.z,
        5 => -d.x + d.z,
        6 => d.x - d.z,
        7 => -d.x - d.z,
        8 => d.y + d.z,
        9 => -d.y + d.z,
        10 => d.y - d.z,
        _ => -d.y - d.z,
    }
}

/// Scrambles the bits of `x` thoroughly, so nearby inputs give unrelated outputs.
pub fn mix(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// A hash as a number from 0 up to 1.
fn unit(hash: u32) -> f32 {
    (hash >> 8) as f32 / (1 << 24) as f32
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many times the wood goes from earlywood to latewood along a line, `steps` samples
    /// from `from` to `to`.
    fn rings_crossed(log: &Log, from: Vec3, to: Vec3, steps: usize) -> usize {
        let mut crossed = 0;
        let mut dark = false;
        let threshold = (PINE.earlywood + PINE.latewood).length() / 2.0;
        for k in 0..=steps {
            let at = from.lerp(to, k as f32 / steps as f32);
            let now = wood(&PINE, log, at).length() < threshold;
            if now && !dark {
                crossed += 1;
            }
            dark = now;
        }
        crossed
    }

    #[test]
    fn the_ends_cut_across_the_rings_and_the_faces_run_along_them() {
        // A 1x4: 3-1/2" wide and 3/4" thick.
        let (width, thickness) = (0.089, 0.019);
        let log = Log::for_stock(width, thickness, 7);
        // Across the end, from one edge to the other at mid-thickness, the line crosses
        // several rings.
        let across_end = rings_crossed(
            &log,
            Vec3::new(0.0, 0.0, thickness / 2.0),
            Vec3::new(0.0, width, thickness / 2.0),
            2000,
        );
        // Up through the thickness at the end, more.
        let up_end = rings_crossed(
            &log,
            Vec3::new(0.0, width / 2.0, 0.0),
            Vec3::new(0.0, width / 2.0, thickness),
            2000,
        );
        // Along the face, a few inches of length, the rings barely change: the stripes run
        // along the board.
        let along_face = rings_crossed(
            &log,
            Vec3::new(0.0, width / 2.0, thickness),
            Vec3::new(0.1, width / 2.0, thickness),
            2000,
        );
        assert!(across_end + up_end >= 4, "{across_end} + {up_end}");
        assert!(along_face <= 3, "{along_face}");
    }

    #[test]
    fn every_stock_board_comes_from_its_own_place_in_a_log() {
        let a = Log::for_stock(0.089, 0.019, 1);
        let b = Log::for_stock(0.089, 0.019, 2);
        assert_ne!(a, b);
        assert_eq!(a, Log::for_stock(0.089, 0.019, 1));
        // The pith is outside the board, below or above it, and somewhere across it.
        for seed in 0..50 {
            let log = Log::for_stock(0.089, 0.019, seed);
            assert!(log.pith.y < 0.0 || log.pith.y > 0.019, "{log:?}");
            assert!((0.0..=0.089).contains(&log.pith.x), "{log:?}");
        }
    }

    #[test]
    fn noise_is_smooth_and_about_one_either_way() {
        let mut most: f32 = 0.0;
        for k in 0..10_000 {
            let p = Vec3::new(k as f32 * 0.137, k as f32 * 0.071, k as f32 * 0.029);
            let n = noise(p, 3);
            most = most.max(n.abs());
            // A small step changes it a little.
            let step = noise(p + Vec3::splat(0.001), 3);
            assert!((n - step).abs() < 0.01, "{n} {step}");
        }
        assert!((0.5..=1.2).contains(&most), "{most}");
    }
}

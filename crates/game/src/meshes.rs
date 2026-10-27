//! Boxes with softly rounded edges, so the shop looks like a set of chunky toys. Boards are
//! the same boxes with wood grain that runs along them and stays put on the wood as it moves or
//! is cut.

use std::f32::consts::FRAC_PI_4;

use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};

use crate::textures::{WOOD_TILE_ACROSS, WOOD_TILE_ALONG};

/// Which way a board's length (and so its grain) runs in its station's coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grain {
    X,
    Z,
}

/// How round box edges are, in meters.
const ROUNDING: f32 = 0.02;
/// A thin box rounds its edges by at most this fraction of its thinnest side, so it keeps its
/// flat faces.
const MAX_ROUNDING: f32 = 0.3;
/// Steps around each half of a rounded edge; the normals are smooth, so a few are plenty.
const STEPS: usize = 2;

/// End grain soaks up more light than face grain, so the ends of a board read darker.
const END_GRAIN_SHADE: f32 = 0.7;

/// A rounded box from the origin to `size`. Its UVs are in meters across each face, so a
/// material's `uv_transform` can scale a texture to repeat at a fixed size.
pub fn rounded_box(size: Vec3) -> Mesh {
    let mut mesh = rounded(size, |normal| (Face::along(normal), 1.0), Vec3::ZERO);
    // Every face is the same shade, so the color would only cost memory.
    mesh.remove_attribute(Mesh::ATTRIBUTE_COLOR);
    mesh
}

/// A rounded box from the origin to `size`, textured as wood. `offset` is where the box sits
/// within the whole board it was cut from, so every piece of one board shows that board's
/// grain.
pub fn board_mesh(size: Vec3, offset: Vec3, grain: Grain) -> Mesh {
    let (along, across) = match grain {
        Grain::X => (Vec3::X, Vec3::Z),
        Grain::Z => (Vec3::Z, Vec3::X),
    };
    rounded(
        size,
        |normal| {
            // Grain runs along U wherever it can.
            let axis = normal.abs();
            if axis == Vec3::Y {
                (Face::scaled(along, across), 1.0)
            } else if axis == across {
                (Face::scaled(along, Vec3::Y), 1.0)
            } else {
                (Face::scaled(Vec3::Y, across), END_GRAIN_SHADE)
            }
        },
        offset,
    )
}

/// How a face maps onto a texture: the directions U and V run, and how many meters one repeat
/// of the texture covers in each.
struct Face {
    u: Vec3,
    v: Vec3,
    tile: Vec2,
}

impl Face {
    /// U and V along the face's own edges, in meters.
    fn along(normal: Vec3) -> Face {
        let (u, v) = face_edges(normal);
        Face {
            u,
            v,
            tile: Vec2::ONE,
        }
    }

    /// U and V along the given directions, one wood texture repeat per tile.
    fn scaled(u: Vec3, v: Vec3) -> Face {
        Face {
            u,
            v,
            tile: Vec2::new(WOOD_TILE_ALONG, WOOD_TILE_ACROSS),
        }
    }
}

/// Each face's outward normal.
const NORMALS: [Vec3; 6] = [
    Vec3::X,
    Vec3::NEG_X,
    Vec3::Y,
    Vec3::NEG_Y,
    Vec3::Z,
    Vec3::NEG_Z,
];

/// Two edges of the face with outward `normal`, `s` x `t` = normal, so a grid stepping along
/// them winds counterclockwise seen from outside.
fn face_edges(normal: Vec3) -> (Vec3, Vec3) {
    let axis = normal.abs();
    let (s, t) = if axis == Vec3::X {
        (Vec3::Y, Vec3::Z)
    } else if axis == Vec3::Y {
        (Vec3::Z, Vec3::X)
    } else {
        (Vec3::X, Vec3::Y)
    };
    if normal.max_element() > 0.0 {
        (s, t)
    } else {
        (t, s)
    }
}

/// Builds the box face by face. Each face is a grid over a flat cube's face, with its rows
/// bunched toward the edges; each grid point is then pulled onto the rounded surface, so the
/// edge strips curve and the corners become eighths of a sphere. `map` gives each face's
/// texture mapping and shade.
fn rounded(size: Vec3, map: impl Fn(Vec3) -> (Face, f32), offset: Vec3) -> Mesh {
    let half = size / 2.0;
    let radius = ROUNDING.min(size.min_element() * MAX_ROUNDING);
    let inner = half - radius;
    // Where grid lines fall along an axis of half-length `h`: the two ends of the flat middle,
    // plus rows across each rounded edge at even angles out to its halfway point.
    let ticks = |h: f32| -> Vec<f32> {
        let flat = h - radius;
        let mut ticks: Vec<f32> = (0..=STEPS)
            .rev()
            .map(|k| -flat - radius * (FRAC_PI_4 * k as f32 / STEPS as f32).tan())
            .collect();
        ticks.extend(
            (0..=STEPS).map(|k| flat + radius * (FRAC_PI_4 * k as f32 / STEPS as f32).tan()),
        );
        ticks
    };

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for normal in NORMALS {
        let (s, t) = face_edges(normal);
        let (face, shade) = map(normal);
        let (s_ticks, t_ticks) = (ticks(half.dot(s)), ticks(half.dot(t)));
        let first = positions.len() as u32;
        for &b in &t_ticks {
            for &a in &s_ticks {
                let on_cube = normal * half + s * a + t * b;
                let core = on_cube.clamp(-inner, inner);
                let outward = (on_cube - core).normalize();
                let corner = core + outward * radius + half;
                let on_board = corner + offset;
                positions.push(corner.to_array());
                normals.push(outward.to_array());
                uvs.push([
                    on_board.dot(face.u) / face.tile.x,
                    on_board.dot(face.v) / face.tile.y,
                ]);
                colors.push([shade, shade, shade, 1.0]);
            }
        }
        let row = s_ticks.len() as u32;
        for j in 0..t_ticks.len() as u32 - 1 {
            for i in 0..row - 1 {
                let a = first + j * row + i;
                let (b, c, d) = (a + 1, a + row + 1, a + row);
                indices.extend([a, b, c, a, c, d]);
            }
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

#[cfg(test)]
mod tests {
    use bevy::mesh::VertexAttributeValues;

    use super::*;

    fn positions(mesh: &Mesh) -> Vec<Vec3> {
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(positions)) => {
                positions.iter().map(|&p| Vec3::from(p)).collect()
            }
            _ => panic!("box meshes have positions"),
        }
    }

    fn normals(mesh: &Mesh) -> Vec<Vec3> {
        match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(VertexAttributeValues::Float32x3(normals)) => {
                normals.iter().map(|&n| Vec3::from(n)).collect()
            }
            _ => panic!("box meshes have normals"),
        }
    }

    fn uvs(mesh: &Mesh) -> Vec<Vec2> {
        match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
            Some(VertexAttributeValues::Float32x2(uvs)) => {
                uvs.iter().map(|&uv| Vec2::from(uv)).collect()
            }
            _ => panic!("box meshes have UVs"),
        }
    }

    #[test]
    fn boxes_fill_their_size() {
        for size in [Vec3::new(1.0, 0.5, 0.25), Vec3::new(0.3, 0.001, 0.02)] {
            let points = positions(&rounded_box(size));
            let min = points.iter().copied().reduce(Vec3::min).unwrap();
            let max = points.iter().copied().reduce(Vec3::max).unwrap();
            assert!(min.abs_diff_eq(Vec3::ZERO, 1e-6), "{min}");
            assert!(max.abs_diff_eq(size, 1e-6), "{max} vs {size}");
        }
    }

    #[test]
    fn grain_runs_along_the_board() {
        // A board lying along X, cut from 0.5 m along a longer board: on its top, U measures
        // meters along the whole board, one texture repeat at a time.
        let offset = Vec3::new(0.5, 0.0, 0.0);
        let mesh = board_mesh(Vec3::new(0.5, 0.02, 0.15), offset, Grain::X);
        let mut checked = 0;
        for ((position, normal), uv) in positions(&mesh).iter().zip(normals(&mesh)).zip(uvs(&mesh))
        {
            if normal.abs_diff_eq(Vec3::Y, 1e-6) {
                assert!((uv.x * WOOD_TILE_ALONG - (position.x + offset.x)).abs() < 1e-5);
                checked += 1;
            }
        }
        assert!(checked > 0);
    }

    #[test]
    fn faces_point_outward() {
        let size = Vec3::new(1.0, 0.3, 0.6);
        let mesh = board_mesh(size, Vec3::ZERO, Grain::Z);
        let points = positions(&mesh);
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("box meshes have indices");
        };
        for triangle in indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| points[triangle[i] as usize]);
            let facing = (b - a).cross(c - a);
            let center = (a + b + c) / 3.0 - size / 2.0;
            assert!(facing.dot(center) > 0.0, "a triangle faces inward");
        }
    }
}

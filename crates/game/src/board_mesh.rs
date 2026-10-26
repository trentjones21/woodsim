//! Meshes for boards, textured so the wood grain runs along the board and stays put on the wood
//! as it moves or is cut.

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

/// End grain soaks up more light than face grain, so the ends of a board read darker.
const END_GRAIN_SHADE: f32 = 0.62;

/// A box from the origin to `size`. `offset` is where the box sits within the whole board it
/// was cut from, so every piece of one board shows that board's grain.
pub fn board_mesh(size: Vec3, offset: Vec3, grain: Grain) -> Mesh {
    let (along, across) = match grain {
        Grain::X => (Vec3::X, Vec3::Z),
        Grain::Z => (Vec3::Z, Vec3::X),
    };
    // Each face: its outward normal and two edges, `s` x `t` = normal, so corners run
    // counterclockwise seen from outside.
    let faces = [
        (Vec3::X, Vec3::Y, Vec3::Z),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::Z, Vec3::X),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::Y, Vec3::X),
    ];
    let mut positions = Vec::with_capacity(24);
    let mut normals = Vec::with_capacity(24);
    let mut uvs = Vec::with_capacity(24);
    let mut tangents = Vec::with_capacity(24);
    let mut colors = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, s, t) in faces {
        // Faces on the positive side of an axis sit at the far end of the box.
        let base = normal.max(Vec3::ZERO) * size;
        // How this face maps the board onto the texture: grain runs along U wherever it can.
        let axis = normal.abs();
        let (u_axis, v_axis, shade) = if axis == Vec3::Y {
            (along, across, 1.0)
        } else if axis == across {
            (along, Vec3::Y, 1.0)
        } else {
            (Vec3::Y, across, END_GRAIN_SHADE)
        };
        let first = positions.len() as u32;
        for (a, b) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
            let corner = base + s * size * a + t * size * b;
            let on_board = corner + offset;
            positions.push(corner.to_array());
            normals.push(normal.to_array());
            uvs.push([
                on_board.dot(u_axis) / WOOD_TILE_ALONG,
                on_board.dot(v_axis) / WOOD_TILE_ACROSS,
            ]);
            // The tangent points toward increasing U; `w` says which way V runs.
            let handedness = if normal.cross(u_axis).dot(v_axis) > 0.0 {
                1.0
            } else {
                -1.0
            };
            tangents.push([u_axis.x, u_axis.y, u_axis.z, handedness]);
            colors.push([shade, shade, shade, 1.0]);
        }
        indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tangents)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

#[cfg(test)]
mod tests {
    use bevy::mesh::VertexAttributeValues;

    use super::*;

    fn uvs(mesh: &Mesh) -> Vec<[f32; 2]> {
        match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
            Some(VertexAttributeValues::Float32x2(uvs)) => uvs.clone(),
            _ => panic!("board meshes have UVs"),
        }
    }

    #[test]
    fn grain_runs_along_the_board() {
        // A 1 m board lying along X: its top face spans 1 m of grain in U.
        let mesh = board_mesh(Vec3::new(1.0, 0.02, 0.15), Vec3::ZERO, Grain::X);
        let top = &uvs(&mesh)[8..12];
        let u_span = top.iter().map(|uv| uv[0]).fold(f32::MIN, f32::max)
            - top.iter().map(|uv| uv[0]).fold(f32::MAX, f32::min);
        assert!((u_span - 1.0 / WOOD_TILE_ALONG).abs() < 1e-5);
    }

    #[test]
    fn pieces_of_one_board_line_up() {
        // The second half of a board starts where the first half's grain left off.
        let first = board_mesh(Vec3::new(0.5, 0.02, 0.15), Vec3::ZERO, Grain::X);
        let second = board_mesh(
            Vec3::new(0.5, 0.02, 0.15),
            Vec3::new(0.5, 0.0, 0.0),
            Grain::X,
        );
        let first_max = uvs(&first).iter().map(|uv| uv[0]).fold(f32::MIN, f32::max);
        let second_min = uvs(&second)[8..12]
            .iter()
            .map(|uv| uv[0])
            .fold(f32::MAX, f32::min);
        assert!((first_max - second_min).abs() < 1e-5);
    }

    #[test]
    fn faces_point_outward() {
        let mesh = board_mesh(Vec3::ONE, Vec3::ZERO, Grain::Z);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("board meshes have positions");
        };
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("board meshes have indices");
        };
        for triangle in indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(positions[triangle[i] as usize]));
            let facing = (b - a).cross(c - a);
            let center = (a + b + c) / 3.0 - Vec3::splat(0.5);
            assert!(facing.dot(center) > 0.0, "a triangle faces inward");
        }
    }
}

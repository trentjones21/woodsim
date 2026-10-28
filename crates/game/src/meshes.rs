//! Boxes with softly rounded edges, so the shop looks like a set of chunky toys, and the
//! boards the simulation cuts, with crisp sawn edges and every slot cut into them. Wood grain
//! runs along each board and stays put on the wood as it moves or is cut.

use std::f32::consts::FRAC_PI_4;

use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};

use woodsim::{Axis, Board, Length};

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

/// A board as the simulation has it, slots, cutaways and all: from its corner at the origin,
/// with `w` along x, `t` along y, and `l` (and the grain) along z. `grain` is where that corner
/// sits in the wood the grain pattern is laid out on, so pieces cut from one board line up.
///
/// The board's face is split into a grid wherever a slot or cutaway has an edge, and at points
/// around each slot's curved ends. At any point in a cell, the wood runs from a bottom to a
/// top: a slot cut up from below raises the bottom, one cut down from above lowers the top, and
/// where they meet there's no wood. Walls go wherever a cell has wood its neighbor doesn't.
pub fn lumber_mesh(board: &Board, grain: Vec3) -> Mesh {
    let size = Vec3::new(
        board.width().as_meters_f32(),
        board.thickness().as_meters_f32(),
        board.length().as_meters_f32(),
    );
    let slots: Vec<Slot> = board.kerfs().iter().map(Slot::from).collect();
    let cutaways: Vec<[f32; 4]> = board
        .cutaways()
        .iter()
        .map(|cutaway| {
            [cutaway.w.0, cutaway.w.1, cutaway.l.0, cutaway.l.1].map(Length::as_meters_f32)
        })
        .collect();

    // Grid lines across the width (x) and along the length (z).
    let (mut xs, mut zs) = (vec![0.0, size.x], vec![0.0, size.z]);
    for slot in &slots {
        let (faces, outline) = match slot.across {
            Axis::Width => (&mut xs, &mut zs),
            Axis::Length => (&mut zs, &mut xs),
        };
        faces.extend([slot.near, slot.far]);
        outline.extend(slot.outline(size.y));
    }
    for [w0, w1, l0, l1] in &cutaways {
        xs.extend([*w0, *w1]);
        zs.extend([*l0, *l1]);
    }
    xs.retain(|x| (0.0..=size.x).contains(x));
    zs.retain(|z| (0.0..=size.z).contains(z));
    let (xs, zs) = (sorted_distinct(xs), sorted_distinct(zs));
    let (columns, rows) = (xs.len() - 1, zs.len() - 1);

    // The span of wood at each corner of each cell, worked out with that cell's slots; `None`
    // for a cell cut away. Corners go (x0, z0), (x1, z0), (x1, z1), (x0, z1).
    let spans: Vec<Option<[(f32, f32); 4]>> = (0..rows)
        .flat_map(|j| (0..columns).map(move |i| (i, j)))
        .map(|(i, j)| {
            let center = ((xs[i] + xs[i + 1]) / 2.0, (zs[j] + zs[j + 1]) / 2.0);
            let cut_away = cutaways.iter().any(|&[w0, w1, l0, l1]| {
                w0 < center.0 && center.0 < w1 && l0 < center.1 && center.1 < l1
            });
            if cut_away {
                return None;
            }
            let cutting: Vec<&Slot> = slots
                .iter()
                .filter(|slot| {
                    let across = match slot.across {
                        Axis::Width => center.0,
                        Axis::Length => center.1,
                    };
                    slot.near < across && across < slot.far
                })
                .collect();
            let corners = [
                (xs[i], zs[j]),
                (xs[i + 1], zs[j]),
                (xs[i + 1], zs[j + 1]),
                (xs[i], zs[j + 1]),
            ];
            Some(corners.map(|(x, z)| wood_at(&cutting, x, z, size.y)))
        })
        .collect();

    let mut mesh = MeshBuilder::new(grain);
    let at = |i: usize, j: usize| j * columns + i;
    for j in 0..rows {
        for i in 0..columns {
            let Some(ours) = spans[at(i, j)] else {
                continue;
            };
            let (x0, x1, z0, z1) = (xs[i], xs[i + 1], zs[j], zs[j + 1]);
            let points = [(x0, z0), (x1, z0), (x1, z1), (x0, z1)];
            mesh.surfaces(points, ours);
            // Each edge: its two corners in this cell, the neighbor across it and the same two
            // points in that neighbor, and which way the edge faces.
            let neighbor = |di: isize, dj: isize| {
                let (ni, nj) = (i as isize + di, j as isize + dj);
                (ni >= 0 && nj >= 0 && (ni as usize) < columns && (nj as usize) < rows)
                    .then(|| spans[at(ni as usize, nj as usize)])
                    .flatten()
            };
            let edges = [
                ([0, 3], neighbor(-1, 0).map(|n| [n[1], n[2]]), Vec3::NEG_X),
                ([1, 2], neighbor(1, 0).map(|n| [n[0], n[3]]), Vec3::X),
                ([0, 1], neighbor(0, -1).map(|n| [n[3], n[2]]), Vec3::NEG_Z),
                ([3, 2], neighbor(0, 1).map(|n| [n[0], n[1]]), Vec3::Z),
            ];
            for ([a, b], theirs, outward) in edges {
                mesh.wall([points[a], points[b]], [ours[a], ours[b]], theirs, outward);
            }
        }
    }
    mesh.build()
}

/// A kerf in meters.
struct Slot {
    across: Axis,
    near: f32,
    far: f32,
    radius: f32,
    center: f32,
    start: f32,
    end: f32,
}

impl From<&woodsim::Kerf> for Slot {
    fn from(kerf: &woodsim::Kerf) -> Slot {
        Slot {
            across: kerf.across,
            near: kerf.near.as_meters_f32(),
            far: kerf.far.as_meters_f32(),
            radius: kerf.radius.as_meters_f32(),
            center: kerf.center.as_meters_f32(),
            start: kerf.start.as_meters_f32(),
            end: kerf.end.as_meters_f32(),
        }
    }
}

/// Points around each curved end of a slot, between where it meets the board's faces.
const ARC_STEPS: usize = 12;

impl Slot {
    /// The span of height the blade cut at `along` on the axis the slot runs along, if it
    /// reached there.
    fn cut_at(&self, along: f32) -> Option<(f32, f32)> {
        let past = (self.start - along).max(along - self.end).max(0.0);
        (past < self.radius).then(|| {
            let half = (self.radius * self.radius - past * past).sqrt();
            (self.center - half, self.center + half)
        })
    }

    /// Points along the slot where its outline needs a vertex: the ends of its straight run,
    /// and points around its curved ends from where they cross one face of a board `thickness`
    /// thick to where they cross the other.
    fn outline(&self, thickness: f32) -> impl Iterator<Item = f32> + '_ {
        let low = (self.center - self.radius).max(0.0);
        let high = (self.center + self.radius).min(thickness);
        let arc = (0..=ARC_STEPS).flat_map(move |step| {
            let t = low + (high - low) * step as f32 / ARC_STEPS as f32;
            let up = t - self.center;
            let along = (self.radius * self.radius - up * up).max(0.0).sqrt();
            [self.start - along, self.end + along]
        });
        [self.start, self.end].into_iter().chain(arc)
    }
}

/// The span of wood at the point `(x, z)` of the board's face, bottom to top, given the slots
/// cutting there. A slot reaches in from the bottom face or the top one; where the wood's gone,
/// bottom and top are equal.
fn wood_at(slots: &[&Slot], x: f32, z: f32, thickness: f32) -> (f32, f32) {
    let (mut bottom, mut top) = (0.0_f32, thickness);
    for slot in slots {
        let along = match slot.across {
            Axis::Width => z,
            Axis::Length => x,
        };
        let Some((low, high)) = slot.cut_at(along) else {
            continue;
        };
        if low <= bottom {
            bottom = bottom.max(high);
        } else if high >= top {
            top = top.min(low);
        }
    }
    let top = top.clamp(0.0, thickness);
    (bottom.clamp(0.0, top), top)
}

/// Sorts and drops values within a hair of the one before.
fn sorted_distinct(mut values: Vec<f32>) -> Vec<f32> {
    values.sort_by(f32::total_cmp);
    values.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    values
}

/// Collects a board's faces, each textured and shaded by which way it faces.
struct MeshBuilder {
    grain: Vec3,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    fn new(grain: Vec3) -> MeshBuilder {
        MeshBuilder {
            grain,
            positions: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            colors: Vec::new(),
            indices: Vec::new(),
        }
    }

    /// The top and bottom of the wood in one cell, whose corners are at `points` on the face
    /// with the spans of wood `spans`.
    fn surfaces(&mut self, points: [(f32, f32); 4], spans: [(f32, f32); 4]) {
        if spans.iter().all(|(bottom, top)| bottom >= top) {
            return;
        }
        for upward in [true, false] {
            let corners: [Vec3; 4] = std::array::from_fn(|k| {
                let height = if upward { spans[k].1 } else { spans[k].0 };
                Vec3::new(points[k].0, height, points[k].1)
            });
            // The diagonals' cross product points up for corners going round from (x0, z0).
            let facing = (corners[2] - corners[0]).cross(corners[1] - corners[3]);
            let facing = if facing.length_squared() > 1e-18 {
                facing
            } else {
                Vec3::Y
            };
            self.quad(corners, if upward { facing } else { -facing });
        }
    }

    /// The face of a cell's wood along one edge, from `points[0]` to `points[1]`, where the
    /// neighboring cell has no wood to cover it. `ours` and `theirs` are the spans of wood at
    /// those points on each side; a neighbor that's off the board or cut away has none.
    fn wall(
        &mut self,
        points: [(f32, f32); 2],
        ours: [(f32, f32); 2],
        theirs: Option<[(f32, f32); 2]>,
        outward: Vec3,
    ) {
        // Below the neighbor's wood, and above it.
        let mut below = [(0.0, 0.0); 2];
        let mut above = [(0.0, 0.0); 2];
        for k in 0..2 {
            let (bottom, top) = ours[k];
            if bottom >= top {
                below[k] = (bottom, bottom);
                above[k] = (top, top);
                continue;
            }
            match theirs.map(|theirs| theirs[k]).filter(|(b, t)| b < t) {
                Some((their_bottom, their_top)) => {
                    below[k] = (bottom, their_bottom.clamp(bottom, top));
                    above[k] = (their_top.clamp(bottom, top), top);
                }
                None => {
                    below[k] = (bottom, top);
                    above[k] = (top, top);
                }
            }
        }
        for part in [below, above] {
            let at = |k: usize, height: f32| Vec3::new(points[k].0, height, points[k].1);
            self.quad(
                [
                    at(0, part[0].0),
                    at(1, part[1].0),
                    at(1, part[1].1),
                    at(0, part[0].1),
                ],
                outward,
            );
        }
    }

    /// Adds a flat four-sided face facing `normal`, as two triangles wound to face that way.
    /// Triangles with no area, where two corners meet, are left out.
    fn quad(&mut self, corners: [Vec3; 4], normal: Vec3) {
        let normal = normal.normalize();
        let axis = normal.abs();
        let end_grain = axis.z > axis.x.max(axis.y);
        let edge = !end_grain && axis.x > axis.y;
        let shade = if end_grain { END_GRAIN_SHADE } else { 1.0 };
        // Grain runs along U wherever it can; on the ends, U runs up through the board.
        let uv = |p: Vec3| {
            let (u, v) = if end_grain {
                (p.y, p.x)
            } else if edge {
                (p.z, p.y)
            } else {
                (p.z, p.x)
            };
            [u / WOOD_TILE_ALONG, v / WOOD_TILE_ACROSS]
        };
        for [a, b, c] in [[0, 1, 2], [0, 2, 3]].map(|triangle| triangle.map(|i| corners[i])) {
            let facing = (b - a).cross(c - a);
            if facing.length_squared() < 1e-14 {
                continue;
            }
            let ordered = if facing.dot(normal) >= 0.0 {
                [a, b, c]
            } else {
                [a, c, b]
            };
            for corner in ordered {
                let in_wood = corner + self.grain;
                self.indices.push(self.positions.len() as u32);
                self.positions.push(corner.to_array());
                self.normals.push(normal.to_array());
                self.uvs.push(uv(in_wood));
                self.colors.push([shade, shade, shade, 1.0]);
            }
        }
    }

    fn build(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
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
    use woodsim::{Blade, Orientation, Pass, TableSaw};

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

    /// A 1x8, 24" long.
    fn plank() -> woodsim::Board {
        woodsim::Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(29, 4),
            Length::from_inch_fraction(3, 4),
        )
    }

    /// The plank after a cut stopped with the blade's center 10" along it, 6" from the fence.
    fn stopped_cut() -> woodsim::Board {
        let saw = TableSaw {
            blade: Blade {
                diameter: Length::from_inches(10),
                kerf: Blade::FULL_KERF,
            },
            fence: Length::from_inches(6),
            height: Length::from_inches(1),
        };
        match saw.feed(
            &plank(),
            Orientation::default(),
            -Length::from_inches(6),
            Length::from_inches(10),
        ) {
            Ok(Pass::Kerfed(board)) => board,
            other => panic!("expected a stopped cut, got {other:?}"),
        }
    }

    /// Each triangle's corners.
    fn triangles(mesh: &Mesh) -> Vec<[Vec3; 3]> {
        let points = positions(mesh);
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("meshes have indices");
        };
        indices
            .chunks(3)
            .map(|triangle| [0, 1, 2].map(|i| points[triangle[i] as usize]))
            .collect()
    }

    #[test]
    fn a_plain_board_is_a_box() {
        let mesh = lumber_mesh(&plank(), Vec3::ZERO);
        let points = positions(&mesh);
        let min = points.iter().copied().reduce(Vec3::min).unwrap();
        let max = points.iter().copied().reduce(Vec3::max).unwrap();
        assert!(min.abs_diff_eq(Vec3::ZERO, 1e-6), "{min}");
        assert!(
            max.abs_diff_eq(Vec3::new(0.18415, 0.01905, 0.6096), 1e-6),
            "{max}"
        );
        assert_eq!(triangles(&mesh).len(), 12);
    }

    #[test]
    fn a_slot_through_the_board_opens_its_top() {
        let board = stopped_cut();
        let kerf = board.kerfs()[0];
        let thickness = board.thickness().as_meters_f32();
        let (near, far) = (kerf.near.as_meters_f32(), kerf.far.as_meters_f32());
        let opening = kerf.span_at(board.thickness()).unwrap().1.as_meters_f32();
        let mesh = lumber_mesh(&board, Vec3::ZERO);

        let in_opening = |p: Vec3| p.x > near + 1e-5 && p.x < far - 1e-5 && p.z < opening - 1e-4;
        for corners in triangles(&mesh) {
            let center = (corners[0] + corners[1] + corners[2]) / 3.0;
            let on_top = corners.iter().all(|p| (p.y - thickness).abs() < 1e-6);
            assert!(
                !(on_top && in_opening(center)),
                "the top covers the slot at {center}"
            );
        }
        // The slot's walls are there: wood faces looking into it from either side.
        let wall = |x: f32| {
            triangles(&mesh).iter().any(|corners| {
                corners.iter().all(|p| (p.x - x).abs() < 1e-6) && corners[0].z < opening
            })
        };
        assert!(wall(near) && wall(far));
    }

    /// The total area of the triangles lying flat on the board's top face.
    fn top_area(mesh: &Mesh, thickness: f32) -> f32 {
        triangles(mesh)
            .iter()
            .filter(|corners| corners.iter().all(|p| (p.y - thickness).abs() < 1e-6))
            .map(|[a, b, c]| (b - a).cross(c - a).length() / 2.0)
            .sum()
    }

    #[test]
    fn a_dado_from_below_leaves_the_top_whole() {
        let saw = TableSaw {
            blade: Blade {
                diameter: Length::from_inches(10),
                kerf: Blade::FULL_KERF,
            },
            fence: Length::from_inches(12),
            height: Length::from_inch_fraction(1, 4),
        };
        let Ok(Pass::Kerfed(board)) = saw.rip(&plank(), Orientation::default().turned_left())
        else {
            panic!("expected a dado");
        };
        let mesh = lumber_mesh(&board, Vec3::ZERO);
        let face = board.width().as_meters_f32() * board.length().as_meters_f32();
        assert!((top_area(&mesh, board.thickness().as_meters_f32()) - face).abs() < 1e-6);
    }

    #[test]
    fn a_cutaway_corner_has_no_wood() {
        // Cut the plank's far corner out with a stopped rip and a stopped crosscut that meet.
        let saw = |fence: Length| TableSaw {
            blade: Blade {
                diameter: Length::from_inches(10),
                kerf: Blade::FULL_KERF,
            },
            fence,
            height: Length::from_inches(1),
        };
        let six = -Length::from_inches(6);
        let Ok(Pass::Kerfed(ripped)) = saw(Length::from_inches(2)).feed(
            &plank(),
            Orientation::default(),
            six,
            Length::from_inches(8),
        ) else {
            panic!("expected a stopped rip");
        };
        let Ok(Pass::Cut(cut)) = saw(Length::from_inch_fraction(119, 8)).feed(
            &ripped,
            Orientation::default().turned_left(),
            six,
            Length::from_inch_fraction(3, 2),
        ) else {
            panic!("expected the corner to come out");
        };
        let board = cut.piece.board;
        let mesh = lumber_mesh(&board, Vec3::ZERO);
        let corner = board.cutaways()[0];
        let [w0, w1, l0, l1] =
            [corner.w.0, corner.w.1, corner.l.0, corner.l.1].map(Length::as_meters_f32);
        for [a, b, c] in triangles(&mesh) {
            let center = (a + b + c) / 3.0;
            let inside = w0 + 1e-5 < center.x
                && center.x < w1 - 1e-5
                && l0 + 1e-5 < center.z
                && center.z < l1 - 1e-5;
            assert!(!inside, "wood in the cutaway at {center}");
        }
    }

    #[test]
    fn lumber_triangles_face_the_way_their_normals_point() {
        let mesh = lumber_mesh(&stopped_cut(), Vec3::ZERO);
        let normals = normals(&mesh);
        for (i, [a, b, c]) in triangles(&mesh).into_iter().enumerate() {
            let facing = (b - a).cross(c - a);
            assert!(
                facing.dot(normals[i * 3]) > 0.0,
                "triangle {i} faces backward"
            );
        }
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

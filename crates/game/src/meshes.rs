//! Boxes with softly rounded edges, so the shop looks like a set of chunky toys, and the
//! boards the simulation cuts, with crisp sawn edges and every slot cut into them. Wood grain
//! runs along each board and stays put on the wood as it moves or is cut.

use std::f32::consts::FRAC_PI_4;

use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};

use woodsim::Board;

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

/// A board as the simulation has it, slots and all: from its corner at the origin, with `w`
/// along x, `t` along y, and `l` (and the grain) along z. `grain` is where that corner sits in
/// the wood the grain pattern is laid out on, so pieces cut from one board line up.
///
/// The board is built in slices across its width, split wherever a slot's face is. Within a
/// slice, each point along the length has one span of wood from bottom to top; a slot cut from
/// below raises the bottom of that span, and a slot that goes through empties it.
pub fn lumber_mesh(board: &Board, grain: Vec3) -> Mesh {
    let size = Vec3::new(
        board.width().as_meters_f32(),
        board.thickness().as_meters_f32(),
        board.length().as_meters_f32(),
    );
    let slots: Vec<Slot> = board.kerfs().iter().map(Slot::from).collect();
    let mut faces = Vec::from([0.0, size.x]);
    faces.extend(
        slots
            .iter()
            .flat_map(|slot| [slot.near, slot.far])
            .map(|w| w.clamp(0.0, size.x)),
    );
    let faces = sorted_distinct(faces);

    let mut mesh = MeshBuilder::new(grain);
    for (&left, &right) in faces.iter().zip(&faces[1..]) {
        let middle = (left + right) / 2.0;
        let cutting: Vec<&Slot> = slots
            .iter()
            .filter(|slot| (slot.near..=slot.far).contains(&middle))
            .collect();
        let mut stations = Vec::from([0.0, size.z]);
        for slot in &cutting {
            stations.extend(slot.outline(size.y));
        }
        stations.retain(|l| (0.0..=size.z).contains(l));
        let columns: Vec<(f32, f32, f32)> = sorted_distinct(stations)
            .into_iter()
            .map(|l| {
                let (bottom, top) = wood_at(&cutting, l, size.y);
                (l, bottom, top)
            })
            .collect();
        mesh.slice(left, right, &columns, size.z);
    }
    mesh.build()
}

/// A kerf in meters, in the mesh's axes.
struct Slot {
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
    /// The span of height the blade cut at `l` along the board, if it reached there.
    fn cut_at(&self, l: f32) -> Option<(f32, f32)> {
        let along = (self.start - l).max(l - self.end).max(0.0);
        (along < self.radius).then(|| {
            let half = (self.radius * self.radius - along * along).sqrt();
            (self.center - half, self.center + half)
        })
    }

    /// Points along the board where the slot's outline needs a vertex: the ends of its straight
    /// run, and points around its curved ends from where they cross one face of a board
    /// `thickness` thick to where they cross the other.
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

/// The span of wood left at `l` along the board, bottom to top, given the slots cutting this
/// slice. A slot reaches in from the bottom face or the top one; where it goes through, the
/// span is empty, with bottom and top equal.
fn wood_at(slots: &[&Slot], l: f32, thickness: f32) -> (f32, f32) {
    let (mut bottom, mut top) = (0.0_f32, thickness);
    for (low, high) in slots.iter().filter_map(|slot| slot.cut_at(l)) {
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

    /// The faces of one slice of a board from `left` to `right` across it. `columns` are points
    /// along it in order, each with the span of wood there: `(l, bottom, top)`.
    fn slice(&mut self, left: f32, right: f32, columns: &[(f32, f32, f32)], length: f32) {
        let at = |w: f32, t: f32, l: f32| Vec3::new(w, t, l);
        for pair in columns.windows(2) {
            let [(l0, bottom0, top0), (l1, bottom1, top1)] = [pair[0], pair[1]];
            if bottom0 >= top0 && bottom1 >= top1 {
                continue;
            }
            let run = l1 - l0;
            self.quad(
                [
                    at(left, top0, l0),
                    at(right, top0, l0),
                    at(right, top1, l1),
                    at(left, top1, l1),
                ],
                Vec3::new(0.0, run, top0 - top1),
            );
            self.quad(
                [
                    at(left, bottom0, l0),
                    at(right, bottom0, l0),
                    at(right, bottom1, l1),
                    at(left, bottom1, l1),
                ],
                Vec3::new(0.0, -run, bottom1 - bottom0),
            );
            for (w, outward) in [(left, Vec3::NEG_X), (right, Vec3::X)] {
                self.quad(
                    [
                        at(w, bottom0, l0),
                        at(w, top0, l0),
                        at(w, top1, l1),
                        at(w, bottom1, l1),
                    ],
                    outward,
                );
            }
        }
        // The board's two ends, where there's wood at them.
        for &(l, bottom, top) in [columns[0], columns[columns.len() - 1]].iter() {
            if bottom < top {
                let outward = if l < length / 2.0 {
                    Vec3::NEG_Z
                } else {
                    Vec3::Z
                };
                self.quad(
                    [
                        at(left, bottom, l),
                        at(right, bottom, l),
                        at(right, top, l),
                        at(left, top, l),
                    ],
                    outward,
                );
            }
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
    use woodsim::{Blade, Length, Pass, TableSaw};

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
        match saw.feed(&plank(), -Length::from_inches(6), Length::from_inches(10)) {
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

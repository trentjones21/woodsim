//! Boxes with softly rounded edges, so the shop looks like a set of chunky toys, and the
//! boards the simulation cuts, with crisp sawn edges and every slot cut into them. Wood grain
//! runs along each board and stays put on the wood as it moves or is cut.

use std::f32::consts::FRAC_PI_4;

use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};

use woodsim::{Across, Board, Direction, Edge, Length};

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

/// A board as the simulation has it, slots, mitered ends, bevels and all: from its corner at
/// the origin, with `w` along x, `t` along y, and `l` (and the grain) along z. `grain` is where
/// that corner sits in the wood the grain pattern is laid out on, so pieces cut from one board
/// line up.
///
/// The board's face starts as the convex regions woodsim gives, each as it's seen from above,
/// and each region is split along every slot's faces and, inside a slot, at points around its
/// curved ends, and around each resaw slot square to the thickness. At any point in a region,
/// the wood runs from a bottom to a top: a side leaning out going up lowers the bottom and one
/// leaning in lowers the top, a slot cut up from below raises the bottom, one cut down from
/// above lowers the top, and where they meet there's no wood. A resaw slot takes a layer out of
/// the middle, and a tilted slot a slanted band, so the wood there is in two spans, or more.
/// Regions are split wherever a leaning side or a tilted slot's face meets a face, and where
/// two leaning sides cross, so tops and bottoms are flat within each piece. Walls go wherever
/// a region has wood the region beside it doesn't.
pub fn lumber_mesh(board: &Board, grain: Vec3) -> Mesh {
    let thickness = board.thickness().as_meters_f32();
    let slots: Vec<Slot> = board.kerfs().iter().filter_map(Slot::of).collect();
    let resaws: Vec<Resaw> = board.kerfs().iter().filter_map(Resaw::of).collect();
    let tilted: Vec<Tilted> = board.kerfs().iter().filter_map(Tilted::of).collect();
    let leans: Vec<Vec<Lean>> = board
        .face()
        .iter()
        .map(|region| region.edges().iter().filter_map(Lean::of).collect())
        .collect();
    // Each cell, and the region of the face it's in.
    let mut cells: Vec<(Vec<Vec2>, usize)> = board
        .face()
        .iter()
        .enumerate()
        .map(|(k, region)| {
            let corners = region
                .footprint(board.thickness())
                .into_iter()
                .map(|(l, w)| Vec2::new(w.as_meters_f32(), l.as_meters_f32()))
                .collect();
            (corners, k)
        })
        .collect();
    let split_all =
        |cells: Vec<(Vec<Vec2>, usize)>, lines: &[(Vec2, f32)], region: Option<usize>| {
            lines.iter().fold(cells, |cells, &(direction, at)| {
                cells
                    .into_iter()
                    .flat_map(|(cell, k)| {
                        if region.is_some_and(|region| region != k) {
                            return vec![(cell, k)];
                        }
                        split(&cell, direction, at)
                            .into_iter()
                            .map(|part| (part, k))
                            .collect()
                    })
                    .collect()
            })
        };
    for (k, leans) in leans.iter().enumerate() {
        cells = split_all(cells, &lines_between(leans, thickness), Some(k));
    }
    for slot in &slots {
        cells = split_all(
            cells,
            &[(slot.across, slot.near), (slot.across, slot.far)],
            None,
        );
    }
    for slot in &tilted {
        cells = split_all(cells, &slot.faces(thickness), None);
    }
    for slot in &slots {
        let stops: Vec<f32> = slot.outline(thickness).collect();
        cells = cells
            .into_iter()
            .flat_map(|(cell, k)| {
                if !slot.covers(centroid(&cell)) {
                    return vec![(cell, k)];
                }
                stops.iter().fold(vec![(cell, k)], |cells, &at| {
                    cells
                        .iter()
                        .flat_map(|(cell, k)| {
                            split(cell, slot.along, at)
                                .into_iter()
                                .map(|part| (part, *k))
                        })
                        .collect()
                })
            })
            .collect();
    }
    for slot in &tilted {
        let stops: Vec<f32> = slot.outline(thickness).collect();
        cells = cells
            .into_iter()
            .flat_map(|(cell, k)| {
                if !slot.cuts(centroid(&cell), thickness) {
                    return vec![(cell, k)];
                }
                stops.iter().fold(vec![(cell, k)], |cells, &at| {
                    cells
                        .iter()
                        .flat_map(|(cell, k)| {
                            split(cell, slot.along, at)
                                .into_iter()
                                .map(|part| (part, *k))
                        })
                        .collect()
                })
            })
            .collect();
    }
    for resaw in &resaws {
        cells = cells
            .iter()
            .flat_map(|(cell, k)| {
                carve(cell, &resaw.outline)
                    .into_iter()
                    .map(|part| (part, *k))
            })
            .collect();
    }

    // Each cell, what's cutting it, and the spans of wood at each of its corners.
    let cells: Vec<Cell> = cells
        .into_iter()
        .map(|(corners, k)| {
            let middle = centroid(&corners);
            let mut cell = Cell {
                corners,
                leans: &leans[k],
                slots: slots.iter().filter(|slot| slot.covers(middle)).collect(),
                cutouts: cutouts(&resaws, &tilted, middle, thickness),
                thickness,
                columns: Vec::new(),
            };
            cell.columns = cell.corners.iter().map(|&p| cell.column(p)).collect();
            cell
        })
        .collect();

    let mut mesh = MeshBuilder::new(grain);
    for cell in &cells {
        let corners = &cell.corners;
        for span in 0..=cell.cutouts.len() {
            let spans: Vec<(f32, f32)> = cell.columns.iter().map(|column| column[span]).collect();
            mesh.surfaces(corners, &spans);
        }
        let middle = centroid(corners);
        let n = corners.len();
        for i in 0..n {
            let (a, b) = (corners[i], corners[(i + 1) % n]);
            let along = b - a;
            let outward = Vec2::new(along.y, -along.x).normalize_or_zero();
            let outward = if outward.dot(a - middle) < 0.0 {
                -outward
            } else {
                outward
            };
            // Where other cells' corners land on this edge, the wood beside it can change.
            let mut stops: Vec<f32> = cells
                .iter()
                .flat_map(|other| other.corners.iter())
                .filter_map(|&p| {
                    let t = (p - a).dot(along) / along.length_squared();
                    let off = (a + along * t - p).length();
                    (off < 1e-6 && t > 1e-4 && t < 1.0 - 1e-4).then_some(t)
                })
                .collect();
            stops.extend([0.0, 1.0]);
            let stops = sorted_distinct(stops);
            let (from, to) = (&cell.columns[i], &cell.columns[(i + 1) % n]);
            for pair in stops.windows(2) {
                let (p, q) = (a + along * pair[0], a + along * pair[1]);
                let beside = (p + q) / 2.0 + outward * 1e-5;
                let theirs = cells
                    .iter()
                    .find(|other| contains(&other.corners, beside))
                    .map(|other| [p, q].map(|point| other.column(point)));
                for span in 0..from.len() {
                    let ours = |t: f32| {
                        let (from, to) = (from[span], to[span]);
                        (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
                    };
                    mesh.wall(
                        [(p.x, p.y), (q.x, q.y)],
                        [ours(pair[0]), ours(pair[1])],
                        theirs.as_ref(),
                        Vec3::new(outward.x, 0.0, outward.y),
                    );
                }
            }
        }
    }
    mesh.build()
}

/// A convex piece of a board's face, what shapes the wood over it, and the spans of wood at
/// each of its corners.
struct Cell<'a> {
    corners: Vec<Vec2>,
    /// The leaning sides of the region of the face it's in.
    leans: &'a [Lean],
    slots: Vec<&'a Slot>,
    /// What resaw and tilted slots take out of the middle of the wood here, bottom to top.
    cutouts: Vec<Cutout<'a>>,
    thickness: f32,
    /// At each corner, a span of wood below each cutout and one above them all.
    columns: Vec<Vec<(f32, f32)>>,
}

impl Cell<'_> {
    /// The spans of wood at `point`, bottom to top: one below each cutout, and one above them
    /// all, each running from where the leaning sides and the slots in the face leave the
    /// wood. A span with no wood is empty where it would have been, so spans always run
    /// upward.
    fn column(&self, point: Vec2) -> Vec<(f32, f32)> {
        let (low, high) = leaning(self.leans, point, self.thickness);
        let (bottom, top) = wood_at(&self.slots, point, (low, high), self.thickness);
        let taken: Vec<(f32, f32)> = self.cutouts.iter().map(|cutout| cutout.at(point)).collect();
        (0..=taken.len())
            .map(|k| {
                let low = if k == 0 {
                    bottom
                } else {
                    taken[k - 1].1.max(bottom)
                };
                let high = if k == taken.len() {
                    top
                } else {
                    taken[k].0.min(top)
                };
                (low.min(high), high)
            })
            .collect()
    }
}

/// A leaning side of a region, as how high or low the wood reaches under it: where `out · p +
/// lean t = at`, with `out` on the face as the mesh lays it out, `w` along x and `l` along y,
/// all in nanometers times the side's scale.
struct Lean {
    out: [f64; 2],
    lean: f64,
    at: f64,
}

impl Lean {
    fn of(edge: &Edge) -> Option<Lean> {
        edge.leans().then_some(Lean {
            out: [edge.out.w as f64, edge.out.l as f64],
            lean: edge.lean as f64,
            at: edge.at as f64,
        })
    }

    /// How high the side is above `point`, in meters.
    fn height(&self, point: Vec2) -> f32 {
        let across = self.out[0] * point.x as f64 + self.out[1] * point.y as f64;
        ((self.at - across * 1e9) / self.lean / 1e9) as f32
    }

    /// Where on the face it's `t` meters up, as a line `direction · p = at` in meters.
    fn line_at(&self, t: f32) -> Option<(Vec2, f32)> {
        let size = self.out[0].hypot(self.out[1]);
        (size > 0.0).then(|| {
            let direction = Vec2::new((self.out[0] / size) as f32, (self.out[1] / size) as f32);
            (
                direction,
                ((self.at - self.lean * t as f64 * 1e9) / size / 1e9) as f32,
            )
        })
    }
}

/// How low and high the wood reaches at `point` under a region's leaning sides, in a board
/// `thickness` thick: a side leaning in going up holds the top down, one leaning out the
/// bottom up.
fn leaning(leans: &[Lean], point: Vec2, thickness: f32) -> (f32, f32) {
    let (mut low, mut high) = (0.0_f32, thickness);
    for lean in leans {
        let height = lean.height(point);
        if lean.lean > 0.0 {
            high = high.min(height);
        } else {
            low = low.max(height);
        }
    }
    (low.min(high), high)
}

/// The lines on the face where a region's tops and bottoms bend: where each leaning side meets
/// the bottom face and the top, and where two of them are at the same height.
fn lines_between(leans: &[Lean], thickness: f32) -> Vec<(Vec2, f32)> {
    let mut lines: Vec<(Vec2, f32)> = leans
        .iter()
        .flat_map(|lean| [lean.line_at(0.0), lean.line_at(thickness)])
        .flatten()
        .collect();
    for (i, a) in leans.iter().enumerate() {
        for b in &leans[i + 1..] {
            // `(at_a - out_a · p) / lean_a = (at_b - out_b · p) / lean_b`.
            let out = [0, 1].map(|k| b.lean * a.out[k] - a.lean * b.out[k]);
            let size = out[0].hypot(out[1]);
            if size < 1e-9 * a.out[0].hypot(a.out[1]) * b.lean.abs() {
                continue;
            }
            let at = (b.lean * a.at - a.lean * b.at) / size / 1e9;
            lines.push((
                Vec2::new((out[0] / size) as f32, (out[1] / size) as f32),
                at as f32,
            ));
        }
    }
    lines
}

/// Something taking wood out of the middle: a layer a resaw slot took, the same height all
/// over, or the slanted band a tilted slot took.
enum Cutout<'a> {
    Layer(f32, f32),
    Band(&'a Tilted),
}

impl Cutout<'_> {
    /// The heights it takes out at `point`, low to high; the same height twice if none.
    fn at(&self, point: Vec2) -> (f32, f32) {
        match self {
            Cutout::Layer(near, far) => (*near, *far),
            Cutout::Band(slot) => slot.band(point),
        }
    }
}

/// What resaw and tilted slots take out of the middle of the wood at `point` of a board
/// `thickness` thick, bottom to top: resaw layers merged where they overlap.
fn cutouts<'a>(
    resaws: &[Resaw],
    tilted: &'a [Tilted],
    point: Vec2,
    thickness: f32,
) -> Vec<Cutout<'a>> {
    let mut cutouts: Vec<Cutout> = layers_out(resaws, point, thickness)
        .into_iter()
        .map(|(near, far)| Cutout::Layer(near, far))
        .chain(
            tilted
                .iter()
                .filter(|slot| slot.cuts(point, thickness))
                .map(Cutout::Band),
        )
        .collect();
    cutouts.sort_by(|a, b| a.at(point).0.total_cmp(&b.at(point).0));
    cutouts
}

/// The parts of the convex `cell` on either side of the line `direction · p = at`.
fn split(cell: &[Vec2], direction: Vec2, at: f32) -> Vec<Vec<Vec2>> {
    let (below, above) = sides(cell, direction, at);
    below.into_iter().chain(above).collect()
}

/// The parts of the convex `cell` below the line `direction · p = at` and above it, if it has
/// any there.
fn sides(cell: &[Vec2], direction: Vec2, at: f32) -> (Option<Vec<Vec2>>, Option<Vec<Vec2>>) {
    let side: Vec<f32> = cell
        .iter()
        .map(|&p| {
            let d = direction.dot(p) - at;
            if d.abs() < 1e-7 { 0.0 } else { d }
        })
        .collect();
    if side.iter().all(|&d| d <= 0.0) {
        return (Some(cell.to_vec()), None);
    }
    if side.iter().all(|&d| d >= 0.0) {
        return (None, Some(cell.to_vec()));
    }
    let (mut below, mut above) = (Vec::new(), Vec::new());
    let n = cell.len();
    for i in 0..n {
        let j = (i + 1) % n;
        if side[i] <= 0.0 {
            below.push(cell[i]);
        }
        if side[i] >= 0.0 {
            above.push(cell[i]);
        }
        if side[i] * side[j] < 0.0 {
            let crossing = cell[i] + (cell[j] - cell[i]) * (side[i] / (side[i] - side[j]));
            below.push(crossing);
            above.push(crossing);
        }
    }
    let keep = |part: Vec<Vec2>| (area(&part) > 1e-12).then_some(part);
    (keep(below), keep(above))
}

/// The convex `cell` cut where the convex `outline` crosses it: the part inside, and the parts
/// around it, each convex.
fn carve(cell: &[Vec2], outline: &[Vec2]) -> Vec<Vec<Vec2>> {
    let middle = centroid(outline);
    let mut around = Vec::new();
    let mut inside = cell.to_vec();
    for (i, &a) in outline.iter().enumerate() {
        let b = outline[(i + 1) % outline.len()];
        let out = Vec2::new(b.y - a.y, a.x - b.x);
        let out = if out.dot(middle - a) > 0.0 { -out } else { out };
        let (within, beyond) = sides(&inside, out, out.dot(a));
        around.extend(beyond);
        match within {
            Some(within) => inside = within,
            None => return around,
        }
    }
    around.push(inside);
    around
}

fn area(cell: &[Vec2]) -> f32 {
    let n = cell.len();
    (0..n)
        .map(|i| cell[i].perp_dot(cell[(i + 1) % n]))
        .sum::<f32>()
        .abs()
        / 2.0
}

fn centroid(cell: &[Vec2]) -> Vec2 {
    cell.iter().copied().sum::<Vec2>() / cell.len() as f32
}

/// Whether `point` is strictly inside the convex `cell`.
fn contains(cell: &[Vec2], point: Vec2) -> bool {
    let n = cell.len();
    let turns: Vec<f32> = (0..n)
        .map(|i| (cell[(i + 1) % n] - cell[i]).perp_dot(point - cell[i]))
        .collect();
    turns.iter().all(|&t| t > 0.0) || turns.iter().all(|&t| t < 0.0)
}

/// A direction on the board's face as the mesh lays it out, `w` along x and `l` along y, and a
/// way to turn positions along it, nanometers times its scale, into meters.
fn unit(direction: Direction) -> (Vec2, impl Fn(Length) -> f32) {
    let scale = direction.scale();
    let vector = Vec2::new(
        (direction.w as f64 / scale) as f32,
        (direction.l as f64 / scale) as f32,
    );
    (vector, move |at: Length| {
        (at.as_nm() as f64 / scale / 1e9) as f32
    })
}

/// A slot in the face, in meters, on the board's face as the mesh lays it out.
struct Slot {
    across: Vec2,
    along: Vec2,
    near: f32,
    far: f32,
    radius: f32,
    center: f32,
    start: f32,
    end: f32,
}

impl Slot {
    fn of(kerf: &woodsim::Kerf) -> Option<Slot> {
        let Across::Face(across) = kerf.across else {
            return None;
        };
        if kerf.lean != 0 {
            return None;
        }
        let (across, across_meters) = unit(across);
        let (along, along_meters) = unit(kerf.along);
        Some(Slot {
            across,
            along,
            near: across_meters(kerf.near),
            far: across_meters(kerf.far),
            radius: kerf.radius.as_meters_f32(),
            center: kerf.center.as_meters_f32(),
            start: along_meters(kerf.start),
            end: along_meters(kerf.end),
        })
    }
}

/// A resaw slot square to the thickness: the layer it takes out, and where on the face, its
/// round ends drawn as points around them.
struct Resaw {
    near: f32,
    far: f32,
    outline: Vec<Vec2>,
}

/// Points around each round end of a resaw slot's outline.
const RESAW_STEPS: usize = 16;

impl Resaw {
    fn of(kerf: &woodsim::Kerf) -> Option<Resaw> {
        let Across::Thickness = kerf.across else {
            return None;
        };
        let (along, along_meters) = unit(kerf.along);
        let (square, _) = unit(kerf.along.square());
        let (start, end, center) = (
            along_meters(kerf.start),
            along_meters(kerf.end),
            along_meters(kerf.center),
        );
        let radius = kerf.radius.as_meters_f32();
        let half = std::f32::consts::PI;
        let arc = |middle: f32, from: f32| {
            (0..=RESAW_STEPS).map(move |step| {
                let angle = from + half * step as f32 / RESAW_STEPS as f32;
                along * (middle + radius * angle.cos()) + square * (center + radius * angle.sin())
            })
        };
        Some(Resaw {
            near: kerf.near.as_meters_f32(),
            far: kerf.far.as_meters_f32(),
            outline: arc(end, -half / 2.0)
                .chain(arc(start, half / 2.0))
                .collect(),
        })
    }
}

/// A slot sawn by a tilted blade, in meters, on the board's face as the mesh lays it out. Its
/// faces lean, so at a point on the face it takes out a band of height between them, as far as
/// the blade reached.
struct Tilted {
    /// The faces' normal across the face, `w` along x and `l` along y, its length, and its
    /// lean, all in the kerf's whole-number units.
    out: [f64; 2],
    size: f64,
    lean: f64,
    /// Where the faces are, in nanometers times the normal's length.
    near: f64,
    far: f64,
    along: Vec2,
    radius: f32,
    center: f32,
    start: f32,
    end: f32,
    /// How much of the blade's reach up its own plane is up the board's thickness.
    cos: f32,
}

impl Tilted {
    fn of(kerf: &woodsim::Kerf) -> Option<Tilted> {
        let Across::Face(across) = kerf.across else {
            return None;
        };
        if kerf.lean == 0 {
            return None;
        }
        let (along, along_meters) = unit(kerf.along);
        let out = [across.w as f64, across.l as f64];
        let size = out[0].hypot(out[1]);
        let lean = kerf.lean as f64;
        Some(Tilted {
            out,
            size,
            lean,
            near: kerf.near.as_nm() as f64,
            far: kerf.far.as_nm() as f64,
            along,
            radius: kerf.radius.as_meters_f32(),
            center: kerf.center.as_meters_f32(),
            start: along_meters(kerf.start),
            end: along_meters(kerf.end),
            cos: (size / size.hypot(lean)) as f32,
        })
    }

    /// Where its face at `face` is above `point`, in meters.
    fn face_height(&self, face: f64, point: Vec2) -> f32 {
        let across = (self.out[0] * point.x as f64 + self.out[1] * point.y as f64) * 1e9;
        ((face - across) / self.lean / 1e9) as f32
    }

    /// The band of height it takes out at `point`, low to high: between its faces, as far up
    /// and down as the blade reached there.
    fn band(&self, point: Vec2) -> (f32, f32) {
        let faces = [self.near, self.far].map(|face| self.face_height(face, point));
        let (low, high) = (faces[0].min(faces[1]), faces[0].max(faces[1]));
        let along = self.along.dot(point);
        let past = (self.start - along).max(along - self.end).max(0.0);
        if past >= self.radius {
            return (high, high);
        }
        let reach = (self.radius * self.radius - past * past).sqrt() * self.cos;
        let (low, high) = (low.max(self.center - reach), high.min(self.center + reach));
        (low.min(high), high)
    }

    /// Whether it takes any wood out at `point` of a board `thickness` thick.
    fn cuts(&self, point: Vec2, thickness: f32) -> bool {
        let (low, high) = self.band(point);
        low < high && high > 0.0 && low < thickness
    }

    /// Where its faces meet the board's bottom face and its top, as lines `direction · p = at`
    /// in meters.
    fn faces(&self, thickness: f32) -> Vec<(Vec2, f32)> {
        let direction = Vec2::new(
            (self.out[0] / self.size) as f32,
            (self.out[1] / self.size) as f32,
        );
        [self.near, self.far]
            .into_iter()
            .flat_map(|face| {
                [0.0, thickness].map(|t| {
                    (
                        direction,
                        ((face - self.lean * t as f64 * 1e9) / self.size / 1e9) as f32,
                    )
                })
            })
            .collect()
    }

    /// Points along the slot where its outline needs a vertex, as for a slot square to the
    /// face, with the blade's reach up its tilted plane.
    fn outline(&self, thickness: f32) -> impl Iterator<Item = f32> + '_ {
        let reach = self.radius * self.cos;
        let low = (self.center - reach).max(0.0);
        let high = (self.center + reach).min(thickness);
        let arc = (0..=ARC_STEPS).flat_map(move |step| {
            let t = low + (high - low) * step as f32 / ARC_STEPS as f32;
            let up = (t - self.center) / self.cos;
            let along = (self.radius * self.radius - up * up).max(0.0).sqrt();
            [self.start - along, self.end + along]
        });
        [self.start, self.end].into_iter().chain(arc)
    }
}

/// The layers the resaw slots covering `point` take out of a board `thickness` thick, merged
/// where they overlap, bottom to top.
fn layers_out(resaws: &[Resaw], point: Vec2, thickness: f32) -> Vec<(f32, f32)> {
    let mut layers: Vec<(f32, f32)> = resaws
        .iter()
        .filter(|resaw| contains(&resaw.outline, point))
        .map(|resaw| (resaw.near.max(0.0), resaw.far.min(thickness)))
        .filter(|(near, far)| near < far)
        .collect();
    layers.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f32, f32)> = Vec::new();
    for (near, far) in layers {
        match merged.last_mut() {
            Some(last) if near <= last.1 => last.1 = last.1.max(far),
            _ => merged.push((near, far)),
        }
    }
    merged
}

/// Points around each curved end of a slot, between where it meets the board's faces.
const ARC_STEPS: usize = 12;

impl Slot {
    /// Whether `point` is between the slot's faces.
    fn covers(&self, point: Vec2) -> bool {
        let across = self.across.dot(point);
        self.near < across && across < self.far
    }

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

/// The span of wood at `point` on the board's face, bottom to top, given the slots cutting
/// there and how low and high the wood reaches without them. A slot reaches in from the bottom
/// face or the top one; where the wood's gone, bottom and top are equal.
fn wood_at(slots: &[&Slot], point: Vec2, (low, high): (f32, f32), thickness: f32) -> (f32, f32) {
    let (mut bottom, mut top) = (low, high);
    for slot in slots {
        let Some((low, high)) = slot.cut_at(slot.along.dot(point)) else {
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

    /// The top and bottom of the wood in one convex cell, whose corners are at `points` on the
    /// face with the spans of wood `spans`.
    fn surfaces(&mut self, points: &[Vec2], spans: &[(f32, f32)]) {
        if spans.iter().all(|(bottom, top)| bottom >= top) {
            return;
        }
        for upward in [true, false] {
            let corner = |k: usize| {
                let height = if upward { spans[k].1 } else { spans[k].0 };
                Vec3::new(points[k].x, height, points[k].y)
            };
            for k in 1..points.len() - 1 {
                let [a, b, c] = [corner(0), corner(k), corner(k + 1)];
                let facing = (b - a).cross(c - a);
                let facing = if facing.y < 0.0 { -facing } else { facing };
                let facing = if facing.length_squared() > 1e-18 {
                    facing
                } else {
                    Vec3::Y
                };
                self.quad([a, b, c, c], if upward { facing } else { -facing });
            }
        }
    }

    /// The face of a cell's wood along one edge, from `points[0]` to `points[1]`, where the
    /// neighboring cell has no wood to cover it. `ours` is a span of wood at those points on
    /// this side, and `theirs` the spans at them on the other; a neighbor that's off the board
    /// or cut away has none.
    fn wall(
        &mut self,
        points: [(f32, f32); 2],
        ours: [(f32, f32); 2],
        theirs: Option<&[Vec<(f32, f32)>; 2]>,
        outward: Vec3,
    ) {
        // The gaps between the neighbor's spans, where ours shows, clamped into ours.
        let gaps = |k: usize| -> Vec<(f32, f32)> {
            let (bottom, top) = ours[k];
            let Some(theirs) = theirs else {
                return vec![(bottom, top)];
            };
            let mut edges = vec![f32::MIN];
            for &(low, high) in &theirs[k] {
                edges.extend([low, high]);
            }
            edges.push(f32::MAX);
            edges
                .chunks(2)
                .map(|gap| (gap[0].clamp(bottom, top), gap[1].clamp(bottom, top)))
                .collect()
        };
        let (near, far) = (gaps(0), gaps(1));
        for (from, to) in near.into_iter().zip(far) {
            let at = |k: usize, height: f32| Vec3::new(points[k].0, height, points[k].1);
            self.quad(
                [at(0, from.0), at(1, to.0), at(1, to.1), at(0, from.1)],
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
            miter_gauge: None,
            gauge_angle: woodsim::Angle::ZERO,
            tilt: woodsim::Angle::ZERO,
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
            miter_gauge: None,
            gauge_angle: woodsim::Angle::ZERO,
            tilt: woodsim::Angle::ZERO,
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
            miter_gauge: None,
            gauge_angle: woodsim::Angle::ZERO,
            tilt: woodsim::Angle::ZERO,
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
        // The 2" x 9" corner at the far edge's `l = 0` end.
        let mesh = lumber_mesh(&cut.piece.board, Vec3::ZERO);
        let [w0, w1, l0, l1] = [
            Length::from_inch_fraction(21, 4),
            Length::from_inch_fraction(29, 4),
            Length::ZERO,
            Length::from_inches(9),
        ]
        .map(Length::as_meters_f32);
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
    fn a_mitered_end_is_drawn_on_the_slant() {
        let saw = woodsim::MiterSaw {
            miter: woodsim::Angle::from_degrees(45),
            ..woodsim::MiterSaw::twelve_inch()
        };
        // A 1x4: at 45°, a 1x8 is too wide for the saw.
        let one_by_four = woodsim::Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        );
        let Ok(Pass::Cut(cut)) = saw.crosscut(
            &one_by_four,
            Orientation::default(),
            Length::from_inches(12),
        ) else {
            panic!("expected a miter");
        };
        let board = cut.piece.board;
        let mesh = lumber_mesh(&board, Vec3::ZERO);
        let width = board.width().as_meters_f32();
        // Nothing in the triangle cut off the front of the end, where l < width - w.
        for [a, b, c] in triangles(&mesh) {
            let center = (a + b + c) / 3.0;
            assert!(
                center.z + center.x > width - 1e-5,
                "wood past the miter at {center}"
            );
        }
        // The top covers the rest of the board's face.
        let face = width * board.length().as_meters_f32() - width * width / 2.0;
        assert!((top_area(&mesh, board.thickness().as_meters_f32()) - face).abs() < 1e-6);
        // The mitered end is a wall facing out at 45°.
        let slanted = normals(&mesh)
            .iter()
            .any(|n| n.abs_diff_eq(Vec3::new(-1.0, 0.0, -1.0).normalize(), 1e-4));
        assert!(slanted);
    }

    /// A table saw with its blade 1" up and tilted 45°, the fence 2" from it.
    fn tilted_saw() -> TableSaw {
        TableSaw {
            blade: Blade {
                diameter: Length::from_inches(10),
                kerf: Blade::FULL_KERF,
            },
            fence: Length::from_inches(2),
            height: Length::from_inches(1),
            miter_gauge: None,
            gauge_angle: woodsim::Angle::ZERO,
            tilt: woodsim::Angle::from_degrees(45),
        }
    }

    /// The total area of the triangles lying flat on the board's bottom face.
    fn bottom_area(mesh: &Mesh) -> f32 {
        triangles(mesh)
            .iter()
            .filter(|corners| corners.iter().all(|p| p.y.abs() < 1e-6))
            .map(|[a, b, c]| (b - a).cross(c - a).length() / 2.0)
            .sum()
    }

    fn square_inches(n: f32) -> f32 {
        n * 0.0254 * 0.0254
    }

    #[test]
    fn a_beveled_edge_is_drawn_leaning_under_the_wider_top() {
        let Ok(Pass::Cut(cut)) = tilted_saw().rip(&plank(), Orientation::default()) else {
            panic!("expected a rip");
        };
        let board = cut.piece.board;
        let mesh = lumber_mesh(&board, Vec3::ZERO);
        let thickness = board.thickness().as_meters_f32();
        // 2" wide on the bottom, 2-3/4" on top, 24" long.
        assert!((top_area(&mesh, thickness) - square_inches(24.0 * 2.75)).abs() < 1e-6);
        assert!((bottom_area(&mesh) - square_inches(24.0 * 2.0)).abs() < 1e-6);
        // The bevel faces down and out at 45°.
        let normals = normals(&mesh);
        assert!(
            normals
                .iter()
                .any(|n| n.y < -0.7 && n.x.abs() > 0.7 && n.z.abs() < 1e-3),
            "no bevel among {normals:?}"
        );
        for (i, [a, b, c]) in triangles(&mesh).into_iter().enumerate() {
            assert!(
                (b - a).cross(c - a).dot(normals[i * 3]) > 0.0,
                "triangle {i} faces backward"
            );
        }
    }

    #[test]
    fn a_tilted_cut_partway_opens_a_slanted_slot() {
        let saw = TableSaw {
            fence: Length::from_inches(6),
            ..tilted_saw()
        };
        let Ok(Pass::Kerfed(board)) = saw.feed(
            &plank(),
            Orientation::default(),
            -Length::from_inches(6),
            Length::from_inches(10),
        ) else {
            panic!("expected a stopped cut");
        };
        let mesh = lumber_mesh(&board, Vec3::ZERO);
        let face = square_inches(24.0 * 7.25);
        // About 12" of a slot 0.18" across shows on each face.
        for area in [
            top_area(&mesh, board.thickness().as_meters_f32()),
            bottom_area(&mesh),
        ] {
            let open = (face - area) / square_inches(1.0);
            assert!(open > 1.5 && open < 3.0, "{open} square inches open");
        }
        let normals = normals(&mesh);
        assert!(normals.iter().any(|n| n.y.abs() > 0.6 && n.x.abs() > 0.6));
        for (i, [a, b, c]) in triangles(&mesh).into_iter().enumerate() {
            assert!(
                (b - a).cross(c - a).dot(normals[i * 3]) > 0.0,
                "triangle {i} faces backward"
            );
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

    #[test]
    fn a_resaw_slot_opens_a_gap_in_the_edge_but_leaves_the_faces_whole() {
        let saw = TableSaw {
            blade: Blade {
                diameter: Length::from_inches(10),
                kerf: Blade::FULL_KERF,
            },
            fence: Length::from_inch_fraction(5, 16),
            height: Length::from_inches(3),
            miter_gauge: None,
            gauge_angle: woodsim::Angle::ZERO,
            tilt: woodsim::Angle::ZERO,
        };
        let Ok(Pass::Kerfed(board)) = saw.rip(&plank(), Orientation::default().tipped()) else {
            panic!("expected a slot in the edge");
        };
        let mesh = lumber_mesh(&board, Vec3::ZERO);
        let (thickness, width, length) = (
            board.thickness().as_meters_f32(),
            board.width().as_meters_f32(),
            board.length().as_meters_f32(),
        );
        // The faces are whole.
        assert!((top_area(&mesh, thickness) - width * length).abs() < 1e-6);
        // Inside the slot, the wood below faces up at its lower face, and the wood above faces
        // down at its upper one, as far up from the edge as the blade reached.
        let (near, far) = (
            Length::from_inch_fraction(5, 16).as_meters_f32(),
            Length::from_inch_fraction(7, 16).as_meters_f32(),
        );
        let inside = |height: f32, facing: f32| {
            triangles(&mesh)
                .iter()
                .zip(normals(&mesh).chunks(3))
                .filter(|(corners, normal)| {
                    corners.iter().all(|p| (p.y - height).abs() < 1e-6)
                        && normal[0].y * facing > 0.9
                })
                .map(|([a, b, c], _)| (b - a).cross(c - a).length() / 2.0)
                .sum::<f32>()
        };
        let slot = Length::from_inches(3).as_meters_f32() * length;
        assert!(
            (inside(near, 1.0) - slot).abs() < 1e-5,
            "{}",
            inside(near, 1.0)
        );
        assert!(
            (inside(far, -1.0) - slot).abs() < 1e-5,
            "{}",
            inside(far, -1.0)
        );
    }
}

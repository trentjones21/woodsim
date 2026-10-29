//! Where a board's surfaces are wood, slots and all. A slot takes wood from whatever it opens
//! onto, so a face, an edge, or an end is wood only where no slot took it; and inside the
//! board's outline a slot leaves flat surfaces of its own: its sides, where the blade's faces
//! ran, and its bottom, where its teeth ran level along its path. A rabbet is its bottom and one
//! side, a groove its bottom and two.
//!
//! Each surface is worked out on its own plane, as convex polygons in floating point, to well
//! within a nanometer: what's there to start with (the outline's face, or a slot's side or
//! bottom), cut back to the board's wood just behind the plane, less wherever a slot took that
//! wood. It's for drawing surfaces and lining parts up against them; which wood is where is
//! worked out exactly elsewhere (`Board::has_wood_in`).

use std::collections::BTreeSet;

use crate::{
    Board, Length, Surface,
    assembly::{axes, solid_outline},
    face::{Edge, Region, Vector},
    kerf::Kerf,
};

/// How far behind a surface its wood is looked for, in nanometers: past any plane at a whole
/// nanometer, and far less than anything worth drawing.
const BEHIND: f64 = 0.5;

/// Points on each round end of a blade, as a polygon around where it reached.
const ARC_STEPS: usize = 32;

/// Corners nearer than this, in nanometers, are the same corner.
const SAME: f64 = 0.01;

/// A convex polygon on a plane, counterclockwise seen from outside, its corners in nanometers
/// across it.
type Polygon = Vec<[f64; 2]>;

/// Where `surface` of `board` is wood, as polygons going around counterclockwise seen from
/// outside, in its own axes (`l`, `w`, `t`): a face or a side is its outline less wherever a
/// slot opens onto it, and a slot's surface (`Surface::Slot`) is wherever the slot left wood
/// behind that plane. Empty if there's none.
pub fn exposed(board: &Board, surface: Surface) -> Vec<Vec<[Length; 3]>> {
    shape(board, &blades(board), surface)
}

/// The flat surfaces `board`'s slots leave inside its outline, each where it's wood behind it:
/// their sides, and their bottoms where the blade's teeth ran level. Slots side by side that
/// together cut a wider one, like a dado nibbled out a stroke at a time, share a bottom, and
/// leave no sides between them. In order by their planes.
pub fn slot_surfaces(board: &Board) -> Vec<Surface> {
    slot_shapes(board)
        .into_iter()
        .map(|(surface, _)| surface)
        .collect()
}

/// The planes `board`'s slots' bottoms are on, where their teeth ran level along their paths:
/// a slot's surface (`Surface::Slot`) on one of them is a bottom rather than a side.
pub fn slot_bottoms(board: &Board) -> Vec<Edge> {
    board.kerfs().iter().flat_map(bottoms).collect()
}

/// The surfaces `board`'s slots leave, each with where it's wood, as `exposed` has them.
pub(crate) fn slot_shapes(board: &Board) -> Vec<(Surface, Vec<Vec<[Length; 3]>>)> {
    let blades = blades(board);
    let planes: BTreeSet<Edge> = board
        .kerfs()
        .iter()
        .flat_map(|kerf| sides(kerf).into_iter().chain(bottoms(kerf)))
        .collect();
    planes
        .into_iter()
        .filter_map(|plane| {
            let surface = Surface::Slot(plane);
            let shape = shape(board, &blades, surface);
            (!shape.is_empty()).then_some((surface, shape))
        })
        .collect()
}

/// The planes a slot's sides are on, where its blade's faces ran, the wood behind each: past
/// `near` on its normal, and short of `far`.
fn sides(kerf: &Kerf) -> [Edge; 2] {
    kerf.faces().map(Edge::flipped)
}

/// The planes a slot's bottom could be on, the wood behind each: as far as the blade reached
/// up its plane from its center's path, and down it. Only a blade whose center traveled has a
/// bottom that's level along its path.
fn bottoms(kerf: &Kerf) -> Vec<Edge> {
    if kerf.start >= kerf.end {
        return Vec::new();
    }
    let up = kerf.up();
    let reach = scaled(kerf.radius, up.scale_squared());
    vec![
        up.reversed().edge(-(kerf.center + reach)),
        up.edge(kerf.center - reach),
    ]
}

/// `length` times the square root of `squared`, to the nearest whole number: exactly, if the
/// root is whole.
fn scaled(length: Length, squared: i128) -> i128 {
    let root = squared.isqrt();
    if root * root == squared {
        return length.as_nm() as i128 * root;
    }
    (length.as_nm() as f64 * (squared as f64).sqrt()).round() as i128
}

/// A slot, ready to find on planes through the board.
struct Blade {
    kerf: Kerf,
    /// The half-spaces its wood is inside of, `m · p <= c` with `p` in nanometers: between its
    /// faces, and within the blade's reach of its center's path.
    within: Vec<([f64; 3], f64)>,
    /// Around where the blade reached, in its plane: positions along its path and up the plane,
    /// in nanometers times `along`'s and `up`'s scales, counterclockwise from `along` toward
    /// `up`.
    reach: Vec<[f64; 2]>,
}

fn blades(board: &Board) -> Vec<Blade> {
    board.kerfs().iter().map(Blade::of).collect()
}

impl Blade {
    fn of(kerf: &Kerf) -> Blade {
        let (along, up) = (kerf.along.scale(), kerf.up().scale());
        let radius = kerf.radius.as_nm() as f64;
        let center = kerf.center as f64;
        let half = std::f64::consts::PI;
        let arc = |middle: f64, from: f64| {
            (0..=ARC_STEPS).map(move |step| {
                let angle = from + half * step as f64 / ARC_STEPS as f64;
                [
                    middle + radius * angle.cos() * along,
                    center + radius * angle.sin() * up,
                ]
            })
        };
        let reach: Vec<[f64; 2]> = arc(kerf.end.as_nm() as f64, -half / 2.0)
            .chain(arc(kerf.start.as_nm() as f64, half / 2.0))
            .collect();
        let (normal, path, rise) = (float(kerf.normal), float(kerf.along), float(kerf.up()));
        let mut within = vec![
            (normal, kerf.far.as_nm() as f64),
            (normal.map(|c| -c), -(kerf.near.as_nm() as f64)),
        ];
        // Inside is to the left of each side going around: `e_up a - e_along u <= ...`.
        let n = reach.len();
        for i in 0..n {
            let (p, q) = (reach[i], reach[(i + 1) % n]);
            let e = [q[0] - p[0], q[1] - p[1]];
            if e[0].abs() + e[1].abs() < 1e-3 {
                continue;
            }
            let m = [0, 1, 2].map(|k| e[1] * path[k] - e[0] * rise[k]);
            within.push((m, e[1] * p[0] - e[0] * p[1]));
        }
        Blade {
            kerf: *kerf,
            within,
            reach,
        }
    }

    /// The point `along` its path, `up` its plane, and `across` it, each in nanometers times
    /// that direction's scale.
    fn point(&self, along: f64, up: f64, across: f64) -> [f64; 3] {
        let kerf = &self.kerf;
        let (a, u, n) = (float(kerf.along), float(kerf.up()), float(kerf.normal));
        [0, 1, 2]
            .map(|k| along * a[k] / dot(a, a) + up * u[k] / dot(u, u) + across * n[k] / dot(n, n))
    }

    /// Where this slot's sides and bottom are on `plane`, laid out on `frame`, before the
    /// board's wood cuts them back: one of its faces, as far as the blade reached, or its
    /// bottom, between its faces as far as its center traveled.
    fn faces_on(&self, plane: Edge, frame: Frame) -> Vec<Polygon> {
        let kerf = &self.kerf;
        let mut faces = Vec::new();
        for (side, across) in sides(kerf).into_iter().zip([kerf.near, kerf.far]) {
            if side == plane {
                let across = across.as_nm() as f64;
                faces.push(
                    self.reach
                        .iter()
                        .map(|&[along, up]| frame.flat(self.point(along, up, across)))
                        .collect(),
                );
            }
        }
        for bottom in bottoms(kerf) {
            if bottom == plane {
                // On the plane, `up · p` is `at` for the wood below it, `-at` for the wood above.
                let up = if Vector::leaning(bottom.out, bottom.lean) == kerf.up() {
                    bottom.at as f64
                } else {
                    -(bottom.at as f64)
                };
                let [start, end, near, far] =
                    [kerf.start, kerf.end, kerf.near, kerf.far].map(|at| at.as_nm() as f64);
                faces.push(
                    [(start, near), (end, near), (end, far), (start, far)]
                        .map(|(along, across)| frame.flat(self.point(along, up, across)))
                        .to_vec(),
                );
            }
        }
        faces.into_iter().map(counterclockwise).collect()
    }

    /// What's left of `piece`, on the plane `behind` is just behind, where this slot didn't
    /// take the wood behind it: `piece` itself, untouched, if it took none.
    fn cut_from(&self, piece: Polygon, behind: Frame) -> Result<Vec<Polygon>, Polygon> {
        let mut hole = around(&piece);
        for &(m, c) in &self.within {
            match behind.within(m, c) {
                Half::All => {}
                Half::Nothing => return Err(piece),
                Half::Some(a, b) => {
                    hole = clip(&hole, a, b);
                    if thin(&hole) {
                        return Err(piece);
                    }
                }
            }
        }
        minus(&piece, &hole).ok_or(piece)
    }
}

/// Where `surface` of `board` is wood, as `exposed` has it, with `blades` its slots.
fn shape(board: &Board, blades: &[Blade], surface: Surface) -> Vec<Vec<[Length; 3]>> {
    let frame = Frame::of(board, surface);
    let behind = frame.behind(BEHIND);
    let to_lengths = |pieces: Vec<Polygon>| -> Vec<Vec<[Length; 3]>> {
        merged(pieces)
            .into_iter()
            .map(|polygon| {
                polygon
                    .into_iter()
                    .map(|q| frame.point(q).map(|x| Length::from_nm(x.round() as i64)))
                    .collect()
            })
            .collect()
    };
    let Surface::Slot(plane) = surface else {
        // The outline's surface, as it was wherever no slot touches it.
        let mut shape = Vec::new();
        for polygon in solid_outline(board, surface) {
            let flat: Polygon = polygon
                .iter()
                .map(|&p| frame.flat(p.map(|at| at.as_nm() as f64)))
                .collect();
            match less(blades, vec![flat], behind) {
                Err(_) => shape.push(polygon),
                Ok(pieces) => shape.extend(to_lengths(pieces)),
            }
        }
        return shape;
    };
    // Every slot's side or bottom on the plane, not counting any of it twice.
    let mut pieces: Vec<Polygon> = Vec::new();
    for blade in blades {
        for face in blade.faces_on(plane, frame) {
            let mut new = vec![face];
            for old in &pieces {
                new = new
                    .into_iter()
                    .flat_map(|piece| minus(&piece, old).unwrap_or_else(|| vec![piece]))
                    .collect();
            }
            pieces.extend(new);
        }
    }
    let in_wood: Vec<Polygon> = pieces
        .into_iter()
        .flat_map(|piece| within_board(board, behind, piece))
        .collect();
    match less(blades, in_wood, behind) {
        Ok(pieces) | Err(pieces) => to_lengths(pieces),
    }
}

/// `pieces` less wherever `blades` took the wood behind them, on the plane `behind` is just
/// behind: `Err` with them untouched if no blade took any.
fn less(
    blades: &[Blade],
    pieces: Vec<Polygon>,
    behind: Frame,
) -> Result<Vec<Polygon>, Vec<Polygon>> {
    let mut cut = false;
    let mut pieces = pieces;
    for blade in blades {
        pieces = pieces
            .into_iter()
            .flat_map(|piece| match blade.cut_from(piece, behind) {
                Ok(left) => {
                    cut = true;
                    left
                }
                Err(piece) => vec![piece],
            })
            .collect();
    }
    if cut { Ok(pieces) } else { Err(pieces) }
}

/// Where `piece` is in the board's wood on `behind`'s plane: its part in each region.
fn within_board(board: &Board, behind: Frame, piece: Polygon) -> Vec<Polygon> {
    board
        .face()
        .iter()
        .filter_map(|region| {
            let mut part = piece.clone();
            for (m, c) in planes(region, board.thickness()) {
                match behind.within(m, c) {
                    Half::All => {}
                    Half::Nothing => return None,
                    Half::Some(a, b) => part = clip(&part, a, b),
                }
            }
            (!thin(&part)).then_some(part)
        })
        .collect()
}

/// The half-spaces `region` of a board `thickness` thick is inside of, `m · p <= c` with `p`
/// in nanometers: its sides, and its bottom and top faces.
fn planes(region: &Region, thickness: Length) -> Vec<([f64; 3], f64)> {
    region
        .edges()
        .iter()
        .map(|edge| {
            (
                [edge.out.l, edge.out.w, edge.lean].map(|c| c as f64),
                edge.at as f64,
            )
        })
        .chain([
            ([0.0, 0.0, -1.0], 0.0),
            ([0.0, 0.0, 1.0], thickness.as_nm() as f64),
        ])
        .collect()
}

/// A plane through a board, laid out flat: `n` the way it faces out of the wood, a unit vector;
/// `at` how far it is that way from the board's corner, in nanometers; and `u` and `v` across
/// it, turning counterclockwise from one to the other seen from outside.
#[derive(Clone, Copy)]
struct Frame {
    n: [f64; 3],
    at: f64,
    u: [f64; 3],
    v: [f64; 3],
}

/// Where a plane is inside a half-space.
enum Half {
    All,
    Nothing,
    /// Where `a · q <= b`, `a` of unit length.
    Some([f64; 2], f64),
}

impl Frame {
    fn of(board: &Board, surface: Surface) -> Frame {
        let (n, u, v) = axes(surface);
        let at = match surface {
            Surface::Bottom => 0.0,
            Surface::Top => board.thickness().as_nm() as f64,
            Surface::Side(edge) | Surface::Slot(edge) => {
                let out = [edge.out.l, edge.out.w, edge.lean].map(|c| c as f64);
                edge.at as f64 / dot(out, out).sqrt()
            }
        };
        Frame { n, at, u, v }
    }

    /// The same plane, moved `by` nanometers back into the wood.
    fn behind(self, by: f64) -> Frame {
        Frame {
            at: self.at - by,
            ..self
        }
    }

    /// Where `p` (nanometers) is across the plane.
    fn flat(self, p: [f64; 3]) -> [f64; 2] {
        [dot(p, self.u), dot(p, self.v)]
    }

    /// The point `q` across the plane, in nanometers.
    fn point(self, q: [f64; 2]) -> [f64; 3] {
        [0, 1, 2].map(|k| self.n[k] * self.at + q[0] * self.u[k] + q[1] * self.v[k])
    }

    /// Where the plane is inside `m · p <= c`.
    fn within(self, m: [f64; 3], c: f64) -> Half {
        let a = [dot(m, self.u), dot(m, self.v)];
        let b = c - self.at * dot(m, self.n);
        let size = a[0].hypot(a[1]);
        if size <= 1e-12 * dot(m, m).sqrt() {
            return if b >= 0.0 { Half::All } else { Half::Nothing };
        }
        Half::Some([a[0] / size, a[1] / size], b / size)
    }
}

/// What's left of `polygon` where `a · q <= b`.
fn clip(polygon: &[[f64; 2]], a: [f64; 2], b: f64) -> Polygon {
    let n = polygon.len();
    let mut left = Vec::with_capacity(n + 1);
    for i in 0..n {
        let (p, q) = (polygon[i], polygon[(i + 1) % n]);
        let (dp, dq) = (a[0] * p[0] + a[1] * p[1] - b, a[0] * q[0] + a[1] * q[1] - b);
        if dp <= 0.0 {
            left.push(p);
        }
        if (dp < 0.0 && dq > 0.0) || (dp > 0.0 && dq < 0.0) {
            let s = dp / (dp - dq);
            left.push([p[0] + (q[0] - p[0]) * s, p[1] + (q[1] - p[1]) * s]);
        }
    }
    left
}

/// `polygon` less `hole`, both convex, as convex pieces: the part beyond each side of the hole
/// that isn't beyond one before it. `None` if they don't overlap.
fn minus(polygon: &[[f64; 2]], hole: &[[f64; 2]]) -> Option<Vec<Polygon>> {
    let mut pieces = Vec::new();
    let mut rest = polygon.to_vec();
    let n = hole.len();
    for i in 0..n {
        let (p, q) = (hole[i], hole[(i + 1) % n]);
        let e = [q[0] - p[0], q[1] - p[1]];
        let size = e[0].hypot(e[1]);
        if size < SAME {
            continue;
        }
        let a = [e[1] / size, -e[0] / size];
        let b = a[0] * p[0] + a[1] * p[1];
        let beyond = clip(&rest, [-a[0], -a[1]], -b);
        if !thin(&beyond) {
            pieces.push(beyond);
        }
        rest = clip(&rest, a, b);
        if thin(&rest) {
            return None;
        }
    }
    Some(pieces)
}

/// A rectangle a micron bigger than `polygon` all around.
fn around(polygon: &[[f64; 2]]) -> Polygon {
    let (mut low, mut high) = ([f64::MAX; 2], [f64::MIN; 2]);
    for q in polygon {
        for k in 0..2 {
            low[k] = low[k].min(q[k]);
            high[k] = high[k].max(q[k]);
        }
    }
    let (low, high) = (low.map(|x| x - 1_000.0), high.map(|x| x + 1_000.0));
    vec![low, [high[0], low[1]], high, [low[0], high[1]]]
}

/// Whether `polygon` is less than a nanometer across anywhere, or not a polygon at all.
fn thin(polygon: &[[f64; 2]]) -> bool {
    let n = polygon.len();
    if n < 3 {
        return true;
    }
    let (mut twice_area, mut longest) = (0.0f64, 0.0f64);
    for i in 0..n {
        let (p, q) = (polygon[i], polygon[(i + 1) % n]);
        twice_area += p[0] * q[1] - p[1] * q[0];
        longest = longest.max((q[0] - p[0]).hypot(q[1] - p[1]));
    }
    twice_area.abs() < longest
}

/// `polygon` going around counterclockwise.
fn counterclockwise(mut polygon: Polygon) -> Polygon {
    let n = polygon.len();
    let twice_area: f64 = (0..n)
        .map(|i| {
            let (p, q) = (polygon[i], polygon[(i + 1) % n]);
            p[0] * q[1] - p[1] * q[0]
        })
        .sum();
    if twice_area < 0.0 {
        polygon.reverse();
    }
    polygon
}

/// `pieces`, with any two that share a side and make a convex polygon together made one, so
/// slots side by side leave one bottom, not a strip for each.
fn merged(mut pieces: Vec<Polygon>) -> Vec<Polygon> {
    'again: loop {
        for i in 0..pieces.len() {
            for j in i + 1..pieces.len() {
                if let Some(union) = union(&pieces[i], &pieces[j]) {
                    pieces[i] = union;
                    pieces.swap_remove(j);
                    continue 'again;
                }
            }
        }
        return pieces;
    }
}

/// Two convex polygons that share a side, as one, if that's convex too.
fn union(a: &[[f64; 2]], b: &[[f64; 2]]) -> Option<Polygon> {
    let same = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).abs() < SAME && (p[1] - q[1]).abs() < SAME;
    let (n, m) = (a.len(), b.len());
    for i in 0..n {
        for j in 0..m {
            if same(a[i], b[(j + 1) % m]) && same(a[(i + 1) % n], b[j]) {
                // All the way around `a` from the end of the side, then around `b` past it.
                let mut joined: Polygon = (1..=n).map(|k| a[(i + k) % n]).collect();
                joined.extend((2..m).map(|k| b[(j + k) % m]));
                let joined = straightened(joined);
                return convex(&joined).then_some(joined);
            }
        }
    }
    None
}

/// `polygon` without corners where it goes straight on, or back over itself.
fn straightened(mut polygon: Polygon) -> Polygon {
    let mut i = 0;
    while polygon.len() > 3 && i < polygon.len() {
        let n = polygon.len();
        let (p, q, r) = (polygon[(i + n - 1) % n], polygon[i], polygon[(i + 1) % n]);
        let (e, f) = ([q[0] - p[0], q[1] - p[1]], [r[0] - q[0], r[1] - q[1]]);
        let turn = e[0] * f[1] - e[1] * f[0];
        let sizes = e[0].hypot(e[1]) * f[0].hypot(f[1]);
        if sizes < SAME * SAME || turn.abs() <= 1e-9 * sizes {
            polygon.remove(i);
            i = 0;
        } else {
            i += 1;
        }
    }
    polygon
}

/// Whether `polygon` turns left at every corner.
fn convex(polygon: &[[f64; 2]]) -> bool {
    let n = polygon.len();
    (0..n).all(|i| {
        let (p, q, r) = (polygon[i], polygon[(i + 1) % n], polygon[(i + 2) % n]);
        let (e, f) = ([q[0] - p[0], q[1] - p[1]], [r[0] - q[0], r[1] - q[1]]);
        e[0] * f[1] - e[1] * f[0] >= -1e-9 * e[0].hypot(e[1]) * f[0].hypot(f[1])
    })
}

fn float(v: Vector) -> [f64; 3] {
    [v.l, v.w, v.t].map(|c| c as f64)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

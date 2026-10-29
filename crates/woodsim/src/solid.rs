//! A region of a board's face as the solid wood it is, between the bottom face and the top.
//!
//! A region whose sides are square to the faces is the same all the way through, so the face
//! alone says everything about it (`face.rs`). A side a tilted blade left leans: the region is
//! narrower at one face than at the other, and its sides can meet anywhere through the
//! thickness. Its corners are where three of its planes meet, each worked out exactly as a
//! fraction of whole numbers, and which side of a plane a corner is on is exact too, in
//! 256-bit arithmetic (`wide.rs`). Measurements for people use `f64`.

use crate::{
    Length,
    assembly::Surface,
    face::{Edge, Point, Region},
    wide::Wide,
};

/// A plane, as whole numbers: wood is where `n · (l, w, t) <= at`, `n` pointing away from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Plane {
    n: [i128; 3],
    at: i128,
}

const BOTTOM: Plane = Plane {
    n: [0, 0, -1],
    at: 0,
};

impl Plane {
    fn side(edge: Edge) -> Plane {
        Plane {
            n: [edge.out.l.into(), edge.out.w.into(), edge.lean.into()],
            at: edge.at,
        }
    }

    fn top(thickness: Length) -> Plane {
        Plane {
            n: [0, 0, 1],
            at: nm(thickness),
        }
    }

    /// Above zero for a corner beyond the plane, away from the wood; zero on it; below inside.
    fn beyond(self, corner: Corner) -> i32 {
        let [x, y, z] = self.n;
        Wide::dot([
            (x, corner.x),
            (y, corner.y),
            (z, corner.z),
            (-self.at, corner.d),
        ])
        .signum()
    }
}

fn nm(length: Length) -> i128 {
    length.as_nm() as i128
}

/// A point where planes meet, exactly: `(x, y, z) / d` nanometers along the length, across the
/// width, and up the thickness, with `d` positive.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Corner {
    x: i128,
    y: i128,
    z: i128,
    d: i128,
}

impl Corner {
    /// A corner of a region's face, `t` up through the board.
    fn flat(point: Point, t: Length) -> Corner {
        Corner {
            x: point.x,
            y: point.y,
            z: nm(t).checked_mul(point.z).expect(TOO_BIG),
            d: point.z,
        }
    }

    /// Where it is, in nanometers, close enough to measure by.
    pub(crate) fn position(self) -> [f64; 3] {
        [self.x, self.y, self.z].map(|n| n as f64 / self.d as f64)
    }

    /// Where it is, to the nearest nanometer.
    pub(crate) fn nearest(self) -> [Length; 3] {
        [self.x, self.y, self.z]
            .map(|n| Length::from_nm((2 * n + self.d).div_euclid(2 * self.d) as i64))
    }

    /// The whole nanometers just below and just above it on each axis.
    fn rounded_out(self) -> [(i128, i128); 3] {
        [self.x, self.y, self.z].map(|n| (n.div_euclid(self.d), -(-n).div_euclid(self.d)))
    }
}

const TOO_BIG: &str = "a board too big to work out exactly";

/// Where three planes meet, unless two of them are parallel.
fn meet(a: Plane, b: Plane, c: Plane) -> Option<Corner> {
    let rows = [a.n, b.n, c.n];
    let d = determinant(rows);
    if d == 0 {
        return None;
    }
    // Cramer's rule: each coordinate's column swapped for where the planes are.
    let with = |k: usize| {
        let mut swapped = rows;
        for (row, plane) in swapped.iter_mut().zip([a, b, c]) {
            row[k] = plane.at;
        }
        determinant(swapped)
    };
    let sign = d.signum();
    Some(Corner {
        x: sign * with(0),
        y: sign * with(1),
        z: sign * with(2),
        d: sign * d,
    })
}

fn determinant(m: [[i128; 3]; 3]) -> i128 {
    let mul = |a: i128, b: i128| a.checked_mul(b).expect(TOO_BIG);
    let minor =
        |a: i128, b: i128, c: i128, d: i128| mul(a, d).checked_sub(mul(b, c)).expect(TOO_BIG);
    [
        mul(m[0][0], minor(m[1][1], m[1][2], m[2][1], m[2][2])),
        -mul(m[0][1], minor(m[1][0], m[1][2], m[2][0], m[2][2])),
        mul(m[0][2], minor(m[1][0], m[1][1], m[2][0], m[2][1])),
    ]
    .into_iter()
    .try_fold(0i128, |sum, term| sum.checked_add(term))
    .expect(TOO_BIG)
}

/// The solid inside some planes: its corners, each with which of the planes it's on.
struct Shape {
    planes: Vec<Plane>,
    corners: Vec<(Corner, u64)>,
}

impl Shape {
    /// The wood inside `sides`, between the bottom face and the top of a board `thickness`
    /// thick. The bottom and top come after the sides.
    fn of(sides: &[Edge], thickness: Length) -> Shape {
        let planes: Vec<Plane> = sides
            .iter()
            .map(|&side| Plane::side(side))
            .chain([BOTTOM, Plane::top(thickness)])
            .collect();
        Shape::within(planes)
    }

    fn within(planes: Vec<Plane>) -> Shape {
        assert!(
            planes.len() <= 64,
            "a region with more sides than it can count"
        );
        let n = planes.len();
        let mut corners: Vec<(Corner, u64)> = Vec::new();
        for i in 0..n {
            for j in i + 1..n {
                for k in j + 1..n {
                    let three = 1 << i | 1 << j | 1 << k;
                    // Four or more planes through one corner meet there three at a time.
                    if corners.iter().any(|&(_, on)| on & three == three) {
                        continue;
                    }
                    let Some(corner) = meet(planes[i], planes[j], planes[k]) else {
                        continue;
                    };
                    let mut on = 0u64;
                    let inside = planes.iter().enumerate().all(|(m, plane)| {
                        let beyond = plane.beyond(corner);
                        if beyond == 0 {
                            on |= 1 << m;
                        }
                        beyond <= 0
                    });
                    if inside {
                        corners.push((corner, on));
                    }
                }
            }
        }
        Shape { planes, corners }
    }

    /// Whether there's any wood: some point strictly inside every plane. Wood with no
    /// thickness anywhere lies on one of its planes all over.
    fn solid(&self) -> bool {
        !self.corners.is_empty()
            && (0..self.planes.len()).all(|m| self.corners.iter().any(|&(_, on)| on & 1 << m == 0))
    }

    /// The corners on plane `m`.
    fn on(&self, m: usize) -> impl Iterator<Item = &(Corner, u64)> {
        self.corners
            .iter()
            .filter(move |&&(_, on)| on & 1 << m != 0)
    }

    /// Whether plane `m` bounds the wood with a face of some area, rather than along an edge or
    /// at a corner: its corners don't all lie on another of the planes too.
    fn face(&self, m: usize) -> bool {
        let on: Vec<u64> = self.on(m).map(|&(_, on)| on).collect();
        on.len() >= 3
            && !(0..self.planes.len())
                .filter(|&j| j != m)
                .any(|j| on.iter().all(|&mask| mask & 1 << j != 0))
    }

    /// The corners of face `m`, in order counterclockwise seen from outside, close enough to
    /// draw and measure by.
    fn face_corners(&self, m: usize) -> Vec<Corner> {
        let corners: Vec<Corner> = self.on(m).map(|&(corner, _)| corner).collect();
        let positions: Vec<[f64; 3]> = corners.iter().map(|c| c.position()).collect();
        let middle = positions
            .iter()
            .fold([0.0; 3], |sum, p| [0, 1, 2].map(|k| sum[k] + p[k]))
            .map(|sum| sum / positions.len() as f64);
        let normal = self.planes[m].n.map(|n| n as f64);
        let u = unit(square_to(normal));
        let v = cross(unit(normal), u);
        let angle = |p: &[f64; 3]| {
            let off = [0, 1, 2].map(|k| p[k] - middle[k]);
            dot(off, v).atan2(dot(off, u))
        };
        let mut order: Vec<usize> = (0..corners.len()).collect();
        order.sort_by(|&a, &b| angle(&positions[a]).total_cmp(&angle(&positions[b])));
        order.into_iter().map(|i| corners[i]).collect()
    }

    /// How much wood, in cubic nanometers: each face fanned into triangles, each with the
    /// origin a tetrahedron.
    fn volume(&self) -> f64 {
        (0..self.planes.len())
            .filter(|&m| self.face(m))
            .map(|m| {
                let corners: Vec<[f64; 3]> = self
                    .face_corners(m)
                    .into_iter()
                    .map(Corner::position)
                    .collect();
                (1..corners.len() - 1)
                    .map(|i| dot(corners[0], cross(corners[i], corners[i + 1])))
                    .sum::<f64>()
            })
            .sum::<f64>()
            / 6.0
    }
}

/// The region of wood inside `sides`, in a board `thickness` thick, if it has any: just the
/// sides that bound it with a face, so the same wood is the same region however it was cut.
/// With none of them leaning, it's a region of the face as `Region` keeps one.
pub(crate) fn region(mut sides: Vec<Edge>, thickness: Length) -> Option<Region> {
    sides.sort();
    sides.dedup();
    if !sides.iter().any(|side| side.leans()) {
        return Region::within(sides);
    }
    let shape = Shape::of(&sides, thickness);
    if !shape.solid() {
        return None;
    }
    let faces: Vec<Edge> = (0..sides.len())
        .filter(|&m| shape.face(m))
        .map(|m| sides[m])
        .collect();
    if faces.iter().any(|side| side.leans()) {
        Some(Region::leaning(faces))
    } else {
        Region::within(faces)
    }
}

impl Region {
    /// Its corners in a board `thickness` thick, `[l, w, t]` to the nearest nanometer: on the
    /// bottom face and the top, and wherever leaning sides meet between.
    pub fn corners_through(&self, thickness: Length) -> Vec<[Length; 3]> {
        self.solid(thickness)
            .corners()
            .into_iter()
            .map(Corner::nearest)
            .collect()
    }

    /// Its outline seen from above, in a board `thickness` thick: its corners, as `corners`
    /// gives them, counterclockwise. With sides leaning, wherever it reaches furthest, on
    /// whichever face or between.
    pub fn footprint(&self, thickness: Length) -> Vec<(Length, Length)> {
        if !self.leans() {
            return self.corners();
        }
        hull(
            self.corners_through(thickness)
                .into_iter()
                .map(|[l, w, _]| (l, w))
                .collect(),
        )
    }
}

/// The convex hull of `points`, counterclockwise from the length toward the width, exactly.
pub(crate) fn hull(mut points: Vec<(Length, Length)>) -> Vec<(Length, Length)> {
    points.sort();
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let turn = |o: (Length, Length), a: (Length, Length), b: (Length, Length)| {
        let (ax, ay) = (nm(a.0 - o.0), nm(a.1 - o.1));
        let (bx, by) = (nm(b.0 - o.0), nm(b.1 - o.1));
        ax * by - ay * bx
    };
    let mut around: Vec<(Length, Length)> = Vec::new();
    for ordered in [points.clone(), points.iter().rev().copied().collect()] {
        let start = around.len();
        for p in ordered {
            while around.len() >= start + 2
                && turn(around[around.len() - 2], around[around.len() - 1], p) <= 0
            {
                around.pop();
            }
            around.push(p);
        }
        around.pop();
    }
    around
}

/// A region as wood in a board `thickness` thick.
#[derive(Clone, Copy)]
pub(crate) struct Solid<'a> {
    region: &'a Region,
    thickness: Length,
}

impl Region {
    pub(crate) fn solid(&self, thickness: Length) -> Solid<'_> {
        Solid {
            region: self,
            thickness,
        }
    }
}

impl Solid<'_> {
    fn shape(&self) -> Shape {
        Shape::of(self.region.edges(), self.thickness)
    }

    /// Its corners: where the face's corners are on the bottom and the top, or, with leaning
    /// sides, wherever three of its planes meet.
    pub(crate) fn corners(&self) -> Vec<Corner> {
        if !self.region.leans() {
            let points = self.region.points();
            return [Length::ZERO, self.thickness]
                .into_iter()
                .flat_map(|t| points.iter().map(move |&point| Corner::flat(point, t)))
                .collect();
        }
        self.shape()
            .corners
            .into_iter()
            .map(|(corner, _)| corner)
            .collect()
    }

    /// The part of it inside `cut`, if there's any wood there.
    pub(crate) fn clip(&self, cut: Edge) -> Option<Region> {
        if !self.region.leans() && !cut.leans() {
            return self.region.clip(cut);
        }
        let mut sides = self.region.edges().to_vec();
        sides.push(cut);
        region(sides, self.thickness)
    }

    /// The part of it inside every one of `cuts`, if there's any wood there.
    pub(crate) fn clip_all(&self, cuts: impl IntoIterator<Item = Edge>) -> Option<Region> {
        cuts.into_iter()
            .try_fold(self.region.clone(), |region, cut| {
                region.solid(self.thickness).clip(cut)
            })
    }

    /// Whether any of its wood is inside every one of `hole`'s planes.
    pub(crate) fn overlaps(&self, hole: &[Edge]) -> bool {
        self.clip_all(hole.iter().copied()).is_some()
    }

    /// The region less the wood inside every one of `hole`'s planes, as regions.
    pub(crate) fn minus(&self, hole: &[Edge]) -> Vec<Region> {
        if !self.overlaps(hole) {
            return vec![self.region.clone()];
        }
        let mut rest = self.region.clone();
        let mut outside = Vec::new();
        for &edge in hole {
            outside.extend(rest.solid(self.thickness).clip(edge.flipped()));
            match rest.solid(self.thickness).clip(edge) {
                Some(inside) => rest = inside,
                None => break,
            }
        }
        outside
    }

    /// Whether it and `other` share some of a face, so wood in one holds onto wood in the
    /// other. Touching along an edge or at a corner doesn't hold.
    pub(crate) fn joins(&self, other: &Region) -> bool {
        if !self.region.leans() && !other.leans() {
            return self.region.joins(other);
        }
        self.region.edges().iter().any(|&side| {
            let back = side.flipped();
            if !other.edges().contains(&back) {
                return false;
            }
            // Where the two faces overlap, in the plane they share: corners where two more of
            // their planes meet it. It has some area unless every corner is on one of them.
            let shared = Plane::side(side);
            let rest: Vec<Plane> = self
                .region
                .edges()
                .iter()
                .filter(|&&edge| edge != side)
                .chain(other.edges().iter().filter(|&&edge| edge != back))
                .map(|&edge| Plane::side(edge))
                .chain([BOTTOM, Plane::top(self.thickness)])
                .collect();
            let mut corners: Vec<u64> = Vec::new();
            for j in 0..rest.len() {
                for k in j + 1..rest.len() {
                    let Some(corner) = meet(shared, rest[j], rest[k]) else {
                        continue;
                    };
                    let mut on = 0u64;
                    let inside = rest.iter().enumerate().all(|(m, plane)| {
                        let beyond = plane.beyond(corner);
                        if beyond == 0 {
                            on |= 1 << m;
                        }
                        beyond <= 0
                    });
                    if inside {
                        corners.push(on);
                    }
                }
            }
            !corners.is_empty()
                && (0..rest.len()).all(|m| corners.iter().any(|&on| on & 1 << m == 0))
        })
    }

    /// It and `other` as one region, if they share a face and together they're convex.
    pub(crate) fn merged(&self, other: &Region) -> Option<Region> {
        if !self.region.leans() && !other.leans() {
            return self.region.merged(other);
        }
        let shared = *self
            .region
            .edges()
            .iter()
            .find(|edge| other.edges().contains(&edge.flipped()))?;
        let inside = |corners: Vec<Corner>, edges: &[Edge], skip: Edge| {
            corners.into_iter().all(|corner| {
                edges
                    .iter()
                    .filter(|&&edge| edge != skip)
                    .all(|&edge| Plane::side(edge).beyond(corner) <= 0)
            })
        };
        let theirs = other.solid(self.thickness).corners();
        if !inside(theirs, self.region.edges(), shared)
            || !inside(self.corners(), other.edges(), shared.flipped())
        {
            return None;
        }
        let sides = self
            .region
            .edges()
            .iter()
            .chain(other.edges())
            .filter(|&&edge| edge != shared && edge != shared.flipped())
            .copied()
            .collect();
        region(sides, self.thickness)
    }

    /// The smallest box of whole nanometers around it: its `l`, `w`, and `t` spans.
    pub(crate) fn bounds(&self) -> [(Length, Length); 3] {
        if !self.region.leans() {
            let (l, w) = self.region.bounds();
            return [l, w, (Length::ZERO, self.thickness)];
        }
        let rounded: Vec<[(i128, i128); 3]> = self
            .corners()
            .into_iter()
            .map(Corner::rounded_out)
            .collect();
        [0, 1, 2].map(|k| {
            let low = rounded
                .iter()
                .map(|r| r[k].0)
                .min()
                .expect("wood has corners");
            let high = rounded
                .iter()
                .map(|r| r[k].1)
                .max()
                .expect("wood has corners");
            (Length::from_nm(low as i64), Length::from_nm(high as i64))
        })
    }

    /// Whether some of it is beyond `side`, away from the wood it bounds, exactly.
    pub(crate) fn beyond(&self, side: Edge) -> bool {
        let plane = Plane::side(side);
        self.corners()
            .into_iter()
            .any(|corner| plane.beyond(corner) > 0)
    }

    /// Whether `(l, w, t)` is strictly inside it.
    pub(crate) fn contains(&self, l: Length, w: Length, t: Length) -> bool {
        Length::ZERO < t
            && t < self.thickness
            && self.region.edges().iter().all(|edge| edge.inside(l, w, t))
    }

    /// Whether it has wood all the way through at `(l, w)`: strictly inside at both faces, and
    /// so everywhere between.
    pub(crate) fn contains_through(&self, l: Length, w: Length) -> bool {
        self.region
            .edges()
            .iter()
            .all(|edge| edge.inside(l, w, Length::ZERO) && edge.inside(l, w, self.thickness))
    }

    /// The outline of one of its flat surfaces, if it has any of it: its corners in order
    /// counterclockwise seen from outside, to the nearest nanometer.
    pub(crate) fn outline(&self, surface: Surface) -> Option<Vec<[Length; 3]>> {
        let shape = self.shape();
        let sides = self.region.edges().len();
        let m = match surface {
            Surface::Bottom => sides,
            Surface::Top => sides + 1,
            Surface::Side(edge) => self.region.edges().iter().position(|&e| e == edge)?,
        };
        shape.face(m).then(|| {
            shape
                .face_corners(m)
                .into_iter()
                .map(Corner::nearest)
                .collect()
        })
    }

    /// How much wood it holds, in cubic nanometers.
    pub(crate) fn volume(&self) -> f64 {
        if !self.region.leans() {
            let corners: Vec<[f64; 2]> = self
                .region
                .points()
                .into_iter()
                .map(|point| {
                    let [l, w, _] = Corner::flat(point, Length::ZERO).position();
                    [l, w]
                })
                .collect();
            let n = corners.len();
            let area = (0..n)
                .map(|i| {
                    let (a, b) = (corners[i], corners[(i + 1) % n]);
                    a[0] * b[1] - a[1] * b[0]
                })
                .sum::<f64>()
                / 2.0;
            return area * self.thickness.as_nm() as f64;
        }
        self.shape().volume()
    }
}

/// Whether the line square to the faces at `(l, w)` passes through the inside of every one of
/// `planes` for some of the way between the faces of a board `thickness` thick: where a hole
/// they bound goes through it there.
pub(crate) fn line_crosses(planes: &[Edge], l: Length, w: Length, thickness: Length) -> bool {
    // The heights where it's inside, as fractions `n / d` with `d` positive.
    let (mut low, mut high) = ((0i128, 1i128), (nm(thickness), 1i128));
    for edge in planes {
        let room = edge.at - edge.out.dot(l, w);
        let lean = i128::from(edge.lean);
        match lean.signum() {
            0 if room <= 0 => return false,
            0 => {}
            // lean * t < room: below room / lean going up, above it going down.
            1 if room * high.1 < high.0 * lean => high = (room, lean),
            -1 if -room * low.1 > low.0 * -lean => low = (-room, -lean),
            _ => {}
        }
    }
    low.0 * high.1 < high.0 * low.1
}

fn square_to(v: [f64; 3]) -> [f64; 3] {
    // Crossed with whichever axis it's least along.
    let axis = (0..3)
        .min_by(|&a, &b| v[a].abs().total_cmp(&v[b].abs()))
        .expect("three axes");
    let mut e = [0.0; 3];
    e[axis] = 1.0;
    cross(v, e)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let size = dot(v, v).sqrt();
    v.map(|c| c / size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Angle, face::Direction};

    fn inches(n: i64) -> Length {
        Length::from_inches(n)
    }

    fn fraction(numerator: i64, denominator: i64) -> Length {
        Length::from_inch_fraction(numerator, denominator)
    }

    const CUBIC_INCH: f64 = 25_400_000.0 * 25_400_000.0 * 25_400_000.0;

    fn cubic_inches(volume: f64) -> f64 {
        volume / CUBIC_INCH
    }

    /// A 4" x 2" piece of a board 1" thick.
    fn block() -> Region {
        Region::rectangle((Length::ZERO, inches(4)), (Length::ZERO, inches(2)))
    }

    /// The side `w + t = at`, leaning in 45° going up, the wood toward `w = 0`.
    fn bevel(at: Length) -> Edge {
        let (out, lean) = Direction::WIDTH.tilted(Angle::from_degrees(45));
        assert_eq!((out, lean), (Direction::WIDTH, 1));
        Edge::leaning(out, lean, at.as_nm() as i128)
    }

    #[test]
    fn a_leaning_cut_leaves_wood_narrower_at_the_top() {
        let thickness = inches(1);
        let cut = block().solid(thickness).clip(bevel(inches(2))).unwrap();
        assert!(cut.leans());
        let solid = cut.solid(thickness);
        assert_eq!(
            solid.bounds(),
            [
                (Length::ZERO, inches(4)),
                (Length::ZERO, inches(2)),
                (Length::ZERO, inches(1)),
            ]
        );
        // 1-1/2" across low down, but not high up.
        let (along, across) = (inches(2), fraction(3, 2));
        assert!(solid.contains(along, across, fraction(1, 4)));
        assert!(!solid.contains(along, across, fraction(3, 4)));
        assert!(!solid.contains_through(along, across));
        assert!(solid.contains_through(along, fraction(1, 2)));
        // 4" long, 2" wide at the bottom and 1" at the top.
        assert!((cubic_inches(solid.volume()) - 6.0).abs() < 1e-9);

        // The other side is a wedge along the top edge.
        let wedge = block()
            .solid(thickness)
            .clip(bevel(inches(2)).flipped())
            .unwrap();
        let wedge = wedge.solid(thickness);
        assert_eq!(wedge.bounds()[1], (inches(1), inches(2)));
        assert!((cubic_inches(wedge.volume()) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_leaning_cut_that_misses_or_only_touches_an_edge_changes_nothing_or_leaves_nothing() {
        let thickness = inches(1);
        let solid = block();
        let solid = solid.solid(thickness);
        // Past the top corner along the far edge, and through it exactly.
        for at in [inches(4), inches(3)] {
            assert_eq!(solid.clip(bevel(at)), Some(block()));
            assert_eq!(solid.clip(bevel(at).flipped()), None);
        }
    }

    #[test]
    fn the_two_sides_of_a_leaning_cut_hold_together_and_merge_back() {
        let thickness = inches(1);
        let cut = bevel(fraction(3, 2));
        let solid = block();
        let solid = solid.solid(thickness);
        let halves = [solid.clip(cut).unwrap(), solid.clip(cut.flipped()).unwrap()];
        assert!(halves[0].solid(thickness).joins(&halves[1]));
        assert!(halves[1].solid(thickness).joins(&halves[0]));
        // Back together, they're the plain block again, exactly.
        assert_eq!(halves[0].solid(thickness).merged(&halves[1]), Some(block()));
        let volumes: f64 = halves
            .iter()
            .map(|half| cubic_inches(half.solid(thickness).volume()))
            .sum();
        assert!((volumes - 8.0).abs() < 1e-9);
    }

    #[test]
    fn faces_on_one_leaning_plane_that_meet_only_along_a_line_do_not_hold() {
        let thickness = inches(1);
        let cut = bevel(fraction(3, 2));
        let middle = |side: Direction, at: Length| Edge::new(side, at.as_nm() as i128);
        let solid = block();
        let solid = solid.solid(thickness);
        // Below the cut on the first half of the length, above it on the second half.
        let low = solid
            .clip_all([cut, middle(Direction::LENGTH, inches(2))])
            .unwrap();
        let high = solid
            .clip_all([
                cut.flipped(),
                middle(Direction::LENGTH.reversed(), -inches(2)),
            ])
            .unwrap();
        assert!(!low.solid(thickness).joins(&high));
        // Overlapping by a little along the length, they do.
        let longer = solid
            .clip_all([
                cut,
                middle(Direction::LENGTH, inches(2) + Length::from_nm(1)),
            ])
            .unwrap();
        assert!(longer.solid(thickness).joins(&high));
    }

    #[test]
    fn a_ridge_between_two_leaning_sides_stops_short_of_the_top() {
        let thickness = inches(1);
        // In 45° from both edges: `w + t <= 1-1/2"` and `t - w <= -1/2"`.
        let (out, lean) = Direction::WIDTH.reversed().tilted(Angle::from_degrees(45));
        let other = Edge::leaning(out, lean, -fraction(1, 2).as_nm() as i128);
        let ridge = block()
            .solid(thickness)
            .clip_all([bevel(fraction(3, 2)), other])
            .unwrap();
        let solid = ridge.solid(thickness);
        assert_eq!(
            solid.bounds(),
            [
                (Length::ZERO, inches(4)),
                (fraction(1, 2), fraction(3, 2)),
                (Length::ZERO, fraction(1, 2)),
            ]
        );
        assert_eq!(solid.outline(Surface::Top), None);
        assert_eq!(
            solid.outline(Surface::Bottom).map(|corners| corners.len()),
            Some(4)
        );
        assert!((cubic_inches(solid.volume()) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_solid_that_loses_its_leaning_side_is_a_plain_region_again() {
        let thickness = inches(1);
        let beveled = block().solid(thickness).clip(bevel(inches(2))).unwrap();
        let narrow = Edge::new(Direction::WIDTH, inches(1).as_nm() as i128);
        assert_eq!(
            beveled.solid(thickness).clip(narrow),
            Some(Region::rectangle(
                (Length::ZERO, inches(4)),
                (Length::ZERO, inches(1))
            ))
        );
    }

    #[test]
    fn a_leaning_side_faces_out_and_up_from_the_wood() {
        let thickness = inches(1);
        let beveled = block().solid(thickness).clip(bevel(inches(2))).unwrap();
        let side = beveled
            .solid(thickness)
            .outline(Surface::Side(bevel(inches(2))))
            .unwrap();
        assert_eq!(side.len(), 4);
        // Going around it counterclockwise seen from outside, its normal points up and out.
        let p: Vec<[f64; 3]> = side
            .iter()
            .map(|corner| corner.map(|length| length.as_nm() as f64))
            .collect();
        let normal = cross(
            [0, 1, 2].map(|k| p[1][k] - p[0][k]),
            [0, 1, 2].map(|k| p[2][k] - p[0][k]),
        );
        assert!(
            normal[0].abs() < 1.0 && normal[1] > 0.0 && normal[2] > 0.0,
            "{normal:?}"
        );
        assert!((normal[1] - normal[2]).abs() / normal[1] < 1e-12);
    }

    #[test]
    fn a_hole_through_a_leaning_slot_is_crossed_where_a_line_up_through_it_passes_inside() {
        let thickness = inches(1);
        // The slab between `w + t = 1"` and `w + t = 1-1/8"`.
        let hole = [
            bevel(inches(1) + fraction(1, 8)),
            bevel(inches(1)).flipped(),
        ];
        let crosses = |w: Length| line_crosses(&hole, inches(1), w, thickness);
        assert!(crosses(inches(1)));
        assert!(crosses(fraction(1, 16)));
        // It reaches 1-1/8" across at the bottom face, and just 1/8" at the top.
        assert!(!crosses(inches(1) + fraction(1, 8)));
        assert!(!crosses(Length::ZERO));
        assert!(crosses(inches(1) + fraction(1, 16)));
    }

    #[test]
    fn tilting_a_direction_keeps_it_facing_the_same_way() {
        let slanted = Direction::new(3_355_400, 2_469);
        let (out, lean) = slanted.tilted(Angle::from_degrees(30));
        // The same way across the face, some whole number of times as long.
        assert_eq!(
            out.l as i128 * slanted.w as i128,
            out.w as i128 * slanted.l as i128
        );
        assert!(out.l > 0 && out.l % slanted.l == 0);
        let tilt = (lean as f64).atan2(out.scale());
        assert!((tilt - 30f64.to_radians()).abs() < 1e-7, "{tilt}");
        let (out, lean) = Direction::LENGTH.tilted(Angle::from_hundredths(-1250));
        assert_eq!(out.w, 0);
        assert!(lean < 0);
        let tilt = (lean as f64).atan2(out.scale());
        assert!((tilt + 12.5f64.to_radians()).abs() < 1e-7, "{tilt}");
    }
}

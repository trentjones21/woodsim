//! The shape of a board's face, worked out exactly: straight edges at any angle, and the convex
//! regions between them.
//!
//! A slanted edge comes from a saw set to an angle. Its direction is a pair of whole numbers
//! within 1/16,000,000 of a radian of that angle, and where it crosses the face is a whole
//! number too, so whether a point is on one side of an edge or the other, or whether two
//! pieces still hold together, always has an exact answer.

use crate::{Angle, Axis, Length};

/// A direction in a board's face, as whole numbers with no common factor: `l` along its length
/// and `w` across its width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Direction {
    pub l: i64,
    pub w: i64,
}

impl Direction {
    pub const LENGTH: Direction = Direction { l: 1, w: 0 };
    pub const WIDTH: Direction = Direction { l: 0, w: 1 };

    /// The direction `(l, w)` points. Panics if both are zero.
    pub fn new(l: i64, w: i64) -> Direction {
        let divisor = gcd(l.unsigned_abs(), w.unsigned_abs());
        assert!(divisor != 0, "a direction can't be zero");
        let divisor = divisor as i64;
        Direction {
            l: l / divisor,
            w: w / divisor,
        }
    }

    /// The direction one of the board's face axes runs. Panics for the thickness, which isn't
    /// a direction in the face.
    pub fn of(axis: Axis) -> Direction {
        match axis {
            Axis::Length => Direction::LENGTH,
            Axis::Width => Direction::WIDTH,
            Axis::Thickness => panic!("the thickness isn't a direction in the board's face"),
        }
    }

    /// The board axis this direction runs along, either way, if it runs along one.
    pub fn axis(self) -> Option<Axis> {
        match (self.l, self.w) {
            (_, 0) => Some(Axis::Length),
            (0, _) => Some(Axis::Width),
            _ => None,
        }
    }

    pub fn reversed(self) -> Direction {
        Direction {
            l: -self.l,
            w: -self.w,
        }
    }

    /// Square to this direction: a quarter turn from the length toward the width.
    pub fn square(self) -> Direction {
        Direction {
            l: -self.w,
            w: self.l,
        }
    }

    /// How long the direction's whole-number components make it. Positions measured along a
    /// slanted direction come out this many times the distance in nanometers.
    pub fn scale(self) -> f64 {
        (self.scale_squared() as f64).sqrt()
    }

    pub(crate) fn scale_squared(self) -> i128 {
        self.dot_direction(self)
    }

    /// How far `(l, w)` is along this direction, times its scale.
    pub(crate) fn dot(self, l: Length, w: Length) -> i128 {
        self.l as i128 * l.as_nm() as i128 + self.w as i128 * w.as_nm() as i128
    }

    fn dot_direction(self, other: Direction) -> i128 {
        self.l as i128 * other.l as i128 + self.w as i128 * other.w as i128
    }

    /// A face facing this way and tilted `tilt` from square to the board's face, leaning back
    /// from this way going up: as `out` and `lean` for an `Edge` or a `Kerf`, whole numbers
    /// with no common factor, within about 1/16,000,000 of a radian of the angle.
    pub(crate) fn tilted(self, tilt: Angle) -> (Direction, i64) {
        if tilt == Angle::ZERO {
            return (self, 0);
        }
        // Long enough across the face for the lean to come out close as a whole number.
        let times = ((1u64 << 24) as f64 / self.scale()).ceil().max(1.0) as i64;
        let (l, w) = (self.l * times, self.w * times);
        let lean = ((l as f64).hypot(w as f64) * tilt.radians().tan()).round() as i64;
        let divisor = gcd(gcd(l.unsigned_abs(), w.unsigned_abs()), lean.unsigned_abs()) as i64;
        (
            Direction {
                l: l / divisor,
                w: w / divisor,
            },
            lean / divisor,
        )
    }
}

/// A direction through a board, as whole numbers with no common factor: `l` along its length,
/// `w` across its width, and `t` up its thickness. A blade's faces face some way through the
/// board, and its center travels some way, whichever way the board lay on the saw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Vector {
    pub l: i64,
    pub w: i64,
    pub t: i64,
}

impl Vector {
    pub const LENGTH: Vector = Vector { l: 1, w: 0, t: 0 };
    pub const WIDTH: Vector = Vector { l: 0, w: 1, t: 0 };
    pub const THICKNESS: Vector = Vector { l: 0, w: 0, t: 1 };

    /// The direction `(l, w, t)` points. Panics if all three are zero.
    pub fn new(l: i64, w: i64, t: i64) -> Vector {
        let divisor = gcd(gcd(l.unsigned_abs(), w.unsigned_abs()), t.unsigned_abs());
        assert!(divisor != 0, "a direction can't be zero");
        let divisor = divisor as i64;
        Vector {
            l: l / divisor,
            w: w / divisor,
            t: t / divisor,
        }
    }

    /// A direction in the board's face, square to its thickness.
    pub fn flat(direction: Direction) -> Vector {
        Vector {
            l: direction.l,
            w: direction.w,
            t: 0,
        }
    }

    /// A face facing `out` across the board's face and leaning `lean`, as an `Edge` has them.
    pub fn leaning(out: Direction, lean: i64) -> Vector {
        Vector {
            l: out.l,
            w: out.w,
            t: lean,
        }
    }

    pub fn reversed(self) -> Vector {
        Vector {
            l: -self.l,
            w: -self.w,
            t: -self.t,
        }
    }

    /// Its part across the board's face, and its part up the thickness: what an `Edge` facing
    /// this way has as `out` and `lean`.
    pub fn split(self) -> (Direction, i64) {
        (
            Direction {
                l: self.l,
                w: self.w,
            },
            self.t,
        )
    }

    /// The side facing this way at `at`, the wood behind it.
    pub(crate) fn edge(self, at: i128) -> Edge {
        let (out, lean) = self.split();
        Edge::leaning(out, lean, at)
    }

    /// How far `(l, w, t)` is along this direction, times its scale.
    pub(crate) fn dot(self, l: Length, w: Length, t: Length) -> i128 {
        self.l as i128 * l.as_nm() as i128
            + self.w as i128 * w.as_nm() as i128
            + self.t as i128 * t.as_nm() as i128
    }

    pub(crate) fn dot_vector(self, other: Vector) -> i128 {
        self.l as i128 * other.l as i128
            + self.w as i128 * other.w as i128
            + self.t as i128 * other.t as i128
    }

    /// The direction square to both, turning from this one toward `other`. Panics if they're
    /// parallel.
    pub(crate) fn cross(self, other: Vector) -> Vector {
        let (a, b) = (
            [self.l, self.w, self.t].map(i128::from),
            [other.l, other.w, other.t].map(i128::from),
        );
        let c = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        let divisor = c
            .iter()
            .fold(0u128, |g, &v| gcd128(g, v.unsigned_abs()))
            .max(1) as i128;
        assert!(
            c.iter().any(|&v| v != 0),
            "parallel directions have no square"
        );
        let [l, w, t] = c.map(|v| i64::try_from(v / divisor).expect("a direction too fine"));
        Vector { l, w, t }
    }

    pub(crate) fn scale_squared(self) -> i128 {
        self.dot_vector(self)
    }

    /// How long its whole-number components make it.
    pub fn scale(self) -> f64 {
        (self.scale_squared() as f64).sqrt()
    }

    /// Written so its biggest component is positive (the first of the length, width, and
    /// thickness, if two are the same size), and whether that took turning it around.
    pub(crate) fn canonical(self) -> (Vector, bool) {
        let parts = [self.l, self.w, self.t];
        let leading = parts
            .iter()
            .copied()
            .reduce(|a, b| if b.abs() > a.abs() { b } else { a })
            .expect("three parts");
        if leading < 0 {
            (self.reversed(), true)
        } else {
            (self, false)
        }
    }
}

fn gcd128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// A straight line across a board's face, and the side of it the wood is on: the points
/// `(l, w)` where `out · (l, w) <= at`, with `out` pointing away from the wood. `at` is in
/// nanometers times `out`'s scale, a whole number wherever the line is moved to.
///
/// That's a side square to the board's faces, the same all the way through. A side a tilted
/// blade left leans (`lean`): the wood is where `out · (l, w) + lean * t <= at`, so the side
/// comes in `lean / |out|` for each nanometer up. Then `(out, lean)` together have no common
/// factor, and `at` is in nanometers times their length.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Edge {
    pub out: Direction,
    pub lean: i64,
    pub at: i128,
}

impl Edge {
    pub(crate) fn new(out: Direction, at: i128) -> Edge {
        Edge { out, lean: 0, at }
    }

    /// A side facing `out` and leaning `lean`, which together have no common factor.
    pub(crate) fn leaning(out: Direction, lean: i64, at: i128) -> Edge {
        Edge { out, lean, at }
    }

    /// Whether the side leans, rather than standing square to the faces.
    pub fn leans(self) -> bool {
        self.lean != 0
    }

    /// The same line with the wood on its other side.
    pub(crate) fn flipped(self) -> Edge {
        Edge {
            out: self.out.reversed(),
            lean: -self.lean,
            at: -self.at,
        }
    }

    /// The same side, measured from a new corner at `(l, w, t)`.
    pub(crate) fn moved_by(self, l: Length, w: Length, t: Length) -> Edge {
        Edge {
            at: self.at - self.out.dot(l, w) - i128::from(self.lean) * t.as_nm() as i128,
            ..self
        }
    }

    /// Whether `(l, w, t)` is strictly on the wood's side.
    pub(crate) fn inside(self, l: Length, w: Length, t: Length) -> bool {
        self.out.dot(l, w) + i128::from(self.lean) * (t.as_nm() as i128) < self.at
    }

    /// Positive for a point beyond the line, away from the wood; zero on it; negative inside.
    fn beyond(self, point: Point) -> i128 {
        self.out.l as i128 * point.x + self.out.w as i128 * point.y - self.at * point.z
    }

    /// Where this edge's line crosses `other`'s, unless they're parallel.
    fn meet(self, other: Edge) -> Option<Point> {
        let (a1, b1, c1) = (self.out.l as i128, self.out.w as i128, self.at);
        let (a2, b2, c2) = (other.out.l as i128, other.out.w as i128, other.at);
        let z = a1 * b2 - a2 * b1;
        if z == 0 {
            return None;
        }
        let sign = z.signum();
        Some(Point {
            x: sign * (c1 * b2 - c2 * b1),
            y: sign * (a1 * c2 - a2 * c1),
            z: sign * z,
        })
    }
}

/// A point on the face where two lines cross, exactly: `x / z` along the length and `y / z`
/// across the width, in nanometers, with `z` positive.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Point {
    pub(crate) x: i128,
    pub(crate) y: i128,
    pub(crate) z: i128,
}

impl Point {
    fn l(self) -> f64 {
        self.x as f64 / self.z as f64
    }

    fn w(self) -> f64 {
        self.y as f64 / self.z as f64
    }

    /// How far the point is along `direction`, times its scale.
    pub(crate) fn along(self, direction: Direction) -> f64 {
        direction.l as f64 * self.l() + direction.w as f64 * self.w()
    }

    /// The same, exactly, as a fraction `(numerator, denominator)` with a positive denominator.
    pub(crate) fn along_exactly(self, direction: Direction) -> (i128, i128) {
        (
            direction.l as i128 * self.x + direction.w as i128 * self.y,
            self.z,
        )
    }
}

/// The largest whole number of nanometers not past `n / z`, and the smallest not short of it.
fn floor_div(n: i128, z: i128) -> i128 {
    n.div_euclid(z)
}

fn ceil_div(n: i128, z: i128) -> i128 {
    -(-n).div_euclid(z)
}

/// A convex region of a board's face, through the whole board: everything inside all of its
/// edges, which go counterclockwise around it (from the length toward the width).
///
/// With some of its sides leaning, it's a convex solid instead, narrower at one face than the
/// other: everything inside its sides between the board's bottom face and its top, which can
/// meet anywhere through the thickness. Its edges are then just the sides it has a face on, in
/// order by `Edge`'s ordering, and working with it takes the board's thickness (`solid.rs`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Region {
    edges: Vec<Edge>,
}

/// Far beyond any board, for building regions out of their edges alone.
const FAR: Length = Length::from_nm(1 << 40);

impl Region {
    /// The rectangle spanning `l` along the length and `w` across the width.
    pub(crate) fn rectangle(l: (Length, Length), w: (Length, Length)) -> Region {
        let nm = |length: Length| length.as_nm() as i128;
        Region::canonical(vec![
            Edge::new(Direction::WIDTH.reversed(), -nm(w.0)),
            Edge::new(Direction::LENGTH, nm(l.1)),
            Edge::new(Direction::WIDTH, nm(w.1)),
            Edge::new(Direction::LENGTH.reversed(), -nm(l.0)),
        ])
    }

    /// The region inside every one of `edges`, if it has any area.
    pub(crate) fn within(edges: impl IntoIterator<Item = Edge>) -> Option<Region> {
        edges.into_iter().try_fold(
            Region::rectangle((-FAR, FAR), (-FAR, FAR)),
            |region, edge| region.clip(edge),
        )
    }

    /// The convex region with `corners`, in order counterclockwise, if it has any area. Edges
    /// run straight between them.
    pub(crate) fn around(corners: &[(Length, Length)]) -> Option<Region> {
        let n = corners.len();
        let edges = (0..n).filter_map(|i| {
            let ((l0, w0), (l1, w1)) = (corners[i], corners[(i + 1) % n]);
            let (dl, dw) = ((l1 - l0).as_nm(), (w1 - w0).as_nm());
            // Counterclockwise, the wood is to the left; the outside, to the right.
            (dl != 0 || dw != 0).then(|| {
                let out = Direction::new(dw, -dl);
                Edge::new(out, out.dot(l0, w0))
            })
        });
        Region::within(edges)
    }

    /// A region with some sides leaning, from just the sides it has a face on.
    pub(crate) fn leaning(mut edges: Vec<Edge>) -> Region {
        edges.sort();
        Region { edges }
    }

    /// Whether any of its sides lean, so it's narrower at one face than the other.
    pub fn leans(&self) -> bool {
        self.edges.iter().any(|edge| edge.leans())
    }

    /// Starts the edges at the same one however the region was made, so equal regions compare
    /// equal.
    fn canonical(mut edges: Vec<Edge>) -> Region {
        let first = (0..edges.len())
            .min_by_key(|&i| edges[i])
            .expect("a region has edges");
        edges.rotate_left(first);
        Region { edges }
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// Where each edge starts: the corner it shares with the edge before it. Only for a region
    /// whose sides are square to the faces: with leaning sides, see `solid.rs`.
    pub(crate) fn points(&self) -> Vec<Point> {
        debug_assert!(
            !self.leans(),
            "a leaning region's corners depend on the thickness"
        );
        let n = self.edges.len();
        (0..n)
            .map(|i| {
                self.edges[(i + n - 1) % n]
                    .meet(self.edges[i])
                    .expect("neighboring edges cross")
            })
            .collect()
    }

    /// The region's corners in order around it, to the nearest nanometer: where each edge
    /// starts. With leaning sides, where they cross the bottom face (`t = 0`), if they do.
    pub fn corners(&self) -> Vec<(Length, Length)> {
        if self.leans() {
            let bottom = self.edges.iter().map(|edge| Edge::new(edge.out, edge.at));
            return Region::within(bottom).map_or(Vec::new(), |region| region.corners());
        }
        let nearest = |n: i128, z: i128| Length::from_nm(floor_div(2 * n + z, 2 * z) as i64);
        self.points()
            .into_iter()
            .map(|p| (nearest(p.x, p.z), nearest(p.y, p.z)))
            .collect()
    }

    /// The part of the region inside `cut`, if it has any area.
    pub(crate) fn clip(&self, cut: Edge) -> Option<Region> {
        let beyond: Vec<i128> = self.points().into_iter().map(|p| cut.beyond(p)).collect();
        if beyond.iter().all(|&b| b <= 0) {
            return Some(self.clone());
        }
        if beyond.iter().all(|&b| b >= 0) {
            return None;
        }
        // Edge i runs from corner i to corner i + 1. It keeps some length if either end is
        // strictly inside, and the cut closes the region off where it leaves through a corner
        // beyond the line, or through one on it toward an edge that's gone.
        let n = self.edges.len();
        let kept = |i: usize| beyond[i] < 0 || beyond[(i + 1) % n] < 0;
        let leaves = (0..n)
            .find(|&i| kept(i) && (!kept((i + 1) % n) || beyond[(i + 1) % n] > 0))
            .expect("a region the line crosses is left somewhere");
        let mut edges: Vec<Edge> = (1..=n)
            .map(|k| (leaves + k) % n)
            .filter(|&i| kept(i))
            .map(|i| self.edges[i])
            .collect();
        edges.push(cut);
        Some(Region::canonical(edges))
    }

    /// The same region, measured from a new corner at `(l, w, t)`.
    pub(crate) fn moved_by(&self, l: Length, w: Length, t: Length) -> Region {
        Region {
            edges: self
                .edges
                .iter()
                .map(|edge| edge.moved_by(l, w, t))
                .collect(),
        }
    }

    /// The smallest rectangle of whole nanometers around the region: its `l` and `w` spans.
    pub(crate) fn bounds(&self) -> ((Length, Length), (Length, Length)) {
        let points = self.points();
        let span = |n: fn(&Point) -> i128| {
            let low = points.iter().map(|p| floor_div(n(p), p.z)).min();
            let high = points.iter().map(|p| ceil_div(n(p), p.z)).max();
            let nm = |v: Option<i128>| Length::from_nm(v.expect("a region has corners") as i64);
            (nm(low), nm(high))
        };
        (span(|p| p.x), span(|p| p.y))
    }

    /// The range of `direction · (l, w)` over the region, times the direction's scale.
    pub(crate) fn extent(&self, direction: Direction) -> (f64, f64) {
        let (low, high) = self.extremes(direction);
        (low.along(direction), high.along(direction))
    }

    /// The corners furthest back and furthest forward along `direction`.
    pub(crate) fn extremes(&self, direction: Direction) -> (Point, Point) {
        let points = self.points();
        let along = |p: &&Point| p.along(direction);
        let low = points.iter().min_by(|a, b| along(a).total_cmp(&along(b)));
        let high = points.iter().max_by(|a, b| along(a).total_cmp(&along(b)));
        (
            *low.expect("a region has corners"),
            *high.expect("a region has corners"),
        )
    }

    /// Whether `(l, w)` is strictly inside the region.
    pub fn contains(&self, l: Length, w: Length) -> bool {
        self.edges.iter().all(|edge| edge.out.dot(l, w) < edge.at)
    }

    /// Whether the two regions share a stretch of an edge, so wood in one holds onto wood in
    /// the other. Touching at a corner doesn't hold.
    pub(crate) fn joins(&self, other: &Region) -> bool {
        let (n, m) = (self.edges.len(), other.edges.len());
        (0..n).any(|i| {
            let edge = self.edges[i];
            let Some(k) = other.edges.iter().position(|&e| e == edge.flipped()) else {
                return false;
            };
            // Each region's edge runs between its neighbors. Along the line, a neighbor bounds
            // it from below or from above; the two stretches overlap if every lower bound is
            // short of every upper one.
            let bounds = [
                self.edges[(i + n - 1) % n],
                self.edges[(i + 1) % n],
                other.edges[(k + m - 1) % m],
                other.edges[(k + 1) % m],
            ];
            let along = edge.out.square();
            let (lower, upper): (Vec<Edge>, Vec<Edge>) = bounds
                .into_iter()
                .partition(|bound| bound.out.dot_direction(along) < 0);
            lower.iter().all(|&low| {
                let point = edge.meet(low).expect("a neighboring edge crosses");
                upper.iter().all(|&high| high.beyond(point) < 0)
            })
        })
    }

    /// The two regions as one, if they share an edge and together they're convex.
    pub(crate) fn merged(&self, other: &Region) -> Option<Region> {
        let shared = *self
            .edges
            .iter()
            .find(|edge| other.edges.contains(&edge.flipped()))?;
        let inside = |region: &Region, edges: &[Edge], skip: Edge| {
            region.points().into_iter().all(|p| {
                edges
                    .iter()
                    .filter(|&&edge| edge != skip)
                    .all(|edge| edge.beyond(p) <= 0)
            })
        };
        if !inside(other, &self.edges, shared) || !inside(self, &other.edges, shared.flipped()) {
            return None;
        }
        Region::within(
            self.edges
                .iter()
                .chain(&other.edges)
                .filter(|&&edge| edge != shared && edge != shared.flipped())
                .copied(),
        )
    }
}

/// Joins regions of a board `thickness` thick that together are convex, until no more will
/// join.
pub(crate) fn merge_all(mut regions: Vec<Region>, thickness: Length) -> Vec<Region> {
    'again: loop {
        for i in 0..regions.len() {
            for j in i + 1..regions.len() {
                if let Some(merged) = regions[i].solid(thickness).merged(&regions[j]) {
                    regions[i] = merged;
                    regions.swap_remove(j);
                    continue 'again;
                }
            }
        }
        regions.sort();
        return regions;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inches(n: i64) -> Length {
        Length::from_inches(n)
    }

    fn square(size: i64) -> Region {
        Region::rectangle((Length::ZERO, inches(size)), (Length::ZERO, inches(size)))
    }

    /// The line `l + w = at` inches, with the wood on the side toward `(0, 0)`.
    fn diagonal(at: i64) -> Edge {
        Edge::new(Direction::new(1, 1), inches(at).as_nm() as i128)
    }

    #[test]
    fn a_rectangle_has_its_four_corners() {
        let mut corners = square(2).corners();
        corners.sort();
        assert_eq!(
            corners,
            [
                (Length::ZERO, Length::ZERO),
                (Length::ZERO, inches(2)),
                (inches(2), Length::ZERO),
                (inches(2), inches(2)),
            ]
        );
    }

    #[test]
    fn clipping_across_a_corner_leaves_a_triangle() {
        let triangle = square(2).clip(diagonal(1)).unwrap();
        let mut corners = triangle.corners();
        corners.sort();
        assert_eq!(
            corners,
            [
                (Length::ZERO, Length::ZERO),
                (Length::ZERO, inches(1)),
                (inches(1), Length::ZERO),
            ]
        );
        // The other side is the rest of the square.
        assert_eq!(
            square(2)
                .clip(diagonal(1).flipped())
                .unwrap()
                .corners()
                .len(),
            5
        );
    }

    #[test]
    fn clipping_along_an_edge_or_past_it_changes_nothing_or_leaves_nothing() {
        let along = Edge::new(Direction::LENGTH, inches(2).as_nm() as i128);
        assert_eq!(square(2).clip(along), Some(square(2)));
        assert_eq!(square(2).clip(along.flipped()), None);
        // Through a corner only.
        assert_eq!(square(2).clip(diagonal(4)), Some(square(2)));
        assert_eq!(square(2).clip(diagonal(0)), None);
    }

    #[test]
    fn a_hole_through_the_middle_leaves_two_sides_that_do_not_join() {
        let slab = Region::rectangle((inches(1), inches(2)), (-inches(1), inches(4)));
        let sides = Region::rectangle((Length::ZERO, inches(3)), (Length::ZERO, inches(3)))
            .solid(inches(1))
            .minus(slab.edges());
        assert_eq!(sides.len(), 2);
        assert!(!sides[0].joins(&sides[1]));
    }

    #[test]
    fn a_notch_leaves_regions_that_hold_together() {
        let notch = Region::rectangle((-inches(1), inches(1)), (inches(1), inches(2)));
        let rest = square(3).solid(inches(1)).minus(notch.edges());
        assert!(rest.len() > 1);
        let joined = (1..rest.len()).all(|i| (0..i).any(|j| rest[i].joins(&rest[j])));
        assert!(joined, "{rest:?}");
        // Together they're not convex, so they can't all merge back into one.
        assert!(merge_all(rest, inches(1)).len() > 1);
    }

    #[test]
    fn regions_that_only_touch_at_a_corner_do_not_join() {
        let below = Region::rectangle((Length::ZERO, inches(1)), (Length::ZERO, inches(1)));
        let above = Region::rectangle((inches(1), inches(2)), (inches(1), inches(2)));
        assert!(!below.joins(&above));
        let beside = Region::rectangle((inches(1), inches(2)), (Length::ZERO, inches(1)));
        assert!(below.joins(&beside));
    }

    #[test]
    fn halves_merge_back_into_the_whole() {
        let cut = diagonal(2);
        let halves = [
            square(2).clip(cut).unwrap(),
            square(2).clip(cut.flipped()).unwrap(),
        ];
        assert!(halves[0].joins(&halves[1]));
        assert_eq!(merge_all(halves.to_vec(), inches(1)), [square(2)]);
    }

    #[test]
    fn bounds_round_out_to_whole_nanometers() {
        // A third of the way across a slanted line lands between nanometers.
        let slanted = Edge::new(Direction::new(1, 3), inches(1).as_nm() as i128 + 1);
        let region = square(2).clip(slanted).unwrap();
        let ((l0, l1), (w0, w1)) = region.bounds();
        assert_eq!(
            (l0, w0, l1),
            (Length::ZERO, Length::ZERO, inches(1) + Length::from_nm(1))
        );
        assert!(w1.as_nm() * 3 > inches(1).as_nm());
        assert!((w1.as_nm() - 1) * 3 < inches(1).as_nm() + 1);
    }

    #[test]
    fn moving_keeps_the_shape() {
        let triangle = square(2).clip(diagonal(1)).unwrap();
        // Measured from (1", 1"), the triangle's corner at (0, 0) is at (-1", -1").
        let moved = triangle.moved_by(inches(1), inches(1), Length::ZERO);
        let nudge = Length::from_nm(1_000);
        assert!(moved.contains(nudge - inches(1), nudge - inches(1)));
        assert!(!moved.contains(-nudge, -nudge));
    }
}

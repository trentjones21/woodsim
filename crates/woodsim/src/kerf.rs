use crate::{
    Angle, Axis, Length,
    face::{Direction, Edge, Region, Vector},
    wide::Wide,
};

/// A slot a round saw blade cut into a board without cutting it apart: everything within the
/// blade's radius of the path its center took, between the slot's two faces. A stopped cut, a
/// groove, a dado, a rabbet, a miter saw's scar, a resaw cut that doesn't reach across, and a
/// bevel cut partway are all kerfs.
///
/// Positions are in the board's own coordinates: `l` along its length from one end, `w` across
/// its width from one edge, and `t` up through its thickness from its bottom face.
///
/// The blade turned in a plane, however the board lay on the saw: its faces are where
/// `normal · (l, w, t)` is `near` and `far`, and its center traveled `along`, in that plane,
/// from `start` to `end`. Up the plane, square to `along`, is `up = normal × along`, and the
/// center's path is where `up · (l, w, t)` is `center`. A blade standing square to the board's
/// face has a `normal` across the face, and its `up` runs up the thickness; one standing
/// parallel to the faces, resawing a board on edge, has its `normal` up the thickness; a
/// tilted one, anything between. Positions along a direction are in nanometers times its
/// scale, which keeps them whole numbers wherever the slot ends up.
///
/// The slot is stored as the saw left it, not trimmed to the board, so it stays exact when the
/// board is cut into pieces. It can reach past the board's edges and ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Kerf {
    /// Square to the slot's faces.
    pub normal: Vector,
    /// The slot's two faces, `near` < `far`: positions on `normal`.
    pub near: Length,
    pub far: Length,
    /// The way the blade's center traveled, in its plane.
    pub along: Vector,
    pub radius: Length,
    /// Where the center's path is up the blade's plane: a position on `up()`.
    pub center: i128,
    /// How far the blade's center traveled along the slot, `start` <= `end`.
    pub start: Length,
    pub end: Length,
}

/// What a slot's faces are square to, roughly: across the board's face, or its thickness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Across {
    /// A direction across the board's face, the faces' normal there: the blade stood square to
    /// the face, or tilted from square.
    Face(Direction),
    /// The board's thickness: the blade stood parallel to the faces, through a board on edge.
    Thickness,
}

/// Where a slot meets a plane through the board: between the lines its two faces make on the
/// plane, as far along them as the blade reached on both.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opening {
    /// The way the lines run, a unit vector `(l, w, t)`.
    pub direction: [f64; 3],
    /// Where each face's line passes nearest the board's corner, in nanometers `(l, w, t)`:
    /// square to `direction` from it.
    pub near: [f64; 3],
    pub far: [f64; 3],
    /// How far along the lines from those points the blade reached, in nanometers.
    pub from: f64,
    pub to: f64,
}

impl Opening {
    /// Its corners, in order around it, in nanometers `(l, w, t)`.
    pub fn corners(&self) -> [[f64; 3]; 4] {
        let at =
            |point: [f64; 3], along: f64| [0, 1, 2].map(|k| point[k] + along * self.direction[k]);
        [
            at(self.near, self.from),
            at(self.near, self.to),
            at(self.far, self.to),
            at(self.far, self.from),
        ]
    }
}

/// `(l, w, t)` as floating point.
fn float(v: Vector) -> [f64; 3] {
    [v.l as f64, v.w as f64, v.t as f64]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A unit vector along one of the board's axes.
pub(crate) fn unit(axis: Axis) -> [f64; 3] {
    match axis {
        Axis::Length => [1.0, 0.0, 0.0],
        Axis::Width => [0.0, 1.0, 0.0],
        Axis::Thickness => [0.0, 0.0, 1.0],
    }
}

/// Points on each round end of a slot, when it's outlined in its plane.
const ARC_STEPS: usize = 32;

fn nm(length: Length) -> i128 {
    length.as_nm() as i128
}

impl Kerf {
    #[cfg(test)]
    /// A slot between its faces at `near` and `far` on `across`, whose blade's center traveled
    /// from `along · (l, w) = start` to `end`. Standing square to the face, its center is
    /// `center` up the thickness; parallel to the faces, `center` along `along.square()`.
    pub(crate) fn new(
        across: Across,
        faces: (Length, Length),
        along: Direction,
        travel: (Length, Length),
        radius: Length,
        center: Length,
    ) -> Kerf {
        let normal = match across {
            Across::Face(direction) => Vector::flat(direction),
            Across::Thickness => Vector::THICKNESS,
        };
        let along = Vector::flat(along);
        let up = normal.cross(along);
        // Square to the face, up is up or down the thickness; parallel to it, along.square().
        let center = match across {
            Across::Face(_) => i128::from(up.t) * nm(center),
            Across::Thickness => nm(center),
        };
        Kerf::blade(normal, faces, along, travel, radius, center)
    }

    #[cfg(test)]
    /// A slot cut by a blade tilted from square to the face, whose faces lean `lean` (as an
    /// `Edge` does), its center's path `center` up the thickness on the plane halfway between
    /// them: `across` is square to `along`, and with `lean` has no common factor.
    pub(crate) fn tilted(
        across: Direction,
        lean: i64,
        faces: (Length, Length),
        along: Direction,
        travel: (Length, Length),
        radius: Length,
        center: Length,
    ) -> Kerf {
        let normal = Vector::leaning(across, lean);
        let along = Vector::flat(along);
        assert_eq!(
            normal.dot_vector(along),
            0,
            "a tilted blade's faces run along its path"
        );
        let up = normal.cross(along);
        // On the middle plane at that height: `up · c = up_t (|n|^2 t - n_t m) / |n_h|^2`.
        let across_squared = (normal.scale_squared() - i128::from(lean).pow(2)) as f64;
        let middle = (nm(faces.0) + nm(faces.1)) as f64 / 2.0;
        let raw = up.t as f64
            * (normal.scale_squared() as f64 * nm(center) as f64 - lean as f64 * middle)
            / across_squared;
        Kerf::blade(normal, faces, along, travel, radius, raw.round() as i128)
    }

    /// A slot cut by a blade turning in any plane: its faces at `near` and `far` on `normal`, its
    /// center traveling from `along · p = start` to `end`, `along` square to `normal`, on the
    /// path where `(normal × along) · p = center`. Each direction is written one way, so equal
    /// slots compare equal.
    pub(crate) fn blade(
        normal: Vector,
        (near, far): (Length, Length),
        along: Vector,
        (start, end): (Length, Length),
        radius: Length,
        center: i128,
    ) -> Kerf {
        assert_eq!(
            normal.dot_vector(along),
            0,
            "a blade travels in its own plane"
        );
        let turned = |(low, high): (Length, Length), reversed: bool| {
            if reversed { (-high, -low) } else { (low, high) }
        };
        let (normal, normal_reversed) = normal.canonical();
        let (along, along_reversed) = along.canonical();
        // Turning either around turns `up` around.
        let center = if normal_reversed != along_reversed {
            -center
        } else {
            center
        };
        let (near, far) = turned((near, far), normal_reversed);
        let (start, end) = turned((start, end), along_reversed);
        Kerf {
            normal,
            near,
            far,
            along,
            radius,
            center,
            start,
            end,
        }
    }

    /// The way the blade's center traveled across the board's face, for one that traveled
    /// across it.
    pub fn along_face(&self) -> Direction {
        Direction {
            l: self.along.l,
            w: self.along.w,
        }
    }

    /// Up the blade's plane, square to the way its center traveled.
    pub fn up(&self) -> Vector {
        self.normal.cross(self.along)
    }

    /// The point the blade reached furthest `toward` (any length), on the plane midway between
    /// the slot's faces, in nanometers `(l, w, t)`: an end of its center's path, out to the
    /// blade's edge.
    pub(crate) fn furthest(&self, toward: [f64; 3]) -> [f64; 3] {
        let (normal, along, up) = (float(self.normal), float(self.along), float(self.up()));
        let (n2, a2, u2) = (dot(normal, normal), dot(along, along), dot(up, up));
        let middle = (nm(self.near) + nm(self.far)) as f64 / 2.0;
        let path = |at: Length| {
            [0, 1, 2].map(|k| {
                middle * normal[k] / n2
                    + self.center as f64 * up[k] / u2
                    + at.as_nm() as f64 * along[k] / a2
            })
        };
        let (start, end) = (path(self.start), path(self.end));
        let center = if dot(start, toward) >= dot(end, toward) {
            start
        } else {
            end
        };
        // Toward it in the blade's plane.
        let out = [0, 1, 2].map(|k| toward[k] - dot(toward, normal) * normal[k] / n2);
        let size = dot(out, out).sqrt();
        if size == 0.0 {
            return center;
        }
        let radius = self.radius.as_nm() as f64;
        [0, 1, 2].map(|k| center[k] + radius * out[k] / size)
    }

    /// Where the slot meets the plane where the board's `axis` is `at` nanometers, or `None`
    /// if the blade didn't reach it between its faces, or its faces run the same way as the
    /// plane. Worked out to within floating point of a nanometer, for measuring and drawing.
    pub fn meets(&self, axis: Axis, at: f64) -> Option<Opening> {
        self.meets_plane(unit(axis), at)
    }

    /// Where the slot meets the plane `plane · p = at` (`p` in nanometers), as `meets` has it.
    pub fn meets_plane(&self, plane: [f64; 3], at: f64) -> Option<Opening> {
        let (near, direction) = self.line(plane, at, self.near.as_nm() as f64)?;
        let (far, _) = self.line(plane, at, self.far.as_nm() as f64)?;
        let (from_near, to_near) = self.reached(near, direction)?;
        let (from_far, to_far) = self.reached(far, direction)?;
        let (from, to) = (from_near.max(from_far), to_near.min(to_far));
        (from <= to).then_some(Opening {
            direction,
            near,
            far,
            from,
            to,
        })
    }

    /// Where a plane parallel to the slot's faces, `normal · p = face` (nanometers times the
    /// normal's length), meets the plane `plane · p = at`: the point on the line nearest the
    /// board's corner, and the unit vector it runs along. `None` if the planes run the same
    /// way.
    pub(crate) fn line(&self, plane: [f64; 3], at: f64, face: f64) -> Option<([f64; 3], [f64; 3])> {
        let normal = float(self.normal);
        let line = [
            normal[1] * plane[2] - normal[2] * plane[1],
            normal[2] * plane[0] - normal[0] * plane[2],
            normal[0] * plane[1] - normal[1] * plane[0],
        ];
        let (n2, g2, ng) = (dot(normal, normal), dot(plane, plane), dot(normal, plane));
        let size = dot(line, line).sqrt();
        if size < 1e-9 * (n2 * g2).sqrt() {
            return None;
        }
        // On both planes, square to the line: some of the normal and some of the other plane's.
        let determinant = n2 * g2 - ng * ng;
        let a = (face * g2 - at * ng) / determinant;
        let b = (at * n2 - face * ng) / determinant;
        Some((
            [0, 1, 2].map(|k| a * normal[k] + b * plane[k]),
            line.map(|v| v / size),
        ))
    }

    /// Along the line through `point` (nanometers) the way `direction` (a unit vector) runs,
    /// where the blade reached in its plane: within its radius of its center's path.
    pub(crate) fn reached(&self, point: [f64; 3], direction: [f64; 3]) -> Option<(f64, f64)> {
        let (along, up) = (float(self.along), float(self.up()));
        let (a, u) = (dot(along, along).sqrt(), dot(up, up).sqrt());
        // Along the path and up the plane from it, in nanometers, as `x + dx s` at `s`.
        let (x, dx) = (dot(along, point) / a, dot(along, direction) / a);
        let (y, dy) = (
            (dot(up, point) - self.center as f64) / u,
            dot(up, direction) / u,
        );
        let (start, end) = (self.start.as_nm() as f64 / a, self.end.as_nm() as f64 / a);
        let radius = self.radius.as_nm() as f64;
        // Where `(x + dx s - past)^2 + (y + dy s)^2 < r^2`, past the end of the path at `past`,
        // or beside it, with `past` where along it the line is.
        let within = |past: Option<f64>| -> Option<(f64, f64)> {
            let (x, dx) = match past {
                Some(past) => (x - past, dx),
                None => (0.0, 0.0),
            };
            let (qa, qb, qc) = (
                dx * dx + dy * dy,
                2.0 * (x * dx + y * dy),
                x * x + y * y - radius * radius,
            );
            if qa == 0.0 {
                return (qc < 0.0).then_some((f64::MIN, f64::MAX));
            }
            let discriminant = qb * qb - 4.0 * qa * qc;
            (discriminant > 0.0).then(|| {
                let root = discriminant.sqrt();
                ((-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa))
            })
        };
        // Where along the line it's before the path's start, beside it, and past its end.
        let between = |low: f64, high: f64| -> (f64, f64) {
            if dx == 0.0 {
                return if low <= x && x <= high {
                    (f64::MIN, f64::MAX)
                } else {
                    (0.0, -1.0)
                };
            }
            let ends = [(low - x) / dx, (high - x) / dx];
            (ends[0].min(ends[1]), ends[0].max(ends[1]))
        };
        [
            (within(Some(start)), between(f64::MIN, start)),
            (within(None), between(start, end)),
            (within(Some(end)), between(end, f64::MAX)),
        ]
        .into_iter()
        .filter_map(|(reached, (low, high))| {
            let (from, to) = reached?;
            let (from, to) = (from.max(low), to.min(high));
            (from <= to).then_some((from, to))
        })
        .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
    }

    /// What the slot's faces are square to, roughly.
    pub fn across(&self) -> Across {
        let (across, _) = self.normal.split();
        if across.l == 0 && across.w == 0 {
            Across::Thickness
        } else {
            Across::Face(across)
        }
    }

    /// Whether the blade stood square to the board's face, its faces straight through it and
    /// its center traveling across the face: a slot in the face, worked out the way it always
    /// was.
    fn square_to_face(&self) -> bool {
        self.normal.t == 0 && self.along.t == 0
    }

    /// Whether the blade stood parallel to the board's faces, resawing it.
    fn parallel_to_faces(&self) -> bool {
        self.normal.l == 0 && self.normal.w == 0
    }

    /// How far the slot is turned from running square across the board, or straight along it:
    /// the angle the saw was set to.
    pub fn slant(&self) -> Angle {
        let (l, w) = (self.along.l.abs() as f64, self.along.w.abs() as f64);
        Angle::nearest(l.min(w).atan2(l.max(w)))
    }

    /// The slot's width, square to its faces.
    pub fn width(&self) -> Length {
        let scaled = (self.far - self.near).as_nm() as f64;
        Length::from_nm((scaled / self.normal.scale()).round() as i64)
    }

    /// How far the blade was tilted from square to the board's face: zero for one standing
    /// square to it, or parallel to it.
    pub fn tilt(&self) -> Angle {
        if self.parallel_to_faces() {
            return Angle::ZERO;
        }
        let (across, lean) = self.normal.split();
        Angle::nearest((lean.abs() as f64).atan2(across.scale()))
    }

    /// The faces of the slot, as sides with the slot inside them.
    pub(crate) fn faces(&self) -> [Edge; 2] {
        [
            self.normal.reversed().edge(-nm(self.near)),
            self.normal.edge(nm(self.far)),
        ]
    }

    /// Where the center's path is up the board's thickness, for a blade that traveled across
    /// the face: exactly, for one standing square to it.
    fn center_height(&self) -> Option<f64> {
        if self.along.t != 0 {
            return None;
        }
        let up = self.up();
        if self.square_to_face() {
            return Some((i128::from(up.t) * self.center) as f64);
        }
        // On the middle plane: the center's part up the normal, and up `up`.
        let middle = (nm(self.near) + nm(self.far)) as f64 / 2.0;
        Some(
            middle * self.normal.t as f64 / self.normal.scale_squared() as f64
                + self.center as f64 * up.t as f64 / up.scale_squared() as f64,
        )
    }

    /// For a slot square to the face, its center's height, exactly.
    fn square_center(&self) -> Length {
        Length::from_nm((i128::from(self.up().t) * self.center) as i64)
    }

    /// Whether any of the slot is inside `region` of a board `thickness` thick. A blade that
    /// only grazes a face takes no wood. Where the slot or the wood leans, it's worked out
    /// to within floating point of a nanometer.
    pub(crate) fn touches(&self, region: &Region, thickness: Length) -> bool {
        if self.parallel_to_faces() && !region.leans() {
            return self.near < thickness
                && self.far > Length::ZERO
                && self
                    .outline()
                    .is_some_and(|outline| region.solid(thickness).overlaps(outline.edges()));
        }
        if !self.square_to_face() || region.leans() {
            return self.touches_solid(region, thickness);
        }
        let center = self.square_center();
        let along = Direction {
            l: self.along.l,
            w: self.along.w,
        };
        let up = gap(center, center, Length::ZERO, thickness);
        if up >= self.radius {
            return false;
        }
        let Some(inside) = region.solid(thickness).clip_all(self.faces()) else {
            return false;
        };
        // How far past either end of the blade's path along the slot the wood is, against
        // how far the blade reached at the height of the nearest face. Both are worked out
        // exactly unless the numbers get too big, where they're within a billionth of a
        // nanometer.
        let (low, high) = inside.extremes(along);
        let reach = (self.radius.squared() - up.squared()) * along.scale_squared();
        let within = |(off, over): (i128, i128)| {
            off <= 0
                || match (
                    off.checked_mul(off),
                    reach.checked_mul(over).and_then(|r| r.checked_mul(over)),
                ) {
                    (Some(off), Some(reach)) => off < reach,
                    _ => (off as f64 / over as f64).powi(2) < reach as f64,
                }
        };
        let (low, over_low) = low.along_exactly(along);
        let (high, over_high) = high.along_exactly(along);
        within((low - nm(self.end) * over_low, over_low))
            && within((nm(self.start) * over_high - high, over_high))
    }

    /// Whether any of the slot is inside `region`, worked out in the plane the blade turned
    /// in: where the wood between the slot's faces lies in that plane, against where the
    /// blade swept.
    fn touches_solid(&self, region: &Region, thickness: Length) -> bool {
        let Some(inside) = region.solid(thickness).clip_all(self.faces()) else {
            return false;
        };
        let points: Vec<(f64, f64)> = inside
            .solid(thickness)
            .corners()
            .into_iter()
            .map(|corner| self.in_plane(corner.position()))
            .collect();
        let scale = self.along.scale();
        let path = (
            self.start.as_nm() as f64 / scale,
            self.end.as_nm() as f64 / scale,
        );
        near_path(&hull(points), path, self.radius.as_nm() as f64)
    }

    /// Where `p` is in the blade's plane, in nanometers: along the slot, and up from its
    /// center's path.
    fn in_plane(&self, p: [f64; 3]) -> (f64, f64) {
        let dot = |v: Vector| v.l as f64 * p[0] + v.w as f64 * p[1] + v.t as f64 * p[2];
        let up = self.up();
        (
            dot(self.along) / self.along.scale(),
            (dot(up) - self.center as f64) / up.scale(),
        )
    }

    /// For a slot square to the face, where along the slot it goes all the way through a board
    /// `thickness` thick, or `None` if it doesn't anywhere. The blade swept a convex region, so
    /// it goes through wherever it reached both faces.
    fn through_span(&self, thickness: Length) -> Option<(Length, Length)> {
        let (bottom_start, bottom_end) = self.span_at(Length::ZERO)?;
        let (top_start, top_end) = self.span_at(thickness)?;
        let (start, end) = (bottom_start.max(top_start), bottom_end.min(top_end));
        (start <= end).then_some((start, end))
    }

    /// Where the slot goes all the way through `region` of a board `thickness` thick, as the
    /// planes around it: the wood inside all of them is gone. It goes through between its faces
    /// wherever the blade reached all the wood between them, up its plane.
    pub(crate) fn hole(&self, region: &Region, thickness: Length) -> Option<Vec<Edge>> {
        if self.parallel_to_faces() && !region.leans() {
            // Only one wider than the board is thick takes all of it.
            return (self.near <= Length::ZERO && self.far >= thickness)
                .then(|| self.outline())
                .flatten()
                .map(|outline| outline.edges().to_vec());
        }
        let [face_near, face_far] = self.faces();
        let ends = |(start, end): (Length, Length)| {
            [
                self.along.edge(nm(end)),
                self.along.reversed().edge(-nm(start)),
            ]
        };
        if self.square_to_face() && !region.leans() {
            let [end, start] = ends(self.through_span(thickness)?);
            return Region::within([face_near, face_far, end, start])
                .map(|hole| hole.edges().to_vec());
        }
        // How far up and down the plane from the center's path the wood between the faces
        // reaches; the blade took all of it where it reached that far.
        let inside = region.solid(thickness).clip_all(self.faces())?;
        let reach = inside
            .solid(thickness)
            .corners()
            .into_iter()
            .map(|corner| self.in_plane(corner.position()).1.abs())
            .fold(0.0, f64::max);
        let radius = self.radius.as_nm() as f64;
        if reach >= radius {
            return None;
        }
        let half = ((radius * radius - reach * reach).sqrt() * self.along.scale()).floor() - 1.0;
        let half = Length::from_nm(half.max(0.0) as i64);
        let [end, start] = ends((self.start - half, self.end + half));
        Some(vec![face_near, face_far, end, start])
    }

    /// Where a slot parallel to the faces reaches on the board's face, as a region just inside
    /// the round ends the blade left: its outline is straight between points on them. `None`
    /// for any other slot.
    pub(crate) fn outline(&self) -> Option<Region> {
        if !self.parallel_to_faces() {
            return None;
        }
        let points: Vec<(Length, Length)> = self
            .swept()
            .into_iter()
            .map(|[l, w, _]| {
                (
                    Length::from_nm(l.round() as i64),
                    Length::from_nm(w.round() as i64),
                )
            })
            .collect();
        Region::around(&points)
    }

    /// Points around where the blade swept, just inside its round ends, on the plane halfway
    /// between its faces, in nanometers: counterclockwise from `along` toward `up`.
    pub(crate) fn swept(&self) -> Vec<[f64; 3]> {
        let up = self.up();
        let scale = self.along.scale();
        let radius = self.radius.as_nm() as f64 * scale;
        let (u0, u1, v) = (
            self.start.as_nm() as f64,
            self.end.as_nm() as f64,
            self.center as f64 * scale / up.scale(),
        );
        // Positions along and up the plane are in nanometers times `along`'s scale.
        let half = std::f64::consts::PI;
        let arc = |middle: f64, from: f64| {
            (0..=ARC_STEPS).map(move |step| {
                let angle = from + half * step as f64 / ARC_STEPS as f64;
                (middle + radius * angle.cos(), v + radius * angle.sin())
            })
        };
        let squared = scale * scale;
        let up_scale = up.scale() / scale;
        let middle =
            (nm(self.near) + nm(self.far)) as f64 / 2.0 / self.normal.scale_squared() as f64;
        arc(u1, -half / 2.0)
            .chain(arc(u0, half / 2.0))
            .map(|(u, v)| {
                // Back from scaled positions to (l, w, t).
                [
                    (u * self.along.l as f64 + v * up.l as f64 / up_scale) / squared
                        + middle * self.normal.l as f64,
                    (u * self.along.w as f64 + v * up.w as f64 / up_scale) / squared
                        + middle * self.normal.w as f64,
                    (u * self.along.t as f64 + v * up.t as f64 / up_scale) / squared
                        + middle * self.normal.t as f64,
                ]
            })
            .collect()
    }

    /// For a slot that ran across the face, how far it runs along the board at height `t`, or
    /// `None` if the blade didn't reach it. The round blade cuts a stopped slot longer on the
    /// face it came through than on the other. Rounded in to the nanometer, times the scale of
    /// `along`. Tilted, the blade reaches further on one face of the slot than the other; this
    /// is as far as it reached on both, to within a nanometer short.
    pub fn span_at(&self, t: Length) -> Option<(Length, Length)> {
        if self.parallel_to_faces() || self.along.t != 0 {
            return None;
        }
        if self.square_to_face() {
            let up = (t - self.square_center()).squared();
            let reach = self.radius.squared();
            return (up <= reach).then(|| {
                let half = ((reach - up) * self.along.scale_squared()).isqrt();
                let half = Length::from_nm(half as i64);
                (self.start - half, self.end + half)
            });
        }
        // On a face at height `t`, `up · p` is `up_t (|n|^2 t - n_t face) / |n_h|^2`.
        let up = self.up();
        let (n2, lean) = (self.normal.scale_squared() as f64, self.normal.t as f64);
        let across = n2 - lean * lean;
        let radius = self.radius.as_nm() as f64;
        let height = t.as_nm() as f64;
        let half = [self.near, self.far]
            .into_iter()
            .map(|face| {
                let at = up.t as f64 * (n2 * height - lean * face.as_nm() as f64) / across;
                let off = (at - self.center as f64) / up.scale();
                (off.abs() <= radius).then(|| (radius * radius - off * off).sqrt())
            })
            .collect::<Option<Vec<f64>>>()?
            .into_iter()
            .fold(f64::MAX, f64::min);
        let half = (half * self.along.scale()).floor() as i64 - 1;
        let half = Length::from_nm(half.max(0));
        Some((self.start - half, self.end + half))
    }

    /// Whether the point `(l, w, t)` is strictly inside the slot, where the blade took the wood.
    pub fn contains(&self, l: Length, w: Length, t: Length) -> bool {
        let face = self.normal.dot(l, w, t);
        if !(nm(self.near) < face && face < nm(self.far)) {
            return false;
        }
        let along = self.along.dot(l, w, t);
        let along = (nm(self.start) - along).max(along - nm(self.end)).max(0);
        let up = self.up();
        let off = up.dot(l, w, t) - self.center;
        // Along over `|along|` and up over `|up|`, squared, against the radius squared.
        let (a2, u2) = (self.along.scale_squared(), up.scale_squared());
        let r = nm(self.radius);
        Wide::product(u2, along * along) + Wide::product(off, off).times(a2)
            < Wide::product(r * r * a2, u2)
    }

    /// For a slot that ran across the face, whether the blade came up through the board's
    /// bottom face, rather than down through its top.
    pub fn from_below(&self) -> bool {
        self.center_height().is_some_and(|height| height < 0.0)
    }

    /// For a slot that ran across the face, how deep it goes into a board `thickness` thick,
    /// from the face the blade came through. At the thickness or more, it goes through. A
    /// tilted blade reaches less high.
    pub fn depth(&self, thickness: Length) -> Length {
        if self.square_to_face() {
            let center = self.square_center();
            return if self.from_below() {
                center + self.radius
            } else {
                thickness - (center - self.radius)
            };
        }
        let up = self.up();
        let reach = self.radius.as_nm() as f64 * (up.t as f64).abs() / up.scale();
        let height = self.center_height().unwrap_or(0.0);
        let depth = if height < 0.0 {
            height + reach
        } else {
            thickness.as_nm() as f64 - (height - reach)
        };
        Length::from_nm(depth.round() as i64)
    }

    /// The same slot, measured from a new corner at `(l, w, t)`: where it sits in a piece cut
    /// from the board.
    pub(crate) fn moved_by(self, l: Length, w: Length, t: Length) -> Kerf {
        let shift = |direction: Vector| Length::from_nm(direction.dot(l, w, t) as i64);
        let (across, along) = (shift(self.normal), shift(self.along));
        Kerf {
            near: self.near - across,
            far: self.far - across,
            start: self.start - along,
            end: self.end - along,
            center: self.center - self.up().dot(l, w, t),
            ..self
        }
    }
}

/// The convex hull of `points`, counterclockwise.
fn hull(mut points: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    points.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let turn = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };
    let mut around: Vec<(f64, f64)> = Vec::new();
    for pass in [false, true] {
        let start = around.len();
        let ordered: Box<dyn Iterator<Item = &(f64, f64)>> = if pass {
            Box::new(points.iter().rev())
        } else {
            Box::new(points.iter())
        };
        for &p in ordered {
            while around.len() >= start + 2
                && turn(around[around.len() - 2], around[around.len() - 1], p) <= 0.0
            {
                around.pop();
            }
            around.push(p);
        }
        around.pop();
    }
    around
}

/// Whether the convex polygon `around` comes strictly within `radius` of the path from
/// `(start, 0)` to `(end, 0)`.
fn near_path(around: &[(f64, f64)], (start, end): (f64, f64), radius: f64) -> bool {
    let n = around.len();
    if n == 0 {
        return false;
    }
    let inside = n >= 3
        && (0..n).all(|i| {
            let (a, b) = (around[i], around[(i + 1) % n]);
            (b.0 - a.0) * (0.0 - a.1) - (b.1 - a.1) * (start - a.0) >= 0.0
        });
    if inside {
        return true;
    }
    let path = ((start, 0.0), (end, 0.0));
    let edges = (0..n).map(|i| (around[i], around[(i + 1) % n]));
    edges
        .map(|edge| segment_gap(path, edge))
        .fold(f64::MAX, f64::min)
        < radius
}

/// The distance between two segments.
fn segment_gap((p0, p1): ((f64, f64), (f64, f64)), (q0, q1): ((f64, f64), (f64, f64))) -> f64 {
    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };
    let (d1, d2) = (cross(q0, q1, p0), cross(q0, q1, p1));
    let (d3, d4) = (cross(p0, p1, q0), cross(p0, p1, q1));
    if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
        return 0.0;
    }
    [
        point_gap(p0, (q0, q1)),
        point_gap(p1, (q0, q1)),
        point_gap(q0, (p0, p1)),
        point_gap(q1, (p0, p1)),
    ]
    .into_iter()
    .fold(f64::MAX, f64::min)
}

/// The distance from `p` to the segment from `a` to `b`.
fn point_gap(p: (f64, f64), (a, b): ((f64, f64), (f64, f64))) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let s = if length == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length).clamp(0.0, 1.0)
    };
    let (x, y) = (a.0 + s * dx - p.0, a.1 + s * dy - p.1);
    (x * x + y * y).sqrt()
}

/// The distance between the spans `a_low..a_high` and `b_low..b_high`, or zero if they overlap.
fn gap(a_low: Length, a_high: Length, b_low: Length, b_high: Length) -> Length {
    if a_high < b_low {
        b_low - a_high
    } else if b_high < a_low {
        a_low - b_high
    } else {
        Length::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inches(n: i64) -> Length {
        Length::from_inches(n)
    }

    /// A 1/8" slot along the board, cut by a 10" blade with its center `center` up, its center
    /// moving from 6" before the board's end to 10" along it.
    fn cut_with_center(center: Length) -> Kerf {
        Kerf::new(
            Across::Face(Direction::WIDTH),
            (inches(1), inches(1) + Length::from_inch_fraction(1, 8)),
            Direction::LENGTH,
            (-inches(6), inches(10)),
            inches(5),
            center,
        )
    }

    /// The same, raised 1" through a board 3/4" thick.
    fn stopped_cut() -> Kerf {
        cut_with_center(inches(1) - inches(5))
    }

    fn board(length: i64, width: i64) -> Region {
        Region::rectangle(
            (Length::ZERO, inches(length)),
            (Length::ZERO, inches(width)),
        )
    }

    #[test]
    fn a_stopped_cut_runs_further_along_the_bottom_than_the_top() {
        let kerf = stopped_cut();
        // 4" below the bottom face, a 5" blade reaches 3" either side of its center.
        assert_eq!(kerf.span_at(Length::ZERO).unwrap().1, inches(13));
        let (_, top) = kerf.span_at(Length::from_inch_fraction(3, 4)).unwrap();
        assert!(inches(11) < top && top < inches(12), "{top}");
    }

    #[test]
    fn a_blade_reaches_no_deeper_than_its_height() {
        let kerf = stopped_cut();
        let thickness = Length::from_inch_fraction(3, 4);
        assert_eq!(kerf.depth(thickness), inches(1));
        assert!(kerf.span_at(inches(1)).is_some());
        assert_eq!(kerf.span_at(inches(1) + Length::from_nm(1)), None);
    }

    #[test]
    fn a_blade_from_above_measures_its_depth_from_the_top() {
        let thickness = Length::from_inch_fraction(3, 4);
        // The same blade, 1/4" into the top face.
        let kerf = cut_with_center(thickness - Length::from_inch_fraction(1, 4) + inches(5));
        assert!(!kerf.from_below());
        assert_eq!(kerf.depth(thickness), Length::from_inch_fraction(1, 4));
    }

    #[test]
    fn goes_through_only_where_it_reached_both_faces() {
        let kerf = stopped_cut();
        // The blade reaches the top of a 3/4" board for less distance than the bottom, so the
        // top decides where the slot goes through.
        let thickness = Length::from_inch_fraction(3, 4);
        assert_eq!(kerf.through_span(thickness), kerf.span_at(thickness));
        // A blade 1" up never goes through a board 1-1/4" thick.
        assert_eq!(
            kerf.through_span(inches(1) + Length::from_inch_fraction(1, 4)),
            None
        );
    }

    #[test]
    fn touches_only_boards_the_slot_is_inside() {
        let kerf = stopped_cut();
        let thickness = Length::from_inch_fraction(3, 4);
        assert!(kerf.touches(&board(24, 6), thickness));
        // The slot is 1" in from the edge, so a board 1" wide is untouched.
        assert!(!kerf.touches(&board(24, 1), thickness));
        let lowered = cut_with_center(-inches(5));
        assert!(!lowered.touches(&board(24, 6), thickness));
    }

    #[test]
    fn touches_a_board_its_blade_reaches_by_a_nanometer() {
        let thickness = Length::from_inch_fraction(3, 4);
        // The blade's 3" reach along the bottom face stops a nanometer into a board starting
        // 13" - 1 nm along, and exactly at one starting at 13".
        let kerf = stopped_cut();
        let start = |at: Length| Region::rectangle((at, inches(24)), (Length::ZERO, inches(6)));
        assert!(kerf.touches(&start(inches(13) - Length::from_nm(1)), thickness));
        assert!(!kerf.touches(&start(inches(13)), thickness));
    }

    #[test]
    fn a_slot_across_the_board_measures_its_faces_along_the_length() {
        let dado = Kerf::new(
            Across::Face(Direction::LENGTH),
            (inches(1), inches(1) + Length::from_inch_fraction(1, 8)),
            Direction::WIDTH,
            (-inches(6), inches(10)),
            inches(5),
            inches(1) - inches(5),
        );
        let thickness = Length::from_inch_fraction(3, 4);
        // Its faces are 1" along the board, so a board 1" long is untouched by it.
        assert!(dado.touches(&board(24, 12), thickness));
        assert!(!dado.touches(&board(1, 12), thickness));
    }

    #[test]
    fn moving_to_a_new_corner_shifts_the_slot() {
        let kerf = stopped_cut().moved_by(inches(2), inches(1), Length::ZERO);
        assert_eq!(
            (kerf.near, kerf.start, kerf.end),
            (Length::ZERO, -inches(8), inches(8))
        );
    }

    #[test]
    fn a_slot_written_backward_is_turned_around() {
        let backward = Kerf::new(
            Across::Face(Direction::WIDTH.reversed()),
            (-inches(2), -inches(1)),
            Direction::LENGTH,
            (Length::ZERO, inches(1)),
            inches(5),
            -inches(4),
        );
        assert_eq!(backward.across(), Across::Face(Direction::WIDTH));
        assert_eq!((backward.near, backward.far), (inches(1), inches(2)));
    }

    #[test]
    fn a_slanted_slot_is_as_wide_as_the_blade_square_to_its_faces() {
        let kerf = Length::from_inch_fraction(1, 8);
        let across = Direction::new(1, 1);
        let scaled = Length::from_nm((kerf.as_nm() as f64 * across.scale()).round() as i64);
        let slanted = Kerf::new(
            Across::Face(across),
            (inches(10), inches(10) + scaled),
            across.square(),
            (Length::ZERO, Length::ZERO),
            inches(6),
            inches(6),
        );
        assert_eq!(slanted.width(), kerf);
    }

    #[test]
    fn a_resaw_slot_is_outlined_on_the_face_with_round_ends() {
        // A 10" blade raised 2" into the bottom edge of a board on edge, its center fed from 2"
        // to 10" along it, slotting the thickness from 1/4" to 3/8".
        let resaw = Kerf::new(
            Across::Thickness,
            (
                Length::from_inch_fraction(1, 4),
                Length::from_inch_fraction(3, 8),
            ),
            Direction::LENGTH,
            (inches(2), inches(10)),
            inches(5),
            inches(2) - inches(5),
        );
        let outline = resaw.outline().unwrap();
        // Between where the center traveled, it reaches 2" up from the edge.
        assert!(outline.contains(inches(6), inches(2) - Length::from_nm(1_000)));
        assert!(!outline.contains(inches(6), inches(2) + Length::from_nm(1_000)));
        // Past the end, the blade's circle reached 4" further along the edge, but less high.
        assert!(outline.contains(inches(13), Length::from_nm(1_000)));
        assert!(!outline.contains(inches(13), inches(1)));
        let thickness = Length::from_inch_fraction(3, 4);
        assert!(resaw.touches(&board(24, 6), thickness));
        assert!(!resaw.touches(&board(24, 6), Length::from_inch_fraction(1, 4)));
    }

    /// A 1/8" slot along the board by a 10" blade tilted 45° and raised 1", its faces crossing
    /// the bottom face 2" across, its center moving from 6" before the board's end to 10"
    /// along it.
    fn tilted_cut() -> Kerf {
        let (across, lean) = Direction::WIDTH.tilted(Angle::from_degrees(45));
        let kerf = Length::from_inch_fraction(1, 8);
        let width = Length::from_nm((kerf.as_nm() as f64 * 2f64.sqrt()).round() as i64);
        let reach = Length::from_nm((inches(5).as_nm() as f64 / 2f64.sqrt()).round() as i64);
        Kerf::tilted(
            across,
            lean,
            (inches(2), inches(2) + width),
            Direction::LENGTH,
            (-inches(6), inches(10)),
            inches(5),
            inches(1) - reach,
        )
    }

    #[test]
    fn a_tilted_slot_is_as_wide_as_the_blade_and_reaches_less_high() {
        let kerf = tilted_cut();
        assert_eq!(kerf.width(), Length::from_inch_fraction(1, 8));
        assert_eq!(kerf.tilt(), Angle::from_degrees(45));
        let depth = kerf.depth(Length::from_inch_fraction(3, 4));
        assert!((depth - inches(1)).as_nm().abs() <= 1, "{depth}");
        // Moved up a layer, its faces lean the same and stay where they were.
        let up = kerf.moved_by(Length::ZERO, Length::ZERO, Length::from_inch_fraction(1, 4));
        assert_eq!(up.far - up.near, kerf.far - kerf.near);
        assert_eq!(up.near, kerf.near - Length::from_inch_fraction(1, 4));
    }

    #[test]
    fn a_tilted_slot_holds_what_its_blade_swept_between_its_faces() {
        let kerf = tilted_cut();
        let t = Length::from_inch_fraction(3, 8);
        // Halfway between the faces at that height, `w + t` is 2" and a half kerf.
        let middle = |l: Length, t: Length| {
            let w = inches(2) + Length::from_nm((kerf.far - kerf.near).as_nm() / 2) - t;
            kerf.contains(l, w, t)
        };
        assert!(middle(inches(5), t));
        assert!(!kerf.contains(inches(5), inches(2) - t, t));
        assert!(!kerf.contains(inches(5), inches(2) + inches(1) - t, t));
        // Past the end of its path, the blade reached further along low down than high up:
        // 3.4" past at 1/32" up, 2.8" at 3/8".
        assert!(middle(inches(13), Length::from_inch_fraction(1, 32)));
        assert!(!middle(inches(13), t));
        assert!(middle(inches(12), t));
        assert!(!middle(inches(20), Length::from_inch_fraction(1, 32)));
        let (_, bottom) = kerf.span_at(Length::ZERO).unwrap();
        let (_, top) = kerf.span_at(Length::from_inch_fraction(3, 4)).unwrap();
        assert!(inches(10) < top && top < bottom, "{top} {bottom}");
    }
}

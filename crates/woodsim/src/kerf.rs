use crate::{
    Angle, Length,
    face::{Direction, Edge, Region},
    wide::Wide,
};

/// A slot a round saw blade cut into a board without cutting it apart: everything within the
/// blade's radius of the path its center took, between the slot's two faces. A stopped cut, a
/// groove, a dado, a rabbet, a miter saw's scar, and a resaw cut that doesn't reach across are
/// all kerfs.
///
/// Positions are in the board's own coordinates: `l` along its length from one end, `w` across
/// its width from one edge, and `t` up through its thickness from its bottom face.
///
/// Most slots are cut by a blade standing square to the board's face (`Across::Face`): the slot
/// runs in some direction across the face (along it for a rip or a groove, across it for a
/// crosscut or a dado, or slanted for a miter), and the blade came at it through the bottom
/// face or the top one. A blade standing parallel to the faces, through a board on edge, cuts a
/// slot square to the thickness (`Across::Thickness`), coming in through an edge.
///
/// Positions along the slot are `along · (l, w)`, and across a slot in the face,
/// `across · (l, w)`. For a slot square to the board, the directions are the board's axes and
/// these are plain positions on them; for a slanted one, they're nanometers times the
/// direction's `scale`, which keeps them whole numbers wherever the slot ends up.
///
/// A blade tilted from square to the face leaves a slot whose faces lean (`lean`): they're
/// where `across · (l, w) + lean * t` is `near` and `far`, `(across, lean)` with no common
/// factor. The blade turned in the plane halfway between them, about a center moving along the
/// slot `center` up, and `across` is square to `along`.
///
/// The slot is stored as the saw left it, not trimmed to the board, so it stays exact when the
/// board is cut into pieces. It can reach past the board's edges and ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Kerf {
    /// What the slot's faces are square to.
    pub across: Across,
    /// How far the faces of a slot in the face lean, for each nanometer up, times the length of
    /// `across`: zero for a blade square to the face.
    pub lean: i64,
    /// The way the slot runs across the board's face, which the blade's center traveled.
    pub along: Direction,
    /// The slot's two faces, `near` < `far`: positions on `across`, or `t` for a slot square to
    /// the thickness.
    pub near: Length,
    pub far: Length,
    pub radius: Length,
    /// Where the blade's center is, square to `along` in the plane the blade turns in. For a
    /// slot in the face, its `t`: below zero when the blade came up through the bottom face,
    /// past the thickness when it came down through the top. For a slot square to the
    /// thickness, its position on `along.square()`, the face direction square to `along`.
    pub center: Length,
    /// How far the blade's center traveled along the slot, `start` <= `end`.
    pub start: Length,
    pub end: Length,
}

/// What a slot's faces are square to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Across {
    /// A direction across the board's face: the blade stood square to the face.
    Face(Direction),
    /// The board's thickness: the blade stood parallel to the faces, through a board on edge.
    Thickness,
}

/// Points on each round end of a slot square to the thickness, when it's outlined on the face.
const ARC_STEPS: usize = 32;

impl Kerf {
    /// A slot between its faces at `near` and `far` on `across`, whose blade's center traveled
    /// from `along · (l, w) = start` to `end`. Each direction is written one way, so equal slots
    /// compare equal.
    pub(crate) fn new(
        across: Across,
        faces: (Length, Length),
        along: Direction,
        travel: (Length, Length),
        radius: Length,
        center: Length,
    ) -> Kerf {
        Kerf::make(across, 0, faces, along, travel, radius, center)
    }

    /// A slot cut by a blade tilted from square to the face, whose faces lean `lean` (see
    /// `Kerf`): `across` is square to `along`, and with `lean` has no common factor.
    #[cfg_attr(not(test), expect(dead_code, reason = "no saw tilts its blade yet"))]
    pub(crate) fn tilted(
        across: Direction,
        lean: i64,
        faces: (Length, Length),
        along: Direction,
        travel: (Length, Length),
        radius: Length,
        center: Length,
    ) -> Kerf {
        assert_eq!(
            across.l as i128 * along.l as i128 + across.w as i128 * along.w as i128,
            0,
            "a tilted blade's faces run along its path"
        );
        Kerf::make(
            Across::Face(across),
            lean,
            faces,
            along,
            travel,
            radius,
            center,
        )
    }

    fn make(
        across: Across,
        lean: i64,
        (near, far): (Length, Length),
        along: Direction,
        (start, end): (Length, Length),
        radius: Length,
        center: Length,
    ) -> Kerf {
        let turned = |(low, high): (Length, Length), reversed: bool| {
            if reversed { (-high, -low) } else { (low, high) }
        };
        let (along, along_reversed) = along.canonical();
        let (start, end) = turned((start, end), along_reversed);
        let (across, lean, (near, far), center) = match across {
            Across::Face(direction) => {
                let (direction, reversed) = direction.canonical();
                let lean = if reversed { -lean } else { lean };
                (
                    Across::Face(direction),
                    lean,
                    turned((near, far), reversed),
                    center,
                )
            }
            // Turning `along` around turns the direction square to it around too.
            Across::Thickness if along_reversed => (Across::Thickness, 0, (near, far), -center),
            Across::Thickness => (Across::Thickness, 0, (near, far), center),
        };
        Kerf {
            across,
            lean,
            along,
            near,
            far,
            radius,
            center,
            start,
            end,
        }
    }

    /// How far the slot is turned from running square across the board, or straight along it:
    /// the angle the saw was set to.
    pub fn slant(&self) -> Angle {
        let (l, w) = (self.along.l.abs() as f64, self.along.w.abs() as f64);
        Angle::nearest(l.min(w).atan2(l.max(w)))
    }

    /// The slot's width, square to its faces.
    pub fn width(&self) -> Length {
        let scale = match self.across {
            Across::Face(direction) => face_scale(direction, self.lean),
            Across::Thickness => 1.0,
        };
        let scaled = (self.far - self.near).as_nm() as f64;
        Length::from_nm((scaled / scale).round() as i64)
    }

    /// How far the blade was tilted from square to the board's face.
    pub fn tilt(&self) -> Angle {
        match self.across {
            Across::Face(direction) => {
                Angle::nearest((self.lean.abs() as f64).atan2(direction.scale()))
            }
            Across::Thickness => Angle::ZERO,
        }
    }

    /// Whether any of the slot is inside `region` of a board `thickness` thick. A blade that
    /// only grazes a face takes no wood. Where the slot or the wood leans, it's worked out
    /// to within floating point of a nanometer.
    pub(crate) fn touches(&self, region: &Region, thickness: Length) -> bool {
        if let Across::Thickness = self.across {
            return self.near < thickness
                && self.far > Length::ZERO
                && self.outline().is_some_and(|outline| {
                    let layer = region
                        .solid(thickness)
                        .layer(self.near.max(Length::ZERO), self.far.min(thickness));
                    layer.is_some_and(|layer| {
                        let depth = self.far.min(thickness) - self.near.max(Length::ZERO);
                        layer.solid(depth).overlaps(outline.edges())
                    })
                });
        }
        if self.lean != 0 || region.leans() {
            return self.touches_solid(region, thickness);
        }
        let up = gap(self.center, self.center, Length::ZERO, thickness);
        if up >= self.radius {
            return false;
        }
        let Some(inside) = region.clip_all(self.faces()) else {
            return false;
        };
        // How far past either end of the blade's path along the slot the wood is, against
        // how far the blade reached at the height of the nearest face. Both are worked out
        // exactly unless the numbers get too big, where they're within a billionth of a
        // nanometer.
        let (low, high) = inside.extremes(self.along);
        let reach = (self.radius.squared() - up.squared()) * self.along.scale_squared();
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
        let nm = |length: Length| length.as_nm() as i128;
        let (low, over_low) = low.along_exactly(self.along);
        let (high, over_high) = high.along_exactly(self.along);
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
        let disc = self.disc();
        let points: Vec<(f64, f64)> = inside
            .solid(thickness)
            .corners()
            .into_iter()
            .map(|corner| disc.at(corner.position()))
            .collect();
        let scale = self.along.scale();
        let path = (
            self.start.as_nm() as f64 / scale,
            self.end.as_nm() as f64 / scale,
        );
        near_path(&hull(points), path, self.radius.as_nm() as f64)
    }

    /// For a slot in the face, where along the slot it goes all the way through a board
    /// `thickness` thick, or `None` if it doesn't anywhere. The blade swept a convex region, so
    /// it goes through wherever it reached both faces.
    pub fn through_span(&self, thickness: Length) -> Option<(Length, Length)> {
        let (bottom_start, bottom_end) = self.span_at(Length::ZERO)?;
        let (top_start, top_end) = self.span_at(thickness)?;
        let (start, end) = (bottom_start.max(top_start), bottom_end.min(top_end));
        (start <= end).then_some((start, end))
    }

    /// Where the slot goes all the way through a board `thickness` thick, as the planes around
    /// it: the wood inside all of them is gone.
    pub(crate) fn through_hole(&self, thickness: Length) -> Option<Vec<Edge>> {
        if let Across::Thickness = self.across {
            // Only one wider than the board is thick takes all of it.
            return (self.near <= Length::ZERO && self.far >= thickness)
                .then(|| self.outline())
                .flatten()
                .map(|outline| outline.edges().to_vec());
        }
        let (start, end) = self.through_span(thickness)?;
        let nm = |length: Length| length.as_nm() as i128;
        let [face_near, face_far] = self.faces();
        let ends = [
            Edge::new(self.along, nm(end)),
            Edge::new(self.along.reversed(), -nm(start)),
        ];
        if self.lean != 0 {
            return Some([face_near, face_far, ends[0], ends[1]].to_vec());
        }
        Region::within([face_near, face_far, ends[0], ends[1]]).map(|hole| hole.edges().to_vec())
    }

    /// The faces of a slot in the face, as edges with the slot inside them. Panics for a slot
    /// square to the thickness, whose faces are parallel to the board's.
    pub(crate) fn faces(&self) -> [Edge; 2] {
        let Across::Face(across) = self.across else {
            panic!("a slot square to the thickness has no faces across the board's face");
        };
        let nm = |length: Length| length.as_nm() as i128;
        if self.lean != 0 {
            return [
                Edge::leaning(across.reversed(), -self.lean, -nm(self.near)),
                Edge::leaning(across, self.lean, nm(self.far)),
            ];
        }
        [
            Edge::new(across.reversed(), -nm(self.near)),
            Edge::new(across, nm(self.far)),
        ]
    }

    /// The plane the blade of a slot in the face turned in, in whole numbers.
    fn disc(&self) -> Disc {
        let Across::Face(across) = self.across else {
            panic!("a slot square to the thickness turned square to the face");
        };
        let square = self.along.square();
        let a2 = self.along.scale_squared();
        let k = across.l as i128 * square.l as i128 + across.w as i128 * square.w as i128;
        debug_assert_eq!(k % a2, 0, "a slot's faces run exactly along its path");
        let k = k / a2;
        let lean = i128::from(self.lean);
        let n2 = k * k * a2 + lean * lean;
        let nm = |length: Length| length.as_nm() as i128;
        Disc {
            along: self.along,
            square,
            k,
            lean,
            a2,
            n2,
            center: 2 * n2 * nm(self.center) - lean * (nm(self.near) + nm(self.far)),
        }
    }

    /// Where a slot square to the thickness reaches on the board's face, as a region just
    /// inside the round ends the blade left: its outline is straight between points on them.
    /// `None` for a slot in the face.
    pub(crate) fn outline(&self) -> Option<Region> {
        let Across::Thickness = self.across else {
            return None;
        };
        let along = self.along;
        let square = along.square();
        let scale = along.scale();
        let radius = self.radius.as_nm() as f64 * scale;
        let (u0, u1, v) = (
            self.start.as_nm() as f64,
            self.end.as_nm() as f64,
            self.center.as_nm() as f64,
        );
        // Points around the slot, counterclockwise from the length toward the width, in
        // positions along and square to `along`.
        let half = std::f64::consts::PI;
        let arc = |middle: f64, from: f64| {
            (0..=ARC_STEPS).map(move |step| {
                let angle = from + half * step as f64 / ARC_STEPS as f64;
                (middle + radius * angle.cos(), v + radius * angle.sin())
            })
        };
        let points: Vec<(Length, Length)> = arc(u1, -half / 2.0)
            .chain(arc(u0, half / 2.0))
            .map(|(u, v)| {
                // Back from scaled positions to (l, w).
                let squared = scale * scale;
                let l = (u * along.l as f64 + v * square.l as f64) / squared;
                let w = (u * along.w as f64 + v * square.w as f64) / squared;
                (
                    Length::from_nm(l.round() as i64),
                    Length::from_nm(w.round() as i64),
                )
            })
            .collect();
        Region::around(&points)
    }

    /// For a slot in the face, how far it runs along the board at height `t`, or `None` if the
    /// blade didn't reach it. The round blade cuts a stopped slot longer on the face it came
    /// through than on the other. Rounded in to the nanometer, times the scale of `along`.
    /// Tilted, the blade reaches further on one face of the slot than the other; this is as far
    /// as it reached on both, to within a nanometer short.
    pub fn span_at(&self, t: Length) -> Option<(Length, Length)> {
        if let Across::Thickness = self.across {
            return None;
        }
        if self.lean != 0 {
            let disc = self.disc();
            let radius = self.radius.as_nm() as f64;
            let half = [self.near, self.far]
                .into_iter()
                .map(|face| {
                    let off = disc.off_center(t, face);
                    (off.abs() <= radius).then(|| (radius * radius - off * off).sqrt())
                })
                .collect::<Option<Vec<f64>>>()?
                .into_iter()
                .fold(f64::MAX, f64::min);
            let half = (half * self.along.scale()).floor() as i64 - 1;
            let half = Length::from_nm(half.max(0));
            return Some((self.start - half, self.end + half));
        }
        let up = (t - self.center).squared();
        let reach = self.radius.squared();
        (up <= reach).then(|| {
            let half = ((reach - up) * self.along.scale_squared()).isqrt();
            let half = Length::from_nm(half as i64);
            (self.start - half, self.end + half)
        })
    }

    /// Whether the point `(l, w, t)` is strictly inside the slot, where the blade took the wood.
    pub fn contains(&self, l: Length, w: Length, t: Length) -> bool {
        let nm = |length: Length| length.as_nm() as i128;
        let between = |at: i128, low: Length, high: Length| nm(low) < at && at < nm(high);
        let off = |at: i128, low: Length, high: Length| (nm(low) - at).max(at - nm(high)).max(0);
        let reach = self.radius.squared() * self.along.scale_squared();
        let along = off(self.along.dot(l, w), self.start, self.end);
        match self.across {
            Across::Face(across) if self.lean != 0 => {
                let face = across.dot(l, w) + i128::from(self.lean) * nm(t);
                between(face, self.near, self.far)
                    && self.disc().reaches(along, l, w, t, self.radius)
            }
            Across::Face(across) => {
                let up = (t - self.center).squared() * self.along.scale_squared();
                between(across.dot(l, w), self.near, self.far) && along * along + up < reach
            }
            Across::Thickness => {
                let square = self.along.square().dot(l, w) - nm(self.center);
                between(nm(t), self.near, self.far) && along * along + square * square < reach
            }
        }
    }

    /// For a slot in the face, whether the blade came up through the board's bottom face,
    /// rather than down through its top.
    pub fn from_below(&self) -> bool {
        self.center < Length::ZERO
    }

    /// For a slot in the face, how deep it goes into a board `thickness` thick, from the face
    /// the blade came through. At the thickness or more, it goes through. A tilted blade
    /// reaches less high.
    pub fn depth(&self, thickness: Length) -> Length {
        let reach = match self.across {
            Across::Face(across) if self.lean != 0 => {
                let cos = across.scale() / face_scale(across, self.lean);
                Length::from_nm((self.radius.as_nm() as f64 * cos).round() as i64)
            }
            _ => self.radius,
        };
        if self.from_below() {
            self.center + reach
        } else {
            thickness - (self.center - reach)
        }
    }

    /// The same slot, measured from a new corner at `(l, w)`: where it sits in a piece cut from
    /// the board.
    pub(crate) fn moved_to(self, l: Length, w: Length) -> Kerf {
        let shift = |direction: Direction| Length::from_nm(direction.dot(l, w) as i64);
        let along = shift(self.along);
        let (near, far, center) = match self.across {
            Across::Face(across) => {
                let across = shift(across);
                (self.near - across, self.far - across, self.center)
            }
            Across::Thickness => (
                self.near,
                self.far,
                self.center - shift(self.along.square()),
            ),
        };
        Kerf {
            near,
            far,
            center,
            start: self.start - along,
            end: self.end - along,
            ..self
        }
    }

    /// The same slot, measured from a new bottom face `t` up: where it sits in a layer of the
    /// board split off by a resaw.
    pub(crate) fn moved_up(self, t: Length) -> Kerf {
        match self.across {
            Across::Face(_) => {
                let shift = Length::from_nm(self.lean * t.as_nm());
                Kerf {
                    center: self.center - t,
                    near: self.near - shift,
                    far: self.far - shift,
                    ..self
                }
            }
            Across::Thickness => Kerf {
                near: self.near - t,
                far: self.far - t,
                ..self
            },
        }
    }
}

/// How long the normal of a face facing `across` and leaning `lean` is.
fn face_scale(across: Direction, lean: i64) -> f64 {
    let l = across.l as f64;
    let w = across.w as f64;
    let lean = lean as f64;
    (l * l + w * w + lean * lean).sqrt()
}

/// The plane a blade turned in, halfway between a slot's faces, as whole numbers. Along it,
/// the slot runs `along`; up it, square to that, the way the blade leans. With the faces'
/// normal `n` (`across` and `lean`), `across` is `k` times `along.square()`, and up the plane
/// is `(-lean along.square(), k |along|^2)`, which is `|along| |n|` long, turned around for
/// `k` below zero.
struct Disc {
    along: Direction,
    square: Direction,
    k: i128,
    lean: i128,
    /// `|along|^2` and `|n|^2`.
    a2: i128,
    n2: i128,
    /// Where the blade's center is up the plane, as `up` measures it, for a point on the plane
    /// halfway between the faces at the center's height.
    center: i128,
}

impl Disc {
    /// How far `(l, w, t)` is up the plane from the blade's center, times
    /// `2 |k| |along| |n|`.
    fn up(&self, l: Length, w: Length, t: Length) -> i128 {
        let across = self.square.dot(l, w);
        let height = t.as_nm() as i128;
        2 * self.k * (self.k * self.a2 * height - self.lean * across) - self.center
    }

    /// Whether the point `(l, w, t)`, `along` past the ends of the blade's center's path
    /// (times `|along|`), is strictly within `radius` of it in the blade's plane.
    fn reaches(&self, along: i128, l: Length, w: Length, t: Length, radius: Length) -> bool {
        let up = self.up(l, w, t);
        // Squared, both over `4 k^2 |along|^2 |n|^2`.
        let r = radius.as_nm() as i128;
        Wide::product(4 * self.k * self.k * self.n2, along * along) + Wide::product(up, up)
            < Wide::product(4 * self.k * self.k * self.a2 * self.n2, r * r)
    }

    /// Where `p` is in the plane, in nanometers: along the slot, and up from the blade's
    /// center.
    fn at(&self, p: [f64; 3]) -> (f64, f64) {
        let a = (self.a2 as f64).sqrt();
        let u = (self.along.l as f64 * p[0] + self.along.w as f64 * p[1]) / a;
        let across = self.square.l as f64 * p[0] + self.square.w as f64 * p[1];
        let (k, lean) = (self.k as f64, self.lean as f64);
        let up = 2.0 * k * (k * self.a2 as f64 * p[2] - lean * across) - self.center as f64;
        (u, up / self.up_scale())
    }

    /// How far up the plane from the blade's center the slot's face at `face` is at height
    /// `t`, in nanometers.
    fn off_center(&self, t: Length, face: Length) -> f64 {
        // On the face, `k along.square() · (l, w)` is `face - lean t`.
        let (t, face) = (t.as_nm() as f64, face.as_nm() as f64);
        let (k, lean) = (self.k as f64, self.lean as f64);
        let up = 2.0 * (k * k * self.a2 as f64 * t - lean * (face - lean * t)) - self.center as f64;
        up / self.up_scale()
    }

    /// `2 |k| |along| |n|`, what `up` measures in.
    fn up_scale(&self) -> f64 {
        2.0 * (self.k as f64).abs() * (self.a2 as f64).sqrt() * (self.n2 as f64).sqrt()
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

    /// A 1/8" slot along the board, cut by a 10" blade raised 1" through a board 3/4" thick,
    /// its center moving from 6" before the board's end to 10" along it.
    fn stopped_cut() -> Kerf {
        Kerf::new(
            Across::Face(Direction::WIDTH),
            (inches(1), inches(1) + Length::from_inch_fraction(1, 8)),
            Direction::LENGTH,
            (-inches(6), inches(10)),
            inches(5),
            inches(1) - inches(5),
        )
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
        let kerf = Kerf {
            center: thickness - Length::from_inch_fraction(1, 4) + inches(5),
            ..stopped_cut()
        };
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
        let lowered = Kerf {
            center: -inches(5),
            ..kerf
        };
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
        let kerf = stopped_cut().moved_to(inches(2), inches(1));
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
        assert_eq!(backward.across, Across::Face(Direction::WIDTH));
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
        let up = kerf.moved_up(Length::from_inch_fraction(1, 4));
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

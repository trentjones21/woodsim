use crate::{
    Angle, Length,
    face::{Direction, Edge, Region},
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
/// The slot is stored as the saw left it, not trimmed to the board, so it stays exact when the
/// board is cut into pieces. It can reach past the board's edges and ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Kerf {
    /// What the slot's faces are square to.
    pub across: Across,
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
        let (across, (near, far), center) = match across {
            Across::Face(direction) => {
                let (direction, reversed) = direction.canonical();
                (
                    Across::Face(direction),
                    turned((near, far), reversed),
                    center,
                )
            }
            // Turning `along` around turns the direction square to it around too.
            Across::Thickness if along_reversed => (Across::Thickness, (near, far), -center),
            Across::Thickness => (Across::Thickness, (near, far), center),
        };
        Kerf {
            across,
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
            Across::Face(direction) => direction.scale(),
            Across::Thickness => 1.0,
        };
        let scaled = (self.far - self.near).as_nm() as f64;
        Length::from_nm((scaled / scale).round() as i64)
    }

    /// Whether any of the slot is inside `region` of a board `thickness` thick. A blade that
    /// only grazes a face takes no wood.
    pub(crate) fn touches(&self, region: &Region, thickness: Length) -> bool {
        if let Across::Thickness = self.across {
            return self.near < thickness
                && self.far > Length::ZERO
                && self
                    .outline()
                    .is_some_and(|outline| outline.overlaps(region));
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

    /// For a slot in the face, where along the slot it goes all the way through a board
    /// `thickness` thick, or `None` if it doesn't anywhere. The blade swept a convex region, so
    /// it goes through wherever it reached both faces.
    pub fn through_span(&self, thickness: Length) -> Option<(Length, Length)> {
        let (bottom_start, bottom_end) = self.span_at(Length::ZERO)?;
        let (top_start, top_end) = self.span_at(thickness)?;
        let (start, end) = (bottom_start.max(top_start), bottom_end.min(top_end));
        (start <= end).then_some((start, end))
    }

    /// Where on the board's face the slot goes all the way through a board `thickness` thick.
    pub(crate) fn through_hole(&self, thickness: Length) -> Option<Region> {
        if let Across::Thickness = self.across {
            // Only one wider than the board is thick takes all of it.
            return (self.near <= Length::ZERO && self.far >= thickness)
                .then(|| self.outline())
                .flatten();
        }
        let (start, end) = self.through_span(thickness)?;
        let nm = |length: Length| length.as_nm() as i128;
        let [face_near, face_far] = self.faces();
        Region::within([
            face_near,
            face_far,
            Edge::new(self.along, nm(end)),
            Edge::new(self.along.reversed(), -nm(start)),
        ])
    }

    /// The faces of a slot in the face, as edges with the slot inside them. Panics for a slot
    /// square to the thickness, whose faces are parallel to the board's.
    pub(crate) fn faces(&self) -> [Edge; 2] {
        let Across::Face(across) = self.across else {
            panic!("a slot square to the thickness has no faces across the board's face");
        };
        let nm = |length: Length| length.as_nm() as i128;
        [
            Edge::new(across.reversed(), -nm(self.near)),
            Edge::new(across, nm(self.far)),
        ]
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
    pub fn span_at(&self, t: Length) -> Option<(Length, Length)> {
        if let Across::Thickness = self.across {
            return None;
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
    /// the blade came through. At the thickness or more, it goes through.
    pub fn depth(&self, thickness: Length) -> Length {
        if self.from_below() {
            self.center + self.radius
        } else {
            thickness - (self.center - self.radius)
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
            Across::Face(_) => Kerf {
                center: self.center - t,
                ..self
            },
            Across::Thickness => Kerf {
                near: self.near - t,
                far: self.far - t,
                ..self
            },
        }
    }
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
}

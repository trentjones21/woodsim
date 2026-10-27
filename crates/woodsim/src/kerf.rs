use crate::Length;

/// A slot a round saw blade cut into a board without cutting it apart: everything within the
/// blade's radius of the path its center took, between the slot's two faces. A stopped cut, a
/// groove, and a notch along an edge are all kerfs.
///
/// Positions are in the board's own coordinates: `l` along its length from one end, `w` across
/// its width from one edge, and `t` up through its thickness from its bottom face. For now every
/// kerf runs along the board's length, the way a table saw cuts.
///
/// The slot is stored as the saw left it, not trimmed to the board, so it stays exact when the
/// board is cut into pieces. It can reach past the board's edges and ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Kerf {
    /// The slot's two faces, as `w` positions, `near` < `far`.
    pub near: Length,
    pub far: Length,
    pub radius: Length,
    /// The `t` of the blade's center. A table saw's blade comes up from below the board, so
    /// this is negative.
    pub center: Length,
    /// How far along the board the blade's center traveled, as `l` positions, `start` <= `end`.
    pub start: Length,
    pub end: Length,
}

impl Kerf {
    /// Whether any of the slot is inside a board of these dimensions. A blade that only grazes
    /// a face takes no wood.
    pub fn touches(&self, length: Length, width: Length, thickness: Length) -> bool {
        let across = self.near < width && self.far > Length::ZERO;
        let along = gap(self.start, self.end, Length::ZERO, length);
        let up = gap(self.center, self.center, Length::ZERO, thickness);
        across && square(along) + square(up) < square(self.radius)
    }

    /// Whether the slot runs the board's whole length and thickness, cutting it apart.
    pub fn cuts_through(&self, length: Length, thickness: Length) -> bool {
        // The region the blade swept is convex, so it covers the board's end-on rectangle if it
        // covers all four corners.
        [Length::ZERO, length].into_iter().all(|l| {
            [Length::ZERO, thickness]
                .into_iter()
                .all(|t| self.reaches(l, t))
        })
    }

    /// How far the slot runs along the board at height `t`, as `l` positions, or `None` if the
    /// blade didn't reach that high. The round blade cuts a stopped slot longer at the bottom
    /// of a board than at the top. Rounded down to the nanometer.
    pub fn span_at(&self, t: Length) -> Option<(Length, Length)> {
        let up = square(t - self.center);
        let reach = square(self.radius);
        (up <= reach).then(|| {
            let half = Length::from_nm((reach - up).isqrt() as i64);
            (self.start - half, self.end + half)
        })
    }

    /// The highest `t` the slot reaches.
    pub fn top(&self) -> Length {
        self.center + self.radius
    }

    /// Whether the blade reached the point `(l, t)` in the slot's plane.
    fn reaches(&self, l: Length, t: Length) -> bool {
        let along = gap(l, l, self.start, self.end);
        square(along) + square(t - self.center) <= square(self.radius)
    }

    /// The same slot, measured from a new corner at `(l, w)` = (`along`, `across`): where it
    /// sits in a piece cut from the board.
    pub(crate) fn moved_to(self, along: Length, across: Length) -> Kerf {
        Kerf {
            near: self.near - across,
            far: self.far - across,
            start: self.start - along,
            end: self.end - along,
            ..self
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

/// Squares in `i128`: a length in nanometers can approach 10^10, and its square overflows `i64`.
fn square(length: Length) -> i128 {
    let nm = length.as_nm() as i128;
    nm * nm
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inches(n: i64) -> Length {
        Length::from_inches(n)
    }

    /// A 1/8" slot cut by a 10" blade raised 1" through a board 3/4" thick, its center moving
    /// from 6" before the board's end to 10" along it.
    fn stopped_cut() -> Kerf {
        Kerf {
            near: inches(1),
            far: inches(1) + Length::from_inch_fraction(1, 8),
            radius: inches(5),
            center: inches(1) - inches(5),
            start: Length::ZERO - inches(6),
            end: inches(10),
        }
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
    fn a_blade_reaches_no_higher_than_its_top() {
        let kerf = stopped_cut();
        assert_eq!(kerf.top(), inches(1));
        assert!(kerf.span_at(inches(1)).is_some());
        assert_eq!(kerf.span_at(inches(1) + Length::from_nm(1)), None);
    }

    #[test]
    fn cuts_through_only_when_it_reaches_every_corner() {
        let kerf = stopped_cut();
        let thickness = Length::from_inch_fraction(3, 4);
        assert!(!kerf.cuts_through(inches(24), thickness));
        // The top corner at the far end is the last to be reached: 4-3/4" above the center.
        let far_end = kerf.span_at(thickness).unwrap().1;
        assert!(kerf.cuts_through(far_end, thickness));
        assert!(!kerf.cuts_through(far_end + Length::from_nm(1), thickness));
    }

    #[test]
    fn touches_only_boards_the_slot_is_inside() {
        let kerf = stopped_cut();
        let thickness = Length::from_inch_fraction(3, 4);
        assert!(kerf.touches(inches(24), inches(6), thickness));
        // The slot is 1" in from the edge, so a board 1" wide is untouched.
        assert!(!kerf.touches(inches(24), inches(1), thickness));
        let lowered = Kerf {
            center: Length::ZERO - inches(5),
            ..kerf
        };
        assert!(!lowered.touches(inches(24), inches(6), thickness));
    }

    #[test]
    fn moving_to_a_new_corner_shifts_the_slot() {
        let kerf = stopped_cut().moved_to(inches(2), inches(1));
        assert_eq!(
            (kerf.near, kerf.start, kerf.end),
            (Length::ZERO, -inches(8), inches(8))
        );
    }
}

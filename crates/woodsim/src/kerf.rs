use crate::{Axis, Length};

/// A slot a round saw blade cut into a board without cutting it apart: everything within the
/// blade's radius of the path its center took, between the slot's two faces. A stopped cut, a
/// groove, a dado, and a rabbet are all kerfs.
///
/// Positions are in the board's own coordinates: `l` along its length from one end, `w` across
/// its width from one edge, and `t` up through its thickness from its bottom face. A slot runs
/// along the board (a rip or a groove) or across it (a crosscut or a dado), and the blade came
/// at it through the bottom face or the top one.
///
/// The slot is stored as the saw left it, not trimmed to the board, so it stays exact when the
/// board is cut into pieces. It can reach past the board's edges and ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Kerf {
    /// The board axis the slot's faces are square to: `Width` for a slot running along the
    /// board, `Length` for one running across it.
    pub across: Axis,
    /// The slot's two faces, as positions on that axis, `near` < `far`.
    pub near: Length,
    pub far: Length,
    pub radius: Length,
    /// The `t` of the blade's center: below zero when the blade came up through the bottom
    /// face, past the thickness when it came down through the top.
    pub center: Length,
    /// How far the blade's center traveled on the board's other axis, `start` <= `end`.
    pub start: Length,
    pub end: Length,
}

impl Kerf {
    /// The board axis the slot runs along.
    pub fn along(&self) -> Axis {
        self.across.other()
    }

    /// Whether any of the slot is inside a board of these dimensions. A blade that only grazes
    /// a face takes no wood.
    pub fn touches(&self, length: Length, width: Length, thickness: Length) -> bool {
        let span = self.across.pick(length, width);
        let reach = self.along().pick(length, width);
        let across = self.near < span && self.far > Length::ZERO;
        let along = gap(self.start, self.end, Length::ZERO, reach);
        let up = gap(self.center, self.center, Length::ZERO, thickness);
        across && square(along) + square(up) < square(self.radius)
    }

    /// Where along the slot it goes all the way through a board `thickness` thick, or `None` if
    /// it doesn't anywhere. The blade swept a convex region, so it goes through wherever it
    /// reached both faces.
    pub fn through_span(&self, thickness: Length) -> Option<(Length, Length)> {
        let (bottom_start, bottom_end) = self.span_at(Length::ZERO)?;
        let (top_start, top_end) = self.span_at(thickness)?;
        let (start, end) = (bottom_start.max(top_start), bottom_end.min(top_end));
        (start <= end).then_some((start, end))
    }

    /// How far the slot runs along the board at height `t`, or `None` if the blade didn't reach
    /// it. The round blade cuts a stopped slot longer on the face it came through than on the
    /// other. Rounded in to the nanometer.
    pub fn span_at(&self, t: Length) -> Option<(Length, Length)> {
        let up = square(t - self.center);
        let reach = square(self.radius);
        (up <= reach).then(|| {
            let half = Length::from_nm((reach - up).isqrt() as i64);
            (self.start - half, self.end + half)
        })
    }

    /// Whether the blade came up through the board's bottom face, rather than down through its
    /// top.
    pub fn from_below(&self) -> bool {
        self.center < Length::ZERO
    }

    /// How deep the slot goes into a board `thickness` thick, from the face the blade came
    /// through. At the thickness or more, it goes through.
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
        let (across, along) = match self.across {
            Axis::Width => (w, l),
            Axis::Length => (l, w),
        };
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

    /// A 1/8" slot along the board, cut by a 10" blade raised 1" through a board 3/4" thick,
    /// its center moving from 6" before the board's end to 10" along it.
    fn stopped_cut() -> Kerf {
        Kerf {
            across: Axis::Width,
            near: inches(1),
            far: inches(1) + Length::from_inch_fraction(1, 8),
            radius: inches(5),
            center: inches(1) - inches(5),
            start: -inches(6),
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
        assert!(kerf.touches(inches(24), inches(6), thickness));
        // The slot is 1" in from the edge, so a board 1" wide is untouched.
        assert!(!kerf.touches(inches(24), inches(1), thickness));
        let lowered = Kerf {
            center: -inches(5),
            ..kerf
        };
        assert!(!lowered.touches(inches(24), inches(6), thickness));
    }

    #[test]
    fn a_slot_across_the_board_measures_its_faces_along_the_length() {
        let dado = Kerf {
            across: Axis::Length,
            ..stopped_cut()
        };
        let thickness = Length::from_inch_fraction(3, 4);
        // Its faces are 1" along the board, so a board 1" long is untouched by it.
        assert!(dado.touches(inches(24), inches(12), thickness));
        assert!(!dado.touches(inches(1), inches(12), thickness));
    }

    #[test]
    fn moving_to_a_new_corner_shifts_the_slot() {
        let kerf = stopped_cut().moved_to(inches(2), inches(1));
        assert_eq!(
            (kerf.near, kerf.start, kerf.end),
            (Length::ZERO, -inches(8), inches(8))
        );
        // A slot across the board shifts its faces with `l` and its travel with `w`.
        let dado = Kerf {
            across: Axis::Length,
            ..stopped_cut()
        }
        .moved_to(inches(1), inches(2));
        assert_eq!(
            (dado.near, dado.start, dado.end),
            (Length::ZERO, -inches(8), inches(8))
        );
    }
}

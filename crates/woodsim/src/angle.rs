use std::fmt;
use std::ops::{Add, Neg, Sub};

/// An angle a saw is set to, as a whole number of hundredths of a degree, so the marks on a
/// saw's scale (15°, 22-1/2°, 31.62°, 45°) are exact.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Angle(i32);

/// How finely a slanted direction is written: its whole-number components are about this big,
/// so it's within 1/16,000,000 of a radian of the angle, a few nanometers across a board's
/// width.
const UNIT: f64 = (1 << 24) as f64;

impl Angle {
    pub const ZERO: Angle = Angle(0);

    pub const fn from_degrees(degrees: i32) -> Angle {
        Angle(degrees * 100)
    }

    pub const fn from_hundredths(hundredths: i32) -> Angle {
        Angle(hundredths)
    }

    pub const fn hundredths(self) -> i32 {
        self.0
    }

    pub fn abs(self) -> Angle {
        Angle(self.0.abs())
    }

    pub fn radians(self) -> f64 {
        (self.0 as f64 / 100.0).to_radians()
    }

    /// The nearest whole hundredth of a degree to `radians`.
    pub(crate) fn nearest(radians: f64) -> Angle {
        Angle((radians.to_degrees() * 100.0).round() as i32)
    }

    /// Whole numbers in the ratio of the angle's cosine to its sine.
    pub(crate) fn unit(self) -> (i64, i64) {
        let radians = self.radians();
        (
            (radians.cos() * UNIT).round() as i64,
            (radians.sin() * UNIT).round() as i64,
        )
    }
}

impl Add for Angle {
    type Output = Angle;

    fn add(self, rhs: Angle) -> Angle {
        Angle(self.0 + rhs.0)
    }
}

impl Sub for Angle {
    type Output = Angle;

    fn sub(self, rhs: Angle) -> Angle {
        Angle(self.0 - rhs.0)
    }
}

impl Neg for Angle {
    type Output = Angle;

    fn neg(self) -> Angle {
        Angle(-self.0)
    }
}

/// Formats as degrees, with only the decimals it needs: `45°`, `22.5°`, `31.62°`.
impl fmt::Display for Angle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let (whole, hundredths) = (self.0.abs() / 100, self.0.abs() % 100);
        match (hundredths, hundredths % 10) {
            (0, _) => write!(f, "{sign}{whole}°"),
            (_, 0) => write!(f, "{sign}{whole}.{}°", hundredths / 10),
            _ => write!(f, "{sign}{whole}.{hundredths:02}°"),
        }
    }
}

impl fmt::Debug for Angle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Angle({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_only_the_decimals_it_needs() {
        let cases = [
            (Angle::from_degrees(45), "45°"),
            (Angle::from_hundredths(2250), "22.5°"),
            (Angle::from_hundredths(3162), "31.62°"),
            (Angle::from_hundredths(-1505), "-15.05°"),
            (Angle::ZERO, "0°"),
        ];
        for (angle, expected) in cases {
            assert_eq!(angle.to_string(), expected);
        }
    }

    #[test]
    fn square_and_forty_five_are_written_exactly() {
        assert_eq!(Angle::ZERO.unit(), (1 << 24, 0));
        let (cos, sin) = Angle::from_degrees(45).unit();
        assert_eq!(cos, sin);
    }
}

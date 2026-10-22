use std::fmt;
use std::iter::Sum;
use std::ops::{Add, Sub};

const NM_PER_INCH: i64 = 25_400_000;
const NM_PER_MM: i64 = 1_000_000;
const NM_PER_64TH: i64 = NM_PER_INCH / 64;

/// A length, stored as a whole number of nanometers so arithmetic is exact.
///
/// Nanometers represent 1/64", 0.001", and 0.01 mm exactly, so a fence set to 6"
/// makes a board exactly 6" wide, not 5.99997".
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Length(i64);

impl Length {
    pub const ZERO: Length = Length(0);

    pub const fn from_nm(nm: i64) -> Length {
        Length(nm)
    }

    pub const fn from_inches(inches: i64) -> Length {
        Length(inches * NM_PER_INCH)
    }

    /// `numerator / denominator` inches, e.g. `from_inch_fraction(3, 32)` for 3/32".
    ///
    /// Panics if the result is not a whole number of nanometers (1/128" is not).
    pub const fn from_inch_fraction(numerator: i64, denominator: i64) -> Length {
        let nm = numerator * NM_PER_INCH;
        assert!(
            nm % denominator == 0,
            "inch fraction is not a whole number of nanometers"
        );
        Length(nm / denominator)
    }

    pub const fn from_mm(mm: i64) -> Length {
        Length(mm * NM_PER_MM)
    }

    pub const fn as_nm(self) -> i64 {
        self.0
    }

    pub fn as_inches_f64(self) -> f64 {
        self.0 as f64 / NM_PER_INCH as f64
    }

    /// Meters as `f32`, for handing to the renderer. Never feed this back into the simulation.
    pub fn as_meters_f32(self) -> f32 {
        (self.0 as f64 / 1e9) as f32
    }
}

impl Add for Length {
    type Output = Length;

    fn add(self, rhs: Length) -> Length {
        Length(self.0 + rhs.0)
    }
}

impl Sub for Length {
    type Output = Length;

    fn sub(self, rhs: Length) -> Length {
        Length(self.0 - rhs.0)
    }
}

impl Sum for Length {
    fn sum<I: Iterator<Item = Length>>(iter: I) -> Length {
        iter.fold(Length::ZERO, Add::add)
    }
}

/// Formats as inches, using a reduced fraction when the length is a whole number of
/// 64ths (`7-1/4"`, `6"`, `3/32"`) and decimal inches otherwise.
impl fmt::Display for Length {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 % NM_PER_64TH != 0 {
            return write!(f, "{:.4}\"", self.as_inches_f64());
        }
        let sign = if self.0 < 0 { "-" } else { "" };
        let sixty_fourths = (self.0 / NM_PER_64TH).abs();
        let whole = sixty_fourths / 64;
        let mut numerator = sixty_fourths % 64;
        let mut denominator = 64;
        while numerator != 0 && numerator % 2 == 0 {
            numerator /= 2;
            denominator /= 2;
        }
        match (whole, numerator) {
            (_, 0) => write!(f, "{sign}{whole}\""),
            (0, _) => write!(f, "{sign}{numerator}/{denominator}\""),
            _ => write!(f, "{sign}{whole}-{numerator}/{denominator}\""),
        }
    }
}

/// Shows both the readable inches and the exact nanometers, so failed assertions
/// make sense even when two lengths differ by less than a 64th.
impl fmt::Debug for Length {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Length({self}, {} nm)", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_fractions_are_exact() {
        assert_eq!(Length::from_inch_fraction(1, 64).as_nm(), 396_875);
        assert_eq!(
            Length::from_inches(7) + Length::from_inch_fraction(1, 4),
            Length::from_inch_fraction(29, 4)
        );
        assert_eq!(Length::from_mm(254), Length::from_inches(10));
        assert_eq!(Length::from_inch_fraction(1, 1000).as_nm(), 25_400);
    }

    #[test]
    #[should_panic(expected = "not a whole number of nanometers")]
    fn fractions_finer_than_a_64th_are_rejected() {
        Length::from_inch_fraction(1, 128);
    }

    #[test]
    fn displays_as_woodworking_fractions() {
        let cases = [
            (Length::from_inch_fraction(29, 4), "7-1/4\""),
            (Length::from_inches(6), "6\""),
            (Length::from_inch_fraction(3, 32), "3/32\""),
            (Length::ZERO, "0\""),
            (Length::ZERO - Length::from_inch_fraction(9, 8), "-1-1/8\""),
            (Length::from_mm(10), "0.3937\""),
        ];
        for (length, expected) in cases {
            assert_eq!(length.to_string(), expected);
        }
    }

    #[test]
    fn converts_to_meters_for_rendering() {
        assert_eq!(Length::from_mm(1500).as_meters_f32(), 1.5);
    }
}

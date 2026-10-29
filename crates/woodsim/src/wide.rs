//! Whole numbers too big for `i128`: just enough to add up products of two `i128`s exactly, and
//! say whether the sum is below zero, zero, or above it. A corner where three slanted planes
//! meet is a fraction whose numerator and denominator each fit an `i128`, but which side of a
//! fourth plane it's on takes more bits than that.

use std::{
    cmp::Ordering,
    ops::{Add, Neg, Sub},
};

/// A signed 256-bit whole number, two's complement: `hi * 2^128 + lo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Wide {
    hi: i128,
    lo: u128,
}

impl Wide {
    pub(crate) const ZERO: Wide = Wide { hi: 0, lo: 0 };

    #[cfg(test)]
    fn from(value: i128) -> Wide {
        Wide {
            hi: if value < 0 { -1 } else { 0 },
            lo: value as u128,
        }
    }

    /// `a * b`, exactly.
    pub(crate) fn product(a: i128, b: i128) -> Wide {
        let (hi, lo) = multiply(a.unsigned_abs(), b.unsigned_abs());
        // Each is at most 2^127, so the product is at most 2^254: its top half fits.
        let magnitude = Wide { hi: hi as i128, lo };
        if (a < 0) != (b < 0) {
            -magnitude
        } else {
            magnitude
        }
    }

    /// The sum of `a * b` over `terms`, exactly.
    pub(crate) fn dot(terms: impl IntoIterator<Item = (i128, i128)>) -> Wide {
        terms
            .into_iter()
            .fold(Wide::ZERO, |sum, (a, b)| sum + Wide::product(a, b))
    }

    /// This times `k`, exactly.
    pub(crate) fn times(self, k: i128) -> Wide {
        let negative = (self.signum() < 0) != (k < 0);
        let magnitude = if self.signum() < 0 { -self } else { self };
        let k = k.unsigned_abs();
        let (carry, lo) = multiply(magnitude.lo, k);
        let (over, high) = multiply(magnitude.hi as u128, k);
        let hi = high
            .checked_add(carry)
            .filter(|&hi| over == 0 && hi < 1 << 127)
            .expect("a product past 2^255");
        let magnitude = Wide { hi: hi as i128, lo };
        if negative { -magnitude } else { magnitude }
    }

    pub(crate) fn signum(self) -> i32 {
        if self.hi < 0 {
            -1
        } else if self.hi == 0 && self.lo == 0 {
            0
        } else {
            1
        }
    }
}

/// `a * b` as its top and bottom 128 bits.
fn multiply(a: u128, b: u128) -> (u128, u128) {
    const LOW: u128 = u64::MAX as u128;
    let (a1, a0) = (a >> 64, a & LOW);
    let (b1, b0) = (b >> 64, b & LOW);
    let (p00, p01, p10, p11) = (a0 * b0, a0 * b1, a1 * b0, a1 * b1);
    let middle = (p00 >> 64) + (p01 & LOW) + (p10 & LOW);
    let lo = (p00 & LOW) | (middle << 64);
    let hi = p11 + (p01 >> 64) + (p10 >> 64) + (middle >> 64);
    (hi, lo)
}

impl Add for Wide {
    type Output = Wide;

    fn add(self, other: Wide) -> Wide {
        let (lo, carry) = self.lo.overflowing_add(other.lo);
        let hi = self
            .hi
            .checked_add(other.hi)
            .and_then(|hi| hi.checked_add(i128::from(carry)))
            .expect("a sum past 2^255");
        Wide { hi, lo }
    }
}

impl Neg for Wide {
    type Output = Wide;

    fn neg(self) -> Wide {
        let lo = (!self.lo).wrapping_add(1);
        let hi = (!self.hi).wrapping_add(i128::from(lo == 0));
        Wide { hi, lo }
    }
}

impl Sub for Wide {
    type Output = Wide;

    fn sub(self, other: Wide) -> Wide {
        self + -other
    }
}

impl PartialOrd for Wide {
    fn partial_cmp(&self, other: &Wide) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Wide {
    fn cmp(&self, other: &Wide) -> Ordering {
        (self.hi, self.lo).cmp(&(other.hi, other.lo))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn products_past_i128_keep_every_bit() {
        let big = (1i128 << 100) + 12345;
        let square = Wide::product(big, big);
        // (2^100 + c)^2 - 2^200 - 2 c 2^100 = c^2.
        let rest = square - Wide::product(1 << 100, 1 << 100) - Wide::product(2 * 12345, 1 << 100);
        assert_eq!(rest, Wide::from(12345 * 12345));
        assert_eq!(Wide::product(-big, big), -square);
        assert_eq!(Wide::product(-big, -big), square);
    }

    #[test]
    fn signs_and_order_hold_far_past_i128() {
        let huge = Wide::product(i128::MAX, i128::MAX);
        assert_eq!(huge.signum(), 1);
        assert_eq!((-huge).signum(), -1);
        assert_eq!((huge - huge).signum(), 0);
        assert!(-huge < Wide::from(-1));
        assert!(Wide::from(-1) < Wide::ZERO);
        assert!(Wide::from(1) < huge);
        assert_eq!(Wide::dot([(3, 4), (-2, 6)]), Wide::ZERO);
    }

    #[test]
    fn multiplying_a_wide_number_keeps_every_bit() {
        let big = (1i128 << 100) + 12345;
        let square = Wide::product(big, big);
        assert_eq!(Wide::product(big, 7).times(big), square.times(7));
        assert_eq!(square.times(-3), -square.times(3));
        assert_eq!((-square).times(-1), square);
        assert_eq!(Wide::from(-5).times(1 << 120), Wide::product(-5, 1 << 120));
    }

    #[test]
    fn negating_zero_and_carrying_across_the_halves() {
        assert_eq!(-Wide::ZERO, Wide::ZERO);
        let carry = Wide::from(-1) + Wide::from(1);
        assert_eq!(carry, Wide::ZERO);
        let top = Wide::product(1 << 64, 1 << 64);
        assert_eq!(top - Wide::from(1) + Wide::from(1), top);
    }
}

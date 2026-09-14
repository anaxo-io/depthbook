//! Fixed-point prices and quantities.
//!
//! [`Scale9`] is an `i64` holding a value multiplied by 10⁹, so nine decimal places are
//! preserved exactly. Money is not representable in binary floating point — `0.1 + 0.2`
//! is not `0.3` — and an order book compares and sums prices constantly, so prices are
//! kept as integers and only converted at the edges.
//!
//! ```
//! use orderbook::{f64_to_scale9, Scale9};
//!
//! let price = f64_to_scale9(123.456789012);
//! assert_eq!(price.raw(), 123_456_789_012);
//! assert_eq!(price.to_string(), "123.456789012");
//!
//! // Arithmetic is checked; scaling is handled for you.
//! let qty = f64_to_scale9(2.0);
//! assert_eq!(price.checked_mul(qty).unwrap(), f64_to_scale9(246.913578024));
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;
use std::num::ParseFloatError;
use std::ops::{Add, Neg, Sub};

/// Number of implied decimal places in a [`Scale9`].
pub const SCALE: u32 = 9;

/// Scale multiplier, 10⁹.
pub const SCALE9: i64 = 1_000_000_000;

/// A fixed-point number with nine implied decimal places.
///
/// The wrapped `i64` is the value multiplied by 10⁹: `1.5` is stored as `1_500_000_000`.
/// Being a distinct type rather than a bare `i64` means an unscaled number cannot be
/// passed where a scaled one is expected — the mistake that silently misprices a book by
/// a factor of a billion.
///
/// `Ord` compares the underlying integers, so levels sort correctly by price.
///
/// ```
/// use orderbook::Scale9;
///
/// let a = Scale9::from_raw(50_000_000_000_000);
/// let b = Scale9::from_f64(50_000.0);
/// assert_eq!(a, b);
/// assert!(Scale9::from_f64(1.0) > Scale9::from_f64(0.5));
/// ```
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Scale9(i64);

impl Scale9 {
    /// Zero.
    pub const ZERO: Scale9 = Scale9(0);

    /// One, that is `1_000_000_000` raw.
    pub const ONE: Scale9 = Scale9(SCALE9);

    /// Wrap an already-scaled integer.
    ///
    /// The argument is the value times 10⁹, not the value itself. Use
    /// [`Scale9::from_f64`] or [`str_to_scale9`] to convert an ordinary number.
    ///
    /// ```
    /// use orderbook::Scale9;
    /// assert_eq!(Scale9::from_raw(1_500_000_000), Scale9::from_f64(1.5));
    /// ```
    #[inline]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// The underlying scaled integer.
    #[inline]
    pub const fn raw(self) -> i64 {
        self.0
    }

    /// Convert from a floating-point value, rounding to the nearest 10⁻⁹.
    ///
    /// Values beyond roughly ±9.2 × 10⁹ cannot be represented and saturate.
    #[inline]
    pub fn from_f64(value: f64) -> Self {
        Self((value * SCALE9 as f64).round() as i64)
    }

    /// Convert to a floating-point value, which may lose precision.
    #[inline]
    pub fn to_f64(self) -> f64 {
        self.0 as f64 / SCALE9 as f64
    }

    /// Whether this is exactly zero.
    #[inline]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Add, returning `None` on overflow.
    #[inline]
    pub const fn checked_add(self, other: Self) -> Option<Self> {
        match self.0.checked_add(other.0) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// Subtract, returning `None` on overflow.
    #[inline]
    pub const fn checked_sub(self, other: Self) -> Option<Self> {
        match self.0.checked_sub(other.0) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// Multiply, returning `None` on overflow.
    ///
    /// Both operands carry a factor of 10⁹, so the raw product carries 10¹⁸ and is
    /// divided back down. The intermediate is computed in `i128` so that realistic
    /// price × quantity products do not overflow.
    ///
    /// ```
    /// use orderbook::Scale9;
    /// let price = Scale9::from_f64(2.5);
    /// let qty = Scale9::from_f64(3.0);
    /// assert_eq!(price.checked_mul(qty), Some(Scale9::from_f64(7.5)));
    /// ```
    #[inline]
    pub fn checked_mul(self, other: Self) -> Option<Self> {
        let product = (self.0 as i128).checked_mul(other.0 as i128)? / SCALE9 as i128;
        i64::try_from(product).ok().map(Self)
    }

    /// Divide, returning `None` on overflow or division by zero.
    ///
    /// ```
    /// use orderbook::Scale9;
    /// let total = Scale9::from_f64(10.0);
    /// assert_eq!(total.checked_div(Scale9::from_f64(4.0)), Some(Scale9::from_f64(2.5)));
    /// assert_eq!(total.checked_div(Scale9::ZERO), None);
    /// ```
    #[inline]
    pub fn checked_div(self, other: Self) -> Option<Self> {
        if other.0 == 0 {
            return None;
        }
        let quotient = (self.0 as i128).checked_mul(SCALE9 as i128)? / other.0 as i128;
        i64::try_from(quotient).ok().map(Self)
    }

    /// Add, saturating at the numeric bounds instead of overflowing.
    #[inline]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }
}

impl Add for Scale9 {
    type Output = Scale9;

    /// Panics on overflow in debug builds, wraps in release, matching `i64`. Use
    /// [`Scale9::checked_add`] where the inputs are not trusted.
    #[inline]
    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl Sub for Scale9 {
    type Output = Scale9;

    /// Panics on overflow in debug builds, wraps in release, matching `i64`. Use
    /// [`Scale9::checked_sub`] where the inputs are not trusted.
    #[inline]
    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl Neg for Scale9 {
    type Output = Scale9;

    #[inline]
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl fmt::Display for Scale9 {
    /// Writes the decimal value, trimming trailing zeros: `50000`, `1.5`, `-0.000000001`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let magnitude = self.0.unsigned_abs();
        let integer = magnitude / SCALE9 as u64;
        let fraction = magnitude % SCALE9 as u64;

        if self.0 < 0 {
            f.write_str("-")?;
        }
        if fraction == 0 {
            write!(f, "{integer}")
        } else {
            let digits = format!("{fraction:09}");
            write!(f, "{integer}.{}", digits.trim_end_matches('0'))
        }
    }
}

impl fmt::Debug for Scale9 {
    /// Shows the decimal value rather than the raw integer, so assertion failures are
    /// readable.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Scale9({self})")
    }
}

/// Convert a floating-point number to [`Scale9`], rounding to the nearest 10⁻⁹.
///
/// ```
/// use orderbook::f64_to_scale9;
///
/// assert_eq!(f64_to_scale9(123.456789012).raw(), 123_456_789_012);
/// assert_eq!(f64_to_scale9(1.0).raw(), 1_000_000_000);
/// ```
#[inline]
pub fn f64_to_scale9(value: f64) -> Scale9 {
    Scale9::from_f64(value)
}

/// Convert a [`Scale9`] to a floating-point number, which may lose precision.
///
/// ```
/// use orderbook::{f64_to_scale9, scale9_to_f64};
///
/// assert_eq!(scale9_to_f64(f64_to_scale9(1.5)), 1.5);
/// ```
#[inline]
pub fn scale9_to_f64(value: Scale9) -> f64 {
    value.to_f64()
}

/// Parse a decimal string into [`Scale9`].
///
/// Exchange feeds quote prices as strings; this is the conversion for that.
///
/// ```
/// use orderbook::str_to_scale9;
///
/// assert_eq!(str_to_scale9("123.456789").unwrap().raw(), 123_456_789_000);
/// assert!(str_to_scale9("not a number").is_err());
/// ```
pub fn str_to_scale9(s: &str) -> Result<Scale9, ParseFloatError> {
    Ok(Scale9::from_f64(s.parse::<f64>()?))
}

/// Format a [`Scale9`] with a fixed number of decimal places, rounding half away from zero.
///
/// More than nine decimals is capped at nine. For the natural representation, use
/// [`Display`](std::fmt::Display).
///
/// ```
/// use orderbook::{f64_to_scale9, scale9_to_string};
///
/// let price = f64_to_scale9(123.456789);
/// assert_eq!(scale9_to_string(price, 6), "123.456789");
/// assert_eq!(scale9_to_string(price, 2), "123.46");
/// assert_eq!(scale9_to_string(price, 0), "123");
/// ```
pub fn scale9_to_string(value: Scale9, decimals: usize) -> String {
    let decimals = decimals.min(SCALE as usize);
    let raw = value.raw();

    if decimals == 0 {
        return format!("{}", raw / SCALE9);
    }

    let integer = raw / SCALE9;
    let fraction = (raw % SCALE9).abs();
    let divisor = 10_i64.pow(SCALE - decimals as u32);
    let rounded = (fraction + divisor / 2) / divisor;

    format!("{integer}.{rounded:0width$}", width = decimals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_and_to_f64() {
        assert_eq!(Scale9::from_f64(123.456789012).raw(), 123_456_789_012);
        assert_eq!(Scale9::from_f64(0.000000001).raw(), 1);
        assert_eq!(Scale9::from_f64(1.0), Scale9::ONE);
        assert_eq!(Scale9::from_f64(0.0), Scale9::ZERO);
        assert_eq!(Scale9::from_f64(-1.5).raw(), -1_500_000_000);
        assert_eq!(Scale9::from_raw(-1_500_000_000).to_f64(), -1.5);
    }

    #[test]
    fn f64_round_trip() {
        let original = 123.456789012;
        assert!((Scale9::from_f64(original).to_f64() - original).abs() < 1e-9);
    }

    #[test]
    fn parses_decimal_strings() {
        assert_eq!(str_to_scale9("123.456789").unwrap().raw(), 123_456_789_000);
        assert_eq!(str_to_scale9("0.000000001").unwrap(), Scale9::from_raw(1));
        assert_eq!(str_to_scale9("1.0").unwrap(), Scale9::ONE);
        assert!(str_to_scale9("invalid").is_err());
    }

    #[test]
    fn display_trims_trailing_zeros() {
        assert_eq!(Scale9::from_f64(50_000.0).to_string(), "50000");
        assert_eq!(Scale9::from_f64(1.5).to_string(), "1.5");
        assert_eq!(Scale9::from_raw(1).to_string(), "0.000000001");
        assert_eq!(Scale9::from_f64(-1.5).to_string(), "-1.5");
        assert_eq!(Scale9::from_raw(-1).to_string(), "-0.000000001");
        assert_eq!(Scale9::ZERO.to_string(), "0");
    }

    #[test]
    fn debug_is_readable() {
        assert_eq!(format!("{:?}", Scale9::from_f64(1.5)), "Scale9(1.5)");
    }

    #[test]
    fn formats_with_fixed_decimals() {
        let price = Scale9::from_f64(123.456789);
        assert_eq!(scale9_to_string(price, 6), "123.456789");
        assert_eq!(scale9_to_string(price, 2), "123.46");
        assert_eq!(scale9_to_string(Scale9::ONE, 0), "1");
        assert_eq!(scale9_to_string(Scale9::from_f64(1.5), 1), "1.5");
    }

    #[test]
    fn checked_add_and_sub() {
        let one = Scale9::ONE;
        let two = Scale9::from_f64(2.0);
        assert_eq!(one.checked_add(two), Some(Scale9::from_f64(3.0)));
        assert_eq!(Scale9::from_f64(3.0).checked_sub(two), Some(one));
        assert_eq!(
            Scale9::from_raw(i64::MAX).checked_add(Scale9::from_raw(1)),
            None
        );
        assert_eq!(
            Scale9::from_raw(i64::MIN).checked_sub(Scale9::from_raw(1)),
            None
        );
    }

    #[test]
    fn checked_mul_handles_scale() {
        assert_eq!(
            Scale9::from_f64(2.5).checked_mul(Scale9::from_f64(3.0)),
            Some(Scale9::from_f64(7.5))
        );
        assert_eq!(Scale9::ONE.checked_mul(Scale9::ONE), Some(Scale9::ONE));
        assert_eq!(
            Scale9::ZERO.checked_mul(Scale9::from_f64(123.0)),
            Some(Scale9::ZERO)
        );
        // Overflows i64 after scaling back down.
        assert_eq!(
            Scale9::from_raw(i64::MAX).checked_mul(Scale9::from_raw(i64::MAX)),
            None
        );
    }

    #[test]
    fn checked_div_handles_scale_and_zero() {
        assert_eq!(
            Scale9::from_f64(10.0).checked_div(Scale9::from_f64(2.0)),
            Some(Scale9::from_f64(5.0))
        );
        assert_eq!(Scale9::ONE.checked_div(Scale9::ONE), Some(Scale9::ONE));
        assert_eq!(Scale9::from_f64(10.0).checked_div(Scale9::ZERO), None);
    }

    #[test]
    fn operators_match_checked_variants() {
        let a = Scale9::from_f64(1.5);
        let b = Scale9::from_f64(2.5);
        assert_eq!(a + b, a.checked_add(b).unwrap());
        assert_eq!(b - a, b.checked_sub(a).unwrap());
        assert_eq!(-a, Scale9::from_f64(-1.5));
    }

    #[test]
    fn ordering_follows_value() {
        assert!(Scale9::from_f64(1.0) > Scale9::from_f64(0.5));
        assert!(Scale9::from_f64(-1.0) < Scale9::ZERO);

        let mut prices = [
            Scale9::from_f64(3.0),
            Scale9::from_f64(1.0),
            Scale9::from_f64(2.0),
        ];
        prices.sort();
        assert_eq!(prices[0], Scale9::from_f64(1.0));
        assert_eq!(prices[2], Scale9::from_f64(3.0));
    }

    #[test]
    fn serde_is_transparent() {
        let price = Scale9::from_f64(1.5);
        assert_eq!(serde_json::to_string(&price).unwrap(), "1500000000");
        let back: Scale9 = serde_json::from_str("1500000000").unwrap();
        assert_eq!(back, price);
    }

    #[test]
    fn is_zero_detects_zero() {
        assert!(Scale9::ZERO.is_zero());
        assert!(!Scale9::from_raw(1).is_zero());
    }
}

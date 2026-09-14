//! Scale-9 decimal helpers for price and quantity conversions.
//!
//! This module provides utilities for converting between floating-point and
//! scale-9 integer representations. Scale-9 means 9 decimal places are preserved.
//!
//! # Examples
//! ```
//! use orderbook::decimal::{f64_to_scale9, scale9_to_f64};
//!
//! let price = 123.456789012;
//! let scale9 = f64_to_scale9(price);
//! assert_eq!(scale9, 123_456_789_012);
//!
//! let back = scale9_to_f64(scale9);
//! assert!((back - price).abs() < 0.000000001);
//! ```

/// A fixed-point number with nine implied decimal places.
///
/// This is an alias for `i64`, not a newtype: prices are compared and sorted with
/// ordinary integer operations on the hot path, and the helpers in this module are
/// explicit about scale where it matters.
pub type Scale9 = i64;

/// Scale-9 multiplier (10^9).
pub const SCALE9: i64 = 1_000_000_000;

/// Convert a floating-point number to scale-9 integer representation.
///
/// # Arguments
/// * `value` - The floating-point value to convert
///
/// # Returns
/// The value multiplied by 10^9 and rounded to the nearest integer
///
/// # Examples
/// ```
/// use orderbook::decimal::f64_to_scale9;
///
/// assert_eq!(f64_to_scale9(123.456789012), 123_456_789_012);
/// assert_eq!(f64_to_scale9(0.000000001), 1);
/// assert_eq!(f64_to_scale9(1.0), 1_000_000_000);
/// ```
#[inline]
pub fn f64_to_scale9(value: f64) -> i64 {
    (value * SCALE9 as f64).round() as i64
}

/// Convert a scale-9 integer to floating-point representation.
///
/// # Arguments
/// * `value` - The scale-9 integer value
///
/// # Returns
/// The value divided by 10^9 as a floating-point number
///
/// # Examples
/// ```
/// use orderbook::decimal::scale9_to_f64;
///
/// assert_eq!(scale9_to_f64(123_456_789_012), 123.456789012);
/// assert_eq!(scale9_to_f64(1), 0.000000001);
/// assert_eq!(scale9_to_f64(1_000_000_000), 1.0);
/// ```
#[inline]
pub fn scale9_to_f64(value: i64) -> f64 {
    value as f64 / SCALE9 as f64
}

/// Convert a string to scale-9 integer representation.
///
/// Parses the string as a floating-point number and converts it to scale-9.
///
/// # Arguments
/// * `s` - The string to parse
///
/// # Returns
/// `Ok(i64)` with the scale-9 value, or `Err` if parsing fails
///
/// # Examples
/// ```
/// use orderbook::decimal::str_to_scale9;
///
/// assert_eq!(str_to_scale9("123.456789").unwrap(), 123_456_789_000);
/// assert_eq!(str_to_scale9("0.000000001").unwrap(), 1);
/// assert!(str_to_scale9("invalid").is_err());
/// ```
pub fn str_to_scale9(s: &str) -> Result<i64, std::num::ParseFloatError> {
    let value = s.parse::<f64>()?;
    Ok(f64_to_scale9(value))
}

/// Format a scale-9 integer as a string with the specified number of decimal places.
///
/// # Arguments
/// * `value` - The scale-9 integer value
/// * `decimals` - Number of decimal places to show (0-9)
///
/// # Returns
/// A formatted string representation
///
/// # Examples
/// ```
/// use orderbook::decimal::scale9_to_string;
///
/// assert_eq!(scale9_to_string(123_456_789_000, 6), "123.456789");
/// assert_eq!(scale9_to_string(123_456_789_000, 2), "123.46");
/// assert_eq!(scale9_to_string(1_000_000_000, 0), "1");
/// ```
pub fn scale9_to_string(value: i64, decimals: usize) -> String {
    let decimals = decimals.min(9); // Cap at 9 decimal places

    if decimals == 0 {
        return format!("{}", value / SCALE9);
    }

    let integer_part = value / SCALE9;
    let fractional_part = (value % SCALE9).abs();

    // Calculate the divisor based on how many decimal places we want
    let divisor = 10_i64.pow((9 - decimals) as u32);
    let rounded_frac = (fractional_part + divisor / 2) / divisor;

    format!(
        "{}.{:0width$}",
        integer_part,
        rounded_frac,
        width = decimals
    )
}

/// Add two scale-9 values with overflow checking.
///
/// # Arguments
/// * `a` - First scale-9 value
/// * `b` - Second scale-9 value
///
/// # Returns
/// `Some(result)` if addition succeeds without overflow, `None` otherwise
///
/// # Examples
/// ```
/// use orderbook::decimal::scale9_add;
///
/// assert_eq!(scale9_add(1_000_000_000, 2_000_000_000), Some(3_000_000_000));
/// assert_eq!(scale9_add(i64::MAX, 1), None); // Overflow
/// ```
#[inline]
pub fn scale9_add(a: i64, b: i64) -> Option<i64> {
    a.checked_add(b)
}

/// Subtract two scale-9 values with overflow checking.
///
/// # Arguments
/// * `a` - First scale-9 value
/// * `b` - Second scale-9 value
///
/// # Returns
/// `Some(result)` if subtraction succeeds without overflow, `None` otherwise
///
/// # Examples
/// ```
/// use orderbook::decimal::scale9_sub;
///
/// assert_eq!(scale9_sub(3_000_000_000, 2_000_000_000), Some(1_000_000_000));
/// assert_eq!(scale9_sub(i64::MIN, 1), None); // Overflow
/// ```
#[inline]
pub fn scale9_sub(a: i64, b: i64) -> Option<i64> {
    a.checked_sub(b)
}

/// Multiply two scale-9 values, returning a scale-9 result.
///
/// Since both inputs are scale-9 (multiplied by 10^9), the raw multiplication
/// would be scale-18. We divide by 10^9 to get back to scale-9.
///
/// # Arguments
/// * `a` - First scale-9 value
/// * `b` - Second scale-9 value
///
/// # Returns
/// `Some(result)` if multiplication succeeds without overflow, `None` otherwise
///
/// # Examples
/// ```
/// use orderbook::decimal::scale9_mul;
///
/// // 2.5 * 3.0 = 7.5
/// assert_eq!(scale9_mul(2_500_000_000, 3_000_000_000), Some(7_500_000_000));
/// ```
#[inline]
pub fn scale9_mul(a: i64, b: i64) -> Option<i64> {
    // Use i128 for intermediate calculation to avoid overflow
    let result = (a as i128)
        .checked_mul(b as i128)?
        .checked_div(SCALE9 as i128)?;

    // Check if result fits in i64
    if result > i64::MAX as i128 || result < i64::MIN as i128 {
        None
    } else {
        Some(result as i64)
    }
}

/// Divide two scale-9 values, returning a scale-9 result.
///
/// # Arguments
/// * `a` - Numerator (scale-9)
/// * `b` - Denominator (scale-9)
///
/// # Returns
/// `Some(result)` if division succeeds without overflow or division by zero, `None` otherwise
///
/// # Examples
/// ```
/// use orderbook::decimal::scale9_div;
///
/// // 10.0 / 2.0 = 5.0
/// assert_eq!(scale9_div(10_000_000_000, 2_000_000_000), Some(5_000_000_000));
/// assert_eq!(scale9_div(10_000_000_000, 0), None); // Division by zero
/// ```
#[inline]
pub fn scale9_div(a: i64, b: i64) -> Option<i64> {
    if b == 0 {
        return None;
    }

    // Multiply numerator by SCALE9 first to maintain precision
    let result = (a as i128)
        .checked_mul(SCALE9 as i128)?
        .checked_div(b as i128)?;

    // Check if result fits in i64
    if result > i64::MAX as i128 || result < i64::MIN as i128 {
        None
    } else {
        Some(result as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_f64_to_scale9() {
        assert_eq!(f64_to_scale9(123.456789012), 123_456_789_012);
        assert_eq!(f64_to_scale9(0.000000001), 1);
        assert_eq!(f64_to_scale9(1.0), SCALE9);
        assert_eq!(f64_to_scale9(0.0), 0);
        assert_eq!(f64_to_scale9(-1.5), -1_500_000_000);
    }

    #[test]
    fn test_scale9_to_f64() {
        assert_eq!(scale9_to_f64(123_456_789_012), 123.456789012);
        assert_eq!(scale9_to_f64(1), 0.000000001);
        assert_eq!(scale9_to_f64(SCALE9), 1.0);
        assert_eq!(scale9_to_f64(0), 0.0);
        assert_eq!(scale9_to_f64(-1_500_000_000), -1.5);
    }

    #[test]
    fn test_round_trip() {
        let original = 123.456789012;
        let scale9 = f64_to_scale9(original);
        let back = scale9_to_f64(scale9);
        assert!((back - original).abs() < 1e-9);
    }

    #[test]
    fn test_str_to_scale9() {
        assert_eq!(str_to_scale9("123.456789").unwrap(), 123_456_789_000);
        assert_eq!(str_to_scale9("0.000000001").unwrap(), 1);
        assert_eq!(str_to_scale9("1.0").unwrap(), SCALE9);
        assert!(str_to_scale9("invalid").is_err());
    }

    #[test]
    fn test_scale9_to_string() {
        assert_eq!(scale9_to_string(123_456_789_000, 6), "123.456789");
        assert_eq!(scale9_to_string(123_456_789_000, 2), "123.46");
        assert_eq!(scale9_to_string(SCALE9, 0), "1");
        assert_eq!(scale9_to_string(1_500_000_000, 1), "1.5");
    }

    #[test]
    fn test_scale9_add() {
        assert_eq!(
            scale9_add(1_000_000_000, 2_000_000_000),
            Some(3_000_000_000)
        );
        assert_eq!(scale9_add(0, 0), Some(0));
        assert_eq!(scale9_add(i64::MAX, 1), None);
    }

    #[test]
    fn test_scale9_sub() {
        assert_eq!(
            scale9_sub(3_000_000_000, 2_000_000_000),
            Some(1_000_000_000)
        );
        assert_eq!(scale9_sub(0, 0), Some(0));
        assert_eq!(scale9_sub(i64::MIN, 1), None);
    }

    #[test]
    fn test_scale9_mul() {
        // 2.5 * 3.0 = 7.5
        assert_eq!(
            scale9_mul(2_500_000_000, 3_000_000_000),
            Some(7_500_000_000)
        );
        // 1.0 * 1.0 = 1.0
        assert_eq!(scale9_mul(SCALE9, SCALE9), Some(SCALE9));
        // 0 * anything = 0
        assert_eq!(scale9_mul(0, 123_456_789_000), Some(0));
    }

    #[test]
    fn test_scale9_div() {
        // 10.0 / 2.0 = 5.0
        assert_eq!(
            scale9_div(10_000_000_000, 2_000_000_000),
            Some(5_000_000_000)
        );
        // 1.0 / 1.0 = 1.0
        assert_eq!(scale9_div(SCALE9, SCALE9), Some(SCALE9));
        // Division by zero
        assert_eq!(scale9_div(10_000_000_000, 0), None);
    }
}

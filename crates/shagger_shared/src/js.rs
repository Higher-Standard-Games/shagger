//! JavaScript number semantics that the TypeScript code relies on.
//!
//! The server and the browser run V8. Where SHAG's shared code calls a
//! `Math` function whose result Rust's standard library computes
//! differently, the port calls the function here instead, so both sides get
//! the same bits. RUST.md 6.2 and 19.6 list where each one is used.

/// `Math.round`: the nearest integer, ties toward positive infinity.
///
/// `f64::round` rounds ties away from zero, so `-2.5` gives `-3.0` where
/// JavaScript gives `-2`. Like JavaScript, a negative input that rounds to
/// zero gives `-0.0`.
#[must_use]
pub fn js_round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let floor = x.floor();
    // For a finite f64 the fractional part `x - floor` is exact.
    let rounded = if x - floor >= 0.5 { floor + 1.0 } else { floor };
    if rounded == 0.0 && x.is_sign_negative() {
        -0.0
    } else {
        rounded
    }
}

/// `Math.hypot`, with V8's algorithm.
///
/// V8 does not call C's `hypot`. It scales every argument by the largest
/// absolute value and sums the squares with Kahan compensation, which can
/// differ from `f64::hypot` in the last bit. SHAG uses `Math.hypot` on every
/// command (`InputSanitizer`) and in the ledge grab, so prediction must use
/// this function.
#[must_use]
pub fn js_hypot(values: &[f64]) -> f64 {
    let mut max = 0.0_f64;
    let mut one_is_nan = false;
    for &value in values {
        let abs = value.abs();
        if abs.is_nan() {
            one_is_nan = true;
        } else if abs > max {
            max = abs;
        }
    }
    // An infinity wins over NaN, as the specification requires.
    if max == f64::INFINITY {
        return f64::INFINITY;
    }
    if one_is_nan {
        return f64::NAN;
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0_f64;
    let mut compensation = 0.0_f64;
    for &value in values {
        let n = value.abs() / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

/// ECMAScript `ToUint32`: what `DataView.setUint32` stores for any number.
///
/// NaN and infinities become 0; everything else is truncated toward zero and
/// wrapped modulo 2^32. The wire encoders take JavaScript numbers in the
/// TypeScript, so a Rust caller holding an `f64` converts with these to get
/// the same bytes.
#[must_use]
pub fn to_uint32(x: f64) -> u32 {
    to_int_modulo(x, 4_294_967_296.0) as u32
}

/// ECMAScript `ToInt32` (`DataView.setInt32`, and `x | 0`).
#[must_use]
pub fn to_int32(x: f64) -> i32 {
    to_uint32(x) as i32
}

/// ECMAScript `ToUint16` (`DataView.setUint16`).
#[must_use]
pub fn to_uint16(x: f64) -> u16 {
    to_int_modulo(x, 65_536.0) as u16
}

/// ECMAScript `ToInt16` (`DataView.setInt16`).
#[must_use]
pub fn to_int16(x: f64) -> i16 {
    to_uint16(x) as i16
}

/// ECMAScript `ToUint8` (`DataView.setUint8`). Not `ToUint8Clamp`.
#[must_use]
pub fn to_uint8(x: f64) -> u8 {
    to_int_modulo(x, 256.0) as u8
}

/// ECMAScript `ToInt8` (`DataView.setInt8`).
#[must_use]
pub fn to_int8(x: f64) -> i8 {
    to_uint8(x) as i8
}

/// Truncate toward zero, then reduce modulo `modulus` into `0..modulus`.
fn to_int_modulo(x: f64, modulus: f64) -> f64 {
    if !x.is_finite() {
        return 0.0;
    }
    let r = x.trunc() % modulus;
    // `%` keeps the dividend's sign; JavaScript's modulo here is mathematical.
    let r = if r < 0.0 { r + modulus } else { r };
    // -0.0 and values that round up to `modulus` after the add both map to 0.
    if r >= modulus { 0.0 } else { r }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_ties_go_up() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.499_999_999_999_999_94), 0.0);
        assert!(js_round(-0.4).is_sign_negative());
        assert_eq!(js_round(4_503_599_627_370_497.0), 4_503_599_627_370_497.0);
    }

    #[test]
    fn int_conversions_wrap_like_dataview() {
        assert_eq!(to_uint32(-1.0), u32::MAX);
        assert_eq!(to_uint32(4_294_967_296.5), 0);
        assert_eq!(to_uint16(70_000.9), 4464);
        assert_eq!(to_int16(-1.9), -1);
        assert_eq!(to_uint8(f64::NAN), 0);
        assert_eq!(to_uint8(-0.5), 0);
        assert_eq!(to_int8(200.0), -56);
        assert_eq!(to_int32(2_147_483_648.0), i32::MIN);
    }

    #[test]
    fn hypot_special_values() {
        assert_eq!(js_hypot(&[]), 0.0);
        assert_eq!(js_hypot(&[f64::NAN, f64::NEG_INFINITY]), f64::INFINITY);
        assert!(js_hypot(&[f64::NAN, 1.0]).is_nan());
        assert_eq!(js_hypot(&[3.0, 4.0]), 5.0);
    }
}

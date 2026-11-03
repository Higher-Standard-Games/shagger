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

/// ECMAScript white space and line terminators: what `String.prototype.trim`
/// removes and what `\s` matches in a regular expression.
///
/// Unicode `White_Space` without U+0085, plus U+FEFF.
#[must_use]
pub fn is_js_whitespace(c: char) -> bool {
    c == '\u{feff}' || (c != '\u{85}' && c.is_whitespace())
}

/// `String.prototype.trim`.
#[must_use]
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// The length of the longest prefix of `s` that is a `StrDecimalLiteral`
/// (sign, digits, fraction, exponent, or `Infinity`), and whether it is
/// `Infinity`.
fn decimal_prefix(s: &str) -> Option<(usize, bool)> {
    let b = s.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    if s[i..].starts_with("Infinity") {
        return Some((i + 8, true));
    }
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let whole = digits(i);
    i += whole;
    let mut fraction = 0;
    if b.get(i) == Some(&b'.') {
        fraction = digits(i + 1);
        if whole > 0 || fraction > 0 {
            i += 1 + fraction;
        }
    }
    if whole == 0 && fraction == 0 {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(b.get(i + 1), Some(b'+' | b'-')));
        let exponent = digits(i + 1 + sign);
        if exponent > 0 {
            i += 1 + sign + exponent;
        }
    }
    Some((i, false))
}

fn decimal_value(literal: &str, infinity: bool) -> f64 {
    if infinity {
        return if literal.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    // Rust's parser is correctly rounded, as V8's is. It wants a digit
    // before an exponent, which a literal such as "1.e5" has.
    literal.parse().unwrap_or(f64::NAN)
}

/// `parseFloat(s)`.
#[must_use]
pub fn js_parse_float(s: &str) -> f64 {
    let s = s.trim_start_matches(is_js_whitespace);
    match decimal_prefix(s) {
        Some((end, infinity)) => decimal_value(&s[..end], infinity),
        None => f64::NAN,
    }
}

/// `Number(s)` for a string (ECMAScript `StringToNumber`).
#[must_use]
pub fn js_string_to_number(s: &str) -> f64 {
    let s = js_trim(s);
    if s.is_empty() {
        return 0.0;
    }
    let radix = match s.get(..2) {
        Some("0x" | "0X") => 16,
        Some("0o" | "0O") => 8,
        Some("0b" | "0B") => 2,
        _ => 0,
    };
    if radix != 0 {
        let digits = &s[2..];
        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
            return f64::NAN;
        }
        return radix_value(digits, radix);
    }
    match decimal_prefix(s) {
        Some((end, infinity)) if end == s.len() => decimal_value(s, infinity),
        _ => f64::NAN,
    }
}

/// An integer in a power-of-two radix, rounded to nearest, ties to even.
fn radix_value(digits: &str, radix: u32) -> f64 {
    let bits_per_digit = radix.trailing_zeros();
    let digits = digits.trim_start_matches('0');
    // Collect the bits, then round once from the exact value.
    let mut bits: Vec<bool> = Vec::with_capacity(digits.len() * 4);
    for c in digits.chars() {
        let d = c.to_digit(radix).unwrap_or(0);
        for k in (0..bits_per_digit).rev() {
            bits.push(d >> k & 1 == 1);
        }
    }
    let first = bits.iter().position(|&b| b).unwrap_or(bits.len());
    let bits = &bits[first..];
    if bits.is_empty() {
        return 0.0;
    }
    if bits.len() > 1024 {
        return f64::INFINITY;
    }
    let mut mantissa: u64 = 0;
    for &b in bits.iter().take(53) {
        mantissa = mantissa << 1 | u64::from(b);
    }
    let mut exponent = i32::try_from(bits.len()).unwrap_or(i32::MAX) - 53;
    if exponent > 0 {
        let rest = &bits[53..];
        let half = rest[0];
        let sticky = rest[1..].iter().any(|&b| b);
        if half && (sticky || mantissa & 1 == 1) {
            mantissa += 1;
            if mantissa == 1 << 53 {
                mantissa >>= 1;
                exponent += 1;
            }
        }
    } else {
        exponent = 0;
    }
    #[allow(clippy::cast_precision_loss)]
    let m = mantissa as f64;
    m * 2f64.powi(exponent)
}

/// `Math.max(a, b)`: NaN if either is NaN, and +0 above -0.
#[must_use]
pub fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a == 0.0 && a.is_sign_negative() { b } else { a }
    } else if a > b {
        a
    } else {
        b
    }
}

/// `Math.min(a, b)`: NaN if either is NaN, and -0 below +0.
#[must_use]
pub fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a == 0.0 && a.is_sign_negative() { a } else { b }
    } else if a < b {
        a
    } else {
        b
    }
}

/// `Math.sign(x)`: -1, 1, or x itself for ±0 and NaN.
#[must_use]
pub fn js_sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

/// `Number.parseInt(s, 10)`: leading whitespace, an optional sign, then the
/// longest run of decimal digits; NaN when there are none.
#[must_use]
pub fn js_parse_int10(s: &str) -> f64 {
    let s = s.trim_start_matches(is_js_whitespace);
    let (negative, rest) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return f64::NAN;
    }
    let value: f64 = rest[..digits].parse().unwrap_or(f64::NAN);
    if negative { -value } else { value }
}

/// `Number.prototype.toString()`: the shortest digits that round-trip,
/// placed as ECMAScript's Number::toString places them (plain notation for
/// exponents from -6 to 20, exponential outside).
#[must_use]
pub fn js_number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    let sign = if x < 0.0 { "-" } else { "" };
    // Rust's `{:e}` is also the shortest round-trip form: "d.ddde[-]n".
    let formatted = format!("{:e}", x.abs());
    let (mantissa, exponent) = formatted.split_once('e').expect("exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let k = digits.len() as i32;
    let n = exponent.parse::<i32>().expect("exponent digits") + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let e = if e < 0 { format!("-{}", -e) } else { format!("+{e}") };
        if k == 1 {
            format!("{digits}e{e}")
        } else {
            format!("{}.{}e{e}", &digits[..1], &digits[1..])
        }
    };
    format!("{sign}{body}")
}

/// `Math.sin`: V8's fdlibm port. The platform `sin` differs from it in the
/// last bit for about 3% of inputs, and differs again across platforms.
#[must_use]
pub fn js_sin(x: f64) -> f64 {
    crate::fdlibm::sin(x)
}

/// `Math.cos`: V8's fdlibm port, as [`js_sin`].
#[must_use]
pub fn js_cos(x: f64) -> f64 {
    crate::fdlibm::cos(x)
}

/// `Math.atan2`: V8's fdlibm port, as [`js_sin`].
#[must_use]
pub fn js_atan2(y: f64, x: f64) -> f64 {
    crate::fdlibm::atan2(y, x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_max_follow_math_min_max() {
        assert!(js_max(-0.0, 0.0).is_sign_positive());
        assert!(js_max(0.0, -0.0).is_sign_positive());
        assert!(js_min(0.0, -0.0).is_sign_negative());
        assert!(js_min(-0.0, 0.0).is_sign_negative());
        assert!(js_max(f64::NAN, 1.0).is_nan());
        assert!(js_min(1.0, f64::NAN).is_nan());
        assert_eq!(js_max(1.0, 2.0), 2.0);
        assert!(js_sign(-0.0).is_sign_negative());
    }

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

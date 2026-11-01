//! `js_sin` and `js_cos` against V8 (`tools/golden/generate.mjs`): small
//! angles, multiples of pi/4, every range-reduction path up to 1e300, and
//! random bit patterns.

use shagger_shared::js::{js_cos, js_sin};

fn f(hex: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(hex, 16).expect("hex f64"))
}

/// Equal bits, except that any NaN equals any NaN.
fn same(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

#[test]
fn matches_v8() {
    let mut count = 0;
    for (number, line) in include_str!("golden/js_trig.txt")
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.is_empty() && !l.starts_with('#'))
    {
        let p: Vec<f64> = line.split(' ').map(f).collect();
        let (x, sin, cos) = (p[0], p[1], p[2]);
        assert!(
            same(js_sin(x), sin),
            "line {}: sin({x:e}) = {:e}, V8 {sin:e}",
            number + 1,
            js_sin(x)
        );
        assert!(
            same(js_cos(x), cos),
            "line {}: cos({x:e}) = {:e}, V8 {cos:e}",
            number + 1,
            js_cos(x)
        );
        count += 1;
    }
    assert!(count > 18_000, "{count} vectors");
}

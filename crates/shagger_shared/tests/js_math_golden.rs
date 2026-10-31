//! `js_round` and `js_hypot` against results computed by V8
//! (`tools/golden/generate.mjs`).

use shagger_shared::js::{js_hypot, js_round};

fn f(hex: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(hex, 16).expect("hex f64"))
}

/// Equal bits, except that any NaN equals any NaN.
fn same(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

#[test]
fn matches_v8() {
    let text = include_str!("golden/js_math.txt");
    let (mut rounds, mut hypots) = (0, 0);
    for (number, line) in text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.is_empty() && !l.starts_with('#'))
    {
        let mut parts = line.split(' ');
        match parts.next() {
            Some("round") => {
                let x = f(parts.next().unwrap());
                let expected = f(parts.next().unwrap());
                let actual = js_round(x);
                assert!(
                    same(actual, expected),
                    "line {}: round({x:e}) = {actual:e}, V8 {expected:e}",
                    number + 1
                );
                rounds += 1;
            }
            Some("hypot") => {
                let n: usize = parts.next().unwrap().parse().unwrap();
                let values: Vec<f64> = (0..n).map(|_| f(parts.next().unwrap())).collect();
                let expected = f(parts.next().unwrap());
                let actual = js_hypot(&values);
                assert!(
                    same(actual, expected),
                    "line {}: hypot({values:?}) = {actual:e}, V8 {expected:e}",
                    number + 1
                );
                hypots += 1;
            }
            other => panic!("line {}: unknown kind {other:?}", number + 1),
        }
    }
    assert!(rounds > 2000 && hypots > 2000, "golden file looks truncated");
}

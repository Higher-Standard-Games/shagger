//! `js_atan2` against V8 (`tools/golden/generate.mjs`): the signed zeros,
//! infinities and NaN, every reduction band of `atan`, and random pairs.

use shagger_shared::js::js_atan2;

fn f(hex: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(hex, 16).expect("hex f64"))
}

#[test]
fn matches_v8() {
    let mut count = 0;
    for (number, line) in include_str!("golden/js_atan2.txt")
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.is_empty() && !l.starts_with('#'))
    {
        let p: Vec<f64> = line.split(' ').map(f).collect();
        let (y, x, want) = (p[0], p[1], p[2]);
        let got = js_atan2(y, x);
        assert!(
            (got.is_nan() && want.is_nan()) || got.to_bits() == want.to_bits(),
            "line {}: atan2({y:e}, {x:e}) = {got:e}, V8 {want:e}",
            number + 1
        );
        count += 1;
    }
    assert!(count > 12_000, "{count} vectors");
}

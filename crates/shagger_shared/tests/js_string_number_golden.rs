//! `js_trim`, `js_parse_float` and `js_string_to_number` against V8
//! (`tools/golden/generate.mjs`).

use shagger_shared::js::{js_parse_float, js_string_to_number, js_trim};

fn text(hex: &str) -> String {
    if hex == "-" {
        return String::new();
    }
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    String::from_utf8(bytes).unwrap()
}

fn f(hex: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(hex, 16).unwrap())
}

fn same(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

#[test]
fn matches_v8() {
    let mut count = 0;
    for line in include_str!("golden/js_string_number.txt")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let parts: Vec<&str> = line.split(' ').collect();
        let input = text(parts[0]);
        assert_eq!(js_trim(&input), text(parts[1]), "trim {input:?}");
        let (p, n) = (js_parse_float(&input), js_string_to_number(&input));
        assert!(
            same(p, f(parts[2])),
            "parseFloat({input:?}) = {p:e}, V8 {:e}",
            f(parts[2])
        );
        assert!(same(n, f(parts[3])), "Number({input:?}) = {n:e}, V8 {:e}", f(parts[3]));
        count += 1;
    }
    assert!(count > 400);
}

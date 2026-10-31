//! `pk3_index_hash` against SHAG's own `pk3IndexHash` (`tools/golden/generate.mjs`).

use shagger::loader::pk3::published_index::pk3_index_hash;

fn utf8(hex: &str) -> String {
    if hex == "-" {
        return String::new();
    }
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex byte"))
        .collect();
    String::from_utf8(bytes).expect("utf-8 input")
}

#[test]
fn matches_shag() {
    let text = include_str!("golden/pk3_index_hash.txt");
    let mut cases = 0;
    for line in text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let (input, expected) = line.split_once(' ').expect("two fields");
        let input = utf8(input);
        assert_eq!(pk3_index_hash(&input), expected, "input {input:?}");
        cases += 1;
    }
    assert!(cases > 300, "golden file looks truncated");
}

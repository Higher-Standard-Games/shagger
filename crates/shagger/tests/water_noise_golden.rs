//! `WaterNoise.ts` against V8: every height and every byte of the normal map
//! (`tools/golden/generate.mjs`).

use shagger::rendering::water::water_noise::{WATER_NOISE_SIZE, generate_water_normal_map, water_height_at};

fn f(hex: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(hex, 16).expect("hex bits"))
}

#[test]
fn water_noise_matches_v8() {
    let text = include_str!("golden/water_noise.txt");
    let mut lines = text.lines().filter(|l| !l.starts_with('#') && !l.is_empty());
    let mut heights = 0;
    let mut map_rows = Vec::new();
    let mut size = 0;
    for line in lines.by_ref() {
        let parts: Vec<&str> = line.split(' ').collect();
        match parts[0] {
            "h" => {
                let (u, v, octaves) = (f(parts[1]), f(parts[2]), parts[3].parse().unwrap());
                assert_eq!(
                    water_height_at(u, v, octaves).to_bits(),
                    f(parts[4]).to_bits(),
                    "{line}"
                );
                heights += 1;
            }
            "map" => {
                size = parts[1].parse::<usize>().unwrap();
                break;
            }
            _ => panic!("unexpected line {line}"),
        }
    }
    map_rows.extend(lines);
    assert_eq!(heights, 400);
    assert_eq!(size, WATER_NOISE_SIZE);
    let map = generate_water_normal_map(WATER_NOISE_SIZE, 2.0);
    for (y, row) in map_rows.iter().enumerate() {
        let expected: Vec<u8> = (0..row.len() / 2)
            .map(|i| u8::from_str_radix(&row[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        assert_eq!(&map[y * size * 4..(y + 1) * size * 4], &expected[..], "row {y}");
    }
    assert_eq!(map_rows.len(), size);
}

//! Port of SHAG `src/engine/loader/bsp/WorldFog.ts`: world (atmospheric)
//! fog as Warfork maps set it in worldspawn, read the way Warfork reads it
//! (r_q3bsp.c). Values stay in Quake units and axes (Z up);
//! [`world_fog_to_engine`] turns them into engine space.
//!
//! ```text
//! fog        "fog_dist r g b [mindist]"             ~95% opaque at fog_dist
//! fog_height "z_clear z_full"                        height band
//! fog_sun    "dx dy dz r g b [intensity] [exponent]" sun glow through the fog
//! fog_sky    "[horizon_bias] [horizon_scale] [zenith_falloff]" sky horizon tint
//! ```
//!
//! Each key may start with an underscore. Density is 3 / fog_dist, so
//! 1 - e^-3 ≈ 95% at fog_dist.

use shagger_shared::js::js_hypot;

/// The height band: clear at `clear`, full at `full` (Quake Z, engine Y).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FogHeight {
    pub clear: f64,
    pub full: f64,
}

/// The sun's glow; `dir` normalised.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FogSun {
    pub dir: [f64; 3],
    pub color: [f64; 3],
    pub intensity: f64,
    pub exponent: f64,
}

/// The sky's horizon tint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FogSky {
    pub horizon_bias: f64,
    pub horizon_scale: f64,
    pub zenith_falloff: f64,
}

/// `WorldFogConfig` (and `EngineWorldFog`, the same shape in engine axes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldFogConfig {
    pub color: [f64; 3],
    /// Extinction per unit: 3 / fog_dist.
    pub density: f64,
    /// Distance at which the fog starts.
    pub min_dist: f64,
    pub height: Option<FogHeight>,
    pub sun: Option<FogSun>,
    pub sky: Option<FogSky>,
}

/// `leadingNumbers`: as C's `sscanf("%f %f ...")`, stopping at the first non-number.
#[must_use]
pub fn leading_numbers(value: Option<&str>, max: usize) -> Vec<f64> {
    let mut out = Vec::new();
    for token in value.unwrap_or("").split_whitespace() {
        if out.len() >= max {
            break;
        }
        match parse_float_prefix(token) {
            Some(n) if n.is_finite() => out.push(n),
            _ => break,
        }
    }
    out
}

/// `Number.parseFloat` of a token: its longest leading decimal number.
fn parse_float_prefix(token: &str) -> Option<f64> {
    let bytes = token.as_bytes();
    let mut end = 0;
    let mut best = None;
    while end < bytes.len() {
        end += 1;
        if let Ok(n) = token[..end].parse::<f64>()
            && !token[..end].ends_with(['e', 'E', '+', '-'])
            && !token[..end].eq_ignore_ascii_case("inf")
            && !token[..end].eq_ignore_ascii_case("nan")
        {
            best = Some(n);
        }
    }
    best
}

/// A key compared without case, with or without a leading underscore.
fn key<'a>(properties: &[(&'a str, &'a str)], name: &str) -> Option<&'a str> {
    properties.iter().find_map(|(k, v)| {
        let lower = k.to_ascii_lowercase();
        (lower == name || lower.strip_prefix('_') == Some(name)).then_some(*v)
    })
}

/// `parseWorldFog`: the map's world fog from its worldspawn keys, or `None`.
#[must_use]
pub fn parse_world_fog(properties: &[(&str, &str)]) -> Option<WorldFogConfig> {
    let fog = leading_numbers(key(properties, "fog"), 5);
    if fog.len() < 4 {
        return None;
    }
    let fog_dist = fog[0];
    let mut config = WorldFogConfig {
        color: [fog[1], fog[2], fog[3]],
        density: if fog_dist > 0.0 { 3.0 / fog_dist } else { 0.0 },
        min_dist: if fog.len() >= 5 { fog[4].max(0.0) } else { 0.0 },
        height: None,
        sun: None,
        sky: None,
    };
    let height = leading_numbers(key(properties, "fog_height"), 2);
    if height.len() >= 2 && height[0] != height[1] {
        config.height = Some(FogHeight {
            clear: height[0],
            full: height[1],
        });
    }
    let sun = leading_numbers(key(properties, "fog_sun"), 8);
    if sun.len() >= 6 {
        let (mut dx, mut dy, mut dz) = (sun[0], sun[1], sun[2]);
        let length = js_hypot(&[dx, dy, dz]);
        if length > 0.0001 {
            dx /= length;
            dy /= length;
            dz /= length;
        }
        config.sun = Some(FogSun {
            dir: [dx, dy, dz],
            color: [sun[3], sun[4], sun[5]],
            intensity: sun.get(6).copied().unwrap_or(1.5),
            exponent: sun.get(7).copied().unwrap_or(16.0).max(1.0),
        });
    }
    let sky = leading_numbers(key(properties, "fog_sky"), 3);
    if let Some(&bias) = sky.first() {
        config.sky = Some(FogSky {
            horizon_bias: bias,
            horizon_scale: sky.get(1).copied().unwrap_or(2.0),
            zenith_falloff: sky.get(2).copied().unwrap_or(3.0),
        });
    }
    Some(config)
}

/// `worldFogToEngine`: Quake Z becomes engine Y (the sun's direction swaps
/// its Y and Z; heights carry over as they are).
#[must_use]
pub fn world_fog_to_engine(fog: &WorldFogConfig) -> WorldFogConfig {
    let mut out = *fog;
    if let Some(sun) = &mut out.sun {
        sun.dir = [sun.dir[0], sun.dir[2], sun.dir[1]];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_like_warfork() {
        let fog = parse_world_fog(&[("_FOG", "1500 0.8 0.5 0.3 256"), ("fog_height", "128 4500")]).unwrap();
        assert_eq!(fog.density, 3.0 / 1500.0);
        assert_eq!(fog.min_dist, 256.0);
        assert_eq!(fog.color, [0.8, 0.5, 0.3]);
        assert_eq!(
            fog.height,
            Some(FogHeight {
                clear: 128.0,
                full: 4500.0
            })
        );
        assert!(parse_world_fog(&[("fog", "1500 0.8 0.5")]).is_none());
        assert_eq!(leading_numbers(Some("1 2x 3"), 5), [1.0, 2.0, 3.0]);
        assert_eq!(leading_numbers(Some(" .5 -1e2 abc 4"), 5), [0.5, -100.0]);
        let sun = parse_world_fog(&[("fog", "1000 0 0 0"), ("fog_sun", "0 0 2 1 1 1")])
            .unwrap()
            .sun
            .unwrap();
        assert_eq!((sun.dir, sun.intensity, sun.exponent), ([0.0, 0.0, 1.0], 1.5, 16.0));
        let engine =
            world_fog_to_engine(&parse_world_fog(&[("fog", "1000 0 0 0"), ("fog_sun", "0 0 2 1 1 1")]).unwrap());
        assert_eq!(engine.sun.unwrap().dir, [0.0, 1.0, 0.0]);
        let sky = parse_world_fog(&[("fog", "1000 0 0 0"), ("fog_sky", "0.1")])
            .unwrap()
            .sky
            .unwrap();
        assert_eq!((sky.horizon_scale, sky.zenith_falloff), (2.0, 3.0));
    }
}

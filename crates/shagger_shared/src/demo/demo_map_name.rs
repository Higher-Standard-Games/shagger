//! SHAG `src/shared/demo/DemoMapName.ts`.

use crate::js::js_trim;

/// `normalizeDemoMapName`: the bare, lowercased map key a demo's map name
/// names (`"maps/WDM7.bsp"` → `"wdm7"`).
#[must_use]
pub fn normalize_demo_map_name(raw: Option<&str>) -> String {
    let Some(raw) = raw.filter(|r| !r.is_empty()) else {
        return String::new();
    };
    let base = js_trim(raw).replace('\\', "/");
    let base = base.rsplit('/').next().unwrap_or("");
    let lower = base.to_ascii_lowercase();
    let stem = [".bsp", ".pk3"]
        .iter()
        .find_map(|ext| lower.ends_with(ext).then(|| &base[..base.len() - ext.len()]))
        .unwrap_or(base);
    js_trim(stem).to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::normalize_demo_map_name as n;

    #[test]
    fn normalizes_like_shag() {
        assert_eq!(n(None), "");
        assert_eq!(n(Some("")), "");
        assert_eq!(n(Some("maps/WDM7.bsp")), "wdm7");
        assert_eq!(n(Some("C:\\maps\\Washoi_DM2.PK3")), "washoi_dm2");
        assert_eq!(n(Some("  washoi_dm14 ")), "washoi_dm14");
        assert_eq!(n(Some("maps/")), "");
        assert_eq!(n(Some("a.bsp.bsp")), "a.bsp");
    }
}

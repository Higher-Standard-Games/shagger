//! SHAG `src/engine/rendering/RenderToggles.ts`: the frame-cost switches
//! (`r_dynamiclight`, `r_marks`, `r_gibs`, `r_teleport_ghost`) that are
//! on unless turned off. The video page's presets (Ultralow) turn them off.

use crate::core::cvar::Cvars;

/// `renderToggle`: whether a switch is on. One nobody registered (tests,
/// headless tools) counts as on, as its default does.
#[must_use]
pub fn render_toggle(cvars: &Cvars, name: &str) -> bool {
    !cvars.exists(name) || cvars.get_bool(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cvar::CvarOptions;

    #[test]
    fn on_unless_turned_off() {
        let mut cvars = Cvars::default();
        assert!(render_toggle(&cvars, "r_gibs"));
        cvars.register("r_gibs", 1, CvarOptions::default());
        assert!(render_toggle(&cvars, "r_gibs"));
        cvars.set("r_gibs", 0);
        assert!(!render_toggle(&cvars, "r_gibs"));
    }
}

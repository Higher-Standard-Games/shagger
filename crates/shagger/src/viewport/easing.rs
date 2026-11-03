//! SHAG `src/engine/viewport/Easing.ts`: the curves the effects use.

/// `Easing.easeOutQuad`.
#[must_use]
pub fn ease_out_quad(t: f64) -> f64 {
    t * (2.0 - t)
}

/// `Easing.easeInOutQuad`.
#[must_use]
pub fn ease_in_out_quad(t: f64) -> f64 {
    if t < 0.5 {
        2.0 * t * t
    } else {
        -1.0 + (4.0 - 2.0 * t) * t
    }
}

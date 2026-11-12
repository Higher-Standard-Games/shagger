//! Native only, no SHAG source: the browser hides the cursor with pointer
//! lock. winit's `set_cursor_visible(false)` does it on macOS through cursor
//! rects, which AppKit applies only when the cursor moves; a locked cursor
//! (`CursorGrabMode::Locked`) never moves, so the arrow stayed drawn over
//! the match. There the cursor is hidden through `NSCursor` as well.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether this module hid the cursor: `NSCursor` counts hides and unhides,
/// so each is sent once per change.
static HIDDEN: AtomicBool = AtomicBool::new(false);

/// Hides the system cursor while `hidden`, on top of the window's own
/// `set_cursor_visible`. Does nothing where that is enough (not macOS).
/// Call it on the main thread.
pub fn set_cursor_hidden(hidden: bool) {
    let changed = HIDDEN.swap(hidden, Ordering::Relaxed) != hidden;
    #[cfg(target_os = "macos")]
    match (changed, hidden) {
        (true, true) => objc2_app_kit::NSCursor::hide(),
        (true, false) => objc2_app_kit::NSCursor::unhide(),
        _ => {}
    }
    #[cfg(not(target_os = "macos"))]
    let _ = changed;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_and_shows_once_per_change() {
        set_cursor_hidden(true);
        set_cursor_hidden(true);
        assert!(HIDDEN.load(Ordering::Relaxed));
        set_cursor_hidden(false);
        set_cursor_hidden(false);
        assert!(!HIDDEN.load(Ordering::Relaxed));
    }
}

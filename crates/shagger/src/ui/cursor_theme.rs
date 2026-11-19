//! SHAG's cursor tokens (`--ui-cursor`, `--ui-cursor-hover`,
//! `--ui-cursor-disabled`, `--ui-cursor-text`; the game fills them in, as
//! wash-fps `client/styles/Cursors.css` does) for native windows: a game
//! installs its art once, and every [`super::egui_layer::EguiLayer`] then
//! shows it in place of the system arrow, hand, not-allowed and text
//! cursors. Without a theme the system cursors stay.
//!
//! winit sizes a cursor in physical pixels on Windows, X11 and Wayland, and
//! in points on macOS, so the art comes at several scales: the largest at
//! or under the window's scale factor is used (always the 1x art on macOS).

use std::cell::RefCell;
use std::rc::Rc;

use winit::event_loop::ActiveEventLoop;
use winit::window::{Cursor, CustomCursor, Window};

/// One cursor image: straight (not premultiplied) RGBA rows.
#[derive(Debug, Clone)]
pub struct CursorImage {
    pub rgba: Vec<u8>,
    pub size: [u16; 2],
    /// The point that clicks, in the image's pixels.
    pub hotspot: [u16; 2],
}

/// Which token a cursor fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CursorRole {
    /// `--ui-cursor`: the arrow.
    Arrow,
    /// `--ui-cursor-hover`: the hand over something to press.
    Hand,
    /// `--ui-cursor-disabled`: over something that can't be pressed.
    Disabled,
    /// `--ui-cursor-text`: over a text field.
    Text,
}

impl CursorRole {
    const ALL: [Self; 4] = [Self::Arrow, Self::Hand, Self::Disabled, Self::Text];

    /// The role egui's icon takes, or `None` for icons the theme leaves to
    /// the system (resize arrows, grab, ...).
    #[must_use]
    pub fn for_icon(icon: egui::CursorIcon) -> Option<Self> {
        use egui::CursorIcon as I;
        match icon {
            I::Default | I::ContextMenu | I::Help | I::Alias | I::Copy | I::Progress | I::Wait => Some(Self::Arrow),
            I::PointingHand | I::ZoomIn | I::ZoomOut => Some(Self::Hand),
            I::NotAllowed | I::NoDrop => Some(Self::Disabled),
            I::Text | I::VerticalText => Some(Self::Text),
            _ => None,
        }
    }
}

/// The game's art at one display scale.
#[derive(Debug, Clone)]
pub struct CursorArt {
    /// The display scale it is drawn for (1 = CSS pixels).
    pub scale: f32,
    pub arrow: CursorImage,
    pub hand: CursorImage,
    pub disabled: CursorImage,
    pub text: CursorImage,
}

impl CursorArt {
    fn image(&self, role: CursorRole) -> &CursorImage {
        match role {
            CursorRole::Arrow => &self.arrow,
            CursorRole::Hand => &self.hand,
            CursorRole::Disabled => &self.disabled,
            CursorRole::Text => &self.text,
        }
    }
}

/// The art made into the system's cursors, by scale.
struct Theme {
    /// Ascending scale; each with the four roles in [`CursorRole::ALL`] order.
    sets: Vec<(f32, Vec<CustomCursor>)>,
}

thread_local! {
    /// Installed on the event loop's thread, which is where windows live.
    static THEME: RefCell<Option<Rc<Theme>>> = const { RefCell::new(None) };
}

/// Makes `arts` the cursors of every window from now on. A cursor is tied to
/// the event loop that made it; call this once the process's loop runs.
/// Art that winit refuses (a size of zero, a hotspot outside) is skipped.
pub fn install(event_loop: &ActiveEventLoop, arts: &[CursorArt]) {
    let mut sets: Vec<(f32, Vec<CustomCursor>)> = arts
        .iter()
        .filter_map(|art| {
            let cursors = CursorRole::ALL
                .iter()
                .map(|&role| {
                    let image = art.image(role);
                    let source = CustomCursor::from_rgba(
                        image.rgba.clone(),
                        image.size[0],
                        image.size[1],
                        image.hotspot[0],
                        image.hotspot[1],
                    )
                    .ok()?;
                    Some(event_loop.create_custom_cursor(source))
                })
                .collect::<Option<Vec<_>>>()?;
            Some((art.scale, cursors))
        })
        .collect();
    sets.sort_by(|a, b| a.0.total_cmp(&b.0));
    THEME.with(|t| *t.borrow_mut() = (!sets.is_empty()).then(|| Rc::new(Theme { sets })));
}

/// Whether a theme is installed.
#[must_use]
pub fn installed() -> bool {
    THEME.with(|t| t.borrow().is_some())
}

/// The scale whose art a window at `scale_factor` shows: the largest at or
/// under it, else the smallest. macOS sizes cursors in points: always 1.
#[must_use]
pub fn pick_scale(scales: &[f32], scale_factor: f64) -> Option<usize> {
    let want = if cfg!(target_os = "macos") {
        1.0
    } else {
        scale_factor as f32
    };
    if scales.is_empty() {
        return None;
    }
    Some(scales.iter().rposition(|&s| s <= want + 0.01).unwrap_or(0))
}

/// The theme's cursor for `role` on `window`, if one is installed.
fn themed(window: &Window, role: CursorRole) -> Option<CustomCursor> {
    THEME.with(|t| {
        let theme = t.borrow().clone()?;
        let scales: Vec<f32> = theme.sets.iter().map(|(s, _)| *s).collect();
        let i = pick_scale(&scales, window.scale_factor())?;
        let at = CursorRole::ALL.iter().position(|r| *r == role)?;
        theme.sets[i].1.get(at).cloned()
    })
}

/// Shows egui's `icon` on `window`: the theme's art when it has the role,
/// otherwise the system cursor. True when the theme drew it.
pub fn apply(window: &Window, icon: egui::CursorIcon) -> bool {
    if let Some(custom) = CursorRole::for_icon(icon).and_then(|role| themed(window, role)) {
        window.set_cursor_visible(true);
        window.set_cursor(Cursor::Custom(custom));
        return true;
    }
    false
}

/// Re-applies the theme as egui's icons change; one per window, beside its
/// `egui_winit::State`, which sets the system cursor first.
#[derive(Debug, Default)]
pub struct CursorFollower {
    /// What was last applied: the icon and the window's scale factor.
    shown: Option<(egui::CursorIcon, u64)>,
}

impl CursorFollower {
    /// After `egui_winit` handled the frame's output: show the theme's
    /// cursor when the icon or the scale changed, or the pointer came back
    /// (`egui_winit` sets the system cursor again then).
    pub fn follow(&mut self, window: &Window, icon: egui::CursorIcon, pointer_in_window: bool) {
        if !pointer_in_window || !installed() {
            self.shown = None;
            return;
        }
        let now = (icon, window.scale_factor().to_bits());
        if self.shown == Some(now) {
            return;
        }
        self.shown = apply(window, icon).then_some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egui_icons_take_the_css_tokens() {
        assert_eq!(CursorRole::for_icon(egui::CursorIcon::Default), Some(CursorRole::Arrow));
        assert_eq!(
            CursorRole::for_icon(egui::CursorIcon::PointingHand),
            Some(CursorRole::Hand)
        );
        assert_eq!(
            CursorRole::for_icon(egui::CursorIcon::NotAllowed),
            Some(CursorRole::Disabled)
        );
        assert_eq!(CursorRole::for_icon(egui::CursorIcon::Text), Some(CursorRole::Text));
        assert_eq!(CursorRole::for_icon(egui::CursorIcon::ResizeEast), None);
        assert_eq!(CursorRole::for_icon(egui::CursorIcon::None), None);
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn the_art_at_or_under_the_scale_is_shown() {
        let scales = [1.0, 1.25, 1.5, 2.0, 3.0];
        assert_eq!(pick_scale(&scales, 1.0), Some(0));
        assert_eq!(pick_scale(&scales, 1.25), Some(1));
        assert_eq!(pick_scale(&scales, 1.75), Some(2));
        assert_eq!(pick_scale(&scales, 2.0), Some(3));
        assert_eq!(pick_scale(&scales, 4.0), Some(4));
        assert_eq!(pick_scale(&scales, 0.75), Some(0));
        assert_eq!(pick_scale(&[], 1.0), None);
    }
}

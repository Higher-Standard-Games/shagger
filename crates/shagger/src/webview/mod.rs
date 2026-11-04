//! A web page inside the game window: the system's own webview (wry), as a
//! child of the winit window, over the wgpu surface. Washoi shows its lobby
//! this way (wash-fps-rust `docs/research/WEBVIEW_LOBBY.md`); nothing here is
//! specific to it.
//!
//! - **The bridge.** The page posts JSON text with `window.ipc.postMessage`;
//!   [`WebviewHost::drain`] hands it over as [`HostEvent::Message`]. The
//!   host answers with [`WebviewHost::send`], which calls
//!   `window.<global>.receive(json)` in the page. `<global>` is set up
//!   before any page script runs, from [`WebviewConfig::info`].
//! - **Origins.** Messages from a page outside
//!   [`WebviewConfig::allowed_origins`] are dropped. The webview never leaves
//!   those origins: any other `http(s)` link is opened in the system browser
//!   and reported as [`HostEvent::External`].
//! - **Hidden is not paused.** Background throttling is switched off, so a
//!   hidden page keeps its timers (W0 found WebView2 slows them to once a
//!   minute otherwise).
//! - **Linux** needs GTK: [`init_platform`] once, before the first webview,
//!   and [`pump`] every frame of every screen while a webview lives. Child
//!   webviews work on X11 only; the game runs winit on X11 (XWayland on a
//!   Wayland desktop).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;

use winit::dpi::PhysicalSize;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// What the page and the webview had to say since the last [`WebviewHost::drain`].
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    /// A bridge message from an allowed page, parsed.
    Message(serde_json::Value),
    /// The page at this URL finished loading.
    Loaded(String),
    /// A link to another site, already opened in the system browser.
    External(String),
    /// Text from the page that was not a JSON object.
    Malformed(String),
}

/// How to build a [`WebviewHost`].
#[derive(Debug, Clone)]
pub struct WebviewConfig {
    /// The page to open.
    pub url: String,
    /// `scheme://host[:port]` the page may live on, and the bridge answers.
    pub allowed_origins: Vec<String>,
    /// The page's global the host defines before page scripts run.
    pub global: String,
    /// Its fields (a JSON object). `receive` is added as a stub the page replaces.
    pub info: serde_json::Value,
    /// Script run after `info` is defined, before page scripts (for example
    /// to mark the document for CSS).
    pub extra_script: String,
    /// Where the webview keeps its profile (local storage, cache).
    pub data_dir: Option<PathBuf>,
    /// The browser's developer tools (right click, Inspect).
    pub devtools: bool,
}

/// `scheme://host[:port]` of `url`, lowercased; `None` for anything else.
#[must_use]
pub fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    if host.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{}", host.to_ascii_lowercase()))
}

/// Whether `url` is on one of `allowed` origins.
#[must_use]
pub fn allowed(url: &str, allowed: &[String]) -> bool {
    origin_of(url).is_some_and(|o| allowed.iter().any(|a| a.eq_ignore_ascii_case(&o)))
}

/// The script defining `global` from `info`, then `extra`.
#[must_use]
pub fn init_script(global: &str, info: &serde_json::Value, extra: &str) -> String {
    format!(
        "(function(){{var i={info};i.receive=function(){{}};Object.defineProperty(window,{global:?},{{value:i,writable:false,configurable:false}});}})();{extra}",
    )
}

/// The call that hands `message` to the page.
#[must_use]
pub fn receive_script(global: &str, message: &serde_json::Value) -> String {
    format!("window[{global:?}]&&window[{global:?}].receive({message});")
}

/// Opens an `https` URL in the system browser. Anything else is refused.
pub fn open_external(url: &str) -> bool {
    if !url.starts_with("https://") || url.chars().any(|c| c.is_control() || c == '"') {
        return false;
    }
    #[cfg(target_os = "linux")]
    let mut c = std::process::Command::new("xdg-open");
    #[cfg(target_os = "macos")]
    let mut c = std::process::Command::new("open");
    #[cfg(windows)]
    let mut c = {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    c.arg(url).spawn().is_ok()
}

/// GTK for the webview on Linux; nothing elsewhere. Call once, on the main
/// thread, before the first [`WebviewHost::new`].
///
/// # Errors
/// GTK did not start (no display).
pub fn init_platform() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    gtk::init().map_err(|e| format!("gtk: {e}"))?;
    Ok(())
}

/// Keeps X errors on GTK's own connection away from winit (Linux, X11).
///
/// winit's Xlib error handler is process wide: it records every error,
/// whichever connection raised it, and fails the screen's event loop on the
/// next sync. WebKitGTK's GL compositor raises harmless ones (`GLXBadWindow`
/// when the page is hidden for a match), so errors on any connection but
/// winit's own are left to GTK. Registered once.
#[cfg(target_os = "linux")]
fn keep_gtk_errors_from_winit(window: &impl HasDisplayHandle) {
    use std::sync::Once;
    use winit::raw_window_handle::RawDisplayHandle;
    static ONCE: Once = Once::new();
    let Ok(handle) = window.display_handle() else {
        return;
    };
    let RawDisplayHandle::Xlib(xlib) = handle.as_raw() else {
        return;
    };
    let Some(ours) = xlib.display.map(|d| d.as_ptr() as usize) else {
        return;
    };
    ONCE.call_once(|| {
        winit::platform::x11::register_xlib_error_hook(Box::new(move |display, _error| display as usize != ours));
    });
}

/// Runs GTK's pending work (Linux; nothing elsewhere). Every screen calls it
/// each frame while a webview lives, hidden or not, or the page stalls.
pub fn pump() {
    #[cfg(target_os = "linux")]
    if gtk::is_initialized_main_thread() {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
    }
}

/// A page in a child webview of a window.
pub struct WebviewHost {
    webview: wry::WebView,
    inbox: Rc<RefCell<VecDeque<HostEvent>>>,
    global: String,
    visible: bool,
    _context: wry::WebContext,
}

impl std::fmt::Debug for WebviewHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebviewHost")
            .field("visible", &self.visible)
            .finish_non_exhaustive()
    }
}

fn bounds(size: PhysicalSize<u32>) -> wry::Rect {
    wry::Rect {
        position: wry::dpi::PhysicalPosition::new(0, 0).into(),
        size: wry::dpi::PhysicalSize::new(size.width.max(1), size.height.max(1)).into(),
    }
}

impl WebviewHost {
    /// Opens `config.url` in a child webview filling `window` (`size`).
    ///
    /// # Errors
    /// The platform's webview could not be created (no WebView2 runtime, no
    /// WebKitGTK, no X11 window).
    pub fn new(
        window: &(impl HasWindowHandle + HasDisplayHandle),
        size: PhysicalSize<u32>,
        config: &WebviewConfig,
    ) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        keep_gtk_errors_from_winit(window);
        let inbox: Rc<RefCell<VecDeque<HostEvent>>> = Rc::default();
        let mut context = wry::WebContext::new(config.data_dir.clone());
        let origins = config.allowed_origins.clone();
        let ipc_inbox = inbox.clone();
        let ipc_origins = origins.clone();
        let nav_inbox = inbox.clone();
        let nav_origins = origins.clone();
        let new_window_inbox = inbox.clone();
        let load_inbox = inbox.clone();
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut builder = wry::WebViewBuilder::new_with_web_context(&mut context)
            .with_url(&config.url)
            .with_bounds(bounds(size))
            .with_devtools(config.devtools)
            .with_autoplay(true)
            .with_background_throttling(wry::BackgroundThrottlingPolicy::Disabled)
            .with_initialization_script_for_main_only(
                init_script(&config.global, &config.info, &config.extra_script),
                true,
            )
            .with_ipc_handler(move |req: wry::http::Request<String>| {
                // The page's own URL: only our origins speak to the host.
                if !allowed(&req.uri().to_string(), &ipc_origins) {
                    return;
                }
                let body = req.into_body();
                let event = match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(v) if v.is_object() => HostEvent::Message(v),
                    _ => HostEvent::Malformed(body.chars().take(200).collect()),
                };
                ipc_inbox.borrow_mut().push_back(event);
            })
            .with_navigation_handler(move |url: String| {
                if allowed(&url, &nav_origins) || url == "about:blank" {
                    return true;
                }
                if (url.starts_with("https://") || url.starts_with("http://")) && open_external(&url) {
                    nav_inbox.borrow_mut().push_back(HostEvent::External(url));
                }
                false
            })
            .with_new_window_req_handler(move |url: String, _features| {
                if open_external(&url) {
                    new_window_inbox.borrow_mut().push_back(HostEvent::External(url));
                }
                wry::NewWindowResponse::Deny
            })
            .with_on_page_load_handler(move |event, url| {
                if matches!(event, wry::PageLoadEvent::Finished) {
                    load_inbox.borrow_mut().push_back(HostEvent::Loaded(url));
                }
            })
            .with_download_started_handler(|_, _| false);
        #[cfg(windows)]
        {
            use wry::WebViewBuilderExtWindows;
            // wry's own switches first: these replace them. Then Chromium's
            // throttling of hidden pages, off (W0).
            builder = builder.with_additional_browser_args(
                "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,IntensiveWakeUpThrottling \
                 --autoplay-policy=no-user-gesture-required \
                 --disable-background-timer-throttling --disable-renderer-backgrounding",
            );
        }
        let webview = builder.build_as_child(window).map_err(|e| format!("webview: {e}"))?;
        Ok(Self {
            webview,
            inbox,
            global: config.global.clone(),
            visible: true,
            _context: context,
        })
    }

    /// Everything since the last call, oldest first.
    pub fn drain(&self) -> Vec<HostEvent> {
        self.inbox.borrow_mut().drain(..).collect()
    }

    /// Hands `message` to the page's `receive`.
    pub fn send(&self, message: &serde_json::Value) {
        let _ = self.webview.evaluate_script(&receive_script(&self.global, message));
    }

    /// Follows the window's new size.
    pub fn resize(&self, size: PhysicalSize<u32>) {
        let _ = self.webview.set_bounds(bounds(size));
    }

    /// Shows the page and gives it the keyboard.
    pub fn show(&mut self) {
        let _ = self.webview.set_visible(true);
        let _ = self.webview.focus();
        self.visible = true;
    }

    /// Hides the page (it keeps running) and gives the keyboard back to the window.
    pub fn hide(&mut self) {
        let _ = self.webview.set_visible(false);
        let _ = self.webview.focus_parent();
        self.visible = false;
    }

    #[must_use]
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Loads `url` (one of the allowed origins).
    pub fn load(&self, url: &str) {
        let _ = self.webview.load_url(url);
    }

    /// Reloads the page.
    pub fn reload(&self) {
        let _ = self.webview.reload();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_scheme_and_host() {
        assert_eq!(
            origin_of("https://EU.washoi.net/client?x=1"),
            Some("https://eu.washoi.net".into())
        );
        assert_eq!(
            origin_of("http://localhost:3100/"),
            Some("http://localhost:3100".into())
        );
        assert_eq!(
            origin_of("https://a@evil.example/x"),
            Some("https://evil.example".into())
        );
        assert_eq!(origin_of("file:///etc/passwd"), None);
        assert_eq!(origin_of("javascript:alert(1)"), None);
        let ok = vec!["https://eu.washoi.net".to_owned()];
        assert!(allowed("https://eu.washoi.net/client", &ok));
        assert!(!allowed("https://eu.washoi.net.evil.example/", &ok));
        assert!(!allowed("https://washoi.net/devlog", &ok));
        assert!(!allowed("about:blank", &ok));
    }

    #[test]
    fn scripts_quote_the_global() {
        let s = init_script("__washoiNative", &serde_json::json!({"bridgeVersion": 1}), "x();");
        assert!(s.contains(r#"defineProperty(window,"__washoiNative""#));
        assert!(s.ends_with("x();"));
        let r = receive_script("__washoiNative", &serde_json::json!({"type": "hello"}));
        assert_eq!(
            r,
            r#"window["__washoiNative"]&&window["__washoiNative"].receive({"type":"hello"});"#
        );
    }

    #[test]
    fn only_https_goes_to_the_system_browser() {
        assert!(!open_external("file:///etc/passwd"));
        assert!(!open_external("http://example.com"));
        assert!(!open_external("https://x\"; rm -rf"));
    }
}

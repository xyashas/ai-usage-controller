//! Widget window. One transparent, always-on-top window that never resizes:
//! the pill and the panel are drawn inside it and animate with CSS, so opening
//! and closing is smooth. The window lets clicks pass through everywhere except
//! the shape the UI reports (see `hover.rs`), so the transparent area never
//! blocks the apps underneath.
//!
//! Mac: the pill hangs from the notch (or the menu bar on Macs without one).
//! Windows: a pill at top centre.

use serde::{Deserialize, Serialize};
use tauri::{LogicalSize, Manager, PhysicalPosition, Runtime, WebviewWindow};

/// The window's fixed size. The panel is at most 520×640; the extra leaves room
/// for its shadow.
pub const WINDOW: (f64, f64) = (580.0, 700.0);

#[derive(Clone, Serialize)]
pub struct Layout {
    pub platform: &'static str,
    pub notch_height: f64,
    pub window: (f64, f64),
}

/// The part of the window that takes the mouse, in CSS pixels from the window's
/// top-left corner. Everything else is click-through.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        self.w > 0.0 && self.h > 0.0 && x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

pub fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

#[cfg(target_os = "macos")]
fn notch_height() -> f64 {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;
    let Some(mtm) = MainThreadMarker::new() else { return 0.0 };
    NSScreen::mainScreen(mtm).map(|s| s.safeAreaInsets().top as f64).unwrap_or(0.0)
}

#[cfg(not(target_os = "macos"))]
fn notch_height() -> f64 {
    0.0
}

pub fn layout() -> Layout {
    Layout { platform: platform(), notch_height: notch_height(), window: WINDOW }
}

/// Float above the menu bar on every Space, including full-screen apps.
#[cfg(target_os = "macos")]
pub fn mac_float<R: Runtime>(win: &WebviewWindow<R>) {
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
    if let Ok(ptr) = win.ns_window() {
        let ns: &NSWindow = unsafe { &*(ptr as *const NSWindow) };
        // NSMainMenuWindowLevel (24) + 1 so the widget can sit over the menu bar.
        ns.setLevel(25);
        ns.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        ns.setHasShadow(false);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn mac_float<R: Runtime>(_win: &WebviewWindow<R>) {}

/// Size the window and pin it to the top centre of its monitor's work area
/// (respects top/side taskbars, mixed-DPI scaling, and negative secondary monitor coordinates).
pub fn place<R: Runtime>(win: &WebviewWindow<R>) -> tauri::Result<()> {
    let (w, h) = WINDOW;
    let monitor = win.current_monitor()?.or(win.primary_monitor()?);
    win.set_size(LogicalSize::new(w, h))?;
    if let Some(m) = &monitor {
        let s = m.scale_factor().max(0.5);
        // Windows: the work area, so a taskbar on top doesn't cover the pill.
        // Mac: the whole screen, so the pill hangs from the notch / menu bar
        // (the work area starts below the menu bar).
        #[cfg(target_os = "macos")]
        let (ax, ay, aw) = (m.position().x, m.position().y, m.size().width as i32);
        #[cfg(not(target_os = "macos"))]
        let (ax, ay, aw) = { let wa = m.work_area(); (wa.position.x, wa.position.y, wa.size.width as i32) };
        let phys_w = (w * s).round() as i32;
        let x = ax + (aw - phys_w) / 2;
        let y = ay;
        win.set_position(PhysicalPosition::new(x, y))?;
    } else {
        win.set_position(PhysicalPosition::new(430, 0))?;
    }
    Ok(())
}

pub fn widget<R: Runtime, M: Manager<R>>(m: &M) -> Option<WebviewWindow<R>> {
    m.get_webview_window("widget")
}

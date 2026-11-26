//! Widget window placement. Mac: hangs from the notch (or the menu bar on Macs
//! without one), floating above the menu bar. Windows: a pill at top centre.

use serde::Serialize;
use tauri::{LogicalPosition, LogicalSize, Manager, Runtime, WebviewWindow};

pub const EXPANDED: (f64, f64) = (380.0, 480.0);
const WIN_TOP_GAP: f64 = 6.0;

#[derive(Clone, Serialize)]
pub struct Layout {
    pub platform: &'static str,
    pub notch_height: f64,
    pub collapsed: (f64, f64),
    pub expanded: (f64, f64),
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
    let notch = notch_height();
    let collapsed = match platform() {
        // Notch Macs: the bar sits just under the camera housing, blending into it.
        "macos" if notch > 0.0 => (220.0, notch + 16.0),
        "macos" => (210.0, 24.0),
        _ => (230.0, 34.0),
    };
    Layout { platform: platform(), notch_height: notch, collapsed, expanded: EXPANDED }
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

/// Resize and re-centre the widget for the given mode. `height` lets the panel
/// fit its content (clamped), so a quiet day doesn't show an empty box.
pub fn apply<R: Runtime>(win: &WebviewWindow<R>, expanded: bool, height: Option<f64>) -> tauri::Result<()> {
    let l = layout();
    let (w, mut h) = if expanded { l.expanded } else { l.collapsed };
    if expanded {
        if let Some(fit) = height {
            h = fit.clamp(180.0, 560.0);
        }
    }
    let monitor = win.current_monitor()?.or(win.primary_monitor()?);
    let (mx, my, mw) = match &monitor {
        Some(m) => {
            let s = m.scale_factor();
            (m.position().x as f64 / s, m.position().y as f64 / s, m.size().width as f64 / s)
        }
        None => (0.0, 0.0, 1440.0),
    };
    let y = my + if l.platform == "macos" { 0.0 } else { WIN_TOP_GAP };
    win.set_size(LogicalSize::new(w, h))?;
    win.set_position(LogicalPosition::new(mx + (mw - w) / 2.0, y))?;
    Ok(())
}

pub fn widget<R: Runtime, M: Manager<R>>(m: &M) -> Option<WebviewWindow<R>> {
    m.get_webview_window("widget")
}

//! Hover tracking for the click-through widget window.
//!
//! The window is mostly transparent and ignores the mouse, so the webview can't
//! see the cursor arrive. This loop watches the cursor instead: inside the shape
//! the UI reported, the window takes the mouse; outside, clicks fall through.
//! It also emits `hover` (true/false), the UI's single source of truth for
//! opening on hover and closing when the cursor leaves.

use crate::window::{self, Rect};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const POLL: Duration = Duration::from_millis(40);
/// A little slack so the edge of the shape doesn't flicker.
const SLACK: f64 = 2.0;

pub fn spawn(app: AppHandle, hit: Arc<Mutex<Rect>>) {
    std::thread::spawn(move || {
        let mut inside: Option<bool> = None;
        loop {
            std::thread::sleep(POLL);
            let Some(win) = window::widget(&app) else { continue };
            if !win.is_visible().unwrap_or(false) {
                continue;
            }
            let (Ok(c), Ok(p), Ok(sf)) = (win.cursor_position(), win.outer_position(), win.scale_factor()) else {
                continue;
            };
            let x = (c.x - p.x as f64) / sf;
            let y = (c.y - p.y as f64) / sf;
            let r = match hit.lock() {
                Ok(r) => *r,
                Err(_) => break,
            };
            let grown = Rect { x: r.x - SLACK, y: r.y - SLACK, w: r.w + 2.0 * SLACK, h: r.h + 2.0 * SLACK };
            let now_in = r.w > 0.0 && grown.contains(x, y);
            if inside != Some(now_in) && std::env::var("AIUC_DEBUG_HOVER").is_ok() {
                eprintln!("hover {now_in}: cursor ({x:.0},{y:.0}) rect {r:?} win ({},{}) sf {sf}", p.x, p.y);
            }
            if inside != Some(now_in) {
                inside = Some(now_in);
                let _ = win.set_ignore_cursor_events(!now_in);
                let _ = app.emit("hover", now_in);
            }
        }
    });
}

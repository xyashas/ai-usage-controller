//! Paste a prompt into the app or web page we just opened, and send it.
//!
//! Safety first: keys are only sent once the window in front is the expected
//! one (its title or app name mentions Claude, Codex, ChatGPT or Gemini). If it
//! never shows up, nothing is typed and the prompt stays on the clipboard.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Pasted and Enter pressed.
    Sent,
    /// Pasted, left for the user to send.
    Pasted,
    /// The window never came to the front; nothing was typed.
    NotFound,
}

/// Wait up to `timeout` for a front window whose title (or app name) contains
/// one of `keywords`, give it `settle` to load, then paste (and send).
pub fn paste_into(keywords: &[&str], send: bool, timeout: Duration, settle: Duration) -> Outcome {
    if cfg!(target_os = "linux") { return Outcome::NotFound; }
    let start = Instant::now();
    let matches = || {
        let title = front_window().to_lowercase();
        // Never type into this widget.
        !title.contains("ai usage controller") && keywords.iter().any(|k| title.contains(k))
    };
    while start.elapsed() < timeout {
        if matches() {
            std::thread::sleep(settle);
            // Still the right window after loading?
            if !matches() { break; }
            return if press(send) { if send { Outcome::Sent } else { Outcome::Pasted } } else { Outcome::NotFound };
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Outcome::NotFound
}

#[cfg(windows)]
fn front_window() -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
    unsafe {
        let h = GetForegroundWindow();
        if h.is_null() { return String::new(); }
        let mut buf = [0u16; 512];
        let n = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

#[cfg(windows)]
fn press(send: bool) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_CONTROL, VK_RETURN};
    unsafe {
        keybd_event(VK_CONTROL as u8, 0, 0, 0);
        keybd_event(b'V', 0, 0, 0);
        keybd_event(b'V', 0, KEYEVENTF_KEYUP, 0);
        keybd_event(VK_CONTROL as u8, 0, KEYEVENTF_KEYUP, 0);
        if send {
            std::thread::sleep(Duration::from_millis(400));
            keybd_event(VK_RETURN as u8, 0, 0, 0);
            keybd_event(VK_RETURN as u8, 0, KEYEVENTF_KEYUP, 0);
        }
    }
    true
}

#[cfg(target_os = "macos")]
fn osascript(script: &str) -> Option<String> {
    let out = std::process::Command::new("/usr/bin/osascript").args(["-e", script]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[cfg(target_os = "macos")]
fn front_window() -> String {
    // App name plus window title (the title needs Accessibility permission).
    let app = osascript("tell application \"System Events\" to get name of first application process whose frontmost is true").unwrap_or_default();
    let title = osascript("tell application \"System Events\" to get title of front window of (first application process whose frontmost is true)").unwrap_or_default();
    format!("{app} {title}")
}

#[cfg(target_os = "macos")]
fn press(send: bool) -> bool {
    let mut script = String::from("tell application \"System Events\"\nkeystroke \"v\" using command down\n");
    if send { script.push_str("delay 0.4\nkey code 36\n"); }
    script.push_str("end tell");
    osascript(&script).is_some()
}

#[cfg(target_os = "linux")]
fn front_window() -> String { String::new() }

#[cfg(target_os = "linux")]
fn press(_send: bool) -> bool { false }

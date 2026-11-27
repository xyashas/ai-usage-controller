//! What you were looking at when you pressed the Ask hotkey: the window, the
//! selected text, what you copied recently and a screenshot. Also the way back:
//! focus that window again and paste an answer into it.
//!
//! Nothing here is stored. The screenshot goes to a temp file that's removed
//! when the next ask starts or the app quits.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The window you were in, so an answer can be pasted back into it.
#[derive(Clone, Debug, Default)]
pub struct Origin {
    #[cfg(windows)]
    pub hwnd: isize,
    /// The app's name (macOS) or the window title.
    pub app: String,
    pub title: String,
}

/// Where the screenshot sits on screen, in physical pixels, and how it was
/// scaled down, so points on it map back to the screen.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct ShotRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub img_width: u32,
    pub img_height: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Captured {
    pub origin: Origin,
    pub selection: Option<String>,
    pub clipboard: Option<String>,
    /// An image you copied (saved as a temp PNG).
    pub clipboard_image: Option<PathBuf>,
    pub screenshot: Option<PathBuf>,
    pub shot: Option<ShotRect>,
    /// A small preview of the screenshot for the chip (data: URL).
    pub thumb: Option<String>,
}

pub fn temp_dir() -> PathBuf {
    let d = aiuc_core::paths::app_data_dir().join("ask-tmp");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Remove screenshots and copied images from earlier asks.
pub fn clean_temp() {
    if let Ok(rd) = std::fs::read_dir(temp_dir()) {
        for e in rd.flatten() { let _ = std::fs::remove_file(e.path()); }
    }
}

// ---------- recently copied ----------

static LAST_COPY: Mutex<Option<(u64, Instant)>> = Mutex::new(None);

fn clipboard_stamp() -> u64 {
    #[cfg(windows)]
    { unsafe { windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber() as u64 } }
    #[cfg(not(windows))]
    {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()).hash(&mut h);
        h.finish()
    }
}

/// Notes when the clipboard changes (a number or hash only, never its content),
/// so the Ask box can offer "what you just copied".
pub fn spawn_clipboard_clock() {
    std::thread::spawn(|| loop {
        let s = clipboard_stamp();
        if let Ok(mut l) = LAST_COPY.lock() {
            if l.is_none_or(|(old, _)| old != s) {
                // The first reading at startup isn't a fresh copy.
                *l = Some((s, if l.is_none() { Instant::now() - Duration::from_secs(3600) } else { Instant::now() }));
            }
        }
        std::thread::sleep(Duration::from_millis(1500));
    });
}

fn copied_recently() -> bool {
    LAST_COPY.lock().ok().and_then(|l| *l).is_some_and(|(s, at)| s == clipboard_stamp() && at.elapsed() < Duration::from_secs(120))
        || LAST_COPY.lock().ok().and_then(|l| *l).is_some_and(|(s, _)| s != clipboard_stamp())
}

// ---------- the front window ----------

#[cfg(windows)]
fn front() -> Origin {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
    unsafe {
        let h = GetForegroundWindow();
        let mut buf = [0u16; 512];
        let n = if h.is_null() { 0 } else { GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32) };
        let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        Origin { hwnd: h as isize, app: title.rsplit(" - ").next().unwrap_or("").trim().to_owned(), title }
    }
}

#[cfg(target_os = "macos")]
fn osascript(script: &str) -> Option<String> {
    let out = std::process::Command::new("/usr/bin/osascript").args(["-e", script]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[cfg(target_os = "macos")]
fn front() -> Origin {
    let app = osascript("tell application \"System Events\" to get name of first application process whose frontmost is true").unwrap_or_default();
    let title = osascript("tell application \"System Events\" to get title of front window of (first application process whose frontmost is true)").unwrap_or_default();
    Origin { app, title }
}

#[cfg(target_os = "linux")]
fn front() -> Origin { Origin::default() }

// ---------- keys ----------

/// Wait (briefly) for the hotkey's modifiers to be let go, so a simulated
/// Ctrl+C isn't read as Ctrl+Alt+Shift+C.
fn wait_for_modifiers() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT};
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(900) {
            let down = [VK_MENU, VK_SHIFT, VK_CONTROL, VK_LWIN].iter().any(|k| unsafe { GetAsyncKeyState(*k as i32) } as u16 & 0x8000 != 0);
            if !down { break; }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    #[cfg(not(windows))]
    std::thread::sleep(Duration::from_millis(250));
}

fn send_copy() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_CONTROL};
        keybd_event(VK_CONTROL as u8, 0, 0, 0);
        keybd_event(b'C', 0, 0, 0);
        keybd_event(b'C', 0, KEYEVENTF_KEYUP, 0);
        keybd_event(VK_CONTROL as u8, 0, KEYEVENTF_KEYUP, 0);
    }
    #[cfg(target_os = "macos")]
    { let _ = osascript("tell application \"System Events\" to keystroke \"c\" using command down"); }
}

/// Copy the selected text in the front app, then put your clipboard back.
fn grab_selection() -> Option<String> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let before_text = cb.get_text().ok();
    let before_img = if before_text.is_none() { cb.get_image().ok() } else { None };
    let stamp = clipboard_stamp();
    wait_for_modifiers();
    send_copy();
    let start = Instant::now();
    let mut got = None;
    while start.elapsed() < Duration::from_millis(400) {
        std::thread::sleep(Duration::from_millis(40));
        if clipboard_stamp() != stamp {
            std::thread::sleep(Duration::from_millis(30));
            got = cb.get_text().ok();
            break;
        }
    }
    // Put back what was there, so we don't clobber your clipboard.
    if got.is_some() {
        match (before_text, before_img) {
            (Some(t), _) => { let _ = cb.set_text(t); }
            (None, Some(img)) => { let _ = cb.set_image(img); }
            _ => { let _ = cb.clear(); }
        }
        // That restore isn't a "fresh copy".
        if let Ok(mut l) = LAST_COPY.lock() { if let Some((_, at)) = *l { *l = Some((clipboard_stamp(), at)); } }
    }
    got.filter(|s| !s.trim().is_empty())
}

fn save_png(img: &image::RgbaImage, name: &str) -> Option<PathBuf> {
    let p = temp_dir().join(format!("{name}-{}.png", chrono::Utc::now().timestamp_millis()));
    img.save(&p).ok()?;
    Some(p)
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
fn data_url(img: &image::RgbaImage, max_w: u32) -> Option<String> {
    use base64_lite::encode;
    let small = image::imageops::thumbnail(img, max_w, (img.height() as f64 * max_w as f64 / img.width().max(1) as f64) as u32);
    let mut buf = std::io::Cursor::new(Vec::new());
    small.write_to(&mut buf, image::ImageFormat::Png).ok()?;
    Some(format!("data:image/png;base64,{}", encode(buf.get_ref())))
}

mod base64_lite {
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub fn encode(bytes: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for c in bytes.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            out.push(T[(n >> 18) as usize & 63] as char);
            out.push(T[(n >> 12) as usize & 63] as char);
            out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
            out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
        }
        out
    }
}

/// Shrink a big screenshot so it's quick to send and read.
fn fit(img: image::RgbaImage, max_w: u32) -> image::RgbaImage {
    if img.width() <= max_w { return img; }
    let h = (img.height() as f64 * max_w as f64 / img.width() as f64) as u32;
    image::imageops::resize(&img, max_w, h, image::imageops::FilterType::Triangle)
}

/// A screenshot of the focused window (or the screen it's on).
#[cfg(any(windows, target_os = "macos"))]
fn screenshot(origin: &Origin) -> Option<(PathBuf, ShotRect, String)> {
    use xcap::Window;
    let windows = Window::all().ok()?;
    let ours = |w: &Window| w.title().is_ok_and(|t| t == "AI Usage Controller");
    let win = windows.iter().filter(|w| !ours(w) && !w.is_minimized().unwrap_or(false))
        .find(|w| w.is_focused().unwrap_or(false))
        .or_else(|| windows.iter().find(|w| !ours(w) && !origin.title.is_empty() && w.title().is_ok_and(|t| t == origin.title)));
    let (img, rect) = match win.and_then(|w| Some((w.capture_image().ok()?, w))) {
        Some((img, w)) if img.width() > 40 && img.height() > 40 => {
            let r = ShotRect { x: w.x().unwrap_or(0), y: w.y().unwrap_or(0), width: img.width(), height: img.height(), img_width: 0, img_height: 0 };
            (img, r)
        }
        _ => {
            let m = xcap::Monitor::all().ok()?.into_iter().find(|m| m.is_primary().unwrap_or(false))?;
            let img = m.capture_image().ok()?;
            let r = ShotRect { x: m.x().unwrap_or(0), y: m.y().unwrap_or(0), width: img.width(), height: img.height(), img_width: 0, img_height: 0 };
            (img, r)
        }
    };
    let img = fit(img, 1600);
    let rect = ShotRect { img_width: img.width(), img_height: img.height(), ..rect };
    let thumb = data_url(&img, 160).unwrap_or_default();
    Some((save_png(&img, "screen")?, rect, thumb))
}

#[cfg(target_os = "linux")]
fn screenshot(_origin: &Origin) -> Option<(PathBuf, ShotRect, String)> { None }

/// Everything at once, right when the hotkey is pressed (before the widget
/// takes focus).
pub fn capture(with_screenshot: bool) -> Captured {
    let origin = front();
    let fresh_copy = copied_recently();
    let (clipboard, clipboard_image) = if fresh_copy {
        let mut cb = arboard::Clipboard::new().ok();
        let text = cb.as_mut().and_then(|c| c.get_text().ok()).filter(|t| !t.trim().is_empty());
        let img = if text.is_none() {
            cb.as_mut().and_then(|c| c.get_image().ok()).and_then(|i| {
                let rgba = image::RgbaImage::from_raw(i.width as u32, i.height as u32, i.bytes.into_owned())?;
                save_png(&fit(rgba, 1600), "copied")
            })
        } else { None };
        (text, img)
    } else { (None, None) };
    let shot = if with_screenshot { screenshot(&origin) } else { None };
    let selection = grab_selection().filter(|s| Some(s) != clipboard.as_ref());
    let (screenshot, shot, thumb) = match shot { Some((p, r, t)) => (Some(p), Some(r), Some(t)), None => (None, None, None) };
    Captured { origin, selection, clipboard, clipboard_image, screenshot, shot, thumb }
}

// ---------- the way back ----------

fn send_paste() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_CONTROL};
        keybd_event(VK_CONTROL as u8, 0, 0, 0);
        keybd_event(b'V', 0, 0, 0);
        keybd_event(b'V', 0, KEYEVENTF_KEYUP, 0);
        keybd_event(VK_CONTROL as u8, 0, KEYEVENTF_KEYUP, 0);
    }
    #[cfg(target_os = "macos")]
    { let _ = osascript("tell application \"System Events\" to keystroke \"v\" using command down"); }
}

/// Bring the window you were in back to the front.
pub fn refocus(origin: &Origin) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindow, SetForegroundWindow, ShowWindow, SW_RESTORE};
        let h = origin.hwnd as windows_sys::Win32::Foundation::HWND;
        if origin.hwnd == 0 || unsafe { IsWindow(h) } == 0 { return false; }
        unsafe { ShowWindow(h, SW_RESTORE); SetForegroundWindow(h) != 0 }
    }
    #[cfg(target_os = "macos")]
    {
        if origin.app.is_empty() { return false; }
        let app = origin.app.replace('"', "");
        osascript(&format!("tell application \"{app}\" to activate")).is_some()
    }
    #[cfg(target_os = "linux")]
    { let _ = origin; false }
}

/// Put `text` where your cursor was (replacing what you'd selected).
pub fn paste_back(origin: &Origin, text: &str) -> Result<(), String> {
    arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_owned())).map_err(|e| format!("Couldn't copy: {e}"))?;
    if !refocus(origin) { return Err("Couldn't go back to that window. The text is on your clipboard.".into()); }
    std::thread::sleep(Duration::from_millis(250));
    send_paste();
    Ok(())
}

pub fn copy_text(text: &str) -> Result<(), String> {
    arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_owned())).map_err(|e| e.to_string())
}

//! The on-screen buddy: a see-through, click-through window over the monitor
//! you're on. It shows "listening…", answers in a bubble next to your cursor,
//! and a little pointer that flies to where you should click, step by step.
//!
//! Hold the Ask hotkey to talk (voice.rs); let go and the answer comes here,
//! without opening the panel. "How do I…" questions become a guided walk:
//! point, wait for your click, look again, point at the next thing.

use crate::ask as app_ask;
use crate::capture::{self, ShotRect};
use crate::AppState;
use aiuc_core::ask::{self, Action, Mode};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Held this long, the hotkey means "talk" rather than "open".
const HOLD: Duration = Duration::from_millis(320);

#[derive(Serialize, Clone, Default)]
pub struct Spot { pub fx: f64, pub fy: f64, pub label: String }

/// What the buddy window shows.
#[derive(Serialize, Clone, Default)]
pub struct Buddy {
    /// "starting" | "listening" | "thinking" | "answer" | "guide" | "done" | "error" | "hide"
    pub mode: String,
    /// What you said.
    pub said: String,
    /// The answer, a guide step, or an error.
    pub text: String,
    /// Where your cursor is (fractions of the monitor): the bubble sits by it.
    pub cursor: (f64, f64),
    pub spots: Vec<Spot>,
    pub step: u32,
    /// Stream id for answers ("voice-…").
    pub id: String,
    /// Read this aloud.
    pub speak: String,
}

#[derive(Clone, Copy)]
struct Mon { x: f64, y: f64, w: f64, h: f64 }

fn monitor(app: &AppHandle) -> Option<(Mon, (f64, f64))> {
    let c = app.cursor_position().ok()?;
    let m = app.monitor_from_point(c.x, c.y).ok().flatten().or_else(|| app.primary_monitor().ok().flatten())?;
    let mon = Mon { x: m.position().x as f64, y: m.position().y as f64, w: m.size().width as f64, h: m.size().height as f64 };
    Some((mon, (((c.x - mon.x) / mon.w).clamp(0.0, 1.0), ((c.y - mon.y) / mon.h).clamp(0.0, 1.0))))
}

static MON: Mutex<Option<(f64, f64, f64, f64)>> = Mutex::new(None);
static LAST: Mutex<Option<serde_json::Value>> = Mutex::new(None);

/// Show (or update) the buddy over the monitor the cursor is on.
fn show(app: &AppHandle, mut b: Buddy) {
    let Some((mon, cursor)) = monitor(app) else { return };
    if b.cursor == (0.0, 0.0) { b.cursor = cursor; }
    if let Ok(mut m) = MON.lock() { *m = Some((mon.x, mon.y, mon.w, mon.h)); }
    if let Ok(mut l) = LAST.lock() { *l = serde_json::to_value(&b).ok(); }
    let app2 = app.clone();
    let hide = b.mode == "hide";
    let _ = app.run_on_main_thread(move || {
        let win = match app2.get_webview_window("buddy") {
            Some(w) => w,
            None if hide => return,
            None => match tauri::WebviewWindowBuilder::new(&app2, "buddy", tauri::WebviewUrl::App("index.html?overlay=buddy".into()))
                .title("AI Usage Controller buddy").decorations(false).transparent(true).always_on_top(true)
                .skip_taskbar(true).resizable(false).shadow(false).focused(false).visible(false).build() {
                Ok(w) => w,
                Err(_) => return,
            },
        };
        if hide { let _ = win.hide(); return; }
        let _ = win.set_ignore_cursor_events(true);
        let _ = win.set_position(tauri::PhysicalPosition::new(mon.x as i32, mon.y as i32));
        let _ = win.set_size(tauri::PhysicalSize::new(mon.w as u32, mon.h as u32));
        crate::window::mac_float(&win);
        let _ = win.show();
    });
    let _ = app.emit_to("buddy", "buddy", &b);
}

pub fn hide(app: &AppHandle) {
    show(app, Buddy { mode: "hide".into(), ..Default::default() });
}

/// The buddy page asks for its state when it loads.
#[tauri::command]
pub fn buddy_state() -> Option<serde_json::Value> {
    LAST.lock().ok().and_then(|l| l.clone())
}

#[tauri::command]
pub fn buddy_hide(app: AppHandle) {
    GUIDING.store(false, Ordering::SeqCst);
    hide(&app);
}

/// A point on a screenshot of a window → a spot on the monitor.
fn spot(shot: ShotRect, p: &ask::Point) -> Option<Spot> {
    let (mx, my, mw, mh) = (*MON.lock().ok()?)?;
    let sx = shot.x as f64 + p.x * shot.width as f64 / shot.img_width.max(1) as f64;
    let sy = shot.y as f64 + p.y * shot.height as f64 / shot.img_height.max(1) as f64;
    Some(Spot { fx: ((sx - mx) / mw).clamp(0.0, 1.0), fy: ((sy - my) / mh).clamp(0.0, 1.0), label: p.label.clone() })
}

/// Arrows for a panel answer (Ask's "where do I…").
pub fn point(app: &AppHandle, shot: ShotRect, points: &[ask::Point], text: &str) {
    if let Some((mon, _)) = monitor(app) { if let Ok(mut m) = MON.lock() { *m = Some((mon.x, mon.y, mon.w, mon.h)); } }
    let spots: Vec<Spot> = points.iter().filter_map(|p| spot(shot, p)).collect();
    show(app, Buddy { mode: "guide".into(), text: text.into(), spots, step: 1, ..Default::default() });
}

// ---------- the hotkey: tap to open, hold to talk ----------

static PRESS: AtomicU64 = AtomicU64::new(0);
static HELD: AtomicBool = AtomicBool::new(false);
static PRESSED_AT: Mutex<Option<Instant>> = Mutex::new(None);
/// The microphone for press `n`, once Windows has it ready (or why it couldn't).
static VOICE: Mutex<Option<(u64, Result<crate::voice::Listening, String>)>> = Mutex::new(None);
static GUIDING: AtomicBool = AtomicBool::new(false);

fn same_press_held(n: u64) -> bool {
    PRESS.load(Ordering::SeqCst) == n && HELD.load(Ordering::SeqCst)
}

pub fn on_press(app: &AppHandle) {
    let n = PRESS.fetch_add(1, Ordering::SeqCst) + 1;
    app_ask::diag("hotkey pressed");
    HELD.store(true, Ordering::SeqCst);
    if let Ok(mut t) = PRESSED_AT.lock() { *t = Some(Instant::now()); }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(HOLD);
        // A tap opens the panel (on release); only a hold starts the mic.
        if !same_press_held(n) { return; }
        GUIDING.store(false, Ordering::SeqCst);
        // Windows takes a moment to open the mic: say so, then chime when ready.
        show(&app, Buddy { mode: "starting".into(), ..Default::default() });
        // Live captions while the keys are held, at most ~12 updates a second.
        let (app2, last) = (app.clone(), Mutex::new(Instant::now() - Duration::from_secs(1)));
        let live = move |words: String| {
            if !same_press_held(n) { return; }
            let Ok(mut l) = last.lock() else { return };
            if l.elapsed() < Duration::from_millis(80) { return; }
            *l = Instant::now();
            show(&app2, Buddy { mode: "listening".into(), said: words, ..Default::default() });
        };
        let started = crate::voice::start(live);
        match &started {
            Ok(_) => {
                app_ask::diag("voice listening");
                if same_press_held(n) { show(&app, Buddy { mode: "listening".into(), ..Default::default() }); }
            }
            Err(e) => {
                app_ask::diag(&format!("voice start failed: {e}"));
                show(&app, Buddy { mode: "error".into(), text: e.clone(), ..Default::default() });
            }
        }
        if let Ok(mut v) = VOICE.lock() { *v = Some((n, started)); }
    });
}

pub fn on_release(app: &AppHandle) {
    HELD.store(false, Ordering::SeqCst);
    let n = PRESS.load(Ordering::SeqCst);
    let held = PRESSED_AT.lock().ok().and_then(|t| *t).map(|t| t.elapsed()).unwrap_or_default();
    app_ask::diag(&format!("hotkey released after {} ms", held.as_millis()));
    if held < HOLD {
        // A tap: stop any guide, then the usual Ask panel.
        if GUIDING.swap(false, Ordering::SeqCst) { hide(app); return; }
        app_ask::on_hotkey(app);
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let said_so_far = LAST.lock().ok().and_then(|l| l.as_ref().and_then(|v| v.get("said")?.as_str().map(str::to_owned))).unwrap_or_default();
        show(&app, Buddy { mode: "thinking".into(), said: said_so_far, ..Default::default() });
        // The mic may still be starting if you let go quickly: wait for it.
        let start = Instant::now();
        let voice = loop {
            let got = VOICE.lock().ok().and_then(|mut v| if v.as_ref().is_some_and(|(id, _)| *id == n) { v.take() } else { None });
            if let Some((_, r)) = got { break Some(r); }
            if start.elapsed() > Duration::from_secs(6) { break None; }
            std::thread::sleep(Duration::from_millis(40));
        };
        match voice {
            Some(Ok(l)) => match l.finish() {
                Ok(said) if !said.is_empty() => voice_ask(&app, said),
                Ok(_) => show(&app, Buddy { mode: "error".into(), text: "I didn't catch that. Hold the keys, wait for the chime, speak, then let go.".into(), ..Default::default() }),
                Err(e) => { app_ask::diag(&format!("voice failed: {e}")); show(&app, Buddy { mode: "error".into(), text: e, ..Default::default() }) }
            },
            Some(Err(_)) => {} // already shown when it failed to start
            None => show(&app, Buddy { mode: "error".into(), text: "The microphone didn't start in time. Try holding the keys a little longer.".into(), ..Default::default() }),
        }
    });
}

// ---------- answering out loud ----------

/// The last spoken question and answer, so the panel can pick it up.
#[derive(Serialize, Clone, Default)]
pub struct VoiceTurn { pub question: String, pub answer: String, pub tool: Option<aiuc_core::model::Tool>, pub reason: String }

static LAST_VOICE: Mutex<Option<(VoiceTurn, Instant)>> = Mutex::new(None);
/// The last answer from voice or the panel: what "paste that" means.
static LAST_ANY: Mutex<Option<(VoiceTurn, Instant)>> = Mutex::new(None);
/// "That" stops meaning the last answer after this long.
const THAT_FOR: Duration = Duration::from_secs(10 * 60);

#[tauri::command]
pub fn last_voice_turn() -> Option<VoiceTurn> {
    LAST_VOICE.lock().ok()?.as_ref().filter(|(_, at)| at.elapsed() < Duration::from_secs(180)).map(|(t, _)| t.clone())
}

pub fn last_answer() -> Option<VoiceTurn> {
    LAST_ANY.lock().ok()?.as_ref().filter(|(_, at)| at.elapsed() < THAT_FOR).map(|(t, _)| t.clone())
}

pub fn set_last_answer(question: &str, answer: &str, tool: aiuc_core::model::Tool, reason: &str) {
    if let Ok(mut l) = LAST_ANY.lock() {
        *l = Some((VoiceTurn { question: question.to_owned(), answer: answer.to_owned(), tool: Some(tool), reason: reason.to_owned() }, Instant::now()));
    }
}

/// Markdown → text that reads well when pasted into any app.
fn plain(md: &str) -> String {
    md.lines()
        .filter(|l| !l.trim_start().starts_with("```"))
        .map(|l| l.trim_start_matches('#').trim_start().replace("**", "").replace('`', ""))
        .collect::<Vec<_>>().join("\n").trim().to_owned()
}

/// A short confirmation by the cursor ("Pasted", "Saved to your inbox").
fn confirm(app: &AppHandle, said: String, text: impl Into<String>, speak: bool) {
    let text = text.into();
    show(app, Buddy { mode: "done".into(), said, speak: if speak { text.clone() } else { String::new() }, text, ..Default::default() });
}

fn fail(app: &AppHandle, said: String, text: impl Into<String>) {
    show(app, Buddy { mode: "error".into(), said, text: text.into(), ..Default::default() });
}

fn voice_ask(app: &AppHandle, said: String) {
    use ask::Intent;
    let state = app.state::<AppState>();
    let last = last_answer();
    let intent = ask::voice_intent(&said, last.is_some());
    match (&intent, last) {
        (Intent::Stop, _) => { GUIDING.store(false, Ordering::SeqCst); hide(app); }
        (Intent::Paste { code_only }, Some(l)) => {
            let text = if *code_only { ask::first_code_block(&l.answer).unwrap_or_else(|| plain(&l.answer)) } else { plain(&l.answer) };
            // Where you are now (the buddy never takes focus).
            let origin = capture::capture(false).origin;
            hide(app);
            match capture::paste_back(&origin, &text) {
                Ok(()) => confirm(app, said, "Pasted", false),
                Err(e) => fail(app, said, e),
            }
        }
        (Intent::Copy, Some(l)) => match capture::copy_text(&plain(&l.answer)) {
            Ok(()) => confirm(app, said, "Copied", false),
            Err(e) => fail(app, said, e),
        },
        (Intent::Repeat, Some(l)) => {
            show(app, Buddy { mode: "answer".into(), said, speak: l.answer.clone(), text: l.answer, id: format!("voice-{}", chrono::Utc::now().timestamp_millis()), ..Default::default() });
        }
        (Intent::Send(tool), Some(l)) => {
            let text = format!("{}\n\n{}", l.question, l.answer);
            let dest = crate::handoff::destinations(*tool, false).first().map(|d| d.dest).unwrap_or(crate::handoff::Dest::Web);
            match crate::handoff::start_task(None, *tool, dest, text.trim(), true) {
                Ok((m, _)) => confirm(app, said, m, false),
                Err(e) => fail(app, said, e),
            }
        }
        (Intent::Redo(how), Some(l)) => {
            let history = [ask::Turn { question: l.question.clone(), answer: l.answer.clone() }];
            app_ask::set_captured(Default::default());
            speak_answer(app, said, &ask::redo_question(how), &history, &l.question);
        }
        (Intent::Remember(_) | Intent::Forget(_) | Intent::Save(_), _) => {
            let c = capture::capture(false);
            match app_ask::handle_note(&state, &intent, &c) {
                Some(Ok(msg)) => confirm(app, said, msg, false),
                Some(Err(e)) => fail(app, said, e),
                None => {}
            }
        }
        _ => {
            let shots = state.engine.lock().map(|e| e.settings.ask_screenshots).unwrap_or(true);
            capture::clean_temp();
            let c = capture::capture(shots);
            app_ask::set_captured(c.clone());
            if ask::wants_guide(&said) && c.screenshot.is_some() {
                guide(app, said);
                return;
            }
            let q = said.clone();
            speak_answer(app, said, &q, &[], &q);
        }
    }
}

/// Ask, stream the answer into the bubble, then read it aloud.
/// `topic` is what the turn is remembered as (the original question on a redo).
fn speak_answer(app: &AppHandle, said: String, question: &str, history: &[ask::Turn], topic: &str) {
    let state = app.state::<AppState>();
    let id = format!("voice-{}", chrono::Utc::now().timestamp_millis());
    show(app, Buddy { mode: "answer".into(), said: said.clone(), id: id.clone(), ..Default::default() });
    let Some((rooms, pick, prompt, image)) = app_ask::prepare(&state, question, Action::Ask, history, "Answer in 1–3 short sentences that sound natural read aloud. No code blocks unless asked.") else {
        fail(app, said, "Install Claude Code, Codex or Gemini to ask by voice.");
        return;
    };
    let a = app_ask::answer(Some(app), &id, rooms, pick, Mode::Quick, &prompt, image.as_deref());
    let text = a.text.trim().to_owned();
    if !text.is_empty() {
        let turn = VoiceTurn { question: topic.to_owned(), answer: text.clone(), tool: Some(a.pick.tool), reason: a.reason.clone() };
        if let Ok(mut l) = LAST_VOICE.lock() { *l = Some((turn.clone(), Instant::now())); }
        set_last_answer(topic, &text, a.pick.tool, &a.reason);
    }
    match a.error {
        Some(e) if text.is_empty() => fail(app, said, e),
        _ => show(app, Buddy { mode: "answer".into(), said, speak: text.clone(), text, id, ..Default::default() }),
    }
}

// ---------- guide me ----------

/// Wait for a left click (Windows), or a pause (elsewhere). False if stopped.
fn wait_for_click(timeout: Duration) -> bool {
    let start = Instant::now();
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
        let down = || unsafe { GetAsyncKeyState(VK_LBUTTON as i32) } as u16 & 0x8000 != 0;
        let mut was = down();
        while start.elapsed() < timeout {
            if !GUIDING.load(Ordering::SeqCst) { return false; }
            let now = down();
            if was && !now { return true; }
            was = now;
            std::thread::sleep(Duration::from_millis(30));
        }
        false
    }
    #[cfg(not(windows))]
    {
        while start.elapsed() < Duration::from_secs(8).min(timeout) {
            if !GUIDING.load(Ordering::SeqCst) { return false; }
            std::thread::sleep(Duration::from_millis(100));
        }
        true
    }
}

fn guide(app: &AppHandle, goal: String) {
    GUIDING.store(true, Ordering::SeqCst);
    let state = app.state::<AppState>();
    let mut done: Vec<String> = vec![];
    for step in 1..=10u32 {
        if !GUIDING.load(Ordering::SeqCst) { return; }
        show(app, Buddy { mode: "thinking".into(), said: goal.clone(), step, ..Default::default() });
        // A fresh look at the screen each step (the first one was just taken).
        if step > 1 { app_ask::set_captured(capture::capture(true)); }
        let c = app_ask::captured_now();
        let (Some(shot_path), Some(shot)) = (c.screenshot.clone(), c.shot) else {
            show(app, Buddy { mode: "error".into(), said: goal, text: "I couldn't see the screen. On a Mac, allow Screen Recording for AI Usage Controller.".into(), ..Default::default() });
            break;
        };
        let window = Some(format!("{} {}", c.origin.app, c.origin.title).trim().to_owned()).filter(|s| !s.is_empty());
        let prompt = ask::guide_prompt(&goal, &done, &shot_path, Some((shot.img_width, shot.img_height)), window.as_deref());
        let Some((rooms, pick, _, _)) = app_ask::prepare(&state, &goal, Action::Point, &[], "") else { break };
        let a = app_ask::answer(None, &format!("guide-{step}"), rooms, pick, Mode::Quick, &prompt, Some(&shot_path));
        if a.text.trim().is_empty() {
            show(app, Buddy { mode: "error".into(), said: goal, text: a.error.unwrap_or_else(|| "No answer came back.".into()), ..Default::default() });
            break;
        }
        let s = ask::parse_guide(&a.text);
        if s.done {
            show(app, Buddy { mode: "done".into(), said: goal, speak: s.say.clone(), text: s.say, step, ..Default::default() });
            break;
        }
        let spots: Vec<Spot> = s.point.as_ref().and_then(|p| spot(shot, p)).into_iter().collect();
        show(app, Buddy { mode: "guide".into(), said: goal.clone(), speak: s.say.clone(), text: s.say.clone(), spots, step, ..Default::default() });
        done.push(s.say);
        // Your click moves things on; then give the screen a moment to change.
        if !wait_for_click(Duration::from_secs(90)) { break; }
        std::thread::sleep(Duration::from_millis(1200));
    }
    GUIDING.store(false, Ordering::SeqCst);
}

//! Ask the notch: run a question on the AI with room, stream the answer, and
//! act on it (paste back, run, open in an app, point on screen).

use crate::capture::{self, Captured};
use crate::{err, handoff, AppState, CmdResult};
use aiuc_core::ask::{self, Action, Context, Mode, Pick, Room, StreamReader, Turn};
use aiuc_core::model::Tool;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, State};

pub const ASK_HOTKEY: &str = "Alt+Shift+Space";
/// Set on every process we start, so our own hooks ignore these runs.
pub const ENV_FLAG: &str = "AIUC_ASK";

fn captured() -> &'static Mutex<Captured> {
    static C: OnceLock<Mutex<Captured>> = OnceLock::new();
    C.get_or_init(Default::default)
}

fn children() -> &'static Mutex<HashMap<String, Child>> {
    static C: OnceLock<Mutex<HashMap<String, Child>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// Called from the hotkey: look at what's on screen, then open the Ask box.
pub fn on_hotkey(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let shots = app.state::<AppState>().engine.lock().map(|e| e.settings.ask_screenshots).unwrap_or(true);
        capture::clean_temp();
        let c = capture::capture(shots);
        if let Ok(mut slot) = captured().lock() { *slot = c; }
        if let Some(w) = crate::window::widget(&app) {
            let _ = w.show();
            let _ = w.set_focus();
        }
        let _ = app.emit("open-view", "ask");
    });
}

/// What the Ask box shows as chips.
#[derive(Serialize, Default)]
pub struct ContextView {
    pub window: Option<String>,
    pub app: Option<String>,
    pub selection: Option<String>,
    pub clipboard: Option<String>,
    pub clipboard_image: bool,
    pub screenshot: Option<String>,
    pub repo: Option<String>,
    /// Which tool would answer now, and why.
    pub pick: Option<Pick>,
    pub installed: Vec<Tool>,
}

fn current_repo(state: &AppState) -> Option<String> {
    let eng = state.engine.lock().ok()?;
    eng.recent_projects(chrono::Utc::now()).ok()?.into_iter().find(|p| p.join(".git").exists()).map(|p| p.to_string_lossy().into_owned())
}

/// Tools that just failed for a reason that won't fix itself in seconds (not
/// signed in, out of room: 10 minutes; no longer supported: a day), by when
/// they can be tried again.
fn broken() -> &'static Mutex<HashMap<Tool, std::time::Instant>> {
    static B: OnceLock<Mutex<HashMap<Tool, std::time::Instant>>> = OnceLock::new();
    B.get_or_init(Default::default)
}

fn mark_broken_for(tool: Tool, secs: u64) {
    if let Ok(mut b) = broken().lock() { b.insert(tool, std::time::Instant::now() + std::time::Duration::from_secs(secs)); }
}

fn is_broken(tool: Tool) -> bool {
    broken().lock().ok().and_then(|b| b.get(&tool).copied()).is_some_and(|until| std::time::Instant::now() < until)
}

fn rooms(state: &AppState) -> Vec<Room> {
    let snap = state.last.lock().ok().and_then(|l| l.clone());
    Tool::ALL.into_iter().map(|tool| {
        let status = snap.as_ref().and_then(|s| s.tools.iter().find(|t| t.tool == tool));
        Room {
            tool,
            installed: aiuc_core::handoff::cli(tool).is_some() && status.is_none_or(|s| s.enabled) && !is_broken(tool),
            used_pct: status.and_then(|s| s.headroom_pct).map(|h| 100.0 - h),
        }
    }).collect()
}

#[tauri::command]
pub fn ask_context(state: State<AppState>, pinned: Option<Tool>) -> CmdResult<ContextView> {
    let c = captured().lock().map_err(err)?.clone();
    let rooms = rooms(&state);
    Ok(ContextView {
        window: Some(c.origin.title.clone()).filter(|t| !t.is_empty()),
        app: Some(c.origin.app.clone()).filter(|t| !t.is_empty()),
        selection: c.selection,
        clipboard: c.clipboard,
        clipboard_image: c.clipboard_image.is_some(),
        screenshot: c.thumb,
        repo: current_repo(&state).and_then(|r| std::path::Path::new(&r).file_name().map(|n| n.to_string_lossy().into_owned())),
        pick: ask::pick(&rooms, pinned),
        installed: rooms.iter().filter(|r| r.installed).map(|r| r.tool).collect(),
    })
}

/// Take a fresh screenshot (e.g. after switching windows) — or none.
#[tauri::command]
pub fn ask_forget(part: String) -> CmdResult<()> {
    let mut c = captured().lock().map_err(err)?;
    match part.as_str() {
        "selection" => c.selection = None,
        "clipboard" => { c.clipboard = None; c.clipboard_image = None; }
        "screenshot" => { c.screenshot = None; c.shot = None; c.thumb = None; }
        "all" => { *c = Captured::default(); capture::clean_temp(); }
        _ => {}
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct AskRequest {
    pub id: String,
    pub question: String,
    #[serde(default)]
    pub action: Action,
    #[serde(default)]
    pub mode: Mode,
    pub tool: Option<Tool>,
    #[serde(default)]
    pub history: Vec<Turn>,
    #[serde(default)]
    pub council: bool,
}

#[derive(Serialize, Clone)]
struct Delta<'a> { id: &'a str, text: &'a str }

#[derive(Serialize, Clone, Default)]
pub struct Done {
    pub id: String,
    pub tool: Option<Tool>,
    pub reason: String,
    pub error: Option<String>,
    /// Arrows to draw (screenshot pixels), if it pointed at something.
    pub points: Vec<ask::Point>,
    /// The answer with the point markers taken out.
    pub text: String,
    /// Opened in the browser because no CLI is installed.
    pub opened_web: bool,
}

/// Failed test output the widget is holding, for "why did the tests fail?".
fn failures(state: &AppState) -> Vec<(i64, String)> {
    let snap = state.last.lock().ok().and_then(|l| l.clone());
    snap.map(|s| s.agents.iter().filter(|a| a.verify.as_deref() == Some("failed"))
        .filter_map(|a| crate::agents::verify_output(a.id).map(|o| (a.id, o))).collect()).unwrap_or_default()
}

fn context_for(c: &Captured, repo: Option<String>) -> Context {
    let mut files = vec![];
    if let Some(i) = &c.clipboard_image { files.push(i.clone()); }
    Context {
        selection: c.selection.clone(),
        clipboard: c.clipboard.clone(),
        window: Some(format!("{} {}", c.origin.app, c.origin.title).trim().to_owned()).filter(|s| !s.is_empty()),
        screenshot: c.screenshot.clone(),
        screenshot_size: c.shot.map(|s| (s.img_width, s.img_height)),
        files,
        repo,
    }
}

/// Run one CLI to the end, sending each piece of the answer as it arrives.
fn run_one(app: &AppHandle, id: &str, tool: Tool, mode: Mode, prompt: &str, image: Option<&std::path::Path>) -> (String, Option<String>) {
    run_cli(Some(app), id, tool, mode, prompt, image)
}

/// Run a CLI and collect its answer; with `app`, stream it to the widget too.
fn run_cli(app: Option<&AppHandle>, id: &str, tool: Tool, mode: Mode, prompt: &str, image: Option<&std::path::Path>) -> (String, Option<String>) {
    let Some(cli) = aiuc_core::handoff::cli(tool) else { return (String::new(), Some("That CLI isn't installed.".into())) };
    let cmd = ask::command(tool, &cli, mode, prompt, image);
    let dir = capture::temp_dir();
    let mut c = Command::new(&cmd.program);
    c.args(&cmd.args).current_dir(&dir).env(ENV_FLAG, "1").env("NO_COLOR", "1")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); }
    let mut child = match c.spawn() {
        Ok(ch) => ch,
        Err(e) => return (String::new(), Some(format!("Couldn't start {}: {e}", crate::tool_name(tool)))),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let p = cmd.stdin.clone();
        std::thread::spawn(move || { let _ = stdin.write_all(p.as_bytes()); });
    }
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let err_tail = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut e) = stderr { let _ = e.read_to_string(&mut s); }
        s
    });
    if let Ok(mut m) = children().lock() { m.insert(id.to_owned(), child); }
    let mut reader = StreamReader::default();
    let mut answer = String::new();
    if let Some(out) = stdout {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(piece) = reader.line(tool, &line) {
                answer.push_str(&piece);
                if let Some(app) = app { let _ = app.emit("ask-delta", Delta { id, text: &piece }); }
            }
        }
    }
    let status = children().lock().ok().and_then(|mut m| m.remove(id)).and_then(|mut ch| ch.wait().ok());
    let stderr = err_tail.join().unwrap_or_default();
    let cancelled = status.is_none();
    let error = reader.error.or_else(|| {
        if cancelled { return Some("Stopped.".into()); }
        if answer.trim().is_empty() {
            let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("No answer came back.").trim().to_owned();
            return Some(format!("{}: {}", crate::tool_name(tool), last.chars().take(240).collect::<String>()));
        }
        None
    });
    (answer, error)
}

#[tauri::command]
pub fn ask_start(app: AppHandle, state: State<AppState>, req: AskRequest) -> CmdResult<()> {
    let c = captured().lock().map_err(err)?.clone();
    let rooms = rooms(&state);
    let repo = current_repo(&state);
    let work = (ask::about_work(&req.question) || req.question.trim().is_empty() && c.selection.is_none() && c.screenshot.is_none()).then(|| {
        state.last.lock().ok().and_then(|l| l.clone()).map(|s| ask::work_brief(&s, &failures(&state), chrono::Utc::now()))
    }).flatten();
    let action = if req.action == Action::Ask && c.screenshot.is_some() && ask::wants_pointing(&req.question) { Action::Point } else { req.action };
    let ctx = context_for(&c, repo);
    let prompt = ask::build_prompt(&req.question, action, &ctx, &req.history, work.as_deref());
    let image = c.screenshot.clone().or(c.clipboard_image.clone());
    let shot = c.shot;

    let Some(pick) = ask::pick(&rooms, req.tool) else {
        // No CLI: open the web app with the question pasted in.
        let target = req.tool.unwrap_or(Tool::ClaudeCode);
        let text = if req.question.trim().is_empty() { prompt.clone() } else { format!("{}\n\n{}", req.question, ctx.selection.clone().unwrap_or_default()) };
        let msg = handoff::start_task(None, target, handoff::Dest::Web, text.trim(), true).map(|(m, _)| m).unwrap_or_else(|e| e);
        let _ = app.emit("ask-done", Done { id: req.id, tool: Some(target), reason: msg, opened_web: true, ..Default::default() });
        return Ok(());
    };

    std::thread::spawn(move || {
        if req.council {
            council(&app, &req, &rooms, &prompt, image.as_deref());
            return;
        }
        // If a tool can't answer (not signed in, out of room), the next one with room tries.
        let mut rooms = rooms;
        let mut pick = pick;
        let mut skipped: Vec<String> = vec![];
        let (answer, error) = loop {
            let (answer, error) = run_one(&app, &req.id, pick.tool, req.mode, &prompt, image.as_deref());
            let Some(e) = error.as_deref() else { break (answer, error) };
            if !answer.trim().is_empty() || e == "Stopped." { break (answer, error); }
            // Any failure: the next tool with room tries. Lasting ones are remembered.
            let failure = ask::classify(e);
            match failure {
                ask::Failure::Unsupported => mark_broken_for(pick.tool, 24 * 3600),
                ask::Failure::Other => {}
                _ => mark_broken_for(pick.tool, 600),
            }
            skipped.push(if failure == ask::Failure::Other { format!("{} failed ({}).", crate::tool_name(pick.tool), e.trim_start_matches(&format!("{}: ", crate::tool_name(pick.tool))).chars().take(120).collect::<String>()) } else { ask::explain_failure(pick.tool, failure) });
            if let Some(r) = rooms.iter_mut().find(|r| r.tool == pick.tool) { r.installed = false; }
            match ask::pick(&rooms, None) {
                Some(next) => pick = next,
                None => break (answer, Some(skipped.join(" "))),
            }
        };
        let reason = if skipped.is_empty() { pick.reason.clone() } else { skipped.join(" ") };
        let (points, text) = if action == Action::Point { ask::parse_points(&answer) } else { (vec![], answer.trim().to_owned()) };
        if !points.is_empty() { if let Some(s) = shot { show_points(&app, s, &points); } }
        let _ = app.emit("ask-done", Done { id: req.id.clone(), tool: Some(pick.tool), reason, error, points, text, opened_web: false });
    });
    Ok(())
}

/// All installed tools answer at once; then the one with most room combines them.
fn council(app: &AppHandle, req: &AskRequest, rooms: &[Room], prompt: &str, image: Option<&std::path::Path>) {
    let tools: Vec<Tool> = rooms.iter().filter(|r| r.installed && r.used_pct.is_none_or(|u| u < 98.0)).map(|r| r.tool).collect();
    let answers = Arc::new(Mutex::new(Vec::<(Tool, String)>::new()));
    let handles: Vec<_> = tools.iter().map(|&t| {
        let (app, answers, id, prompt, image, mode) = (app.clone(), answers.clone(), format!("{}:{}", req.id, t.as_str()), prompt.to_owned(), image.map(|p| p.to_path_buf()), req.mode);
        std::thread::spawn(move || {
            let (a, e) = run_one(&app, &id, t, mode, &prompt, image.as_deref());
            let e = e.map(|e| match ask::classify(&e) {
                ask::Failure::Other => e,
                f => { mark_broken_for(t, if f == ask::Failure::Unsupported { 24 * 3600 } else { 600 }); ask::explain_failure(t, f) }
            });
            let _ = app.emit("ask-done", Done { id, tool: Some(t), reason: String::new(), error: e.clone(), text: a.trim().to_owned(), ..Default::default() });
            if e.is_none() && !a.trim().is_empty() { if let Ok(mut v) = answers.lock() { v.push((t, a)); } }
        })
    }).collect();
    for h in handles { let _ = h.join(); }
    let answers = answers.lock().map(|v| v.clone()).unwrap_or_default();
    let id = format!("{}:combined", req.id);
    if answers.len() < 2 {
        let _ = app.emit("ask-done", Done { id, error: Some("Council needs at least two tools to answer.".into()), ..Default::default() });
        return;
    }
    let Some(judge) = ask::pick(rooms, None) else { return };
    let (a, e) = run_one(app, &id, judge.tool, Mode::Quick, &ask::council_prompt(&req.question, &answers), None);
    let _ = app.emit("ask-done", Done { id, tool: Some(judge.tool), reason: format!("combined by {}", crate::tool_name(judge.tool)), error: e, text: a.trim().to_owned(), ..Default::default() });
}

#[tauri::command]
pub fn ask_cancel(id: String) {
    if let Ok(mut m) = children().lock() {
        let keys: Vec<String> = m.keys().filter(|k| **k == id || k.starts_with(&format!("{id}:"))).cloned().collect();
        for k in keys {
            if let Some(mut ch) = m.remove(&k) {
                #[cfg(windows)]
                { let _ = Command::new("taskkill").args(["/T", "/F", "/PID", &ch.id().to_string()]).output(); }
                let _ = ch.kill();
                let _ = ch.wait();
            }
        }
    }
}

/// Replace / Draft reply: paste the answer where your cursor was.
#[tauri::command]
pub async fn ask_paste_back(text: String) -> CmdResult<()> {
    let origin = captured().lock().map_err(err)?.origin.clone();
    tauri::async_runtime::spawn_blocking(move || capture::paste_back(&origin, &text)).await.map_err(err)?
}

#[tauri::command]
pub fn ask_copy(text: String) -> CmdResult<()> {
    capture::copy_text(&text)
}

/// Run: open a terminal in your repo with the command typed in (not run).
#[tauri::command]
pub async fn ask_run(state: State<'_, AppState>, command: String) -> CmdResult<String> {
    let cwd = current_repo(&state).map(std::path::PathBuf::from).or_else(aiuc_core::paths::home).ok_or("No folder to open.")?;
    tauri::async_runtime::spawn_blocking(move || {
        capture::copy_text(&command)?;
        handoff::open_shell(&cwd)?;
        let typed = crate::autosend::paste_into(&["powershell", "terminal", "command prompt", "cmd.exe", "iterm"], false, std::time::Duration::from_secs(10), std::time::Duration::from_millis(1200));
        Ok(if typed == crate::autosend::Outcome::Pasted { "Typed into a new terminal. Check it, then press Enter.".into() } else { "Opened a terminal. The command is on your clipboard.".into() })
    }).await.map_err(err)?
}

/// Continue in a full chat: the app if installed, else the web, with the
/// conversation pasted in.
#[tauri::command]
pub async fn ask_open_in(tool: Tool, text: String) -> CmdResult<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dest = handoff::destinations(tool, false).first().map(|d| d.dest).unwrap_or(handoff::Dest::Web);
        handoff::start_task(None, tool, dest, &text, true).map(|(m, _)| m)
    }).await.map_err(err)?
}

// ---------- pointing ----------

/// Draw arrows over the screen where the answer points, for a few seconds.
fn show_points(app: &AppHandle, shot: capture::ShotRect, points: &[ask::Point]) {
    #[derive(Serialize, Clone)]
    struct Spot { fx: f64, fy: f64, label: String }
    let spots: Vec<Spot> = points.iter().map(|p| Spot {
        fx: (p.x / shot.img_width.max(1) as f64).clamp(0.0, 1.0),
        fy: (p.y / shot.img_height.max(1) as f64).clamp(0.0, 1.0),
        label: p.label.clone(),
    }).collect();
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        let win = match app2.get_webview_window("pointer") {
            Some(w) => w,
            None => match tauri::WebviewWindowBuilder::new(&app2, "pointer", tauri::WebviewUrl::App("index.html?overlay=pointer".into()))
                .title("AI Usage Controller pointer").decorations(false).transparent(true).always_on_top(true)
                .skip_taskbar(true).resizable(false).shadow(false).focused(false).visible(false).build() {
                Ok(w) => w,
                Err(_) => return,
            },
        };
        let _ = win.set_ignore_cursor_events(true);
        let _ = win.set_position(tauri::PhysicalPosition::new(shot.x, shot.y));
        let _ = win.set_size(tauri::PhysicalSize::new(shot.width, shot.height));
        crate::window::mac_float(&win);
        let _ = win.show();
        if let Ok(mut l) = LAST_SPOTS.lock() { *l = serde_json::to_value(&spots).unwrap_or_default(); }
        let _ = app2.emit_to("pointer", "points", &spots);
    });
}

static LAST_SPOTS: Mutex<serde_json::Value> = Mutex::new(serde_json::Value::Null);

/// The pointer overlay asks for its arrows when it loads (the first event can
/// arrive before the page is ready).
#[tauri::command]
pub fn ask_points() -> serde_json::Value {
    LAST_SPOTS.lock().map(|l| l.clone()).unwrap_or_default()
}

#[tauri::command]
pub fn ask_hide_points(app: AppHandle) {
    if let Some(w) = app.get_webview_window("pointer") { let _ = w.hide(); }
}

// ---------- prompt polish (Gemini's polish.rs) ----------

/// Rewrite a rough prompt with the repo's context, on the tool with most room
/// (Quick mode). `repo` is a repo id from the Agents box, or none.
#[tauri::command]
pub async fn polish_prompt(state: State<'_, AppState>, text: String, repo: Option<String>) -> CmdResult<String> {
    if text.trim().is_empty() { return Ok(text); }
    let root = match repo.as_deref() {
        Some(id) => match crate::dispatch::resolve(&state.engine, id)? { crate::dispatch::Repo::Local(r) => Some(r), crate::dispatch::Repo::GitHub(_) => None },
        None => current_repo(&state).map(std::path::PathBuf::from),
    };
    // The newest failed test output for this repo, if the widget holds one.
    let snap = state.last.lock().map_err(err)?.clone();
    let failure = snap.and_then(|s| s.agents.iter()
        .filter(|a| a.verify.as_deref() == Some("failed"))
        .filter(|a| root.as_ref().is_none_or(|r| std::path::Path::new(&a.cwd).starts_with(r) || a.cwd.contains(&*r.file_name().map(|n| n.to_string_lossy()).unwrap_or_default())))
        .find_map(|a| crate::agents::verify_output(a.id)));
    let mut rooms = rooms(&state);
    if ask::pick(&rooms, None).is_none() { return Err("Polishing needs Claude Code, Codex or Gemini installed.".into()); }
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = aiuc_core::polish::gather(root.as_deref(), failure.as_deref());
        let request = aiuc_core::polish::request(&text, &ctx);
        let mut last = String::new();
        // Like Ask: if one tool can't answer, the next one with room does.
        while let Some(pick) = ask::pick(&rooms, None) {
            let (out, e) = run_cli(None, &format!("polish{}", chrono::Utc::now().timestamp_millis()), pick.tool, Mode::Quick, &request, None);
            match e {
                None if !out.trim().is_empty() => return Ok(aiuc_core::polish::clean(&out)),
                e => {
                    let e = e.unwrap_or_default();
                    match ask::classify(&e) {
                        ask::Failure::Unsupported => mark_broken_for(pick.tool, 24 * 3600),
                        ask::Failure::Other => {}
                        _ => mark_broken_for(pick.tool, 600),
                    }
                    last = e;
                    if let Some(r) = rooms.iter_mut().find(|r| r.tool == pick.tool) { r.installed = false; }
                }
            }
        }
        Err(if last.is_empty() { "No tool could polish this right now.".into() } else { last })
    }).await.map_err(err)?
}

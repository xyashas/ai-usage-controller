//! Ask the notch: run a question on the AI with room, stream the answer, and
//! act on it (paste back, run, open in an app, point on screen).

use crate::capture::{self, Captured};
use crate::{err, handoff, AppState, CmdResult};
use aiuc_core::ask::{self, Action, Context, Mode, Pick, Room, StreamReader, Turn};
use aiuc_core::model::Tool;
use aiuc_core::{inbox, memory};
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

pub fn set_captured(c: Captured) {
    if let Ok(mut slot) = captured().lock() { *slot = c; }
}

pub fn captured_now() -> Captured {
    captured().lock().map(|c| c.clone()).unwrap_or_default()
}

/// Everything an ask needs: which tools can answer, the first pick, the
/// prompt (with the screen context and, for work questions, your AI work) and
/// the image to attach. `extra` is added to the request (e.g. "answer briefly").
pub fn prepare(state: &AppState, question: &str, action: Action, history: &[Turn], extra: &str) -> Option<(Vec<Room>, Pick, String, Option<std::path::PathBuf>)> {
    let c = captured_now();
    let rooms = rooms(state);
    let pick = ask::pick(&rooms, None)?;
    let work = ask::about_work(question).then(|| {
        state.last.lock().ok().and_then(|l| l.clone()).map(|s| ask::work_brief(&s, &failures(state), chrono::Utc::now()))
    }).flatten();
    let ctx = context_for(&c, current_repo(state), about_me(state));
    let q = if extra.is_empty() { question.to_owned() } else { format!("{question}\n\n({extra})") };
    let prompt = ask::build_prompt(&q, action, &ctx, history, work.as_deref());
    Some((rooms, pick, prompt, c.screenshot.clone().or(c.clipboard_image.clone())))
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

/// What you asked Ask to remember, for the prompt (if that's switched on).
fn about_me(state: &AppState) -> Option<String> {
    let e = state.engine.lock().ok()?;
    if !e.settings.ask_memory { return None; }
    memory::prompt_block(&memory::list(&e.store))
}

/// "Remember that…", "forget…", "save this…": handled here, with no AI call.
/// Returns what to tell you, or None when it's an ordinary question.
pub fn handle_note(state: &AppState, intent: &ask::Intent, c: &Captured) -> Option<Result<String, String>> {
    use ask::Intent;
    let mut e = state.engine.lock().ok()?;
    let now = chrono::Utc::now();
    // "Remind me at 5 to…", "remind me when Claude resets": a watch (it notifies you).
    if let Intent::Save(t) = intent {
        if t.to_lowercase().starts_with("remind me") {
            if let Some(w) = e.snapshot(now).ok().and_then(|snap| aiuc_core::watch::parse(t, &snap, now)) {
                let mut list = aiuc_core::watch::load(&e.store);
                list.push(w.clone());
                return Some(aiuc_core::watch::save(&e.store, &list).map(|_| match &w.condition {
                    aiuc_core::watch::Condition::At { at } => format!("Okay, I'll remind you at {}.", at.with_timezone(&chrono::Local).format("%-I:%M %p")),
                    _ => format!("Okay, I'll tell you {}.", lower_first(&w.label)),
                }).map_err(|x| x.to_string()));
            }
        }
    }
    Some(match intent {
        Intent::Remember(t) if !e.settings.ask_memory => { let _ = t; Err("Remembering is off. Switch it on in Settings → Ask.".into()) }
        Intent::Remember(t) => memory::add(&e.store, t, now).map(|p| format!("Got it. I'll remember: {}", p.text)).map_err(|x| x.to_string()),
        Intent::Forget(t) if ["everything", "everything about me", "all of it", "all that"].contains(&t.trim()) => {
            memory::clear(&e.store).map(|_| "Done. I've forgotten everything you told me.".to_owned()).map_err(|x| x.to_string())
        }
        Intent::Forget(t) => match memory::forget_matching(&e.store, t) {
            Ok(0) => Ok("I didn't have anything saved about that.".into()),
            Ok(1) => Ok("Forgotten.".into()),
            Ok(n) => Ok(format!("Forgot {n} things.")),
            Err(x) => Err(x.to_string()),
        },
        Intent::Save(t) => {
            let (text, source) = if !t.trim().is_empty() { (t.clone(), inbox::Source::Voice) }
                else if let Some(s) = c.selection.clone().filter(|s| !s.trim().is_empty()) { (s, inbox::Source::Selection) }
                else if let Some(s) = c.clipboard.clone().filter(|s| !s.trim().is_empty()) { (s, inbox::Source::Clipboard) }
                else if let Some(v) = crate::buddy::last_answer() { (v.answer, inbox::Source::Ask) }
                else { return Some(Err("Select some text first, or say what to save: \"save this: call Sam at 5\".".into())) };
            let app = Some(c.origin.app.clone()).filter(|a| !a.is_empty());
            inbox::capture(&e.store, &text, source, app.as_deref(), now).map(|i| match i.remind_at {
                Some(at) => format!("Saved. I'll remind you {}.", at.with_timezone(&chrono::Local).format("at %-I:%M %p on %a")),
                None => "Saved to your inbox.".into(),
            }).map_err(|x| x.to_string())
        }
        _ => return None,
    })
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|h| h.to_lowercase().chain(c).collect()).unwrap_or_default()
}

#[tauri::command]
pub fn memory_list(state: State<AppState>) -> CmdResult<Vec<memory::Pref>> {
    Ok(memory::list(&state.engine.lock().map_err(err)?.store))
}

#[tauri::command]
pub fn memory_add(state: State<AppState>, text: String) -> CmdResult<memory::Pref> {
    memory::add(&state.engine.lock().map_err(err)?.store, &text, chrono::Utc::now()).map_err(err)
}

#[tauri::command]
pub fn memory_remove(state: State<AppState>, id: i64) -> CmdResult<()> {
    memory::remove(&state.engine.lock().map_err(err)?.store, id).map_err(err)
}

#[tauri::command]
pub fn memory_clear(state: State<AppState>) -> CmdResult<()> {
    memory::clear(&state.engine.lock().map_err(err)?.store).map_err(err)
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

fn context_for(c: &Captured, repo: Option<String>, about_me: Option<String>) -> Context {
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
        about_me,
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
    // A CLI that hangs (waiting on a login, a stuck network) is stopped, not left spinning.
    let timed_out = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let (key, flag) = (id.to_owned(), timed_out.clone());
        let limit = std::time::Duration::from_secs(if mode == Mode::Deep { 600 } else { 180 });
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while start.elapsed() < limit {
                std::thread::sleep(std::time::Duration::from_millis(500));
                if !children().lock().map(|m| m.contains_key(&key)).unwrap_or(false) { return; }
            }
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            kill_one(&key);
        });
    }
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
        if timed_out.load(std::sync::atomic::Ordering::SeqCst) && answer.trim().is_empty() {
            return Some(format!("{} didn't answer in time. Try again, or pick another tool.", crate::tool_name(tool)));
        }
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
    // "Remember that I use TypeScript", "save this": no AI needed.
    if req.history.is_empty() {
        if let Some(r) = handle_note(&state, &ask::voice_intent(&req.question, false), &c) {
            let (text, error) = match r { Ok(t) => (t, None), Err(e) => (String::new(), Some(e)) };
            let _ = app.emit("ask-done", Done { id: req.id, text, error, reason: "noted".into(), ..Default::default() });
            return Ok(());
        }
    }
    let rooms = rooms(&state);
    let repo = current_repo(&state);
    let work = (ask::about_work(&req.question) || req.question.trim().is_empty() && c.selection.is_none() && c.screenshot.is_none()).then(|| {
        state.last.lock().ok().and_then(|l| l.clone()).map(|s| ask::work_brief(&s, &failures(&state), chrono::Utc::now()))
    }).flatten();
    let action = if req.action == Action::Ask && c.screenshot.is_some() && ask::wants_pointing(&req.question) { Action::Point } else { req.action };
    let ctx = context_for(&c, repo, about_me(&state));
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
        let a = answer(Some(&app), &req.id, rooms, pick, req.mode, &prompt, image.as_deref());
        let (answer, error, pick, reason) = (a.text, a.error, a.pick, a.reason);
        let (points, text) = if action == Action::Point { ask::parse_points(&answer) } else { (vec![], answer.trim().to_owned()) };
        if !points.is_empty() { if let Some(s) = shot { crate::buddy::point(&app, s, &points, &text); } }
        if error.is_none() && !text.is_empty() { crate::buddy::set_last_answer(&req.question, &text, pick.tool, &reason); }
        let _ = app.emit("ask-done", Done { id: req.id.clone(), tool: Some(pick.tool), reason, error, points, text, opened_web: false });
    });
    Ok(())
}

pub struct Answered {
    pub text: String,
    pub error: Option<String>,
    /// The tool that answered (or tried last).
    pub pick: Pick,
    /// Why that tool, or why others were skipped.
    pub reason: String,
}

/// Ask `pick`; if it can't answer (not signed in, out of room, any error with
/// no answer), the next tool with room tries. With `app`, the answer streams
/// as `ask-delta` events under `id`.
pub fn answer(app: Option<&AppHandle>, id: &str, mut rooms: Vec<Room>, mut pick: Pick, mode: Mode, prompt: &str, image: Option<&std::path::Path>) -> Answered {
    let mut skipped: Vec<String> = vec![];
    loop {
        let (text, error) = run_cli(app, id, pick.tool, mode, prompt, image);
        let failed = match error.as_deref() {
            Some(e) if text.trim().is_empty() && e != "Stopped." => e.to_owned(),
            _ => {
                let reason = if skipped.is_empty() { pick.reason.clone() } else { skipped.join(" ") };
                return Answered { text, error, reason, pick };
            }
        };
        let failure = ask::classify(&failed);
        match failure {
            ask::Failure::Unsupported => mark_broken_for(pick.tool, 24 * 3600),
            ask::Failure::Other => {}
            _ => mark_broken_for(pick.tool, 600),
        }
        let prefix = format!("{}: ", crate::tool_name(pick.tool));
        skipped.push(if failure == ask::Failure::Other {
            format!("{} failed ({}).", crate::tool_name(pick.tool), failed.trim_start_matches(prefix.as_str()).chars().take(120).collect::<String>())
        } else { ask::explain_failure(pick.tool, failure) });
        if let Some(r) = rooms.iter_mut().find(|r| r.tool == pick.tool) { r.installed = false; }
        match ask::pick(&rooms, None) {
            Some(next) => pick = next,
            None => return Answered { text, error: Some(skipped.join(" ")), reason: String::new(), pick },
        }
    }
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
            if let Some(ch) = m.remove(&k) { stop_child(ch); }
        }
    }
}

fn stop_child(mut ch: Child) {
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; let _ = Command::new("taskkill").args(["/T", "/F", "/PID", &ch.id().to_string()]).creation_flags(0x0800_0000).output(); }
    let _ = ch.kill();
    let _ = ch.wait();
}

fn kill_one(key: &str) {
    let ch = children().lock().ok().and_then(|mut m| m.remove(key));
    if let Some(ch) = ch { stop_child(ch); }
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

// ---------- pointing (drawn by buddy.rs) ----------

#[tauri::command]
pub fn ask_hide_points(app: AppHandle) {
    crate::buddy::hide(&app);
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

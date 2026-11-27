//! Agent status, approvals, "verified done" and the queue at limit reset.
//!
//! - The same program doubles as the hook Claude Code and Codex call
//!   (`--aiuc-hook claude|codex`); see `hook_main` and core's agent_status.rs.
//! - Each worker tick (`tick`) runs tests for agents that just finished, sends
//!   queued tasks whose limit has reset, and notifies on what changed.

use crate::{dispatch, err, AppState, CmdResult};
use aiuc_core::agent_status::{self as st, Answer, InstallResult};
use aiuc_core::model::Tool;
use aiuc_core::snapshot::AgentRun;
use aiuc_core::{verify, Engine, Snapshot};
use chrono::{Duration, Utc};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, State};

/// Called first thing in `main`. True when this run was a hook call, which is
/// handled without starting the app.
pub fn hook_main() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some(st::HOOK_FLAG) { return false; }
    // Our own Ask runs: not an agent, nothing to report or approve.
    if std::env::var_os(crate::ask::ENV_FLAG).is_some() { return true; }
    let base = st::dir();
    match args.get(2).map(String::as_str) {
        Some("claude") => {
            use std::io::Read;
            let mut input = String::new();
            let _ = std::io::stdin().take(4 << 20).read_to_string(&mut input);
            // Only wait for an answer if the widget is running to give one, and
            // you're not already at the terminal: Claude Code shows its own
            // prompt only after this hook returns, so waiting there would stall it.
            let wait = if alive(&base) && !crate::capture::at_terminal() { std::time::Duration::from_secs(st::APPROVAL_WAIT_SECS) } else { std::time::Duration::ZERO };
            if let Some(out) = st::handle_claude(&base, &input, wait) { println!("{out}"); }
        }
        Some("codex") => { if let Some(json) = args.last() { st::handle_codex(&base, json); } }
        _ => {}
    }
    true
}

const ALIVE: &str = "alive";

fn alive(base: &Path) -> bool {
    std::fs::metadata(base.join(ALIVE)).and_then(|m| m.modified()).ok()
        .and_then(|t| t.elapsed().ok()).is_some_and(|e| e < std::time::Duration::from_secs(20))
}

fn heartbeat(base: &Path) {
    let _ = std::fs::create_dir_all(base);
    let _ = std::fs::write(base.join(ALIVE), Utc::now().to_rfc3339());
}

fn exe() -> CmdResult<PathBuf> {
    std::env::current_exe().map_err(err)
}

/// Turn agent status on or off: installs (or removes) the hooks.
#[tauri::command]
pub async fn set_agent_status(state: State<'_, AppState>, on: bool, approvals: Option<bool>) -> CmdResult<InstallResult> {
    let engine = state.engine.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let approvals = approvals.unwrap_or_else(|| engine.lock().map(|e| e.settings.approvals).unwrap_or(true));
        let result = if on { st::install(&exe()?, approvals).map_err(err)? } else { st::uninstall().map_err(err)?; InstallResult::default() };
        let mut eng = engine.lock().map_err(err)?;
        eng.settings.agent_status = on;
        eng.settings.approvals = approvals;
        eng.save_settings().map_err(err)?;
        Ok(result)
    }).await.map_err(err)?
}

/// Keep the hooks pointing at this copy of the app (it may have moved or updated).
pub fn refresh_hooks(engine: &Mutex<Engine>) {
    let (on, approvals) = match engine.lock() { Ok(e) => (e.settings.agent_status, e.settings.approvals), Err(_) => return };
    if on { if let Ok(p) = exe() { let _ = st::install(&p, approvals); } }
}

#[tauri::command]
pub fn answer_approval(id: String, answer: Answer) -> CmdResult<()> {
    st::answer(&st::dir(), &id, answer).map_err(err)
}

// ---------- verified done ----------

static RUNNING: AtomicBool = AtomicBool::new(false);

fn outputs() -> &'static Mutex<HashMap<i64, String>> {
    static O: OnceLock<Mutex<HashMap<i64, String>>> = OnceLock::new();
    O.get_or_init(Default::default)
}

/// The end of the last test run's output (kept in memory only).
#[tauri::command]
pub fn verify_output(id: i64) -> Option<String> {
    outputs().lock().ok()?.get(&id).cloned()
}

/// Run the tests for a task now.
#[tauri::command]
pub fn verify_task(app: AppHandle, state: State<AppState>, id: i64) -> CmdResult<()> {
    let row = state.engine.lock().map_err(err)?.store.task(id).map_err(err)?.ok_or("That task is gone.")?;
    let dir = PathBuf::from(&row.cwd);
    if !dir.is_dir() { return Err("This task's folder isn't on this computer.".into()); }
    let cmd = verify::detect(&dir).ok_or("No tests found. Add the command to .aiuc/verify in the repo.")?;
    if RUNNING.swap(true, Ordering::SeqCst) { return Err("Tests are already running for another task. Try again when they finish.".into()); }
    spawn_verify(app, state.engine.clone(), id, row.tool, dir, cmd);
    Ok(())
}

fn spawn_verify(app: AppHandle, engine: std::sync::Arc<Mutex<Engine>>, id: i64, tool: Tool, dir: PathBuf, cmd: String) {
    if let Ok(e) = engine.lock() { let _ = e.store.set_verify(id, "running", Utc::now(), None); }
    std::thread::spawn(move || {
        let run = verify::run(&dir, &cmd, std::time::Duration::from_secs(15 * 60));
        RUNNING.store(false, Ordering::SeqCst);
        let mut tail = format!("$ {cmd}\n{}", run.tail);
        if let verify::Outcome::Error(e) = &run.outcome { tail.push_str(&format!("\nCouldn't run it: {e}")); }
        if let Ok(mut o) = outputs().lock() { o.insert(id, tail); }
        let notify = match engine.lock() {
            Ok(e) => { let _ = e.store.set_verify(id, run.outcome.as_str(), Utc::now(), Some(run.secs)); e.settings.alerts.notifications }
            Err(_) => return,
        };
        if notify {
            let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let (title, body) = match run.outcome {
                verify::Outcome::Passed => (format!("{} finished: tests pass", crate::tool_name(tool)), format!("{folder} · {}", secs(run.secs))),
                verify::Outcome::Failed => (format!("{} finished: tests fail", crate::tool_name(tool)), format!("{folder} · open the widget to see the output")),
                verify::Outcome::TimedOut => (format!("{} finished: tests took too long", crate::tool_name(tool)), format!("{folder} · stopped after 15 min")),
                verify::Outcome::Error(_) => (format!("{} finished", crate::tool_name(tool)), format!("{folder} · couldn't run the tests")),
            };
            crate::notify(&app, &title, &body);
        }
    });
}

fn secs(s: f64) -> String {
    if s < 90.0 { format!("{s:.0} s") } else { format!("{:.0} min", s / 60.0) }
}

/// An agent that finished since its last test run, and has been quiet long
/// enough that it's really done.
fn needs_verify(a: &AgentRun, now: chrono::DateTime<Utc>) -> bool {
    let Some(last) = a.last_activity_at else { return false };
    matches!(a.state.as_str(), "done" | "idle")
        && now - last >= Duration::seconds(if a.state == "done" { 20 } else { 0 })
        && now - last < Duration::hours(2)
        && a.verify.as_deref() != Some("running")
        && a.verify_at.is_none_or(|v| v < last)
        && Path::new(&a.cwd).is_dir()
}

// ---------- the tick ----------

#[derive(Default)]
pub struct Ticker {
    states: HashMap<i64, String>,
    approvals: std::collections::HashSet<String>,
    no_tests: std::collections::HashSet<(i64, i64)>,
    sending: bool,
}

impl Ticker {
    pub fn tick(&mut self, app: &AppHandle, engine: &std::sync::Arc<Mutex<Engine>>, snap: &Snapshot) {
        let base = st::dir();
        let (status_on, verify_on, notify_on) = match engine.lock() {
            Ok(e) => (e.settings.agent_status, e.settings.verify_done, e.settings.alerts.notifications),
            Err(_) => return,
        };
        if status_on { heartbeat(&base); }
        let now = Utc::now();

        // A tool wants your OK: say so once (the widget also opens on it).
        for a in &snap.approvals {
            if self.approvals.insert(a.id.clone()) && notify_on {
                crate::notify(app, &format!("{} needs your OK", crate::tool_name(a.tool)), &format!("{} · {}: {}", a.repo_name, a.tool_name, a.summary));
            }
        }
        self.approvals.retain(|id| snap.approvals.iter().any(|a| &a.id == id));

        for a in &snap.agents {
            let before = self.states.insert(a.id, a.state.clone());
            let changed = before.as_deref().is_some_and(|b| b != a.state);
            if changed && notify_on && a.state == "needs_approval" && snap.approvals.is_empty() {
                crate::notify(app, &format!("{} is waiting for you", crate::tool_name(a.tool)), &a.repo_name);
            }
            if verify_on && needs_verify(a, now) {
                let key = (a.id, a.last_activity_at.map(|t| t.timestamp()).unwrap_or(0));
                if self.no_tests.contains(&key) { continue; }
                match verify::detect(Path::new(&a.cwd)) {
                    Some(cmd) if !RUNNING.swap(true, Ordering::SeqCst) => spawn_verify(app.clone(), engine.clone(), a.id, a.tool, PathBuf::from(&a.cwd), cmd),
                    Some(_) => {} // one run at a time; this one goes next tick
                    None => {
                        self.no_tests.insert(key);
                        if changed && notify_on { crate::notify(app, &format!("{} finished", crate::tool_name(a.tool)), &a.repo_name); }
                    }
                }
            } else if changed && notify_on && a.state == "done" && !verify_on {
                crate::notify(app, &format!("{} finished", crate::tool_name(a.tool)), &a.repo_name);
            }
        }

        if !self.sending { self.send_due(app, engine, snap, notify_on); }
    }

    /// Queued tasks whose tool has room again. Terminal tasks go at once; app and
    /// web ones wait for a quiet keyboard, since they're pasted in.
    fn send_due(&mut self, app: &AppHandle, engine: &std::sync::Arc<Mutex<Engine>>, snap: &Snapshot, notify_on: bool) {
        let due = match engine.lock() { Ok(e) => e.store.queued_due(Utc::now()).unwrap_or_default(), Err(_) => return };
        let room = |t: Tool| snap.tools.iter().find(|s| s.tool == t).and_then(|s| s.headroom_pct);
        let quiet = crate::autosend::idle_secs().is_none_or(|s| s >= 20);
        let ready: Vec<_> = due.into_iter()
            .filter(|r| room(r.tool).is_none_or(|h| h > 5.0))
            .filter(|r| r.dest.as_deref() == Some("terminal") || quiet)
            .collect();
        if ready.is_empty() { return; }
        self.sending = true;
        let (app, engine) = (app.clone(), engine.clone());
        // Sending can take a while (opening apps), so off the worker thread.
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        std::thread::spawn(move || {
            let mut sent = Vec::new();
            for r in ready.into_iter().take(3) {
                match dispatch::send_queued(&engine, &r, r.tool) {
                    Ok(_) => sent.push(r.tool),
                    Err(e) => { if notify_on { crate::notify(&app, "Couldn't send a queued task", &e); } let _ = engine.lock().map(|e| e.store.dismiss_task(r.id)); }
                }
            }
            if notify_on && !sent.is_empty() {
                let n = sent.len();
                crate::notify(&app, &format!("{} reset: sent {n} queued task{}", crate::tool_name(sent[0]), if n == 1 { "" } else { "s" }), "Open the widget to follow them.");
            }
            flag.store(true, Ordering::SeqCst);
        });
        SENDING.lock().map(|mut s| *s = Some(done)).ok();
    }

    /// Called every tick before `tick`: clears `sending` once the last batch is out.
    pub fn poll(&mut self) {
        if !self.sending { return; }
        let finished = SENDING.lock().ok().and_then(|s| s.as_ref().map(|d| d.load(Ordering::SeqCst))).unwrap_or(true);
        if finished { self.sending = false; }
    }
}

static SENDING: Mutex<Option<std::sync::Arc<AtomicBool>>> = Mutex::new(None);

//! The dispatcher's native side: plan a request, then start each task in its
//! own worktree and terminal, and record it in the local task ledger.

use crate::{err, handoff, AppState, CmdResult};
use aiuc_core::dispatch::{self, Routed, ToolRoom};
use aiuc_core::model::Tool;
use aiuc_core::repo_handoff as rh;
use aiuc_core::store::TaskRow;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

#[derive(Serialize)]
pub struct Plan {
    pub tasks: Vec<Routed>,
    pub rooms: Vec<ToolRoom>,
    /// Where each tool can open for this repo, best first.
    pub dests: BTreeMap<Tool, Vec<handoff::DestOption>>,
}

/// Each tool's room (5-hour limit first). Every enabled tool can run a task:
/// at worst on the web.
fn rooms(state: &AppState) -> CmdResult<Vec<ToolRoom>> {
    let snap = match state.last.lock().map_err(err)?.clone() {
        Some(s) => s,
        None => crate::snapshot_now(state)?,
    };
    Ok(Tool::ALL.into_iter().map(|tool| {
        let status = snap.tools.iter().find(|t| t.tool == tool);
        let primary = status.and_then(|t| {
            let w: Vec<_> = t.windows.iter().filter(|w| w.source != aiuc_core::metrics::Source::Learning).collect();
            // At a limit, the window that's used up is the one to wait for.
            let full = w.iter().filter(|w| w.used_pct >= 98.0).max_by_key(|w| w.resets_at).copied();
            full.or(w.iter().find(|w| w.key == "five_hour").copied()).or(w.iter().min_by(|a, b| a.headroom_pct.total_cmp(&b.headroom_pct)).copied())
        });
        ToolRoom {
            tool, ready: status.is_none_or(|t| t.enabled), active: snap.active_tool == Some(tool),
            used_pct: primary.map(|w| w.used_pct), resets_at: primary.and_then(|w| w.resets_at),
        }
    }).collect())
}

/// A repo id from the UI: a folder on this computer, or `github:owner/name`.
pub enum Repo {
    Local(PathBuf),
    GitHub(String),
}

pub fn resolve(engine: &Mutex<aiuc_core::Engine>, id: &str) -> CmdResult<Repo> {
    if let Some(name) = id.strip_prefix("github:") {
        let valid = name.split('/').count() == 2 && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c));
        return if valid { Ok(Repo::GitHub(name.to_owned())) } else { Err("Not a GitHub repository.".into()) };
    }
    let root = rh::repo_root(std::path::Path::new(id)).map_err(err)?;
    // Only repos the widget lists, so a page can't point this at any folder.
    if !crate::repo_candidates(engine)?.contains(&root) { return Err("That repository isn't in your list.".into()); }
    Ok(Repo::Local(root))
}

#[tauri::command]
pub async fn plan_dispatch(state: State<'_, AppState>, text: String, repo: Option<String>) -> CmdResult<Plan> {
    let rooms = rooms(&state)?;
    let engine = state.engine.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let local = match repo.as_deref() { Some(id) => matches!(resolve(&engine, id)?, Repo::Local(_)), None => true };
        let tasks = dispatch::route(&dispatch::split(&text), &rooms);
        let dests = Tool::ALL.into_iter().map(|t| (t, handoff::destinations(t, local))).collect();
        Ok(Plan { tasks, rooms, dests })
    }).await.map_err(err)?
}

#[derive(Deserialize)]
pub struct RunTask {
    pub text: String,
    pub tool: Tool,
    pub dest: handoff::Dest,
    /// Hold it until the tool's limit resets.
    #[serde(default)]
    pub queue: bool,
}

#[derive(Serialize)]
pub struct Started {
    pub text: String,
    pub tool: Tool,
    pub dest: handoff::Dest,
    pub branch: Option<String>,
    /// What happened, e.g. "Sent to Codex in the browser".
    pub message: Option<String>,
    /// The task reached the tool (started, or pasted in), so the next one can go.
    pub delivered: bool,
    pub error: Option<String>,
}

/// What an app or web session is told: the repo and branch, since it can't see
/// which folder the task came from.
fn pasted_prompt(repo_name: &str, folder: Option<&std::path::Path>, branch: Option<&str>, text: &str) -> String {
    match (folder, branch) {
        (Some(f), Some(b)) => format!("In the project folder {} (branch `{b}`):\n\n{text}", f.display()),
        (Some(f), None) => format!("In the project folder {}:\n\n{text}", f.display()),
        (None, _) => format!("In the GitHub repo {repo_name}, on a new branch:\n\n{text}"),
    }
}

/// (branch, (message, delivered), folder or repo name)
type Launched = (Option<String>, (String, bool), String);

/// Start one task: make its worktree if asked, open the tool, hand it the prompt.
pub fn launch_task(root: Option<&std::path::Path>, repo_name: &str, text: &str, tool: Tool, dest: handoff::Dest, worktrees: bool, auto: bool) -> Result<Launched, String> {
    // Local tasks get a worktree when asked; web tasks run in the cloud.
    let (cwd, branch) = match (root, dest) {
        (Some(root), handoff::Dest::Terminal | handoff::Dest::App) if worktrees => {
            let (dir, branch) = dispatch::create_worktree(root, text).map_err(|e| format!("Couldn't create a worktree: {e}"))?;
            (Some(dir), Some(branch))
        }
        (Some(root), _) => (Some(root.to_path_buf()), rh::git(root, &["rev-parse", "--abbrev-ref", "HEAD"]).ok().map(|s| s.trim().to_string())),
        (None, _) => (None, None),
    };
    let prompt = if dest == handoff::Dest::Terminal { text.to_owned() } else {
        let folder = if dest == handoff::Dest::App { cwd.as_deref() } else { None };
        pasted_prompt(repo_name, folder, branch.as_deref(), text)
    };
    let message = handoff::start_task(cwd.as_deref(), tool, dest, &prompt, auto)?;
    let where_ = cwd.as_ref().map(|c| c.to_string_lossy().into_owned()).unwrap_or_else(|| repo_name.to_owned());
    Ok((branch, message, where_))
}

pub fn dest_str(d: handoff::Dest) -> Option<String> {
    serde_json::to_value(d).ok().and_then(|v| v.as_str().map(str::to_owned))
}

fn at_local(t: chrono::DateTime<Utc>) -> String {
    let l = t.with_timezone(&chrono::Local);
    if l.date_naive() == chrono::Local::now().date_naive() { l.format("%-I:%M %p").to_string() } else { l.format("%a %-I:%M %p").to_string() }
}

#[tauri::command]
pub async fn run_dispatch(window: tauri::WebviewWindow, state: State<'_, AppState>, repo: String, tasks: Vec<RunTask>, worktrees: bool) -> CmdResult<Vec<Started>> {
    if window.label() != "widget" { return Err("Open the widget to run tasks.".into()); }
    if tasks.is_empty() || tasks.len() > 6 { return Err("Run between 1 and 6 tasks at a time.".into()); }
    let engine = state.engine.clone();
    let rooms = if tasks.iter().any(|t| t.queue) { rooms(&state)? } else { vec![] };
    tauri::async_runtime::spawn_blocking(move || {
        let target = resolve(&engine, &repo)?;
        let (root, repo_name) = match &target {
            Repo::Local(root) => (Some(root.clone()), rh::status(root).name),
            Repo::GitHub(name) => (None, name.clone()),
        };
        let repo_field = root.as_ref().map(|r| r.to_string_lossy().into_owned()).unwrap_or_else(|| repo_name.clone());
        let auto = engine.lock().map_err(err)?.settings.auto_send;
        let mut out = Vec::new();
        for t in tasks {
            let text = t.text.trim().to_string();
            if text.is_empty() { continue; }
            if t.queue {
                let Some(at) = rooms.iter().find(|r| r.tool == t.tool).and_then(|r| r.resets_at).filter(|a| *a > Utc::now()) else {
                    out.push(Started { text, tool: t.tool, dest: t.dest, branch: None, message: None, delivered: false, error: Some(format!("No reset time known for {}.", crate::tool_name(t.tool))) });
                    continue;
                };
                let row = TaskRow { queued_until: Some(at), worktree: worktrees, ..TaskRow::new(repo_field.clone(), repo_field.clone(), None, t.tool, text.clone(), dest_str(t.dest)) };
                engine.lock().map_err(err)?.record_task(&row).map_err(err)?;
                let message = format!("Queued. Sends when {} resets at {}", crate::tool_name(t.tool), at_local(at));
                out.push(Started { text, tool: t.tool, dest: t.dest, branch: None, message: Some(message), delivered: true, error: None });
                continue;
            }
            match launch_task(root.as_deref(), &repo_name, &text, t.tool, t.dest, worktrees, auto) {
                Ok((branch, (message, delivered), cwd)) => {
                    let row = TaskRow::new(repo_field.clone(), cwd, branch.clone(), t.tool, text.clone(), dest_str(t.dest));
                    engine.lock().map_err(err)?.record_task(&row).map_err(err)?;
                    out.push(Started { text, tool: t.tool, dest: t.dest, branch, message: Some(message), delivered, error: None });
                }
                Err(e) => out.push(Started { text, tool: t.tool, dest: t.dest, branch: None, message: None, delivered: false, error: Some(e) }),
            }
        }
        Ok(out)
    }).await.map_err(err)?
}

/// Send a queued task: the one stored, optionally on another tool. Its repo is
/// a folder on this computer or `owner/name` on GitHub.
pub fn send_queued(engine: &Mutex<aiuc_core::Engine>, row: &TaskRow, tool: Tool) -> Result<String, String> {
    let path = std::path::Path::new(&row.repo);
    let (root, repo_name) = if path.is_absolute() {
        if !path.is_dir() { return Err(format!("{} isn't on this computer any more.", row.repo)); }
        (Some(path.to_path_buf()), rh::status(path).name)
    } else { (None, row.repo.clone()) };
    let dest = row.dest.as_deref().and_then(|d| serde_json::from_value::<handoff::Dest>(serde_json::Value::String(d.into())).ok()).unwrap_or(handoff::Dest::Terminal);
    // The other tool may not open where this one did.
    let dests = handoff::destinations(tool, root.is_some());
    let dest = if dests.iter().any(|d| d.dest == dest) { dest } else { dests.first().map(|d| d.dest).unwrap_or(handoff::Dest::Web) };
    let auto = engine.lock().map_err(err)?.settings.auto_send;
    let (branch, (message, _), cwd) = launch_task(root.as_deref(), &repo_name, &row.text, tool, dest, row.worktree, auto)?;
    let eng = engine.lock().map_err(err)?;
    eng.store.start_queued(row.id, Utc::now(), tool, &cwd, branch.as_deref()).map_err(err)?;
    if row.dest != dest_str(dest) {
        let _ = eng.store.set_task_dest(row.id, dest_str(dest).as_deref());
    }
    Ok(message)
}

/// "Send now" on a queued task, or "Move to <tool>".
#[tauri::command]
pub async fn run_queued(state: State<'_, AppState>, id: i64, tool: Option<Tool>) -> CmdResult<String> {
    let engine = state.engine.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let row = engine.lock().map_err(err)?.store.task(id).map_err(err)?.filter(|r| r.queued_until.is_some()).ok_or("That task isn't queued any more.")?;
        send_queued(&engine, &row, tool.unwrap_or(row.tool))
    }).await.map_err(err)?
}

#[tauri::command]
pub fn dismiss_agent(state: State<AppState>, id: i64) -> CmdResult<()> {
    state.engine.lock().map_err(err)?.dismiss_task(id).map_err(err)
}

/// Start the system's own dictation into the focused text box: Windows voice
/// typing (Win+H) or macOS Dictation. Nothing is recorded by this app.
#[tauri::command]
pub fn start_dictation(app: tauri::AppHandle) -> CmdResult<()> {
    #[cfg(windows)]
    {
        let _ = app;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_LWIN};
        unsafe {
            keybd_event(VK_LWIN as u8, 0, 0, 0);
            keybd_event(b'H', 0, 0, 0);
            keybd_event(b'H', 0, KEYEVENTF_KEYUP, 0);
            keybd_event(VK_LWIN as u8, 0, KEYEVENTF_KEYUP, 0);
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        app.run_on_main_thread(|| {
            use objc2::sel;
            use objc2::MainThreadMarker;
            use objc2_app_kit::NSApplication;
            if let Some(mtm) = MainThreadMarker::new() {
                let ns_app = NSApplication::sharedApplication(mtm);
                unsafe { ns_app.sendAction_to_from(sel!(startDictation:), None, None) };
            }
        }).map_err(err)
    }
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        Err("Dictation isn't available on Linux.".into())
    }
}

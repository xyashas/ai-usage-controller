//! The dispatcher's native side: plan a request, then start each task in its
//! own worktree and terminal, and record it in the local task ledger.

use crate::{err, handoff, AppState, CmdResult};
use aiuc_core::dispatch::{self, Routed, ToolRoom};
use aiuc_core::model::Tool;
use aiuc_core::repo_handoff as rh;
use aiuc_core::store::TaskRow;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

#[derive(Serialize)]
pub struct Plan {
    pub tasks: Vec<Routed>,
    pub rooms: Vec<ToolRoom>,
}

/// Each tool's room (5-hour limit first) and whether its CLI is installed.
fn rooms(state: &AppState) -> CmdResult<Vec<ToolRoom>> {
    let snap = match state.last.lock().map_err(err)?.clone() {
        Some(s) => s,
        None => crate::snapshot_now(state)?,
    };
    Ok(Tool::ALL.into_iter().map(|tool| {
        let status = snap.tools.iter().find(|t| t.tool == tool);
        let used_pct = status.and_then(|t| {
            let known = t.windows.iter().filter(|w| w.source != aiuc_core::metrics::Source::Learning);
            let w: Vec<_> = known.collect();
            w.iter().find(|w| w.key == "five_hour").or(w.iter().min_by(|a, b| a.headroom_pct.total_cmp(&b.headroom_pct))).map(|w| w.used_pct)
        });
        let enabled = status.is_none_or(|t| t.enabled);
        ToolRoom { tool, ready: enabled && aiuc_core::handoff::cli(tool).is_some(), used_pct, active: snap.active_tool == Some(tool) }
    }).collect())
}

#[tauri::command]
pub async fn plan_dispatch(state: State<'_, AppState>, text: String) -> CmdResult<Plan> {
    let rooms = rooms(&state)?;
    let tasks = dispatch::route(&dispatch::split(&text), &rooms);
    Ok(Plan { tasks, rooms })
}

#[derive(Deserialize)]
pub struct RunTask {
    pub text: String,
    pub tool: Tool,
}

#[derive(Serialize)]
pub struct Started {
    pub text: String,
    pub tool: Tool,
    pub branch: Option<String>,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn run_dispatch(window: tauri::WebviewWindow, state: State<'_, AppState>, repo: PathBuf, tasks: Vec<RunTask>, worktrees: bool) -> CmdResult<Vec<Started>> {
    if window.label() != "widget" { return Err("Open the widget to run tasks.".into()); }
    if tasks.is_empty() || tasks.len() > 6 { return Err("Run between 1 and 6 tasks at a time.".into()); }
    let engine = state.engine.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = rh::repo_root(&repo).map_err(err)?;
        let mut out = Vec::new();
        for t in tasks {
            let text = t.text.trim().to_string();
            if text.is_empty() { continue; }
            let started = (|| -> Result<Option<String>, String> {
                let (cwd, branch) = if worktrees {
                    let (dir, branch) = dispatch::create_worktree(&root, &text).map_err(|e| format!("Couldn't create a worktree: {e}"))?;
                    (dir, Some(branch))
                } else {
                    let b = rh::git(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).ok().map(|s| s.trim().to_string());
                    (root.clone(), b)
                };
                if !handoff::open_terminal(&cwd, t.tool, &text)? {
                    return Err("No terminal opened.".into());
                }
                let row = TaskRow { id: 0, created_at: Utc::now(), repo: root.to_string_lossy().into_owned(), cwd: cwd.to_string_lossy().into_owned(), branch: branch.clone(), tool: t.tool, text: text.clone() };
                engine.lock().map_err(err)?.record_task(&row).map_err(err)?;
                Ok(branch)
            })();
            match started {
                Ok(branch) => out.push(Started { text, tool: t.tool, branch, error: None }),
                Err(e) => out.push(Started { text, tool: t.tool, branch: None, error: Some(e) }),
            }
        }
        Ok(out)
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

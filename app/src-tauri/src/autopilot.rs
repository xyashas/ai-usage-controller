//! Autopilot runner: when a tool hits its limit mid-task, builds a handoff
//! prompt using the existing handoff helpers, launches the roomiest installed
//! CLI in the same folder, records the continuation task, and notifies.

use aiuc_core::autopilot;
use aiuc_core::model::Tool;
use aiuc_core::repo_handoff as rh;
use aiuc_core::store::TaskRow;
use aiuc_core::{Engine, Snapshot};
use chrono::Utc;
use std::path::Path;
use std::sync::Mutex;
use tauri::AppHandle;

fn short_tool(t: Tool) -> &'static str {
    match t {
        Tool::ClaudeCode => "Claude",
        Tool::Codex => "Codex",
        Tool::Gemini => "Gemini",
    }
}

fn build_handoff_prompt(engine: &Mutex<Engine>, root: &Path, from: Tool, orig_text: &str) -> String {
    // 1. Prefer repo-based handoff (.aiuc/handoff.md) via repo_handoff::continue_prompt.
    let status = rh::status(root);
    if let Some(latest) = status.latest.as_ref() {
        return rh::continue_prompt(&status.name, latest, false);
    }

    // 2. Fall back to local session log handoff via aiuc_core::handoff::prepare.
    if let Ok(eng) = engine.lock() {
        let roots = eng.roots(from);
        if let Ok(offers) = eng.test_handoff_offers(Utc::now()) {
            if let Some(offer) = offers.into_iter().find(|o| o.source == from) {
                if let Ok((_, prompt)) = aiuc_core::handoff::prepare(&roots, from, &offer.session) {
                    return prompt;
                }
            }
        }
    }

    // 3. Clean fallback if neither handoff file nor session log was ready yet.
    format!(
        "Continue this task in the current folder where {} left off:\n\n{}",
        short_tool(from),
        orig_text
    )
}

/// Called from the background worker on each snapshot.
pub fn tick(app: &AppHandle, engine: &Mutex<Engine>, snap: &Snapshot) {
    let (enabled, auto_send, notifications, already_moved) = {
        let Ok(eng) = engine.lock() else { return };
        if !eng.settings.autopilot {
            return;
        }
        let moved: Vec<i64> = autopilot::load_moves(&eng.store, snap.generated_at)
            .into_iter()
            .map(|m| m.task_id)
            .collect();
        (true, eng.settings.auto_send, eng.settings.alerts.notifications, moved)
    };
    if !enabled {
        return;
    }

    let installed: Vec<Tool> = Tool::ALL
        .into_iter()
        .filter(|&t| aiuc_core::handoff::cli(t).is_some())
        .collect();

    let moves = autopilot::decide(snap, &already_moved, &installed);
    if moves.is_empty() {
        return;
    }

    for mv in moves {
        let orig = snap.agents.iter().find(|a| a.id == mv.task_id);
        let orig_text = orig.map(|a| a.text.as_str()).unwrap_or("current task");
        let repo_name = orig
            .map(|a| a.repo_name.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                Path::new(&mv.cwd)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "repo".into())
            });

        let root = Path::new(&mv.cwd);
        if !root.is_dir() {
            continue;
        }

        let prompt = build_handoff_prompt(engine, root, mv.from, orig_text);
        let launched = crate::dispatch::launch_task(
            Some(root),
            &repo_name,
            &prompt,
            mv.to,
            crate::handoff::Dest::Terminal,
            false,
            auto_send,
        );

        if let Ok((branch, _, where_)) = launched {
            if let Ok(eng) = engine.lock() {
                let row = TaskRow::new(
                    mv.cwd.clone(),
                    where_,
                    branch,
                    mv.to,
                    format!("Continue: {orig_text}"),
                    crate::dispatch::dest_str(crate::handoff::Dest::Terminal),
                );
                let _ = eng.record_task(&row);
                let _ = autopilot::record_move(&eng.store, mv.task_id, mv.to, Utc::now());
            }
            if notifications {
                crate::notify(
                    app,
                    "Autopilot handoff",
                    &format!(
                        "{} ran out: {} took over {}",
                        short_tool(mv.from),
                        short_tool(mv.to),
                        repo_name
                    ),
                );
            }
        }
    }
}

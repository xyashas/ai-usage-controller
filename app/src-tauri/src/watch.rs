//! Tauri commands and snapshot ticker for "Tell me when / remind me" watches.

use crate::{err, AppState, CmdResult};
use aiuc_core::watch::{self, Watch};
use aiuc_core::{Engine, Snapshot};
use chrono::Utc;
use std::collections::HashSet;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub fn add_watch(state: State<AppState>, text: String) -> CmdResult<Watch> {
    let mut eng = state.engine.lock().map_err(err)?;
    let now = Utc::now();
    let snap = eng.snapshot(now).map_err(err)?;
    let w = watch::parse(&text, &snap, now).ok_or_else(|| {
        "Couldn't understand that watch. Try \"when Claude resets\", \"when Codex is done\", \"when tests pass\", \"in 20 minutes\", or \"remind me at 3pm to push\".".to_owned()
    })?;
    let mut list = watch::load(&eng.store);
    list.push(w.clone());
    watch::save(&eng.store, &list).map_err(err)?;
    Ok(w)
}

#[tauri::command]
pub fn list_watches(state: State<AppState>) -> CmdResult<Vec<Watch>> {
    let eng = state.engine.lock().map_err(err)?;
    Ok(watch::load(&eng.store))
}

#[tauri::command]
pub fn remove_watch(state: State<AppState>, id: String) -> CmdResult<()> {
    let eng = state.engine.lock().map_err(err)?;
    let mut list = watch::load(&eng.store);
    list.retain(|w| w.id != id);
    watch::save(&eng.store, &list).map_err(err)
}

/// Called from the background worker on each snapshot. Evaluates stored
/// watches, removes any that fired, emits `"watches-changed"`, and fires
/// desktop notifications when enabled.
#[allow(dead_code)]
pub fn tick(app: &AppHandle, engine: &Mutex<Engine>, snap: &Snapshot) {
    let (fired, notifications_on) = {
        let Ok(eng) = engine.lock() else { return };
        let list = watch::load(&eng.store);
        if list.is_empty() {
            return;
        }
        let fired = watch::check(&list, snap, Utc::now());
        if fired.is_empty() {
            return;
        }
        let fired_ids: HashSet<String> = fired.iter().map(|(w, _)| w.id.clone()).collect();
        let remaining: Vec<Watch> = list.into_iter().filter(|w| !fired_ids.contains(&w.id)).collect();
        let _ = watch::save(&eng.store, &remaining);
        (fired, eng.settings.alerts.notifications)
    };

    let _ = app.emit("watches-changed", ());
    if notifications_on {
        for (_, msg) in fired {
            crate::notify(app, "Reminder", &msg);
        }
    }
}

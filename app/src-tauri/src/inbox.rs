//! Tauri commands for the local "Remember this" inbox.
//!
//! Reminders reuse `core/src/watch.rs` (`Condition::At`) — no second scheduler.

use crate::{err, AppState, CmdResult};
use aiuc_core::inbox::{self, Item};
use aiuc_core::watch::{self, Condition, Watch};
use chrono::Utc;
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub fn inbox_list(state: State<AppState>) -> CmdResult<Vec<Item>> {
    let eng = state.engine.lock().map_err(err)?;
    Ok(inbox::list(&eng.store))
}

#[tauri::command]
pub fn inbox_done(app: AppHandle, state: State<AppState>, id: i64, done: bool) -> CmdResult<Option<Item>> {
    let eng = state.engine.lock().map_err(err)?;
    let updated = inbox::set_done(&eng.store, id, done).map_err(err)?;
    drop(eng);
    let _ = app.emit("inbox-changed", ());
    Ok(updated)
}

#[tauri::command]
pub fn inbox_remove(app: AppHandle, state: State<AppState>, id: i64) -> CmdResult<bool> {
    let eng = state.engine.lock().map_err(err)?;
    let removed = inbox::remove(&eng.store, id).map_err(err)?;
    drop(eng);
    let _ = app.emit("inbox-changed", ());
    Ok(removed)
}

#[tauri::command]
pub fn inbox_clear_done(app: AppHandle, state: State<AppState>) -> CmdResult<usize> {
    let eng = state.engine.lock().map_err(err)?;
    let count = inbox::clear_done(&eng.store).map_err(err)?;
    drop(eng);
    let _ = app.emit("inbox-changed", ());
    Ok(count)
}

/// Schedule a reminder for an inbox item by creating a `Condition::At` watch.
#[tauri::command]
pub fn inbox_remind(app: AppHandle, state: State<AppState>, id: i64, when_text: String) -> CmdResult<Watch> {
    let eng = state.engine.lock().map_err(err)?;
    let now = Utc::now();
    let item = inbox::get(&eng.store, id).ok_or("Inbox item not found.")?;
    let at = inbox::parse_when(&when_text, now)
        .ok_or("Couldn't understand that reminder time. Try \"later today\" or \"tomorrow 9am\".")?;

    let short: String = item.text.chars().take(60).collect();
    let label = if item.text.chars().count() > 60 {
        format!("{short}…")
    } else {
        short
    };
    let w = Watch {
        id: format!("w-inbox-{}-{}", id, now.timestamp_millis()),
        condition: Condition::At { at },
        label,
        created_at: now,
    };

    let mut watches = watch::load(&eng.store);
    watches.push(w.clone());
    watch::save(&eng.store, &watches).map_err(err)?;
    let _ = inbox::set_remind_at(&eng.store, id, Some(at));
    drop(eng);

    let _ = app.emit("watches-changed", ());
    let _ = app.emit("inbox-changed", ());
    Ok(w)
}

/// Return the inbox item's text so the UI can prefill the Agents task box.
#[tauri::command]
pub fn inbox_to_task(app: AppHandle, state: State<AppState>, id: i64) -> CmdResult<String> {
    let eng = state.engine.lock().map_err(err)?;
    let item = inbox::get(&eng.store, id).ok_or("Inbox item not found.")?;
    let text = item.text.clone();
    drop(eng);
    let _ = app.emit("inbox-changed", ());
    Ok(text)
}

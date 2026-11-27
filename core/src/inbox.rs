//! "Remember this": a local inbox of things you saved by voice ("save this",
//! "remind me at 5 to call Sam"), from a selection, or from an Ask answer.
//!
//! Stored only on this computer. Never synced.
//!
//! CONTRACT (Claude's voice code calls this; keep the signature):
//!   `capture(store, text, source, app, now) -> Result<Item>`
//! It works out a reminder time from the text itself ("remind me in 20
//! minutes to…", "at 5pm", "tomorrow"), and strips that from `text`.
//!
//! This first version is a minimal starter (kv JSON, no time parsing);
//! Gemini owns this file and the inbox UI from here.

use crate::store::Store;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const KV_INBOX: &str = "inbox";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source { Voice, Selection, Clipboard, Ask }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: i64,
    pub text: String,
    pub source: Source,
    /// The app you were in ("Chrome"), never the window title.
    pub app: Option<String>,
    pub at: DateTime<Utc>,
    pub remind_at: Option<DateTime<Utc>>,
    pub done: bool,
}

pub fn list(store: &Store) -> Vec<Item> {
    store.kv_get(KV_INBOX).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Save something to the inbox. See the contract above.
pub fn capture(store: &Store, text: &str, source: Source, app: Option<&str>, now: DateTime<Utc>) -> Result<Item> {
    let text = text.trim();
    anyhow::ensure!(!text.is_empty(), "nothing to save");
    let mut items = list(store);
    let id = items.iter().map(|i| i.id).max().unwrap_or(0).max(now.timestamp_millis()) + 1;
    let item = Item { id, text: text.chars().take(4000).collect(), source, app: app.map(str::to_owned), at: now, remind_at: None, done: false };
    items.push(item.clone());
    store.kv_set(KV_INBOX, &serde_json::to_string(&items)?)?;
    Ok(item)
}

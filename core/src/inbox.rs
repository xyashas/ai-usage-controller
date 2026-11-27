//! "Remember this": a local inbox of things you saved by voice ("save this",
//! "note this", "make a note: …"), from a selection, or from an Ask answer.
//!
//! Stored only on this computer (`kv` key `"inbox"`). Never synced.
//!
//! CONTRACT (Claude's voice code calls this; keep the signature):
//!   `capture(store, text, source, app, now) -> Result<Item>`

use crate::store::Store;
use anyhow::Result;
use chrono::{DateTime, Duration, Local, NaiveTime, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};

pub const KV_INBOX: &str = "inbox";
const MAX_TEXT_CHARS: usize = 4000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Voice,
    Selection,
    Clipboard,
    Ask,
}

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

fn save(store: &Store, items: &[Item]) -> Result<()> {
    store.kv_set(KV_INBOX, &serde_json::to_string(items)?)
}

/// Return all inbox items, newest first.
pub fn list(store: &Store) -> Vec<Item> {
    let mut items: Vec<Item> = store
        .kv_get(KV_INBOX)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    items.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| b.id.cmp(&a.id)));
    items
}

pub fn get(store: &Store, id: i64) -> Option<Item> {
    list(store).into_iter().find(|i| i.id == id)
}

/// Save something to the inbox. See the contract above.
pub fn capture(
    store: &Store,
    text: &str,
    source: Source,
    app: Option<&str>,
    now: DateTime<Utc>,
) -> Result<Item> {
    let text = text.trim();
    anyhow::ensure!(!text.is_empty(), "nothing to save");
    let mut items = list(store);
    let id = items
        .iter()
        .map(|i| i.id)
        .max()
        .unwrap_or(0)
        .max(now.timestamp_millis())
        + 1;
    let item = Item {
        id,
        text: text.chars().take(MAX_TEXT_CHARS).collect(),
        source,
        app: app.map(str::to_owned),
        at: now,
        remind_at: None,
        done: false,
    };
    items.push(item.clone());
    save(store, &items)?;
    Ok(item)
}

/// Mark an inbox item done or open again. Returns the updated item, or `None` if not found.
pub fn set_done(store: &Store, id: i64, done: bool) -> Result<Option<Item>> {
    let mut items = list(store);
    let Some(item) = items.iter_mut().find(|i| i.id == id) else {
        return Ok(None);
    };
    item.done = done;
    let updated = item.clone();
    save(store, &items)?;
    Ok(Some(updated))
}

/// Remove a single inbox item by id. Returns true if an item was removed.
pub fn remove(store: &Store, id: i64) -> Result<bool> {
    let mut items = list(store);
    let before = items.len();
    items.retain(|i| i.id != id);
    if items.len() == before {
        return Ok(false);
    }
    save(store, &items)?;
    Ok(true)
}

/// Remove all completed inbox items. Returns the number removed.
pub fn clear_done(store: &Store) -> Result<usize> {
    let mut items = list(store);
    let before = items.len();
    items.retain(|i| !i.done);
    let removed = before - items.len();
    if removed > 0 {
        save(store, &items)?;
    }
    Ok(removed)
}

/// Update an inbox item's text. Returns the updated item, or `None` if not found.
pub fn update_text(store: &Store, id: i64, text: &str) -> Result<Option<Item>> {
    let trimmed = text.trim();
    anyhow::ensure!(!trimmed.is_empty(), "text cannot be empty");
    let mut items = list(store);
    let Some(item) = items.iter_mut().find(|i| i.id == id) else {
        return Ok(None);
    };
    item.text = trimmed.chars().take(MAX_TEXT_CHARS).collect();
    let updated = item.clone();
    save(store, &items)?;
    Ok(Some(updated))
}

/// Store the scheduled reminder timestamp on an inbox item.
pub fn set_remind_at(
    store: &Store,
    id: i64,
    remind_at: Option<DateTime<Utc>>,
) -> Result<Option<Item>> {
    let mut items = list(store);
    let Some(item) = items.iter_mut().find(|i| i.id == id) else {
        return Ok(None);
    };
    item.remind_at = remind_at;
    let updated = item.clone();
    save(store, &items)?;
    Ok(Some(updated))
}

/// Parse quick reminder phrases ("later today", "tomorrow 9am", "tomorrow",
/// "in 30 minutes", "at 5pm") into a UTC timestamp.
pub fn parse_when(when_text: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let lower = when_text.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }

    let local_now = now.with_timezone(&Local);

    if lower == "later today" || lower == "later" {
        // 3 hours from now, or 5:00 PM local if it's still morning (before 14:00)
        if local_now.hour() < 14 {
            let five_pm = NaiveTime::from_hms_opt(17, 0, 0)?;
            if let Some(dt) = Local
                .from_local_datetime(&local_now.date_naive().and_time(five_pm))
                .earliest()
            {
                return Some(dt.with_timezone(&Utc));
            }
        }
        return Some(now + Duration::hours(3));
    }

    if lower.starts_with("tomorrow") {
        let next_day = local_now.date_naive() + Duration::days(1);
        let time = NaiveTime::from_hms_opt(9, 0, 0)?;
        return Local
            .from_local_datetime(&next_day.and_time(time))
            .earliest()
            .map(|d| d.with_timezone(&Utc));
    }

    // Relative: "in N min/hours"
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    for i in 0..tokens.len() {
        if tokens[i] == "in" && i + 2 < tokens.len() {
            if let Ok(n) = tokens[i + 1]
                .trim_matches(|c: char| !c.is_ascii_digit())
                .parse::<i64>()
            {
                if n > 0 {
                    let unit = tokens[i + 2].trim_matches(|c: char| !c.is_ascii_alphabetic());
                    if matches!(unit, "m" | "min" | "mins" | "minute" | "minutes") {
                        return Some(now + Duration::minutes(n));
                    }
                    if matches!(unit, "h" | "hr" | "hrs" | "hour" | "hours") {
                        return Some(now + Duration::hours(n));
                    }
                }
            }
        }
    }

    // Clock time: "at 5pm", "at 14:30", "5pm"
    let clock_tok = tokens
        .iter()
        .position(|&t| t == "at")
        .and_then(|i| tokens.get(i + 1).copied())
        .or_else(|| tokens.first().copied())?;
    let next_ampm = tokens
        .iter()
        .position(|&t| t == clock_tok)
        .and_then(|i| tokens.get(i + 1).copied())
        .filter(|t| matches!(*t, "am" | "pm"));
    let (digits, ampm) = if let Some(s) = clock_tok.strip_suffix("am") {
        (s, Some("am"))
    } else if let Some(s) = clock_tok.strip_suffix("pm") {
        (s, Some("pm"))
    } else {
        (clock_tok, next_ampm)
    };
    let (mut h, m): (u32, u32) = if let Some((hs, ms)) = digits.split_once(':') {
        (hs.parse().ok()?, ms.parse().ok()?)
    } else {
        ampm?;
        (digits.parse().ok()?, 0)
    };
    if m >= 60 {
        return None;
    }
    match ampm {
        Some("am") if (1..=12).contains(&h) => {
            if h == 12 {
                h = 0;
            }
        }
        Some("pm") if (1..=12).contains(&h) => {
            if h < 12 {
                h += 12;
            }
        }
        None if h < 24 => {}
        _ => return None,
    }
    let time = NaiveTime::from_hms_opt(h, m, 0)?;
    let mut date = local_now.date_naive();
    let mut target = Local
        .from_local_datetime(&date.and_time(time))
        .earliest()?
        .with_timezone(&Utc);
    if target <= now {
        date += Duration::days(1);
        target = Local
            .from_local_datetime(&date.and_time(time))
            .earliest()?
            .with_timezone(&Utc);
    }
    Some(target)
}

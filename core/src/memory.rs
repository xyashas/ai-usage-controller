//! What Ask remembers about you: short preferences you told it ("I use
//! TypeScript", "keep it casual"), added to every Ask prompt.
//!
//! Stored only on this computer, in `kv` under `"ask_prefs"` (JSON). Never
//! synced. You can see, delete or switch them off in Settings.

use crate::store::Store;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const KV_PREFS: &str = "ask_prefs";
/// At most this many; the oldest drop off.
pub const MAX: usize = 40;
const MAX_LEN: usize = 240;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pref {
    pub id: i64,
    pub text: String,
    pub at: DateTime<Utc>,
}

pub fn list(store: &Store) -> Vec<Pref> {
    store.kv_get(KV_PREFS).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(store: &Store, prefs: &[Pref]) -> Result<()> {
    store.kv_set(KV_PREFS, &serde_json::to_string(prefs)?)
}

/// Tidy what you said into a preference: drop "remember that", fix the case,
/// end with a full stop. "remember that i use typescript" → "I use typescript."
pub fn tidy(text: &str) -> String {
    let mut t = text.trim().trim_end_matches(['.', '!', ' ']).to_owned();
    let lower = t.to_lowercase();
    for lead in ["please remember that ", "please remember ", "remember that ", "remember ", "note that ", "for future reference ", "from now on "] {
        if lower.starts_with(lead) { t = t[lead.len()..].trim_start_matches([',', ' ']).to_owned(); break; }
    }
    let t: String = t.chars().take(MAX_LEN).collect();
    let mut words: Vec<String> = t.split(' ').map(|w| if w == "i" || w.starts_with("i'") { w.replacen('i', "I", 1) } else { w.to_owned() }).collect();
    if let Some(f) = words.first_mut() {
        let mut c = f.chars();
        if let Some(h) = c.next() { *f = h.to_uppercase().chain(c).collect(); }
    }
    let t = words.join(" ");
    if t.is_empty() { t } else { format!("{t}.") }
}

/// Add a preference (a near-duplicate replaces the old one). Returns it.
pub fn add(store: &Store, text: &str, now: DateTime<Utc>) -> Result<Pref> {
    let text = tidy(text);
    anyhow::ensure!(!text.is_empty(), "nothing to remember");
    let key = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    let mut prefs = list(store);
    prefs.retain(|p| key(&p.text) != key(&text));
    let id = prefs.iter().map(|p| p.id).max().unwrap_or(0).max(now.timestamp_millis()) + 1;
    let p = Pref { id, text, at: now };
    prefs.push(p.clone());
    if prefs.len() > MAX { prefs.drain(..prefs.len() - MAX); }
    save(store, &prefs)?;
    Ok(p)
}

pub fn remove(store: &Store, id: i64) -> Result<()> {
    let mut prefs = list(store);
    prefs.retain(|p| p.id != id);
    save(store, &prefs)
}

/// "Forget that I use Vim": remove the preferences that mention these words.
/// Returns how many were removed.
pub fn forget_matching(store: &Store, words: &str) -> Result<usize> {
    let needle: Vec<String> = words.to_lowercase().split_whitespace()
        .filter(|w| w.len() > 2 && !["that", "the", "about", "use", "like", "prefer"].contains(w))
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_owned()).filter(|w| !w.is_empty()).collect();
    if needle.is_empty() { return Ok(0); }
    let mut prefs = list(store);
    let before = prefs.len();
    prefs.retain(|p| { let t = p.text.to_lowercase(); !needle.iter().all(|w| t.contains(w.as_str())) });
    let n = before - prefs.len();
    if n > 0 { save(store, &prefs)?; }
    Ok(n)
}

pub fn clear(store: &Store) -> Result<()> {
    store.kv_del(KV_PREFS)
}

/// The lines added to an Ask prompt, or None when there's nothing to add.
pub fn prompt_block(prefs: &[Pref]) -> Option<String> {
    if prefs.is_empty() { return None; }
    Some(prefs.iter().map(|p| format!("- {}\n", p.text)).collect())
}

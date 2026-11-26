//! One parser per source, each versioned. When a vendor changes its log
//! format, bump that parser's version and add a fixture under `tests/fixtures/`.

pub mod claude;
pub mod codex;
pub mod gemini;

use chrono::{DateTime, Utc};
use serde_json::Value;

pub fn parse_ts(v: Option<&Value>) -> Option<DateTime<Utc>> {
    let v = v?;
    if let Some(s) = v.as_str() {
        return DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc));
    }
    if let Some(n) = v.as_i64() {
        // seconds or milliseconds
        return if n > 10_000_000_000 {
            DateTime::from_timestamp_millis(n)
        } else {
            DateTime::from_timestamp(n, 0)
        };
    }
    None
}

pub fn u(v: Option<&Value>) -> u64 {
    v.and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f.max(0.0) as u64)))
        .unwrap_or(0)
}

/// Build a short, privacy-safe signature of a tool call: tool name plus the
/// file path or the first word(s) of the command. Never includes content.
pub fn tool_sig(name: &str, input: Option<&Value>) -> String {
    let target = input.and_then(|i| {
        for key in ["file_path", "path", "notebook_path", "absolute_path", "pattern", "url"] {
            if let Some(s) = i.get(key).and_then(|v| v.as_str()) {
                return Some(s.to_string());
            }
        }
        if let Some(c) = i.get("command") {
            let s = match c {
                Value::String(s) => s.clone(),
                Value::Array(a) => a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" "),
                _ => String::new(),
            };
            let short: String = s.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
            return Some(short);
        }
        None
    });
    match target {
        Some(t) if !t.is_empty() => format!("{name}:{}", truncate(&t, 120)),
        _ => name.to_string(),
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

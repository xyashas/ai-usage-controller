//! "Tell me when / remind me" watchers evaluated against each snapshot.
//!
//! Stored locally in `kv` under key `"watches"` (JSON). Never synced.

use crate::metrics::Source;
use crate::model::Tool;
use crate::snapshot::Snapshot;
use crate::store::Store;
use anyhow::Result;
use chrono::{DateTime, Duration, Local, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

pub const KV_WATCHES: &str = "watches";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Condition {
    LimitReset { tool: Tool },
    LimitBelow { tool: Tool, used_pct: f64 },
    AgentDone { task_id: i64 },
    AgentNeedsYou { task_id: Option<i64> },
    TestsPass { task_id: i64 },
    At { at: DateTime<Utc> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Watch {
    pub id: String,
    pub condition: Condition,
    pub label: String,
    pub created_at: DateTime<Utc>,
}

fn next_id(now: DateTime<Utc>) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("w-{}-{n}", now.timestamp_millis())
}

fn short_tool(tool: Tool) -> &'static str {
    match tool {
        Tool::ClaudeCode => "Claude",
        Tool::Codex => "Codex",
        Tool::Gemini => "Gemini",
    }
}

pub fn load(store: &Store) -> Vec<Watch> {
    store
        .kv_get(KV_WATCHES)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<Vec<Watch>>(&s).ok())
        .unwrap_or_default()
}

pub fn save(store: &Store, watches: &[Watch]) -> Result<()> {
    store.kv_set(KV_WATCHES, &serde_json::to_string(watches)?)
}

fn detect_tool(lower: &str) -> Option<Tool> {
    if lower.contains("claude") {
        Some(Tool::ClaudeCode)
    } else if lower.contains("codex") || lower.contains("chatgpt") {
        Some(Tool::Codex)
    } else if lower.contains("gemini") {
        Some(Tool::Gemini)
    } else {
        None
    }
}

fn find_agent_id(snap: &Snapshot, tool: Option<Tool>) -> Option<i64> {
    snap.agents
        .iter()
        .find(|a| tool.is_none_or(|t| a.tool == t) && a.state != "no_activity")
        .or_else(|| snap.agents.iter().find(|a| tool.is_none_or(|t| a.tool == t)))
        .map(|a| a.id)
}

fn parse_relative_time(lower: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    for i in 0..tokens.len() {
        if tokens[i] == "in" && i + 2 < tokens.len() {
            let n_str = tokens[i + 1].trim_matches(|c: char| !c.is_ascii_digit());
            if let Ok(n) = n_str.parse::<i64>() {
                if n > 0 {
                    let unit = tokens[i + 2].trim_matches(|c: char| !c.is_ascii_alphabetic());
                    if matches!(unit, "m" | "min" | "mins" | "minute" | "minutes") {
                        return Some(now + Duration::minutes(n));
                    }
                    if matches!(unit, "h" | "hr" | "hrs" | "hour" | "hours") {
                        return Some(now + Duration::hours(n));
                    }
                    if matches!(unit, "s" | "sec" | "secs" | "second" | "seconds") {
                        return Some(now + Duration::seconds(n));
                    }
                }
            }
        }
    }
    None
}

fn parse_clock_time(lower: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    for i in 0..tokens.len() {
        if tokens[i] == "at" && i + 1 < tokens.len() {
            let raw = tokens[i + 1].trim_end_matches([',', '.', '!', '?']);
            let next_ampm = tokens
                .get(i + 2)
                .map(|t| t.trim_end_matches([',', '.', '!', '?']))
                .filter(|t| matches!(*t, "am" | "pm"));
            if let Some(time) = parse_hm(raw, next_ampm) {
                let local_now = now.with_timezone(&Local);
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
                return Some(target);
            }
        }
    }
    None
}

fn parse_hm(raw: &str, trailing_ampm: Option<&str>) -> Option<NaiveTime> {
    let (digits, ampm) = if let Some(s) = raw.strip_suffix("am") {
        (s, Some("am"))
    } else if let Some(s) = raw.strip_suffix("pm") {
        (s, Some("pm"))
    } else {
        (raw, trailing_ampm)
    };

    let (mut h, m): (u32, u32) = if let Some((hs, ms)) = digits.split_once(':') {
        (hs.parse().ok()?, ms.parse().ok()?)
    } else {
        // Without a colon, require explicit am/pm so "at 3" doesn't match random numbers
        ampm?;
        (digits.parse().ok()?, 0)
    };

    if m >= 60 {
        return None;
    }
    match ampm {
        Some("am") => {
            if h == 0 || h > 12 {
                return None;
            }
            if h == 12 {
                h = 0;
            }
        }
        Some("pm") => {
            if h == 0 || h > 12 {
                return None;
            }
            if h < 12 {
                h += 12;
            }
        }
        _ => {
            if h >= 24 {
                return None;
            }
        }
    }
    NaiveTime::from_hms_opt(h, m, 0)
}

fn parse_percent(lower: &str) -> Option<f64> {
    for tok in lower.split_whitespace() {
        if let Some(num) = tok.trim_end_matches([',', '.', '!', '?']).strip_suffix('%') {
            if let Ok(v) = num.parse::<f64>() {
                if (0.0..=100.0).contains(&v) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Parse a natural-language watch/reminder request into a `Watch`.
pub fn parse(text: &str, snap: &Snapshot, now: DateTime<Utc>) -> Option<Watch> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    let tool = detect_tool(&lower);

    // 1. Limit reset: "tell me when Claude resets"
    if lower.contains("reset") {
        let t = tool.or(snap.active_tool)?;
        return Some(Watch {
            id: next_id(now),
            condition: Condition::LimitReset { tool: t },
            label: format!("When {} resets", short_tool(t)),
            created_at: now,
        });
    }

    // 2. Limit below X%: "when Claude is below 50%" / "under 30%"
    if (lower.contains("below") || lower.contains("under")) && lower.contains('%') {
        if let Some(pct) = parse_percent(&lower) {
            let t = tool.or(snap.active_tool)?;
            return Some(Watch {
                id: next_id(now),
                condition: Condition::LimitBelow {
                    tool: t,
                    used_pct: pct,
                },
                label: format!("When {} is below {:.0}%", short_tool(t), pct),
                created_at: now,
            });
        }
    }

    // 3. Tests pass: "when tests pass"
    if lower.contains("test") && lower.contains("pass") {
        let task_id = find_agent_id(snap, tool)?;
        return Some(Watch {
            id: next_id(now),
            condition: Condition::TestsPass { task_id },
            label: match tool {
                Some(t) => format!("When {} tests pass", short_tool(t)),
                None => "When tests pass".into(),
            },
            created_at: now,
        });
    }

    // 4. Agent needs you / approval
    if lower.contains("needs me")
        || lower.contains("needs you")
        || lower.contains("approval")
        || lower.contains("waiting for me")
    {
        let task_id = tool.and_then(|t| find_agent_id(snap, Some(t)));
        return Some(Watch {
            id: next_id(now),
            condition: Condition::AgentNeedsYou { task_id },
            label: match tool {
                Some(t) => format!("When {} needs you", short_tool(t)),
                None => "When an agent needs you".into(),
            },
            created_at: now,
        });
    }

    // 5. Agent done: "when codex is done" / "when claude finishes"
    if lower.contains("done") || lower.contains("finish") || lower.contains("complete") {
        let task_id = find_agent_id(snap, tool)?;
        return Some(Watch {
            id: next_id(now),
            condition: Condition::AgentDone { task_id },
            label: match tool {
                Some(t) => format!("When {} is done", short_tool(t)),
                None => "When agent is done".into(),
            },
            created_at: now,
        });
    }

    // 6. Relative time: "in 20 minutes", "in 2 hours"
    if let Some(at) = parse_relative_time(&lower, now) {
        return Some(Watch {
            id: next_id(now),
            condition: Condition::At { at },
            label: trimmed.to_owned(),
            created_at: now,
        });
    }

    // 7. Clock time: "remind me at 3pm to push"
    if let Some(at) = parse_clock_time(&lower, now) {
        return Some(Watch {
            id: next_id(now),
            condition: Condition::At { at },
            label: trimmed.to_owned(),
            created_at: now,
        });
    }

    None
}

fn primary_window_used(snap: &Snapshot, tool: Tool) -> Option<(f64, Option<DateTime<Utc>>)> {
    let ts = snap.tools.iter().find(|t| t.tool == tool && t.enabled && !t.locked)?;
    let w = ts
        .windows
        .iter()
        .find(|w| w.key == "five_hour" && w.source != Source::Learning)
        .or_else(|| ts.windows.iter().find(|w| w.key == "daily" && w.source != Source::Learning))
        .or_else(|| ts.windows.iter().find(|w| w.source != Source::Learning))?;
    Some((w.used_pct, w.resets_at))
}

/// Evaluate all watches against `snap` and `now`. Returns the watches that
/// fired paired with a plain notification message.
pub fn check(watches: &[Watch], snap: &Snapshot, now: DateTime<Utc>) -> Vec<(Watch, String)> {
    let mut fired = Vec::new();
    for w in watches {
        let msg = match &w.condition {
            Condition::LimitReset { tool } => primary_window_used(snap, *tool).and_then(|(used, resets)| {
                let reset_passed = resets.is_some_and(|r| r <= now);
                let cleared = used <= 5.0;
                (reset_passed || cleared).then(|| format!("{} limit has reset.", short_tool(*tool)))
            }),
            Condition::LimitBelow { tool, used_pct } => {
                primary_window_used(snap, *tool).and_then(|(used, _)| {
                    (used <= *used_pct)
                        .then(|| format!("{} is now at {:.0}% used.", short_tool(*tool), used))
                })
            }
            Condition::AgentDone { task_id } => snap
                .agents
                .iter()
                .find(|a| a.id == *task_id && matches!(a.state.as_str(), "done" | "idle"))
                .map(|a| format!("{} finished its task.", short_tool(a.tool))),
            Condition::AgentNeedsYou { task_id } => {
                let approval_waiting = task_id.is_none() && !snap.approvals.is_empty();
                let agent_waiting = snap.agents.iter().find(|a| {
                    task_id.is_none_or(|id| a.id == id)
                        && matches!(a.state.as_str(), "needs_approval" | "idle")
                });
                if let Some(a) = agent_waiting {
                    Some(format!("{} needs your attention.", short_tool(a.tool)))
                } else if approval_waiting {
                    Some("An agent is waiting for your approval.".into())
                } else {
                    None
                }
            }
            Condition::TestsPass { task_id } => snap
                .agents
                .iter()
                .find(|a| a.id == *task_id && a.verify.as_deref() == Some("passed"))
                .map(|a| format!("Tests passed for {}.", short_tool(a.tool))),
            Condition::At { at } => (now >= *at).then(|| w.label.clone()),
        };

        if let Some(m) = msg {
            fired.push((w.clone(), m));
        }
    }
    fired
}

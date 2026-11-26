//! Runaway detection. A session is flagged when all three hold:
//! 1. spend in the last 5 minutes > 4× the user's normal 5-minute average,
//! 2. at least 10 agent turns ran without a user message in between,
//! 3. the same files / tool calls repeat in most of those turns.

use crate::model::{EventKind, Tool, UsageEvent};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, Serialize)]
pub struct RunawayConfig {
    pub spike_multiplier: f64,
    pub min_turns: usize,
    pub repeat_share: f64,
    /// Floor for the baseline so a brand-new user with tiny history isn't flagged on day one.
    pub baseline_floor_usd: f64,
}

impl Default for RunawayConfig {
    fn default() -> Self {
        RunawayConfig { spike_multiplier: 4.0, min_turns: 10, repeat_share: 0.5, baseline_floor_usd: 0.50 }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RunawaySignal {
    pub tool: Tool,
    pub session_id: String,
    pub project: Option<String>,
    pub spend_usd: f64,
    pub minutes: u32,
    pub turns: usize,
    pub spend_5m: f64,
    pub baseline_5m: f64,
    pub repeat_share: f64,
    pub burn_usd_per_min: f64,
    pub message: String,
}

fn bucket(t: DateTime<Utc>) -> i64 {
    t.timestamp() / 300
}

/// Average spend per active 5-minute bucket over the given history, excluding the last 30 minutes.
pub fn baseline_5m(history: &[UsageEvent], now: DateTime<Utc>, floor: f64) -> f64 {
    let cutoff = now - Duration::minutes(30);
    let mut buckets: BTreeMap<i64, f64> = BTreeMap::new();
    for e in history.iter().filter(|e| e.kind == EventKind::Assistant && e.ts < cutoff) {
        *buckets.entry(bucket(e.ts)).or_default() += e.cost_usd;
    }
    let active: Vec<f64> = buckets.values().cloned().filter(|c| *c > 0.0).collect();
    if active.is_empty() {
        return floor;
    }
    (active.iter().sum::<f64>() / active.len() as f64).max(floor)
}

/// `session` must be one session's events, oldest first.
pub fn detect(session: &[UsageEvent], baseline: f64, now: DateTime<Utc>, cfg: &RunawayConfig) -> Option<RunawaySignal> {
    let first = session.first()?;
    let five_ago = now - Duration::minutes(5);
    let spend_5m: f64 = session.iter().filter(|e| e.ts >= five_ago && e.ts <= now).map(|e| e.cost_usd).sum();
    if spend_5m <= cfg.spike_multiplier * baseline {
        return None;
    }

    let start = session.iter().rposition(|e| e.kind == EventKind::UserPrompt).map(|i| i + 1).unwrap_or(0);
    let run = &session[start..];

    // Group into turns: each assistant event is a turn; Codex logs tool calls on their own
    // lines just before the token count, so pending tool-call signatures attach to the next turn.
    let mut turns: Vec<(DateTime<Utc>, f64, Vec<String>)> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    for e in run {
        let sigs = || e.tool_sig.iter().flat_map(|s| s.split('|').map(String::from)).collect::<Vec<_>>();
        match e.kind {
            EventKind::ToolCall => pending.extend(sigs()),
            EventKind::Assistant => {
                let mut s = std::mem::take(&mut pending);
                s.extend(sigs());
                turns.push((e.ts, e.cost_usd, s));
            }
            EventKind::UserPrompt => {}
        }
    }
    if turns.len() < cfg.min_turns {
        return None;
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut with_sigs = 0usize;
    let mut repeating = 0usize;
    for (_, _, sigs) in &turns {
        if sigs.is_empty() {
            continue;
        }
        with_sigs += 1;
        if sigs.iter().any(|s| seen.contains(s)) {
            repeating += 1;
        }
        seen.extend(sigs.iter().cloned());
    }
    if with_sigs < cfg.min_turns / 2 {
        return None;
    }
    let share = repeating as f64 / with_sigs as f64;
    if share < cfg.repeat_share {
        return None;
    }

    // Report the spend of the recent part of the run (last 15 minutes), like a taxi meter.
    let recent_from = now - Duration::minutes(15);
    let recent: Vec<&(DateTime<Utc>, f64, Vec<String>)> = turns.iter().filter(|t| t.0 >= recent_from).collect();
    let spend: f64 = recent.iter().map(|t| t.1).sum();
    let from = recent.first().map(|t| t.0).unwrap_or(now);
    let minutes = (((now - from).num_seconds() as f64) / 60.0).ceil().max(1.0) as u32;
    let burn = spend / minutes as f64;

    Some(RunawaySignal {
        tool: first.tool,
        session_id: first.session_id.clone(),
        project: run.iter().rev().find_map(|e| e.project.clone()),
        spend_usd: spend,
        minutes,
        turns: turns.len(),
        spend_5m,
        baseline_5m: baseline,
        repeat_share: share,
        burn_usd_per_min: burn,
        message: format!("${spend:.2} in {minutes} min, possible loop. Stop?"),
    })
}

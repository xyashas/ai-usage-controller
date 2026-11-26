//! Headroom, reset windows, burn rate and time-to-cutoff.
//!
//! headroom      = 1 - used_in_window / window_allowance
//! minutes_left  = (allowance - used) / average usage per minute over the last 15 min
//!
//! Live gauges (Codex logs, Claude /usage) are preferred. Without one, usage is
//! measured in API-equivalent dollars against an allowance that is either set by
//! the user, calibrated from a past live reading, or the user's own historical
//! peak window (the same approach ccusage uses for its block limit).

use crate::model::{EventKind, GaugeSample, Tool, UsageEvent};
use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc};
use chrono_tz::America::Los_Angeles;
use serde::Serialize;

pub const FIVE_HOURS: i64 = 5 * 60;
pub const BURN_WINDOW_MIN: i64 = 15;
pub const ACTIVE_GAP_MIN: i64 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Read from the vendor's own gauge.
    Live,
    /// Computed from local logs against a known or calibrated allowance.
    Estimate,
    /// Not enough history yet to know the allowance.
    Learning,
}

#[derive(Clone, Debug, Serialize)]
pub struct WindowStatus {
    /// "five_hour", "weekly", "weekly_opus", "daily"
    pub key: String,
    pub label: String,
    pub used_pct: f64,
    pub headroom_pct: f64,
    pub resets_at: Option<DateTime<Utc>>,
    pub source: Source,
    pub used_usd: Option<f64>,
    pub allowance_usd: Option<f64>,
    pub used_requests: Option<u64>,
    pub allowance_requests: Option<u64>,
}

pub fn label_for(key: &str) -> &'static str {
    match key {
        "five_hour" => "5-hour",
        "weekly" => "Weekly",
        "weekly_opus" => "Weekly (Opus)",
        "daily" => "Daily",
        _ => "Window",
    }
}

impl WindowStatus {
    pub fn from_pct(key: &str, used_pct: f64, resets_at: Option<DateTime<Utc>>, source: Source) -> Self {
        let used = used_pct.clamp(0.0, 100.0);
        WindowStatus {
            key: key.into(),
            label: label_for(key).into(),
            used_pct: used,
            headroom_pct: 100.0 - used,
            resets_at,
            source,
            used_usd: None,
            allowance_usd: None,
            used_requests: None,
            allowance_requests: None,
        }
    }
}

pub fn floor_hour(t: DateTime<Utc>) -> DateTime<Utc> {
    t.with_minute(0).and_then(|t| t.with_second(0)).and_then(|t| t.with_nanosecond(0)).unwrap_or(t)
}

/// Claude-style 5-hour blocks: a block starts at the first message (floored to the hour)
/// and lasts 5 hours; the next message after it ends starts a new block.
/// Returns (start, end, cost) for each block, oldest first.
pub fn five_hour_blocks(events: &[UsageEvent]) -> Vec<(DateTime<Utc>, DateTime<Utc>, f64)> {
    let mut blocks: Vec<(DateTime<Utc>, DateTime<Utc>, f64)> = Vec::new();
    for e in events.iter().filter(|e| e.kind == EventKind::Assistant) {
        match blocks.last_mut() {
            Some((_, end, cost)) if e.ts < *end => *cost += e.cost_usd,
            _ => {
                let start = floor_hour(e.ts);
                blocks.push((start, start + Duration::minutes(FIVE_HOURS), e.cost_usd));
            }
        }
    }
    blocks
}

/// The block containing `now`, if any.
pub fn current_block(events: &[UsageEvent], now: DateTime<Utc>) -> Option<(DateTime<Utc>, DateTime<Utc>, f64)> {
    five_hour_blocks(events).into_iter().rev().find(|(s, e, _)| *s <= now && now < *e)
}

/// Peak cost of completed 5-hour blocks. Needs at least 3 blocks to be meaningful.
pub fn historical_peak_block(events: &[UsageEvent], now: DateTime<Utc>) -> Option<f64> {
    let done: Vec<f64> = five_hour_blocks(events)
        .into_iter()
        .filter(|(_, end, _)| *end <= now)
        .map(|(_, _, c)| c)
        .collect();
    if done.len() < 3 {
        return None;
    }
    done.into_iter().fold(None, |m: Option<f64>, c| Some(m.map_or(c, |m| m.max(c)))).filter(|m| *m > 0.0)
}

/// Peak cost over completed calendar weeks (UTC, Monday start). Needs 2 weeks.
pub fn historical_peak_week(events: &[UsageEvent], now: DateTime<Utc>) -> Option<f64> {
    use std::collections::BTreeMap;
    let this_week = week_start(now);
    let mut weeks: BTreeMap<DateTime<Utc>, f64> = BTreeMap::new();
    for e in events.iter().filter(|e| e.kind == EventKind::Assistant) {
        let w = week_start(e.ts);
        if w < this_week {
            *weeks.entry(w).or_default() += e.cost_usd;
        }
    }
    if weeks.len() < 2 {
        return None;
    }
    weeks.values().cloned().fold(None, |m: Option<f64>, c| Some(m.map_or(c, |m| m.max(c)))).filter(|m| *m > 0.0)
}

pub fn week_start(t: DateTime<Utc>) -> DateTime<Utc> {
    let d = t.date_naive() - Duration::days(t.weekday().num_days_from_monday() as i64);
    Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
}

/// Gemini quotas reset at midnight Pacific time.
pub fn gemini_day_bounds(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let local = now.with_timezone(&Los_Angeles);
    let day = local.date_naive();
    let start = Los_Angeles
        .from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .unwrap()
        .with_timezone(&Utc);
    let next = Los_Angeles
        .from_local_datetime(&(day + Duration::days(1)).and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .unwrap()
        .with_timezone(&Utc);
    (start, next)
}

/// Interpret a gauge reading at `now`: if its reset time has passed, the window has reset.
pub fn gauge_status(g: &GaugeSample, now: DateTime<Utc>) -> WindowStatus {
    match g.resets_at {
        Some(r) if r <= now => WindowStatus::from_pct(&g.window, 0.0, None, Source::Live),
        r => WindowStatus::from_pct(&g.window, g.used_pct, r, Source::Live),
    }
}

/// Dollar-based window status against an allowance (or "learning" if none).
pub fn usd_status(key: &str, used: f64, allowance: Option<(f64, Source)>, resets_at: Option<DateTime<Utc>>) -> WindowStatus {
    let mut w = match allowance {
        Some((a, src)) if a > 0.0 => WindowStatus::from_pct(key, used / a * 100.0, resets_at, src),
        _ => WindowStatus::from_pct(key, 0.0, resets_at, Source::Learning),
    };
    w.used_usd = Some(used);
    w.allowance_usd = allowance.map(|(a, _)| a);
    w
}

pub fn is_active(events: &[UsageEvent], now: DateTime<Utc>) -> bool {
    events
        .iter()
        .rev()
        .find(|e| e.kind == EventKind::Assistant)
        .map(|e| now - e.ts <= Duration::minutes(ACTIVE_GAP_MIN))
        .unwrap_or(false)
}

/// Average API-equivalent $/minute over the last 15 minutes.
pub fn burn_usd_per_min(events: &[UsageEvent], now: DateTime<Utc>) -> f64 {
    let from = now - Duration::minutes(BURN_WINDOW_MIN);
    let spent: f64 = events.iter().filter(|e| e.ts >= from && e.ts <= now).map(|e| e.cost_usd).sum();
    spent / BURN_WINDOW_MIN as f64
}

pub fn requests_per_min(events: &[UsageEvent], now: DateTime<Utc>) -> f64 {
    let from = now - Duration::minutes(BURN_WINDOW_MIN);
    let n = events.iter().filter(|e| e.kind == EventKind::Assistant && e.ts >= from && e.ts <= now).count();
    n as f64 / BURN_WINDOW_MIN as f64
}

/// %/minute from a series of gauge samples over the last 15 minutes. Needs ≥3 minutes of span.
pub fn gauge_pct_per_min(samples: &[GaugeSample], now: DateTime<Utc>) -> Option<f64> {
    let from = now - Duration::minutes(BURN_WINDOW_MIN);
    let s: Vec<&GaugeSample> = samples.iter().filter(|g| g.ts >= from && g.ts <= now).collect();
    let (first, last) = (s.first()?, s.last()?);
    // A reset inside the window makes the delta meaningless.
    if first.resets_at != last.resets_at && last.used_pct < first.used_pct {
        return None;
    }
    let mins = (last.ts - first.ts).num_seconds() as f64 / 60.0;
    if mins < 3.0 {
        return None;
    }
    let d = last.used_pct - first.used_pct;
    if d <= 0.0 {
        return Some(0.0);
    }
    Some(d / mins)
}

/// Round minutes to the nearest 5 so the number doesn't jitter. Never shows below 5.
pub fn round5(m: f64) -> u32 {
    let r = ((m / 5.0).round() * 5.0).max(5.0);
    r.min(24.0 * 60.0) as u32
}

/// minutes until the window's headroom runs out at `pct_per_min`, or None if not burning.
pub fn minutes_to_cutoff(headroom_pct: f64, pct_per_min: f64) -> Option<f64> {
    if pct_per_min <= 1e-9 {
        return None;
    }
    Some(headroom_pct.max(0.0) / pct_per_min)
}

/// Local-day bounds for "today" stats.
pub fn local_day_bounds(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let local = now.with_timezone(&chrono::Local);
    let day = local.date_naive();
    let start = chrono::Local
        .from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or(now - Duration::hours(24));
    (start, start + Duration::days(1))
}

pub fn month_start_local(now: DateTime<Utc>) -> DateTime<Utc> {
    let local = now.with_timezone(&chrono::Local);
    let d = local.date_naive().with_day(1).unwrap();
    chrono::Local
        .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or(now - Duration::days(30))
}

pub fn tool_label(t: Tool) -> &'static str {
    t.label()
}

//! Daily (morning brief) and weekly usage recaps.
//!
//! All database reads live here as read-only queries against `Store.conn` so
//! `store.rs` stays untouched. Privacy rule: counts, costs, percentages and
//! timestamps only — never prompt text, code, file names or project names.

use crate::metrics;
use crate::model::Tool;
use crate::snapshot::ToolStatus;
use crate::store::Store;
use anyhow::Result;
use chrono::{DateTime, Duration, Local, TimeZone, Timelike, Utc};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::BTreeMap;

const KV_HANDOFFS: &str = "recap_handoffs";
const HIT_THRESHOLD_PCT: f64 = 95.0;
const HIT_RESET_PCT: f64 = 90.0;

#[derive(Clone, Debug, Serialize)]
pub struct DailyToolUsage {
    pub tool: Tool,
    pub label: String,
    pub requests: u64,
    pub cost_usd: f64,
    /// Peak % of the 5-hour (or daily for Gemini) limit used yesterday.
    pub five_hour_peak_pct: Option<f64>,
    /// Peak % of the weekly limit used yesterday.
    pub weekly_peak_pct: Option<f64>,
    /// When the 5-hour (or daily for Gemini) limit resets today, if later today.
    pub five_hour_resets_at: Option<DateTime<Utc>>,
    /// When the weekly limit resets today, if later today.
    pub weekly_resets_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DailyRecap {
    /// Local calendar date (YYYY-MM-DD) for today.
    pub date: String,
    /// Yesterday's usage and today's resets per tool.
    pub tools: Vec<DailyToolUsage>,
    /// Number of non-dismissed dispatcher tasks created yesterday (count only).
    pub open_tasks_yesterday: u64,
    /// One plain suggestion, e.g. "Codex has the most room this morning".
    pub suggestion: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WeeklyToolComparison {
    pub tool: Tool,
    pub label: String,
    pub this_week_requests: u64,
    pub last_week_requests: u64,
    pub this_week_cost_usd: f64,
    pub last_week_cost_usd: f64,
    /// Percent change vs last week (cost-based, or request-based if cost is 0).
    pub change_pct: Option<f64>,
    /// Number of times the 5-hour (or daily) limit was hit this week.
    pub five_hour_hits: u32,
    /// Number of times the weekly limit was hit this week.
    pub weekly_hits: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct HourBucket {
    /// Local hour of day (0..23).
    pub hour: u8,
    pub requests: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct WeeklyRecap {
    pub tools: Vec<WeeklyToolComparison>,
    /// Up to 3 busiest local hours of the day, highest first.
    pub busiest_hours: Vec<HourBucket>,
    /// 24 local-hour request counts (index 0..23) for a glanceable bar strip.
    pub hourly_requests: Vec<u64>,
    /// Total handoffs completed this week.
    pub handoffs_count: u64,
    /// Total limit hits across all tools this week.
    pub total_limit_hits: u32,
}

fn short_tool_name(tool: Tool) -> &'static str {
    match tool {
        Tool::ClaudeCode => "Claude",
        Tool::Codex => "Codex",
        Tool::Gemini => "Gemini",
    }
}

fn local_day_key(now: DateTime<Utc>) -> String {
    now.with_timezone(&Local).format("%Y-%m-%d").to_string()
}

fn dismiss_key(now: DateTime<Utc>) -> String {
    format!("daily_recap_dismissed:{}", local_day_key(now))
}

/// Mark today's daily recap as dismissed. It reappears automatically tomorrow.
pub fn dismiss_daily_recap(store: &Store, now: DateTime<Utc>) -> Result<()> {
    store.kv_set(&dismiss_key(now), "1")
}

/// Whether today's daily recap has already been dismissed.
pub fn is_daily_recap_dismissed(store: &Store, now: DateTime<Utc>) -> bool {
    store
        .kv_get(&dismiss_key(now))
        .ok()
        .flatten()
        .as_deref()
        == Some("1")
}

/// Record a completed handoff timestamp (timestamps only, pruned to 35 days).
pub fn record_handoff(store: &Store, at: DateTime<Utc>) -> Result<()> {
    let mut stamps = load_handoff_timestamps(store);
    let cutoff = (at - Duration::days(35)).timestamp_millis();
    stamps.retain(|&ts| ts >= cutoff);
    stamps.push(at.timestamp_millis());
    store.kv_set(KV_HANDOFFS, &serde_json::to_string(&stamps)?)
}

/// Count completed handoffs in [from, to).
pub fn count_handoffs(store: &Store, from: DateTime<Utc>, to: DateTime<Utc>) -> u64 {
    let from_ms = from.timestamp_millis();
    let to_ms = to.timestamp_millis();
    load_handoff_timestamps(store)
        .into_iter()
        .filter(|&ts| ts >= from_ms && ts < to_ms)
        .count() as u64
}

fn load_handoff_timestamps(store: &Store) -> Vec<i64> {
    store
        .kv_get(KV_HANDOFFS)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<Vec<i64>>(&s).ok())
        .unwrap_or_default()
}

// ---------- read-only DB queries ----------

fn query_tool_totals(conn: &Connection, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<BTreeMap<Tool, (u64, f64)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT tool, COUNT(*), COALESCE(SUM(cost), 0.0) FROM events
         WHERE kind = 'assistant' AND ts >= ?1 AND ts < ?2
         GROUP BY tool",
    )?;
    let rows = stmt.query_map(params![from.timestamp_millis(), to.timestamp_millis()], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?))
    })?;
    let mut out = BTreeMap::new();
    for row in rows {
        let (tool_str, count, cost) = row?;
        if let Some(tool) = Tool::parse(&tool_str) {
            out.insert(tool, (count.max(0) as u64, cost));
        }
    }
    Ok(out)
}

fn query_peak_gauges(
    conn: &Connection,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<BTreeMap<(Tool, String), f64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT tool, window, MAX(used_pct) FROM gauges
         WHERE ts >= ?1 AND ts < ?2
         GROUP BY tool, window",
    )?;
    let rows = stmt.query_map(params![from.timestamp_millis(), to.timestamp_millis()], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?))
    })?;
    let mut out = BTreeMap::new();
    for row in rows {
        let (tool_str, window, peak) = row?;
        if let Some(tool) = Tool::parse(&tool_str) {
            out.insert((tool, window), peak.clamp(0.0, 100.0));
        }
    }
    Ok(out)
}

/// Count of non-dismissed dispatcher tasks created in [from, to). Count only.
fn query_open_tasks_count(conn: &Connection, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<u64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE created_at >= ?1 AND created_at < ?2 AND dismissed = 0",
        params![from.timestamp_millis(), to.timestamp_millis()],
        |r| r.get(0),
    )?;
    Ok(n.max(0) as u64)
}

fn query_assistant_timestamps(conn: &Connection, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT ts FROM events WHERE kind = 'assistant' AND ts >= ?1 AND ts < ?2",
    )?;
    let rows = stmt.query_map(params![from.timestamp_millis(), to.timestamp_millis()], |r| r.get::<_, i64>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn query_gemini_timestamps(conn: &Connection, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT ts FROM events WHERE tool = 'gemini' AND kind = 'assistant' AND ts >= ?1 AND ts < ?2",
    )?;
    let rows = stmt.query_map(params![from.timestamp_millis(), to.timestamp_millis()], |r| r.get::<_, i64>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

struct GaugeRow {
    tool: Tool,
    window: String,
    ts_ms: i64,
    used_pct: f64,
    resets_at_ms: Option<i64>,
}

fn query_gauge_series(conn: &Connection, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<GaugeRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT tool, window, ts, used_pct, resets_at FROM gauges
         WHERE ts >= ?1 AND ts < ?2
         ORDER BY tool, window, ts ASC",
    )?;
    let rows = stmt.query_map(params![from.timestamp_millis(), to.timestamp_millis()], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, f64>(3)?,
            r.get::<_, Option<i64>>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (tool_str, window, ts_ms, used_pct, resets_at_ms) = row?;
        if let Some(tool) = Tool::parse(&tool_str) {
            out.push(GaugeRow {
                tool,
                window,
                ts_ms,
                used_pct,
                resets_at_ms,
            });
        }
    }
    Ok(out)
}

// ---------- builders ----------

/// Compute the daily recap (morning brief). Returns `None` if disabled,
/// already dismissed today, or there is no usage/task/reset info to report.
pub fn build_daily_recap(
    store: &Store,
    tools: &[ToolStatus],
    gemini_daily_allowance: u64,
    enabled: bool,
    now: DateTime<Utc>,
) -> Result<Option<DailyRecap>> {
    if !enabled || is_daily_recap_dismissed(store, now) {
        return Ok(None);
    }

    let (today_start, today_end) = metrics::local_day_bounds(now);
    let yesterday_start = today_start - Duration::days(1);
    let yesterday_end = today_start;

    let totals = query_tool_totals(&store.conn, yesterday_start, yesterday_end)?;
    let peaks = query_peak_gauges(&store.conn, yesterday_start, yesterday_end)?;
    let open_tasks_yesterday = query_open_tasks_count(&store.conn, yesterday_start, yesterday_end)?;

    let mut items = Vec::new();
    for t in tools.iter().filter(|t| t.enabled && !t.locked) {
        let (requests, cost_usd) = totals.get(&t.tool).copied().unwrap_or((0, 0.0));
        let five_hour_peak_pct = if t.tool == Tool::Gemini {
            peaks.get(&(t.tool, "daily".into())).copied().or_else(|| {
                (requests > 0 && gemini_daily_allowance > 0)
                    .then(|| (requests as f64 / gemini_daily_allowance as f64 * 100.0).min(100.0))
            })
        } else {
            peaks.get(&(t.tool, "five_hour".into())).copied()
        };
        let weekly_peak_pct = if t.tool == Tool::Gemini {
            None
        } else {
            peaks.get(&(t.tool, "weekly".into())).copied()
        };

        let resets_today = |key: &str| -> Option<DateTime<Utc>> {
            t.windows
                .iter()
                .find(|w| w.key == key)
                .and_then(|w| w.resets_at)
                .filter(|&r| r >= now && r < today_end)
        };
        let primary_key = if t.tool == Tool::Gemini { "daily" } else { "five_hour" };
        let five_hour_resets_at = resets_today(primary_key);
        let weekly_resets_at = if t.tool == Tool::Gemini { None } else { resets_today("weekly") };

        if requests > 0
            || cost_usd > 0.0
            || five_hour_peak_pct.is_some()
            || weekly_peak_pct.is_some()
            || five_hour_resets_at.is_some()
            || weekly_resets_at.is_some()
        {
            items.push(DailyToolUsage {
                tool: t.tool,
                label: short_tool_name(t.tool).into(),
                requests,
                cost_usd,
                five_hour_peak_pct,
                weekly_peak_pct,
                five_hour_resets_at,
                weekly_resets_at,
            });
        }
    }

    if items.is_empty() && open_tasks_yesterday == 0 {
        return Ok(None);
    }

    let suggestion = build_suggestion(tools, now);

    Ok(Some(DailyRecap {
        date: local_day_key(now),
        tools: items,
        open_tasks_yesterday,
        suggestion,
    }))
}

fn build_suggestion(tools: &[ToolStatus], now: DateTime<Utc>) -> String {
    let time_phrase = if now.with_timezone(&Local).hour() < 12 {
        "this morning"
    } else {
        "today"
    };

    let mut candidates: Vec<(Tool, f64)> = Vec::new();
    for t in tools.iter().filter(|t| t.detected && t.enabled && !t.locked) {
        let primary_used = t
            .windows
            .iter()
            .find(|w| w.key == "five_hour" && w.source != metrics::Source::Learning)
            .or_else(|| {
                t.windows
                    .iter()
                    .find(|w| w.key == "daily" && w.source != metrics::Source::Learning)
            })
            .map(|w| w.used_pct)
            .or_else(|| t.headroom_pct.map(|h| (100.0 - h).clamp(0.0, 100.0)));
        if let Some(used) = primary_used {
            candidates.push((t.tool, used));
        }
    }

    if candidates.is_empty() {
        return "All clear to start your day".into();
    }

    candidates.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (best_tool, best_used) = candidates[0];
    if best_used >= 80.0 {
        return "Limits are tight today — watch for resets".into();
    }
    format!("{} has the most room {}", short_tool_name(best_tool), time_phrase)
}

/// Compute the weekly recap: this week vs last week per tool, busiest hours,
/// limit hits, and handoffs completed.
pub fn build_weekly_recap(
    store: &Store,
    tools: &[ToolStatus],
    gemini_daily_allowance: u64,
    now: DateTime<Utc>,
) -> Result<WeeklyRecap> {
    let this_week_start = metrics::week_start(now);
    let last_week_start = this_week_start - Duration::days(7);
    let this_week_end = now + Duration::seconds(1);

    let this_totals = query_tool_totals(&store.conn, this_week_start, this_week_end)?;
    let last_totals = query_tool_totals(&store.conn, last_week_start, this_week_start)?;
    let gauge_rows = query_gauge_series(&store.conn, this_week_start, this_week_end)?;
    let mut hits = count_limit_hits(&gauge_rows);

    // Fallback for Gemini when tracked via request logs without gauge rows.
    if !hits.contains_key(&(Tool::Gemini, "daily".into())) && gemini_daily_allowance > 0 {
        let gemini_ts = query_gemini_timestamps(&store.conn, this_week_start, this_week_end)?;
        let mut by_day: BTreeMap<chrono::NaiveDate, u64> = BTreeMap::new();
        for ts in gemini_ts {
            if let Some(dt) = DateTime::from_timestamp_millis(ts) {
                let d = dt.with_timezone(&chrono_tz::America::Los_Angeles).date_naive();
                *by_day.entry(d).or_default() += 1;
            }
        }
        let threshold = (gemini_daily_allowance as f64 * (HIT_THRESHOLD_PCT / 100.0)).ceil() as u64;
        let day_hits = by_day.values().filter(|&&n| n >= threshold).count() as u32;
        if day_hits > 0 {
            hits.insert((Tool::Gemini, "daily".into()), day_hits);
        }
    }

    let mut comparisons = Vec::new();
    let mut total_limit_hits: u32 = 0;

    for t in tools.iter().filter(|t| t.enabled && !t.locked) {
        let (this_req, this_cost) = this_totals.get(&t.tool).copied().unwrap_or((0, 0.0));
        let (last_req, last_cost) = last_totals.get(&t.tool).copied().unwrap_or((0, 0.0));
        let primary_key = if t.tool == Tool::Gemini { "daily" } else { "five_hour" };
        let five_hour_hits = hits.get(&(t.tool, primary_key.into())).copied().unwrap_or(0);
        let weekly_hits = if t.tool == Tool::Gemini {
            0
        } else {
            hits.get(&(t.tool, "weekly".into())).copied().unwrap_or(0)
        };
        total_limit_hits += five_hour_hits + weekly_hits;

        if this_req == 0 && last_req == 0 && five_hour_hits == 0 && weekly_hits == 0 {
            continue;
        }

        let change_pct = if last_cost > 0.001 {
            Some(((this_cost - last_cost) / last_cost) * 100.0)
        } else if last_req > 0 {
            Some(((this_req as f64 - last_req as f64) / last_req as f64) * 100.0)
        } else {
            None
        };

        comparisons.push(WeeklyToolComparison {
            tool: t.tool,
            label: short_tool_name(t.tool).into(),
            this_week_requests: this_req,
            last_week_requests: last_req,
            this_week_cost_usd: this_cost,
            last_week_cost_usd: last_cost,
            change_pct,
            five_hour_hits,
            weekly_hits,
        });
    }

    let timestamps = query_assistant_timestamps(&store.conn, this_week_start, this_week_end)?;
    let mut hourly_requests = vec![0u64; 24];
    for ts in timestamps {
        if let Some(dt) = DateTime::from_timestamp_millis(ts) {
            let h = Local.from_utc_datetime(&dt.naive_utc()).hour() as usize;
            if h < 24 {
                hourly_requests[h] += 1;
            }
        }
    }

    let mut busiest_hours: Vec<HourBucket> = hourly_requests
        .iter()
        .enumerate()
        .filter(|(_, &req)| req > 0)
        .map(|(h, &requests)| HourBucket {
            hour: h as u8,
            requests,
        })
        .collect();
    busiest_hours.sort_by(|a, b| b.requests.cmp(&a.requests).then_with(|| a.hour.cmp(&b.hour)));
    busiest_hours.truncate(3);

    let handoffs_count = count_handoffs(store, this_week_start, this_week_end);

    Ok(WeeklyRecap {
        tools: comparisons,
        busiest_hours,
        hourly_requests,
        handoffs_count,
        total_limit_hits,
    })
}

/// Count distinct limit-hit episodes (>= 95% used) per (tool, window) so
/// 60-second polling doesn't double-count the same capped window.
fn count_limit_hits(rows: &[GaugeRow]) -> BTreeMap<(Tool, String), u32> {
    let mut counts: BTreeMap<(Tool, String), u32> = BTreeMap::new();
    let mut state: BTreeMap<(Tool, String), (bool, Option<i64>, i64)> = BTreeMap::new();

    for r in rows {
        let key = (r.tool, r.window.clone());
        let gap_ms = if r.window == "five_hour" {
            5 * 3600 * 1000
        } else {
            24 * 3600 * 1000
        };
        let entry = state.entry(key.clone()).or_insert((false, None, i64::MIN));
        if r.used_pct >= HIT_THRESHOLD_PCT {
            let new_window = r.resets_at_ms.is_some() && entry.1.is_some() && r.resets_at_ms != entry.1;
            let gap_elapsed = entry.2 != i64::MIN && r.ts_ms - entry.2 > gap_ms;
            if !entry.0 || new_window || gap_elapsed {
                *counts.entry(key).or_default() += 1;
            }
            *entry = (true, r.resets_at_ms, r.ts_ms);
        } else if r.used_pct < HIT_RESET_PCT {
            entry.0 = false;
            entry.1 = r.resets_at_ms;
        }
    }

    counts
}

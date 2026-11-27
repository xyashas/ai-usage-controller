//! Autopilot: automatically hand a terminal task to another installed CLI tool
//! when its current tool hits its limit mid-task.
//!
//! Moved task IDs are persisted locally in `kv` under `"autopilot_moved"`
//! (pruned > 24 h). Counts/IDs only — never prompt text.

use crate::metrics::Source;
use crate::model::Tool;
use crate::snapshot::{Snapshot, ToolStatus};
use crate::store::Store;
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

pub const KV_MOVED: &str = "autopilot_moved";
const SOURCE_LIMIT_PCT: f64 = 97.0;
const TARGET_MAX_USED_PCT: f64 = 80.0;
const RECENT_IDLE_MIN: i64 = 5;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Move {
    pub task_id: i64,
    pub from: Tool,
    pub to: Tool,
    pub cwd: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MovedRecord {
    pub task_id: i64,
    pub to: Tool,
    pub at: DateTime<Utc>,
}

/// The 5-hour (or tightest non-learning) % used for a tool.
fn binding_used_pct(t: &ToolStatus) -> Option<f64> {
    t.windows
        .iter()
        .find(|w| w.key == "five_hour" && w.source != Source::Learning)
        .or_else(|| {
            t.windows
                .iter()
                .filter(|w| w.source != Source::Learning)
                .min_by(|a, b| a.headroom_pct.total_cmp(&b.headroom_pct))
        })
        .map(|w| w.used_pct)
        .or_else(|| t.headroom_pct.map(|h| (100.0 - h).clamp(0.0, 100.0)))
}

/// Decide which active terminal tasks should be automatically handed off.
///
/// A task qualifies when:
/// - its tool's 5-hour (or tightest) limit is >= 97% used
/// - the task is `"working"`, `"needs_approval"`, or went `"idle"` within 5 min
/// - `dest` is `"terminal"` (never app/web) and `cwd` is non-empty
/// - its `task_id` is not in `already_moved`
///
/// Target is the other installed, enabled, unlocked tool with the most room
/// (< 80% used).
pub fn decide(snap: &Snapshot, already_moved: &[i64], installed: &[Tool]) -> Vec<Move> {
    let now = snap.generated_at;
    let mut moves = Vec::new();

    for a in &snap.agents {
        if already_moved.contains(&a.id) {
            continue;
        }
        if a.dest.as_deref() != Some("terminal") || a.cwd.trim().is_empty() {
            continue;
        }
        let eligible_state = matches!(a.state.as_str(), "working" | "needs_approval")
            || (a.state == "idle"
                && a.last_activity_at
                    .is_some_and(|t| now - t <= Duration::minutes(RECENT_IDLE_MIN)));
        if !eligible_state {
            continue;
        }

        let Some(src) = snap
            .tools
            .iter()
            .find(|t| t.tool == a.tool && t.enabled && !t.locked)
        else {
            continue;
        };
        let Some(src_used) = binding_used_pct(src) else {
            continue;
        };
        if src_used < SOURCE_LIMIT_PCT {
            continue;
        }

        let target = snap
            .tools
            .iter()
            .filter(|t| t.tool != a.tool && t.enabled && !t.locked && installed.contains(&t.tool))
            .filter_map(|t| {
                let used = binding_used_pct(t).unwrap_or(0.0);
                (used < TARGET_MAX_USED_PCT).then_some((t.tool, used))
            })
            .min_by(|x, y| x.1.total_cmp(&y.1));

        if let Some((to, _)) = target {
            moves.push(Move {
                task_id: a.id,
                from: a.tool,
                to,
                cwd: a.cwd.clone(),
            });
        }
    }

    moves
}

/// Load autopilot move records from the last 24 hours.
pub fn load_moves(store: &Store, now: DateTime<Utc>) -> Vec<MovedRecord> {
    let cutoff = now - Duration::hours(24);
    store
        .kv_get(KV_MOVED)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<Vec<MovedRecord>>(&s).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|m| m.at >= cutoff)
        .collect()
}

/// Record a completed autopilot move (pruning records older than 24 h).
pub fn record_move(store: &Store, task_id: i64, to: Tool, now: DateTime<Utc>) -> Result<()> {
    let mut list = load_moves(store, now);
    if !list.iter().any(|m| m.task_id == task_id) {
        list.push(MovedRecord { task_id, to, at: now });
    }
    store.kv_set(KV_MOVED, &serde_json::to_string(&list)?)
}

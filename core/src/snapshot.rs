use crate::advice::{AuditResult, Nudge};
use crate::entitlements::Entitlements;
use crate::metrics::{Source, WindowStatus};
use crate::model::Tool;
use crate::sync::RemoteSummary;
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct ToolStatus {
    pub tool: Tool,
    pub label: String,
    pub detected: bool,
    pub enabled: bool,
    pub plan_id: Option<String>,
    pub plan_name: Option<String>,
    /// Headroom of the most constraining window, 0–100.
    pub headroom_pct: Option<f64>,
    /// "green" | "amber" | "red" | "grey"
    pub color: String,
    pub source: Source,
    pub windows: Vec<WindowStatus>,
    /// "About N min left at this pace", rounded to 5. Only while a session is active.
    pub minutes_left: Option<u32>,
    pub active: bool,
    pub today_cost_usd: f64,
    pub today_requests: u64,
    pub burn_usd_per_min: f64,
    /// True when hidden behind the Pro paywall.
    pub locked: bool,
    pub reconnect: bool,
    pub live_note: Option<String>,
    pub live_error: Option<String>,
    /// Claude only: no fresh plan gauge, so numbers are estimates from Claude Code
    /// logs (which miss claude.ai and desktop-app use). The UI offers to connect.
    pub needs_connect: bool,
}

/// What the app can see on this machine, for the "Check setup" screen.
#[derive(Clone, Debug, Serialize)]
pub struct ToolDiagnostics {
    pub tool: Tool,
    pub label: String,
    pub folders_found: Vec<String>,
    pub folders_checked: Vec<String>,
    pub requests_total: u64,
    pub requests_today: u64,
    pub last_request_at: Option<chrono::DateTime<chrono::Utc>>,
    pub gauge_source: Option<String>,
    pub gauge_at: Option<chrono::DateTime<chrono::Utc>>,
    pub gauge_used_pct: Option<f64>,
    pub live_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionCost {
    pub tool: Tool,
    pub session_id: String,
    pub project_name: Option<String>,
    pub cost_usd: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TodayStats {
    pub cost_usd: f64,
    pub sessions: usize,
    pub top_session: Option<SessionCost>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionMeter {
    pub tool: Tool,
    pub session_id: String,
    pub project_name: Option<String>,
    pub cost_usd: f64,
    pub started_at: DateTime<Utc>,
    pub minutes: i64,
    pub burn_usd_per_min: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AlertAction {
    /// "stop" | "switch" | "dismiss" | "upgrade"
    pub kind: String,
    pub label: String,
    pub target_tool: Option<Tool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Alert {
    pub id: String,
    /// "amber" | "red"
    pub level: String,
    /// "low_headroom" | "runaway"
    pub kind: String,
    pub tool: Tool,
    pub message: String,
    pub action: AlertAction,
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub burn_usd_per_min: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GaugePoint {
    pub at: i64,
    pub used: f64,
    pub reset_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HandoffOffer {
    pub source: Tool,
    pub session: String,
    pub targets: Vec<Tool>,
}

/// A task started from the dispatcher, with what its session is doing.
#[derive(Clone, Debug, Serialize)]
pub struct AgentRun {
    pub id: i64,
    pub tool: Tool,
    pub text: String,
    pub repo_name: String,
    pub branch: Option<String>,
    pub cwd: String,
    pub started_at: DateTime<Utc>,
    /// "starting" (no activity yet), "working" (active in the last 90 s),
    /// "idle" (quiet for a while), "done" (the tool says it finished its turn),
    /// "needs_approval" (waiting for you to allow a tool), "no_activity" (never
    /// started), "opened" (handed to an app or the web; not tracked from here),
    /// "queued" (waiting for its tool's limit to reset).
    pub state: String,
    /// "terminal", "app" or "web".
    pub dest: Option<String>,
    pub cost_usd: f64,
    pub requests: u64,
    pub last_activity_at: Option<DateTime<Utc>>,
    pub queued_until: Option<DateTime<Utc>>,
    /// Last test run: "running", "passed", "failed", "timed_out" or "error".
    pub verify: Option<String>,
    pub verify_secs: Option<f64>,
    pub verify_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub generated_at: DateTime<Utc>,
    /// Dispatcher tasks from the last 24 hours, newest first.
    pub agents: Vec<AgentRun>,
    pub approvals: Vec<crate::agent_status::Approval>,
    pub handoffs: Vec<HandoffOffer>,
    pub gauge_history: Vec<GaugePoint>,
    pub idle_hidden: bool,
    pub onboarded: bool,
    pub entitlements: Entitlements,
    pub active_tool: Option<Tool>,
    pub tools: Vec<ToolStatus>,
    pub today: TodayStats,
    pub session_meter: Option<SessionMeter>,
    /// Highest-priority alert first.
    pub alerts: Vec<Alert>,
    pub nudge: Option<Nudge>,
    pub savings_month_usd: Option<f64>,
    pub audit: Vec<AuditResult>,
    /// The user's own projected monthly saving; the paywall headline.
    pub projected_saving_usd: f64,
    /// Feature ids hidden for this tier.
    pub locked: Vec<String>,
    pub remote: Option<RemoteSummary>,
    pub daily_recap: Option<crate::recap::DailyRecap>,
    pub weekly_recap: crate::recap::WeeklyRecap,
}

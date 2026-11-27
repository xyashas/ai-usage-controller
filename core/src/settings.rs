use crate::model::Tool;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolSettings {
    pub enabled: bool,
    /// Plan id from plans.json, e.g. "max5". None = not chosen yet.
    pub plan_id: Option<String>,
    /// Manual allowance override for the 5-hour (Claude/Codex) window, in API-equivalent USD.
    pub five_hour_allowance_usd: Option<f64>,
    /// Manual allowance override for the weekly window, in API-equivalent USD.
    pub weekly_allowance_usd: Option<f64>,
    /// Extra log roots, e.g. for Antigravity CLI.
    pub extra_roots: Vec<PathBuf>,
}

impl Default for ToolSettings {
    fn default() -> Self {
        ToolSettings { enabled: true, plan_id: None, five_hour_allowance_usd: None, weekly_allowance_usd: None, extra_roots: vec![] }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AlertSettings {
    /// Amber when headroom falls to this % or below.
    pub amber_pct: f64,
    /// Red when headroom falls to this % or below.
    pub red_pct: f64,
    pub runaway_alerts: bool,
    /// One-click stop sends an interrupt to the session process. Off until thresholds are proven.
    pub one_click_stop: bool,
    pub notifications: bool,
}

impl Default for AlertSettings {
    fn default() -> Self {
        AlertSettings { amber_pct: 20.0, red_pct: 5.0, runaway_alerts: true, one_click_stop: false, notifications: true }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncSettings {
    pub enabled: bool,
    pub server_url: Option<String>,
    pub device_token: Option<String>,
    pub device_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub onboarded: bool,
    pub launch_at_login: bool,
    pub hide_when_idle: bool,
    pub tools: BTreeMap<Tool, ToolSettings>,
    /// The single tool a Free user tracks.
    pub primary_tool: Option<Tool>,
    pub alerts: AlertSettings,
    pub hotkey: String,
    pub sync: SyncSettings,
    /// Read Claude's live 5-hour/weekly gauges with the Claude Code login (unofficial, best-effort).
    pub live_gauges: bool,
    /// Signed in to claude.ai inside the app, so the plan gauge works without Claude Code.
    pub claude_web: bool,
    pub api_base: String,
    /// Repositories the user added for handoffs, besides the ones found from
    /// recent sessions. Local only.
    pub repos: Vec<PathBuf>,
    /// After opening an app or web page for a task or handoff, paste the prompt
    /// and press Enter once that window is in front.
    pub auto_send: bool,
    /// Claude Code and Codex report when they need you or finish (hooks in
    /// ~/.claude/settings.json and ~/.codex/config.toml). Turned on from the widget.
    pub agent_status: bool,
    /// Allow or deny Claude Code's permission prompts from the widget.
    pub approvals: bool,
    /// Run the project's tests when an agent finishes, and show if they passed.
    pub verify_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let mut tools = BTreeMap::new();
        for t in Tool::ALL {
            tools.insert(t, ToolSettings::default());
        }
        Settings {
            onboarded: false,
            launch_at_login: true,
            hide_when_idle: false,
            tools,
            primary_tool: None,
            alerts: AlertSettings::default(),
            hotkey: "Alt+A".into(),
            sync: SyncSettings::default(),
            live_gauges: true,
            claude_web: false,
            api_base: "https://api.aiusage.dev".into(),
            repos: vec![],
            auto_send: true,
            agent_status: false,
            approvals: true,
            verify_done: true,
        }
    }
}

impl Settings {
    pub fn tool(&self, t: Tool) -> ToolSettings {
        self.tools.get(&t).cloned().unwrap_or_default()
    }
}

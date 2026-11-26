use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    ClaudeCode,
    Codex,
    Gemini,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::ClaudeCode, Tool::Codex, Tool::Gemini];

    pub fn as_str(&self) -> &'static str {
        match self {
            Tool::ClaudeCode => "claude_code",
            Tool::Codex => "codex",
            Tool::Gemini => "gemini",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Tool::ClaudeCode => "Claude",
            Tool::Codex => "Codex",
            Tool::Gemini => "Gemini",
        }
    }

    pub fn parse(s: &str) -> Option<Tool> {
        match s {
            "claude_code" | "claude" => Some(Tool::ClaudeCode),
            "codex" => Some(Tool::Codex),
            "gemini" => Some(Tool::Gemini),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A model response that consumed tokens (one API request).
    Assistant,
    /// A message typed by the human. Used to count agent turns between prompts.
    UserPrompt,
    /// A tool call recorded separately from token usage (Codex logs these on their own line).
    ToolCall,
}

impl EventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventKind::Assistant => "assistant",
            EventKind::UserPrompt => "user_prompt",
            EventKind::ToolCall => "tool_call",
        }
    }
    pub fn parse(s: &str) -> EventKind {
        match s {
            "user_prompt" => EventKind::UserPrompt,
            "tool_call" => EventKind::ToolCall,
            _ => EventKind::Assistant,
        }
    }
}

/// One normalized usage record. Token fields are normalized so that
/// `input_tokens` never includes cached tokens and `output_tokens`
/// includes reasoning/thinking tokens (both are billed at output price).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageEvent {
    pub dedup_key: String,
    pub tool: Tool,
    pub kind: EventKind,
    pub ts: DateTime<Utc>,
    pub session_id: String,
    /// Working directory of the session. Stays on device; never synced.
    pub project: Option<String>,
    pub model: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_read_tokens: u64,
    /// Estimated API-equivalent cost in USD, filled by the pricing table.
    pub cost_usd: f64,
    /// Short signature of tool calls in this turn ("Edit:src/main.rs"), for loop detection only.
    pub tool_sig: Option<String>,
}

impl UsageEvent {
    pub fn new(tool: Tool, kind: EventKind, dedup_key: String, ts: DateTime<Utc>, session_id: String) -> Self {
        UsageEvent {
            dedup_key,
            tool,
            kind,
            ts,
            session_id,
            project: None,
            model: None,
            input_tokens: 0,
            output_tokens: 0,
            cache_write_tokens: 0,
            cache_read_tokens: 0,
            cost_usd: 0.0,
            tool_sig: None,
        }
    }

    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_write_tokens + self.cache_read_tokens
    }
}

/// A reading of a vendor's own usage gauge (e.g. Codex's rate_limits, Claude's /usage).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GaugeSample {
    pub tool: Tool,
    /// "five_hour", "weekly", "weekly_opus", "daily"
    pub window: String,
    pub used_pct: f64,
    pub resets_at: Option<DateTime<Utc>>,
    pub window_minutes: Option<u32>,
    pub ts: DateTime<Utc>,
    /// "codex_log", "claude_oauth", "gemini_quota"
    pub source: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ParsedItem {
    Event(UsageEvent),
    Gauge(GaugeSample),
}

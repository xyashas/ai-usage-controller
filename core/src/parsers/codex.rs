//! Codex CLI rollout logs: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`.
//!
//! Every line is `{ "timestamp", "type", "payload" }`:
//! - `session_meta` → `payload.id`, `payload.cwd`
//! - `turn_context` → `payload.model`, `payload.cwd`
//! - `event_msg` / `payload.type == "user_message"` → a human prompt
//! - `event_msg` / `payload.type == "token_count"` → `payload.info.last_token_usage`
//!   (per-request) and `payload.info.total_token_usage` (cumulative), plus
//!   `payload.rate_limits.primary|secondary` = { used_percent, window_minutes,
//!   resets_in_seconds | resets_at } — Codex's own live gauges.
//! - `response_item` / `payload.type == "function_call"` → a tool call
//!
//! OpenAI's `input_tokens` includes cached tokens, so we subtract them;
//! `output_tokens` already includes reasoning tokens.

use super::{parse_ts, tool_sig, u};
use crate::model::{EventKind, GaugeSample, ParsedItem, Tool, UsageEvent};
use chrono::Duration;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PARSER_VERSION: u32 = 1;

/// Per-file state carried between lines (and persisted between scans).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CodexCtx {
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<String>,
    pub line_no: u64,
}

pub fn parse_line(line: &str, ctx: &mut CodexCtx, file_stem: &str) -> Vec<ParsedItem> {
    ctx.line_no += 1;
    let line = line.trim();
    if line.is_empty() || !line.starts_with('{') {
        return vec![];
    }
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let Some(ts) = parse_ts(v.get("timestamp")) else { return vec![] };
    let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let empty = Value::Null;
    let p = v.get("payload").unwrap_or(&empty);
    let session = || ctx.session_id.clone().unwrap_or_else(|| file_stem.to_string());

    match ty {
        "session_meta" => {
            if let Some(id) = p.get("id").and_then(|s| s.as_str()) {
                ctx.session_id = Some(id.to_string());
            }
            if let Some(c) = p.get("cwd").and_then(|s| s.as_str()) {
                ctx.cwd = Some(c.to_string());
            }
            vec![]
        }
        "turn_context" => {
            if let Some(m) = p.get("model").and_then(|s| s.as_str()) {
                ctx.model = Some(m.to_string());
            }
            if let Some(c) = p.get("cwd").and_then(|s| s.as_str()) {
                ctx.cwd = Some(c.to_string());
            }
            vec![]
        }
        "event_msg" => match p.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "user_message" => {
                let key = format!("cxu:{}:{}", session(), ctx.line_no);
                let mut ev = UsageEvent::new(Tool::Codex, EventKind::UserPrompt, key, ts, session());
                ev.project = ctx.cwd.clone();
                vec![ParsedItem::Event(ev)]
            }
            "token_count" => {
                let mut out = Vec::new();
                if let Some(info) = p.get("info").filter(|i| !i.is_null()) {
                    if let Some(last) = info.get("last_token_usage") {
                        let total = info
                            .get("total_token_usage")
                            .map(|t| u(t.get("total_tokens")))
                            .unwrap_or(0);
                        let input = u(last.get("input_tokens"));
                        let cached = u(last.get("cached_input_tokens")).min(input);
                        let key = format!("cx:{}:{}", session(), total);
                        let mut ev = UsageEvent::new(Tool::Codex, EventKind::Assistant, key, ts, session());
                        ev.project = ctx.cwd.clone();
                        ev.model = ctx.model.clone().or_else(|| Some("gpt-5-codex".into()));
                        ev.input_tokens = input - cached;
                        ev.cache_read_tokens = cached;
                        ev.output_tokens = u(last.get("output_tokens"));
                        if ev.total_tokens() > 0 {
                            out.push(ParsedItem::Event(ev));
                        }
                    }
                }
                if let Some(rl) = p.get("rate_limits").filter(|r| !r.is_null()) {
                    for (slot, fallback_window) in [("primary", "five_hour"), ("secondary", "weekly")] {
                        if let Some(g) = rl.get(slot).filter(|g| !g.is_null()) {
                            let mins = g.get("window_minutes").and_then(|m| m.as_u64()).map(|m| m as u32);
                            let window = match mins {
                                Some(m) if m <= 24 * 60 => "five_hour",
                                Some(_) => "weekly",
                                None => fallback_window,
                            };
                            let resets_at = parse_ts(g.get("resets_at")).or_else(|| {
                                g.get("resets_in_seconds")
                                    .and_then(|s| s.as_i64())
                                    .map(|s| ts + Duration::seconds(s))
                            });
                            out.push(ParsedItem::Gauge(GaugeSample {
                                tool: Tool::Codex,
                                window: window.to_string(),
                                used_pct: g.get("used_percent").and_then(|x| x.as_f64()).unwrap_or(0.0),
                                resets_at,
                                window_minutes: mins,
                                ts,
                                source: "codex_log".into(),
                            }));
                        }
                    }
                }
                out
            }
            _ => vec![],
        },
        "response_item" => {
            let pty = p.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if pty == "function_call" || pty == "custom_tool_call" || pty == "local_shell_call" {
                let name = p.get("name").and_then(|s| s.as_str()).unwrap_or("shell");
                let args: Option<Value> = p
                    .get("arguments")
                    .and_then(|a| a.as_str())
                    .and_then(|s| serde_json::from_str(s).ok())
                    .or_else(|| p.get("action").cloned());
                let key = format!("cxt:{}:{}", session(), ctx.line_no);
                let mut ev = UsageEvent::new(Tool::Codex, EventKind::ToolCall, key, ts, session());
                ev.project = ctx.cwd.clone();
                ev.tool_sig = Some(tool_sig(name, args.as_ref()));
                return vec![ParsedItem::Event(ev)];
            }
            vec![]
        }
        _ => vec![],
    }
}

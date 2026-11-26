//! Claude Code session logs: `~/.claude/projects/<project>/<session>.jsonl`.
//!
//! Relevant line shapes:
//! - `type: "assistant"` with `message.id`, `message.model`, `message.usage`
//!   (`input_tokens`, `output_tokens`, `cache_creation_input_tokens`,
//!   `cache_read_input_tokens`) and `message.content[]` (may contain `tool_use`).
//!   One API response is often written as several lines (one per content
//!   block) sharing `message.id` + `requestId`; we dedup on that pair.
//! - `type: "user"` where `message.content` is a string or text blocks is a
//!   human prompt; arrays of `tool_result` blocks are tool output, not prompts.

use super::{parse_ts, tool_sig, u};
use crate::model::{EventKind, ParsedItem, Tool, UsageEvent};
use serde_json::Value;

pub const PARSER_VERSION: u32 = 1;

pub fn parse_line(line: &str) -> Vec<ParsedItem> {
    let line = line.trim();
    if line.is_empty() || !line.starts_with('{') {
        return vec![];
    }
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let ts = match parse_ts(v.get("timestamp")) {
        Some(t) => t,
        None => return vec![],
    };
    let session = v
        .get("sessionId")
        .and_then(|s| s.as_str())
        .unwrap_or("unknown")
        .to_string();
    let cwd = v.get("cwd").and_then(|s| s.as_str()).map(String::from);
    let msg = v.get("message");

    match ty {
        "assistant" => {
            let Some(msg) = msg else { return vec![] };
            let Some(usage) = msg.get("usage") else { return vec![] };
            let model = msg.get("model").and_then(|m| m.as_str()).map(String::from);
            if model.as_deref() == Some("<synthetic>") {
                return vec![];
            }
            let msg_id = msg.get("id").and_then(|s| s.as_str());
            let req_id = v.get("requestId").and_then(|s| s.as_str());
            let key = match (msg_id, req_id) {
                (Some(m), Some(r)) => format!("cc:{m}:{r}"),
                (Some(m), None) => format!("cc:{m}"),
                _ => format!(
                    "cc:{}",
                    v.get("uuid").and_then(|s| s.as_str()).unwrap_or_default()
                ),
            };
            let mut ev = UsageEvent::new(Tool::ClaudeCode, EventKind::Assistant, key, ts, session);
            ev.project = cwd;
            ev.model = model;
            ev.input_tokens = u(usage.get("input_tokens"));
            ev.output_tokens = u(usage.get("output_tokens"));
            ev.cache_write_tokens = u(usage.get("cache_creation_input_tokens"));
            ev.cache_read_tokens = u(usage.get("cache_read_input_tokens"));
            let sigs: Vec<String> = msg
                .get("content")
                .and_then(|c| c.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                        .map(|b| {
                            tool_sig(
                                b.get("name").and_then(|n| n.as_str()).unwrap_or("tool"),
                                b.get("input"),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            if !sigs.is_empty() {
                ev.tool_sig = Some(sigs.join("|"));
            }
            vec![ParsedItem::Event(ev)]
        }
        "user" => {
            if v.get("isMeta").and_then(|b| b.as_bool()) == Some(true) {
                return vec![];
            }
            let Some(content) = msg.and_then(|m| m.get("content")) else { return vec![] };
            let is_prompt = match content {
                Value::String(s) => !s.starts_with("<command-") && !s.starts_with("<local-command"),
                Value::Array(arr) => {
                    !arr.is_empty()
                        && arr.iter().all(|b| b.get("type").and_then(|t| t.as_str()) != Some("tool_result"))
                }
                _ => false,
            };
            if !is_prompt {
                return vec![];
            }
            let key = format!(
                "ccu:{}",
                v.get("uuid").and_then(|s| s.as_str()).unwrap_or_default()
            );
            let mut ev = UsageEvent::new(Tool::ClaudeCode, EventKind::UserPrompt, key, ts, session);
            ev.project = cwd;
            vec![ParsedItem::Event(ev)]
        }
        _ => vec![],
    }
}

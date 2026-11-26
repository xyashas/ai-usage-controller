//! Gemini CLI chat recordings: `~/.gemini/tmp/<project-hash>/chats/session-*.json`.
//!
//! The whole file is one JSON document, rewritten as the session grows:
//! `{ sessionId, projectHash, startTime, lastUpdated, messages: [...] }`
//! where each message has `id`, `timestamp`, `type` ("user" | "gemini" | "info" | "error"),
//! and for `gemini` messages `model`, `tokens` { input, output, cached, thoughts, tool, total }
//! and optional `toolCalls[]` { name, args }.
//!
//! Gemini's limit is requests per day, so every `gemini` message counts as one request.
//! `tokens.input` includes cached tokens; `thoughts` are billed as output.

use super::{parse_ts, tool_sig, u};
use crate::model::{EventKind, ParsedItem, Tool, UsageEvent};
use serde_json::Value;

pub const PARSER_VERSION: u32 = 1;

pub fn parse_file(content: &str, file_stem: &str) -> Vec<ParsedItem> {
    let v: Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let session = v
        .get("sessionId")
        .and_then(|s| s.as_str())
        .unwrap_or(file_stem)
        .to_string();
    let project = v.get("projectHash").and_then(|s| s.as_str()).map(String::from);
    let Some(messages) = v.get("messages").and_then(|m| m.as_array()) else { return vec![] };

    let mut out = Vec::new();
    for (i, m) in messages.iter().enumerate() {
        let Some(ts) = parse_ts(m.get("timestamp")) else { continue };
        let id = m
            .get("id")
            .and_then(|s| s.as_str())
            .map(String::from)
            .unwrap_or_else(|| i.to_string());
        match m.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "user" => {
                let mut ev = UsageEvent::new(
                    Tool::Gemini,
                    EventKind::UserPrompt,
                    format!("gmu:{session}:{id}"),
                    ts,
                    session.clone(),
                );
                ev.project = project.clone();
                out.push(ParsedItem::Event(ev));
            }
            "gemini" => {
                let mut ev = UsageEvent::new(
                    Tool::Gemini,
                    EventKind::Assistant,
                    format!("gm:{session}:{id}"),
                    ts,
                    session.clone(),
                );
                ev.project = project.clone();
                ev.model = m.get("model").and_then(|s| s.as_str()).map(String::from);
                if let Some(t) = m.get("tokens") {
                    let input = u(t.get("input"));
                    let cached = u(t.get("cached")).min(input);
                    ev.input_tokens = input - cached;
                    ev.cache_read_tokens = cached;
                    ev.output_tokens = u(t.get("output")) + u(t.get("thoughts"));
                }
                if let Some(calls) = m.get("toolCalls").and_then(|c| c.as_array()) {
                    let sigs: Vec<String> = calls
                        .iter()
                        .map(|c| {
                            tool_sig(
                                c.get("name").and_then(|n| n.as_str()).unwrap_or("tool"),
                                c.get("args"),
                            )
                        })
                        .collect();
                    if !sigs.is_empty() {
                        ev.tool_sig = Some(sigs.join("|"));
                    }
                }
                out.push(ParsedItem::Event(ev));
            }
            _ => {}
        }
    }
    out
}

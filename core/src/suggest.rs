//! Proactive suggestion chips ("Speaks up first"): pure detection logic for
//! copied errors, recent test failures, and high limit usage mid-task.

use crate::metrics::Source;
use crate::model::Tool;
use crate::snapshot::Snapshot;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Kind {
    CopiedError,
    TestsFailed { task_id: i64 },
    LimitMidTask { tool: Tool, to: Tool },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    pub id: String,
    pub kind: Kind,
    /// Short chip headline (<= 40 chars), e.g. "Fix this error?".
    pub title: String,
    pub detail: String,
    /// "ask_fix" | "send_failure" | "handoff"
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub text: Option<String>,
}

fn short_tool(tool: Tool) -> &'static str {
    match tool {
        Tool::ClaudeCode => "Claude",
        Tool::Codex => "Codex",
        Tool::Gemini => "Gemini",
    }
}

/// Heuristic detector for stack traces, compiler/runtime errors, and non-zero
/// exit codes. Tuned to avoid false positives on normal English prose.
pub fn looks_like_error(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() < 12 || trimmed.len() > 20_000 {
        return false;
    }

    // Strong unambiguous error/traceback signatures
    let strong_markers = [
        "Traceback (most recent call last):",
        "panicked at ",
        "npm ERR!",
        "npm error ",
        "error[E",
        "UnhandledPromiseRejection",
        "Segmentation fault",
        "NullPointerException",
        "AssertionError:",
        "TypeError:",
        "ReferenceError:",
        "SyntaxError:",
        "RangeError:",
        "ValueError:",
        "KeyError:",
        "AttributeError:",
        "ModuleNotFoundError:",
        "ImportError:",
        "RuntimeError:",
        "Compilation failed",
        "BUILD FAILED",
        "cargo:rustc-link",
        "Caused by:",
        "fatal:",
        "FATAL:",
    ];
    if strong_markers.iter().any(|m| trimmed.contains(m)) {
        return true;
    }

    // Line-anchored error prefixes ("error:", "ERROR:", "FAIL ", "test ... FAILED")
    let mut stack_frames = 0usize;
    for line in trimmed.lines().take(60) {
        let l = line.trim();
        if l.starts_with("error:")
            || l.starts_with("Error:")
            || l.starts_with("ERROR:")
            || l.starts_with("FAIL ")
            || l.starts_with("FAILED ")
            || l.ends_with("... FAILED")
        {
            return true;
        }
        // JS/TS/Java/Rust stack frame lines
        if (l.starts_with("at ") && (l.contains(':') && (l.contains('(') || l.contains('/') || l.contains('\\'))))
            || (l.starts_with("File \"") && l.contains(", line "))
            || l.starts_with("--> ") && l.contains(':')
        {
            stack_frames += 1;
            if stack_frames >= 2 {
                return true;
            }
        }
        // Non-zero exit codes from CLI/build tools
        let lower = l.to_ascii_lowercase();
        if (lower.contains("exit code ") || lower.contains("exited with code ") || lower.contains("exit status "))
            && !lower.contains("exit code 0")
            && !lower.contains("exited with code 0")
            && !lower.contains("exit status 0")
        {
            return true;
        }
    }

    false
}

/// Derive proactive suggestions from the current snapshot:
/// - tests that just failed (within the last 15 minutes)
/// - a tool at >= 90% used while one of its tasks is actively working (suggesting the roomiest other tool)
pub fn from_snapshot(snap: &Snapshot, now: DateTime<Utc>) -> Vec<Suggestion> {
    let mut out = Vec::new();

    // 1. Recently failed tests on an agent task
    for a in &snap.agents {
        let failed = matches!(a.verify.as_deref(), Some("failed" | "timed_out"));
        let recent = a
            .verify_at
            .is_none_or(|t| now - t <= Duration::minutes(15));
        if failed && recent {
            let title = if a.verify.as_deref() == Some("timed_out") {
                "Tests timed out — fix?".to_owned()
            } else {
                "Tests failed — fix?".to_owned()
            };
            out.push(Suggestion {
                id: format!("tests-failed:{}", a.id),
                kind: Kind::TestsFailed { task_id: a.id },
                title,
                detail: format!("{} tests did not pass", short_tool(a.tool)),
                action: "send_failure".into(),
                text: None,
            });
        }
    }

    // 2. Tool >= 90% used while one of its tasks is working
    for t in snap.tools.iter().filter(|t| t.enabled && t.detected && !t.locked) {
        let used = t
            .windows
            .iter()
            .find(|w| w.key == "five_hour" && w.source != Source::Learning)
            .or_else(|| t.windows.iter().find(|w| w.key == "daily" && w.source != Source::Learning))
            .map(|w| w.used_pct)
            .or_else(|| t.headroom_pct.map(|h| 100.0 - h));

        let Some(used_pct) = used else { continue };
        if used_pct < 90.0 {
            continue;
        }

        let working = snap
            .agents
            .iter()
            .any(|a| a.tool == t.tool && a.state == "working");
        if !working {
            continue;
        }

        // Pick the roomiest alternative tool (> 15% headroom)
        let alt = snap
            .tools
            .iter()
            .filter(|o| o.tool != t.tool && o.enabled && o.detected && !o.locked)
            .filter_map(|o| o.headroom_pct.filter(|&h| h > 15.0).map(|h| (o.tool, h)))
            .max_by(|a, b| a.1.total_cmp(&b.1));

        if let Some((to, _)) = alt {
            out.push(Suggestion {
                id: format!("limit-mid-task:{}:{}", t.tool.as_str(), to.as_str()),
                kind: Kind::LimitMidTask { tool: t.tool, to },
                title: format!("Hand off to {}?", short_tool(to)),
                detail: format!("{} at {:.0}% mid-task", short_tool(t.tool), used_pct),
                action: "handoff".into(),
                text: None,
            });
        }
    }

    out
}

//! Where each tool keeps its logs, on Mac, Windows and Linux.
//!
//! | Tool        | Default root                                   | Override env      |
//! |-------------|------------------------------------------------|-------------------|
//! | Claude Code | `~/.claude/projects`, `~/.config/claude/projects` | `CLAUDE_CONFIG_DIR` (comma-separated) |
//! | Codex CLI   | `~/.codex/sessions`                            | `CODEX_HOME`      |
//! | Gemini CLI  | `~/.gemini/tmp/<project-hash>/chats`           | `GEMINI_DIR`      |
//!
//! `~` is `%USERPROFILE%` on Windows; all three CLIs use the same dot-folders there.
//! Extra roots (e.g. Antigravity CLI once its layout is confirmed) come from settings.

use crate::model::Tool;
use std::path::PathBuf;

pub fn home() -> Option<PathBuf> {
    dirs::home_dir()
}

pub fn default_roots(tool: Tool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let home = match home() {
        Some(h) => h,
        None => return out,
    };
    match tool {
        Tool::ClaudeCode => {
            if let Ok(v) = std::env::var("CLAUDE_CONFIG_DIR") {
                for p in v.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    out.push(PathBuf::from(p).join("projects"));
                }
            }
            out.push(home.join(".claude").join("projects"));
            out.push(home.join(".config").join("claude").join("projects"));
        }
        Tool::Codex => {
            if let Ok(v) = std::env::var("CODEX_HOME") {
                out.push(PathBuf::from(v).join("sessions"));
            }
            out.push(home.join(".codex").join("sessions"));
        }
        Tool::Gemini => {
            if let Ok(v) = std::env::var("GEMINI_DIR") {
                out.push(PathBuf::from(v).join("tmp"));
            }
            out.push(home.join(".gemini").join("tmp"));
        }
    }
    out.dedup();
    out
}

/// Claude Code OAuth credentials file (Linux and Windows). On macOS they live in the Keychain.
pub fn claude_credentials_file() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("CLAUDE_CONFIG_DIR") {
        if let Some(first) = v.split(',').next() {
            return Some(PathBuf::from(first.trim()).join(".credentials.json"));
        }
    }
    home().map(|h| h.join(".claude").join(".credentials.json"))
}

/// App data directory for our own SQLite database.
pub fn app_data_dir() -> PathBuf {
    if let Ok(v) = std::env::var("AIUC_DATA_DIR") {
        return PathBuf::from(v);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ai-usage-controller")
}

/// Which roots exist on this machine, used by onboarding to detect installed tools.
pub fn detect(tool: Tool, extra: &[PathBuf]) -> Vec<PathBuf> {
    default_roots(tool)
        .into_iter()
        .chain(extra.iter().cloned())
        .filter(|p| p.is_dir())
        .collect()
}

//! Pure GitHub Actions CI run parsing, branch filtering, failure diffing, and
//! log tail cleaning. Network calls live in `app/src-tauri/src/ci_watch.rs`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CiRun {
    pub id: u64,
    /// "owner/name"
    pub repo: String,
    pub branch: String,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub html_url: String,
    pub updated_at: String,
}

/// Parse GitHub `GET /repos/{o}/{r}/actions/runs?per_page=20` JSON.
pub fn parse_runs(json: &str, repo: &str) -> Vec<CiRun> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return vec![];
    };
    v["workflow_runs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            Some(CiRun {
                id: r["id"].as_u64()?,
                repo: repo.to_owned(),
                branch: r["head_branch"].as_str()?.to_owned(),
                name: r["name"].as_str().unwrap_or("CI").to_owned(),
                status: r["status"].as_str().unwrap_or("unknown").to_owned(),
                conclusion: r["conclusion"].as_str().map(str::to_owned),
                html_url: r["html_url"].as_str().unwrap_or("").to_owned(),
                updated_at: r["updated_at"].as_str().unwrap_or("").to_owned(),
            })
        })
        .collect()
}

/// True if `run.branch` is one of the dispatcher's task branches or uses a
/// standard agent branch prefix (`agents/`, `claude/`, `codex/`, `gemini/`).
pub fn watched(run: &CiRun, task_branches: &[String]) -> bool {
    let b = run.branch.as_str();
    if task_branches.iter().any(|tb| tb == b) {
        return true;
    }
    b.starts_with("agents/")
        || b.starts_with("claude/")
        || b.starts_with("codex/")
        || b.starts_with("gemini/")
}

/// Returns runs in `now` whose conclusion is `"failure"` and that were not
/// already failed in `prev`.
pub fn newly_failed(prev: &[CiRun], now: &[CiRun]) -> Vec<CiRun> {
    let prev_failed: HashSet<u64> = prev
        .iter()
        .filter(|r| r.conclusion.as_deref() == Some("failure"))
        .map(|r| r.id)
        .collect();
    now.iter()
        .filter(|r| r.conclusion.as_deref() == Some("failure") && !prev_failed.contains(&r.id))
        .cloned()
        .collect()
}

/// Strip ANSI escape sequences and leading GitHub Actions ISO timestamps, then
/// return the last `lines` non-empty lines.
pub fn log_tail(text: &str, lines: usize) -> String {
    if lines == 0 {
        return String::new();
    }
    let cleaned: Vec<String> = text
        .lines()
        .map(|line| {
            let no_ansi = strip_ansi(line);
            strip_gh_timestamp(&no_ansi).trim_end().to_owned()
        })
        .filter(|l| !l.is_empty())
        .collect();
    let start = cleaned.len().saturating_sub(lines);
    cleaned[start..].join("\n")
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for nc in chars.by_ref() {
                if nc.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn strip_gh_timestamp(s: &str) -> &str {
    // GitHub Actions log lines start with e.g. "2026-11-27T12:34:56.1234567Z "
    if let Some((first, rest)) = s.split_once(' ') {
        let b = first.as_bytes();
        if b.len() >= 20
            && b[4] == b'-'
            && b[7] == b'-'
            && b[10] == b'T'
            && b[13] == b':'
            && b[16] == b':'
            && b[b.len() - 1] == b'Z'
        {
            return rest;
        }
    }
    s
}

/// Extract `"owner/name"` from a GitHub HTTPS URL or `"owner/name"` string.
pub fn owner_repo(s: &str) -> Option<String> {
    let trimmed = s.trim().trim_end_matches('/').trim_end_matches(".git");
    let path = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .unwrap_or(trimmed);
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    if parts.next().is_some() || owner.is_empty() || name.is_empty() {
        return None;
    }
    if owner.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    {
        Some(format!("{owner}/{name}"))
    } else {
        None
    }
}

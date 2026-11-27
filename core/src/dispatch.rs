//! The agent dispatcher: one typed or spoken request becomes tasks, each routed
//! to the AI tool with the most room, each in its own git worktree.
//!
//! "Have Codex fix the failing checkout test, and Claude update the docs"
//! → [Codex: "Fix the failing checkout test"], [Claude Code: "Update the docs"].
//!
//! Parsing is plain rules (no AI call, nothing leaves the machine): a task
//! boundary is a newline, a semicolon, or a comma / "and" / "then" followed by
//! a tool being named ("…, and Claude update the docs"). "Fix login and add
//! tests" stays one task.

use crate::model::Tool;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskPlan {
    pub text: String,
    /// The tool the request named for this task, if any.
    pub tool: Option<Tool>,
}

const VERBS: [&str; 8] = ["have", "ask", "let", "get", "tell", "use", "make", "send"];
const CONNECTORS: [&str; 6] = ["and", "then", "also", "while", "plus", "meanwhile"];
const SUFFIX_PREPS: [&str; 5] = ["in", "with", "using", "on", "via"];

fn bare(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

/// A tool named at `words[i..]`: returns the tool and how many words it took
/// ("claude code" is two).
fn tool_at(words: &[&str], i: usize) -> Option<(Tool, usize)> {
    match bare(words.get(i)?).as_str() {
        "claude" => {
            let code = words.get(i + 1).is_some_and(|w| bare(w) == "code") && !words[i].ends_with([',', ':']);
            Some((Tool::ClaudeCode, if code { 2 } else { 1 }))
        }
        "codex" => Some((Tool::Codex, 1)),
        "gemini" => Some((Tool::Gemini, 1)),
        _ => None,
    }
}

/// "[please] [have|ask|…] <tool>[:|,] [to]" at `i`: the tool and words consumed.
fn directive_at(words: &[&str], i: usize) -> Option<(Tool, usize)> {
    let mut j = i;
    if words.get(j).is_some_and(|w| bare(w) == "please") { j += 1; }
    if words.get(j).is_some_and(|w| VERBS.contains(&bare(w).as_str())) { j += 1; }
    let (tool, n) = tool_at(words, j)?;
    j += n;
    // "Claude and Codex …" names two tools: that's not a directive for one.
    if words.get(j).is_some_and(|w| bare(w) == "and" || bare(w) == "or") { return None; }
    if words.get(j).is_some_and(|w| bare(w) == "to") { j += 1; }
    // Nothing after the tool's name isn't a task.
    (j < words.len()).then_some((tool, j - i))
}

fn tidy(words: &[&str]) -> String {
    let mut s = words.join(" ");
    s = s.trim().trim_end_matches([',', ';']).trim().to_string();
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => s,
    }
}

fn plan_chunk(chunk: &str, out: &mut Vec<TaskPlan>) {
    let words: Vec<&str> = chunk.split_whitespace().collect();
    if words.is_empty() { return; }
    // Where new tasks start: after a comma/full stop or a connector word, when
    // the next words name a tool.
    let mut starts = vec![0];
    for i in 1..words.len() {
        let prev = words[i - 1];
        let joined = prev.ends_with([',', '.', '!', '?']) || CONNECTORS.contains(&bare(prev).as_str());
        // "Claude and Codex …" lists tools; it doesn't hand Codex a new task.
        let listing = i >= 2 && !prev.ends_with([',', '.', '!', '?']) && tool_at(&words, i - 2).is_some();
        if listing { continue; }
        if joined && directive_at(&words, i).is_some() && *starts.last().unwrap() < i {
            starts.push(i);
        }
    }
    starts.push(words.len());
    for pair in starts.windows(2) {
        let mut part: Vec<&str> = words[pair[0]..pair[1]].to_vec();
        // Drop the connector that joined this part to the next.
        while part.last().is_some_and(|w| CONNECTORS.contains(&bare(w).as_str())) { part.pop(); }
        while part.first().is_some_and(|w| CONNECTORS.contains(&bare(w).as_str())) { part.remove(0); }
        let mut tool = None;
        if let Some((t, n)) = directive_at(&part, 0) {
            tool = Some(t);
            part.drain(..n);
        } else if part.len() >= 3 {
            // "… in Codex" / "… with Gemini" at the end.
            let k = part.len();
            for (start, n) in [(k - 2, 1usize), (k.saturating_sub(3), 2)] {
                if SUFFIX_PREPS.contains(&bare(part[start]).as_str()) {
                    if let Some((t, m)) = tool_at(&part, start + 1) {
                        if m == n && start + 1 + m == k {
                            tool = Some(t);
                            part.truncate(start);
                            break;
                        }
                    }
                }
            }
        }
        let text = tidy(&part);
        if text.chars().any(char::is_alphanumeric) {
            out.push(TaskPlan { text, tool });
        }
    }
}

/// Split a request into tasks.
pub fn split(input: &str) -> Vec<TaskPlan> {
    let mut out = Vec::new();
    for chunk in input.split(['\n', ';']) {
        // Bullet lists: "- fix x" / "1. fix x".
        let chunk = chunk.trim().trim_start_matches(['-', '*', '•']).trim();
        let chunk = chunk.split_once(". ").filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).map(|(_, r)| r).unwrap_or(chunk);
        plan_chunk(chunk, &mut out);
    }
    out
}

/// How much room a tool has, for routing.
#[derive(Debug, Clone, Serialize)]
pub struct ToolRoom {
    pub tool: Tool,
    /// Can be started from here (its CLI is installed).
    pub ready: bool,
    /// Percent used of its primary (5-hour) limit, if known.
    pub used_pct: Option<f64>,
    /// The tool you're using right now.
    pub active: bool,
    /// When its primary limit resets, if known (for queueing a task until then).
    pub resets_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Routed {
    pub text: String,
    pub tool: Option<Tool>,
    /// The request named this tool.
    pub explicit: bool,
    pub reason: String,
}

fn name(t: Tool) -> &'static str {
    match t { Tool::ClaudeCode => "Claude Code", Tool::Codex => "Codex", Tool::Gemini => "Gemini" }
}

/// Give each task a tool. A tool the request named wins. Otherwise the ready
/// tool with the most room, counting the tasks already given to it in this
/// batch, so parallel work spreads out instead of draining one limit.
pub fn route(plans: &[TaskPlan], room: &[ToolRoom]) -> Vec<Routed> {
    let mut load: Vec<(Tool, f64)> = room.iter().map(|r| (r.tool, 0.0)).collect();
    let mut out = Vec::new();
    for p in plans {
        if let Some(t) = p.tool {
            let r = room.iter().find(|r| r.tool == t);
            let reason = match r {
                Some(r) if !r.ready => format!("{} isn't installed on this computer", name(t)),
                Some(ToolRoom { used_pct: Some(u), .. }) if *u >= 90.0 => format!("You asked for {} ({}% used)", name(t), u.round()),
                _ => format!("You asked for {}", name(t)),
            };
            if let Some(l) = load.iter_mut().find(|l| l.0 == t) { l.1 += 10.0; }
            out.push(Routed { text: p.text.clone(), tool: Some(t), explicit: true, reason });
            continue;
        }
        let pick = room.iter().filter(|r| r.ready)
            .map(|r| {
                let extra = load.iter().find(|l| l.0 == r.tool).map(|l| l.1).unwrap_or(0.0);
                // Unknown room counts as half full; nearly used-up tools come last.
                let used = r.used_pct.unwrap_or(50.0);
                let score = used + extra + if used >= 90.0 { 1000.0 } else { 0.0 } - if r.active { 0.5 } else { 0.0 };
                (score, r)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        match pick {
            Some((_, r)) => {
                if let Some(l) = load.iter_mut().find(|l| l.0 == r.tool) { l.1 += 10.0; }
                let reason = match r.used_pct {
                    Some(u) if u >= 90.0 => format!("All tools are low; {} has {}% used", name(r.tool), u.round()),
                    Some(u) => format!("Most room: {}% used", u.round()),
                    None => "Ready".to_string(),
                };
                out.push(Routed { text: p.text.clone(), tool: Some(r.tool), explicit: false, reason });
            }
            None => out.push(Routed { text: p.text.clone(), tool: None, explicit: false, reason: "Install Claude Code, Codex or Gemini CLI to run tasks".into() }),
        }
    }
    out
}

/// A short branch-safe name for a task.
pub fn slug(text: &str) -> String {
    let words: Vec<String> = text.split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase())
        .filter(|w| !w.is_empty() && !["the", "a", "an", "to", "of", "and", "for", "in", "on"].contains(&w.as_str()))
        .take(5).collect();
    let mut s = words.join("-");
    s.truncate(40);
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() { "task".into() } else { s }
}

/// Create a worktree for a task next to the repository:
/// `<parent>/<repo>.agents/<slug>` on a new branch `agents/<slug>`, from the
/// current commit. Returns the folder and branch.
pub fn create_worktree(root: &Path, text: &str) -> Result<(PathBuf, String)> {
    let repo = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".into());
    let Some(parent) = root.parent() else { bail!("The repository has no parent folder for worktrees.") };
    let base = slug(text);
    for n in 1..50 {
        let name = if n == 1 { base.clone() } else { format!("{base}-{n}") };
        let dir = parent.join(format!("{repo}.agents")).join(&name);
        let branch = format!("agents/{name}");
        let branch_exists = crate::repo_handoff::git(root, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).is_ok();
        if dir.exists() || branch_exists { continue; }
        let dir_s = dir.to_string_lossy().into_owned();
        crate::repo_handoff::git(root, &["worktree", "add", "--quiet", "-b", &branch, &dir_s])?;
        return Ok((dir, branch));
    }
    bail!("Too many worktrees with that name.")
}

/// Compare folder paths the way the OS does (Windows: case-insensitive, either slash).
pub fn same_folder(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> String {
        let s = s.trim_end_matches(['/', '\\']);
        if cfg!(windows) { s.replace('/', "\\").to_lowercase() } else { s.to_string() }
    }
    norm(a) == norm(b)
}

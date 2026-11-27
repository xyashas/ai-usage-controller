//! What each agent is doing right now, as reported by the tools themselves.
//!
//! Claude Code runs our hook on a few events (installed in
//! `~/.claude/settings.json` when you turn agent status on); Codex calls our
//! `notify` program when a turn finishes. Each call writes one small status
//! file per session: the folder, what happened and when. Never the prompt,
//! the reply or any code.
//!
//! Approvals: on `PermissionRequest`, the hook writes a pending request (the
//! tool and a one-line summary such as the command) and waits a little for the
//! widget to answer. Allow or deny there, or "answer in the terminal" to let
//! Claude Code ask as usual. The file is deleted as soon as it's answered.

use crate::model::Tool;
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The flag our hook command carries, so we can find (and update) our own entries.
pub const HOOK_FLAG: &str = "--aiuc-hook";
/// How long the hook waits for an answer from the widget before Claude Code asks
/// in the terminal.
pub const APPROVAL_WAIT_SECS: u64 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A prompt was sent; the agent is on it.
    Working,
    /// Waiting for you to allow a tool.
    NeedsApproval,
    /// Finished a while ago and waiting for your next message.
    WaitingInput,
    /// Finished its turn.
    Done,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SessionStatus {
    pub session: String,
    pub tool: Tool,
    pub cwd: String,
    pub kind: Kind,
    pub at: DateTime<Utc>,
}

/// A tool call waiting for your OK.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Approval {
    pub id: String,
    pub tool: Tool,
    pub session: String,
    pub cwd: String,
    /// Folder name, for display.
    pub repo_name: String,
    /// E.g. "Bash", "Edit", "WebFetch".
    pub tool_name: String,
    /// One line: the command, file or URL. Shown in the widget, deleted when answered.
    pub summary: String,
    pub at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Allow,
    Deny,
    /// Let Claude Code ask in the terminal as usual.
    Ask,
}

pub fn dir() -> PathBuf {
    crate::paths::app_data_dir().join("agents")
}

fn safe(id: &str) -> String {
    id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(80).collect()
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> Result<()> {
    if let Some(d) = path.parent() { std::fs::create_dir_all(d)?; }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(v)?)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

fn read_dir_json<T: for<'de> Deserialize<'de>>(d: &Path) -> Vec<(PathBuf, T)> {
    let Ok(rd) = std::fs::read_dir(d) else { return vec![] };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| Some((p.clone(), serde_json::from_slice(&std::fs::read(&p).ok()?).ok()?)))
        .collect()
}

pub fn record(base: &Path, s: &SessionStatus) -> Result<()> {
    write_json(&base.join("sessions").join(format!("{}-{}.json", s.tool.as_str(), safe(&s.session))), s)
}

/// Every session reported in the last day, newest first. Older files are removed.
pub fn sessions(base: &Path, now: DateTime<Utc>) -> Vec<SessionStatus> {
    let mut out: Vec<SessionStatus> = read_dir_json::<SessionStatus>(&base.join("sessions"))
        .into_iter()
        .filter_map(|(p, s)| if now - s.at > Duration::hours(24) { let _ = std::fs::remove_file(p); None } else { Some(s) })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.at));
    out
}

/// Tool calls waiting for an answer. Stale ones (the hook gave up) are removed.
pub fn pending(base: &Path, now: DateTime<Utc>) -> Vec<Approval> {
    let mut out: Vec<Approval> = read_dir_json::<Approval>(&base.join("pending"))
        .into_iter()
        .filter_map(|(p, a)| {
            if now - a.at > Duration::seconds(APPROVAL_WAIT_SECS as i64 + 15) { let _ = std::fs::remove_file(p); None } else { Some(a) }
        })
        .collect();
    out.sort_by_key(|a| a.at);
    out
}

/// The widget's answer to a pending request.
pub fn answer(base: &Path, id: &str, a: Answer) -> Result<()> {
    let id = safe(id);
    if !base.join("pending").join(format!("{id}.json")).exists() {
        anyhow::bail!("That request was already answered.");
    }
    write_json(&base.join("answers").join(format!("{id}.json")), &a)
}

fn folder_name(cwd: &str) -> String {
    Path::new(cwd).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| cwd.to_owned())
}

fn clip(s: &str, n: usize) -> String {
    let line = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    if line.chars().count() > n { format!("{}…", line.chars().take(n).collect::<String>()) } else { line.to_owned() }
}

/// One line about what the tool wants to do.
pub fn summarize(tool_name: &str, input: &Value) -> String {
    let s = |k: &str| input.get(k).and_then(Value::as_str);
    let text = match tool_name {
        "Bash" | "PowerShell" => s("command"),
        "Edit" | "Write" | "MultiEdit" | "Read" | "NotebookEdit" => s("file_path").or(s("notebook_path")).map(|p| Path::new(p).file_name().and_then(|n| n.to_str()).unwrap_or(p)),
        "WebFetch" => s("url"),
        "WebSearch" => s("query"),
        _ => s("description").or(s("command")).or(s("url")).or(s("file_path")),
    };
    clip(text.unwrap_or(""), 140)
}

/// The decision Claude Code reads from the hook's stdout.
pub fn decision_json(a: Answer) -> Option<String> {
    let behavior = match a { Answer::Allow => "allow", Answer::Deny => "deny", Answer::Ask => return None };
    let mut decision = serde_json::json!({ "behavior": behavior });
    if a == Answer::Deny { decision["message"] = "Denied from AI Usage Controller.".into(); }
    Some(serde_json::json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": decision } }).to_string())
}

/// Handle one Claude Code hook call. Returns what to print on stdout.
/// `wait` is how long to wait for an answer to a permission request.
pub fn handle_claude(base: &Path, input: &str, wait: std::time::Duration) -> Option<String> {
    let v: Value = serde_json::from_str(input).ok()?;
    let get = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    let (session, cwd, event) = (get("session_id"), get("cwd"), get("hook_event_name"));
    if session.is_empty() { return None; }
    let now = Utc::now();
    let kind = match event.as_str() {
        "UserPromptSubmit" => Some(Kind::Working),
        "Stop" => Some(Kind::Done),
        "Notification" => match get("notification_type").as_str() {
            "permission_prompt" | "elicitation_dialog" | "agent_needs_input" => Some(Kind::NeedsApproval),
            "idle_prompt" => Some(Kind::WaitingInput),
            _ => None,
        },
        "PermissionRequest" => Some(Kind::NeedsApproval),
        _ => None,
    }?;
    let _ = record(base, &SessionStatus { session: session.clone(), tool: Tool::ClaudeCode, cwd: cwd.clone(), kind, at: now });
    if event != "PermissionRequest" || wait.is_zero() { return None; }

    let tool_name = get("tool_name");
    let id = format!("{}-{}", safe(&session).chars().take(12).collect::<String>(), now.timestamp_millis());
    let req = Approval {
        id: id.clone(), tool: Tool::ClaudeCode, session: session.clone(), repo_name: folder_name(&cwd), cwd: cwd.clone(),
        summary: summarize(&tool_name, v.get("tool_input").unwrap_or(&Value::Null)), tool_name, at: now,
    };
    let pending = base.join("pending").join(format!("{id}.json"));
    let answer_file = base.join("answers").join(format!("{id}.json"));
    write_json(&pending, &req).ok()?;
    let start = std::time::Instant::now();
    let mut got = None;
    while start.elapsed() < wait {
        if let Some(a) = std::fs::read(&answer_file).ok().and_then(|b| serde_json::from_slice::<Answer>(&b).ok()) {
            got = Some(a);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    let _ = std::fs::remove_file(&pending);
    let _ = std::fs::remove_file(&answer_file);
    let a = got?;
    // Answered here: back to work (or not) right away.
    let _ = record(base, &SessionStatus { session, tool: Tool::ClaudeCode, cwd, kind: if a == Answer::Allow { Kind::Working } else { Kind::NeedsApproval }, at: Utc::now() });
    decision_json(a)
}

/// Handle Codex's `notify` call (the JSON is its last argument).
pub fn handle_codex(base: &Path, arg: &str) {
    let Ok(v) = serde_json::from_str::<Value>(arg) else { return };
    let get = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    let kind = match get("type").as_str() {
        "agent-turn-complete" => Kind::Done,
        t if t.contains("approval") => Kind::NeedsApproval,
        _ => return,
    };
    let cwd = get("cwd");
    if cwd.is_empty() { return; }
    let session = [get("thread-id"), get("thread_id"), get("session_id")].into_iter().find(|s| !s.is_empty()).unwrap_or_else(|| safe(&cwd));
    let _ = record(base, &SessionStatus { session, tool: Tool::Codex, cwd, kind, at: Utc::now() });
}

// ---------- installing the hooks ----------

/// The hook command for a given program path. Forward slashes work in every
/// shell Claude Code uses, including Git Bash on Windows.
pub fn hook_command(exe: &Path) -> String {
    format!("\"{}\" {HOOK_FLAG} claude", exe.to_string_lossy().replace('\\', "/"))
}

const EVENTS: [(&str, Option<&str>, u64); 4] = [
    ("UserPromptSubmit", None, 10),
    ("Stop", None, 10),
    ("Notification", Some("permission_prompt|idle_prompt|elicitation_dialog|agent_needs_input"), 10),
    ("PermissionRequest", None, APPROVAL_WAIT_SECS + 10),
];

fn ours(h: &Value) -> bool {
    h.get("command").and_then(Value::as_str).is_some_and(|c| c.contains(HOOK_FLAG))
}

/// Add (or update) our hooks in a Claude Code settings object, leaving every
/// other hook alone. `approvals: false` leaves out the permission hook.
pub fn merge_claude_hooks(settings: &mut Value, command: &str, approvals: bool) {
    remove_claude_hooks(settings);
    if !settings.is_object() { *settings = serde_json::json!({}); }
    let Some(root) = settings.as_object_mut() else { return };
    let hooks = root.entry("hooks").or_insert_with(|| serde_json::json!({}));
    if !hooks.is_object() { *hooks = serde_json::json!({}); }
    let Some(hooks) = hooks.as_object_mut() else { return };
    for (event, matcher, timeout) in EVENTS {
        if event == "PermissionRequest" && !approvals { continue; }
        let mut group = serde_json::json!({ "hooks": [{ "type": "command", "command": command, "timeout": timeout }] });
        if let Some(m) = matcher { group["matcher"] = m.into(); }
        let list = hooks.entry(event).or_insert_with(|| serde_json::json!([]));
        if let Some(a) = list.as_array_mut() { a.push(group); } else { *list = serde_json::json!([group]); }
    }
}

/// Take our hooks back out (and any group or event left empty by that).
pub fn remove_claude_hooks(settings: &mut Value) {
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else { return };
    for list in hooks.values_mut() {
        if let Some(groups) = list.as_array_mut() {
            for g in groups.iter_mut() {
                if let Some(hs) = g.get_mut("hooks").and_then(Value::as_array_mut) { hs.retain(|h| !ours(h)); }
            }
            groups.retain(|g| g.get("hooks").and_then(Value::as_array).is_none_or(|h| !h.is_empty()));
        }
    }
    hooks.retain(|_, v| v.as_array().is_none_or(|a| !a.is_empty()));
    if hooks.is_empty() { settings.as_object_mut().map(|o| o.remove("hooks")); }
}

pub fn claude_settings_path() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("CLAUDE_CONFIG_DIR") {
        if let Some(first) = v.split(',').map(str::trim).find(|s| !s.is_empty()) { return Some(PathBuf::from(first).join("settings.json")); }
    }
    crate::paths::home().map(|h| h.join(".claude").join("settings.json"))
}

fn codex_config_path() -> Option<PathBuf> {
    std::env::var("CODEX_HOME").ok().map(PathBuf::from).or_else(|| crate::paths::home().map(|h| h.join(".codex"))).map(|d| d.join("config.toml"))
}

/// Add our `notify` line to a Codex config, unless it already has one of its own.
/// Returns the new text, or None when nothing should change.
pub fn merge_codex_notify(toml: &str, exe: &Path) -> Option<String> {
    let line = format!("notify = [{:?}, {:?}, \"codex\"]", exe.to_string_lossy(), HOOK_FLAG);
    let mut lines: Vec<String> = toml.lines().map(str::to_owned).collect();
    // Top-level keys only: stop at the first [table].
    let top = lines.iter().position(|l| l.trim_start().starts_with('[')).unwrap_or(lines.len());
    if let Some(i) = lines[..top].iter().position(|l| l.trim_start().starts_with("notify")) {
        if !lines[i].contains(HOOK_FLAG) { return None; } // someone else's: leave it
        if lines[i] == line { return None; }
        lines[i] = line;
    } else {
        lines.insert(0, line);
    }
    let mut out = lines.join("\n");
    out.push('\n');
    Some(out)
}

pub fn remove_codex_notify(toml: &str) -> Option<String> {
    if !toml.lines().any(|l| l.trim_start().starts_with("notify") && l.contains(HOOK_FLAG)) { return None; }
    let mut out: String = toml.lines().filter(|l| !(l.trim_start().starts_with("notify") && l.contains(HOOK_FLAG))).collect::<Vec<_>>().join("\n");
    out.push('\n');
    Some(out)
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct InstallResult {
    pub claude: bool,
    pub codex: bool,
    /// Codex already calls another program when a turn ends.
    pub codex_busy: bool,
}

/// Install (or update) the hooks for this program. Idempotent.
pub fn install(exe: &Path, approvals: bool) -> Result<InstallResult> {
    let mut r = InstallResult::default();
    if let Some(p) = claude_settings_path() {
        let mut v: Value = match std::fs::read_to_string(&p) {
            Ok(s) if !s.trim().is_empty() => serde_json::from_str(&s).map_err(|e| anyhow::anyhow!("{} isn't valid JSON ({e}); fix it and try again.", p.display()))?,
            _ => serde_json::json!({}),
        };
        let before = v.clone();
        merge_claude_hooks(&mut v, &hook_command(exe), approvals);
        if v != before {
            if let Some(d) = p.parent() { std::fs::create_dir_all(d)?; }
            std::fs::write(&p, serde_json::to_string_pretty(&v)? + "\n")?;
        }
        r.claude = true;
    }
    if let Some(p) = codex_config_path().filter(|p| p.parent().is_some_and(Path::is_dir)) {
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        match merge_codex_notify(&text, exe) {
            Some(new) => { std::fs::write(&p, new)?; r.codex = true; }
            None => {
                let ours = text.lines().any(|l| l.trim_start().starts_with("notify") && l.contains(HOOK_FLAG));
                r.codex = ours;
                r.codex_busy = !ours && text.lines().any(|l| l.trim_start().starts_with("notify"));
            }
        }
    }
    Ok(r)
}

pub fn uninstall() -> Result<()> {
    if let Some(p) = claude_settings_path() {
        if let Ok(s) = std::fs::read_to_string(&p) {
            if let Ok(mut v) = serde_json::from_str::<Value>(&s) {
                let before = v.clone();
                remove_claude_hooks(&mut v);
                if v != before { std::fs::write(&p, serde_json::to_string_pretty(&v)? + "\n")?; }
            }
        }
    }
    if let Some(p) = codex_config_path() {
        if let Some(new) = std::fs::read_to_string(&p).ok().and_then(|t| remove_codex_notify(&t)) { std::fs::write(&p, new)?; }
    }
    Ok(())
}

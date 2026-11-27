//! "Ask the notch": quick questions answered by whichever AI subscription has
//! room, through the CLIs the user already has (no API keys).
//!
//! This module is the pure part: what context to send, which tool answers,
//! the command line for each CLI, reading their streamed output, Council
//! (all three, then a combined answer) and arrows to point at on screen.
//! The app runs the processes (app/src-tauri/src/ask.rs).
//!
//! Privacy: questions, answers and screenshots live in memory (and a temp file
//! for the screenshot, deleted after); nothing here is stored or synced.

use crate::model::Tool;
use crate::snapshot::Snapshot;
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Fast model, short answers.
    #[default]
    Quick,
    /// The strongest model.
    Deep,
}

/// One-tap actions. `Ask` is a free question.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    #[default]
    Ask,
    Explain,
    Fix,
    FindBug,
    Rewrite,
    Shorter,
    Professional,
    Translate,
    Reply,
    Summarize,
    /// Where to click: the answer carries arrows to draw on screen.
    Point,
}

impl Action {
    /// The answer replaces the selected text, so it must be only that text.
    pub fn is_rewrite(self) -> bool {
        matches!(self, Action::Rewrite | Action::Shorter | Action::Professional | Action::Translate | Action::Reply)
    }

    fn instruction(self) -> &'static str {
        match self {
            Action::Ask => "",
            Action::Explain => "Explain this clearly and briefly: what it is, what it means, and anything worth knowing.",
            Action::Fix => "Find what's wrong and give the fix. Lead with the cause in one sentence, then the corrected code or the exact steps.",
            Action::FindBug => "Look for bugs. List each real problem with the line and a one-line fix, most serious first. Say so if you find none.",
            Action::Rewrite => "Rewrite the selected text so it reads better. Keep the meaning, language and roughly the length.",
            Action::Shorter => "Make the selected text shorter. Keep every point that matters.",
            Action::Professional => "Rewrite the selected text in a clear, friendly, professional tone.",
            Action::Translate => "Translate the selected text. Into English if it isn't English; otherwise into the language the user asks for (default Spanish).",
            Action::Reply => "Draft a reply to the selected message. Match its tone and language, and keep it short.",
            Action::Summarize => "Summarize this in a few bullet points.",
            Action::Point => "",
        }
    }
}

/// Everything the notch knows when you press the hotkey.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Context {
    /// Text selected in the app you were in.
    pub selection: Option<String>,
    /// Text you copied in the last couple of minutes.
    pub clipboard: Option<String>,
    /// The app and window you were in, e.g. "Visual Studio Code — checkout.ts".
    pub window: Option<String>,
    /// A screenshot (PNG) of that window or the screen, and its size in pixels.
    pub screenshot: Option<PathBuf>,
    pub screenshot_size: Option<(u32, u32)>,
    /// Files you dropped on the notch.
    pub files: Vec<PathBuf>,
    /// The repo you're working in, if known.
    pub repo: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Turn {
    pub question: String,
    pub answer: String,
}

fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max { return s.to_owned(); }
    format!("{}\n[…cut, {} more characters]", s.chars().take(max).collect::<String>(), s.chars().count() - max)
}

fn local_time(t: DateTime<Utc>) -> String {
    t.with_timezone(&Local).format("%-I:%M %p").to_string()
}

fn tool_label(t: Tool) -> &'static str {
    match t { Tool::ClaudeCode => "Claude", Tool::Codex => "Codex", Tool::Gemini => "Gemini" }
}

/// Words that mean the question is about the user's own AI work.
pub fn about_work(q: &str) -> bool {
    let q = q.to_lowercase();
    ["agent", "limit", "reset", "test", "today", "yesterday", "this week", "usage", "quota", "room", "claude", "codex", "gemini", "task", "handoff", "done", "doing", "spent", "cost", "refactor", "before my"]
        .iter().any(|w| q.contains(w))
}

/// A short, factual brief of the user's AI work right now, from the snapshot.
/// `failures` are the ends of failed test runs, by task id (kept in memory by the app).
pub fn work_brief(snap: &Snapshot, failures: &[(i64, String)], now: DateTime<Utc>) -> String {
    let mut out = format!("Now: {}.\n", now.with_timezone(&Local).format("%a %-I:%M %p"));
    out.push_str("Limits (percent used):\n");
    for t in snap.tools.iter().filter(|t| t.enabled && t.detected) {
        let windows: Vec<String> = t.windows.iter().map(|w| {
            let reset = w.resets_at.map(|r| format!(", resets {}", local_time(r))).unwrap_or_default();
            format!("{} {:.0}%{reset}", w.label, w.used_pct)
        }).collect();
        let windows = if windows.is_empty() { "unknown".to_owned() } else { windows.join("; ") };
        out.push_str(&format!("- {}{}: {windows}. Today {} requests, ${:.2}.\n",
            t.label, t.plan_name.as_ref().map(|p| format!(" ({p})")).unwrap_or_default(), t.today_requests, t.today_cost_usd));
    }
    if !snap.agents.is_empty() {
        out.push_str("Agent tasks (last 24 h):\n");
        for a in snap.agents.iter().take(12) {
            let mins = (now - a.started_at).num_minutes().max(0);
            let tests = match a.verify.as_deref() {
                Some("passed") => ", tests pass",
                Some("failed") => ", tests FAIL",
                Some("running") => ", tests running",
                Some("timed_out") => ", tests timed out",
                _ => "",
            };
            let queued = a.queued_until.map(|q| format!(", queued until {}", local_time(q))).unwrap_or_default();
            out.push_str(&format!("- #{} {}: \"{}\" in {}{} — {}{tests}{queued}, {mins} min, ${:.2}\n",
                a.id, tool_label(a.tool), clip(&a.text, 120), a.repo_name,
                a.branch.as_ref().map(|b| format!(" ({b})")).unwrap_or_default(), a.state.replace('_', " "), a.cost_usd));
        }
    }
    for (id, tail) in failures.iter().take(2) {
        out.push_str(&format!("Failed test output for task #{id} (end):\n{}\n", clip(tail, 2500)));
    }
    out.push_str(&format!("Today across tools: ${:.2}, {} sessions.\n", snap.today.cost_usd, snap.today.sessions));
    out
}

/// The full prompt sent to the CLI.
pub fn build_prompt(question: &str, action: Action, ctx: &Context, history: &[Turn], work: Option<&str>) -> String {
    let mut p = String::from(
        "You are a helpful assistant that lives in the user's menu bar (\"the notch\"). Answer the question directly and concisely, \
         in plain language with short paragraphs or bullets. Use Markdown. Put code and commands in fenced code blocks. \
         Don't run tools or change any files; just answer.\n",
    );
    if action.is_rewrite() {
        p.push_str("Reply with ONLY the new text: no preamble, no quotes, no explanation, no code fences.\n");
    }
    if action == Action::Point {
        p.push_str(&point_instructions(ctx.screenshot_size));
    }
    if let Some(w) = work {
        p.push_str("\n<my_ai_work>\nFacts about the user's AI coding tools, tracked by the app (use them when relevant; times are local):\n");
        p.push_str(w);
        p.push_str("</my_ai_work>\n");
    }
    if let Some(w) = &ctx.window { p.push_str(&format!("\nThe user was in: {}\n", clip(w, 200))); }
    if let Some(r) = &ctx.repo { p.push_str(&format!("Their current repo: {r}\n")); }
    if let Some(s) = &ctx.selection { p.push_str(&format!("\n<selected_text>\n{}\n</selected_text>\n", clip(s, 12000))); }
    if let Some(c) = &ctx.clipboard { p.push_str(&format!("\n<copied_text>\n{}\n</copied_text>\n", clip(c, 8000))); }
    if let Some(img) = &ctx.screenshot {
        p.push_str(&format!("\nA screenshot of what they're looking at is at {} — look at it to understand \"this\" or \"here\".\n", img.display()));
    }
    for f in &ctx.files { p.push_str(&format!("Attached file: {}\n", f.display())); }
    if !history.is_empty() {
        p.push_str("\n<conversation_so_far>\n");
        for t in history.iter().rev().take(4).rev() {
            p.push_str(&format!("User: {}\nYou: {}\n\n", clip(&t.question, 2000), clip(&t.answer, 3000)));
        }
        p.push_str("</conversation_so_far>\n");
    }
    let instruction = action.instruction();
    let q = question.trim();
    p.push_str("\nRequest: ");
    match (instruction.is_empty(), q.is_empty()) {
        (false, true) => p.push_str(instruction),
        (false, false) => p.push_str(&format!("{instruction}\n{q}")),
        (true, _) => p.push_str(if q.is_empty() { "What is this, and what should I do about it?" } else { q }),
    }
    p.push('\n');
    p
}

// ---------- which tool answers ----------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Room {
    pub tool: Tool,
    /// Its CLI is installed, so it can answer here.
    pub installed: bool,
    /// Percent used of its tightest limit, if known.
    pub used_pct: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Pick {
    pub tool: Tool,
    /// Why, in a few words: "most room", "you picked it", "Claude is at 96%".
    pub reason: String,
}

/// The installed tool with the most room. A pinned tool wins unless it's out.
pub fn pick(rooms: &[Room], pinned: Option<Tool>) -> Option<Pick> {
    let ok: Vec<&Room> = rooms.iter().filter(|r| r.installed).collect();
    if let Some(p) = pinned {
        if let Some(r) = ok.iter().find(|r| r.tool == p) {
            if r.used_pct.is_none_or(|u| u < 98.0) { return Some(Pick { tool: p, reason: "your pick".into() }); }
        }
    }
    let best = ok.iter().min_by(|a, b| a.used_pct.unwrap_or(50.0).total_cmp(&b.used_pct.unwrap_or(50.0)))?;
    let full: Vec<String> = rooms.iter().filter(|r| r.tool != best.tool && r.used_pct.is_some_and(|u| u >= 80.0))
        .map(|r| format!("{} is at {:.0}%", tool_label(r.tool), r.used_pct.unwrap())).collect();
    let reason = if !full.is_empty() { full.join(", ") }
        else if pinned.is_some_and(|p| p != best.tool) { format!("{} is out of room", tool_label(pinned.unwrap())) }
        else if ok.len() == 1 { "the only one installed".into() }
        else { "most room".into() };
    Some(Pick { tool: best.tool, reason })
}

// ---------- the command line ----------

pub struct Cmd {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Sent on stdin.
    pub stdin: String,
}

/// How to ask each CLI one question, non-interactively and read-only, with
/// output we can stream.
pub fn command(tool: Tool, cli: &Path, mode: Mode, prompt: &str, image: Option<&Path>) -> Cmd {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    match tool {
        Tool::ClaudeCode => {
            let mut args = s(&["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages",
                "--model", if mode == Mode::Quick { "haiku" } else { "opus" }]);
            // Reading the screenshot is the only tool it needs.
            args.extend(s(&["--allowedTools", "Read", "--disallowedTools", "Bash,Edit,Write,NotebookEdit,WebFetch,WebSearch"]));
            Cmd { program: cli.into(), args, stdin: prompt.into() }
        }
        Tool::Codex => {
            let mut args = s(&["exec", "--json", "--skip-git-repo-check", "--sandbox", "read-only", "-c",
                if mode == Mode::Quick { "model_reasoning_effort=\"low\"" } else { "model_reasoning_effort=\"high\"" }]);
            if let Some(i) = image { args.push("-i".into()); args.push(i.to_string_lossy().into()); }
            args.push("-".into());
            Cmd { program: cli.into(), args, stdin: prompt.into() }
        }
        Tool::Gemini => {
            // Gemini adds stdin to the -p prompt; @path attaches the screenshot.
            let lead = match image { Some(i) => format!("@{} Answer the request below.", i.display()), None => "Answer the request below.".into() };
            // No model name: Google retires them often, and the CLI's default
            // follows along. (Quick vs Deep only changes the prompt here.)
            let _ = mode;
            Cmd { program: cli.into(), args: s(&["-p", &lead]), stdin: prompt.into() }
        }
    }
}

/// Reads one CLI's output line by line and returns the text to add.
#[derive(Default)]
pub struct StreamReader {
    /// Claude: text already shown from partial deltas, so the final message
    /// isn't added twice.
    streamed: bool,
    /// Codex: deltas seen for the current message.
    codex_delta: bool,
    pub error: Option<String>,
}

impl StreamReader {
    pub fn line(&mut self, tool: Tool, line: &str) -> Option<String> {
        match tool {
            Tool::Gemini => Some(format!("{line}\n")),
            Tool::ClaudeCode => {
                let v: Value = serde_json::from_str(line).ok()?;
                match v.get("type").and_then(Value::as_str)? {
                    "stream_event" => {
                        let d = v.pointer("/event/delta")?;
                        if d.get("type").and_then(Value::as_str) == Some("text_delta") {
                            self.streamed = true;
                            return d.get("text").and_then(Value::as_str).map(str::to_owned);
                        }
                        None
                    }
                    "assistant" if !self.streamed => {
                        let text: String = v.pointer("/message/content")?.as_array()?.iter()
                            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                            .filter_map(|b| b.get("text").and_then(Value::as_str)).collect();
                        (!text.is_empty()).then_some(text)
                    }
                    "assistant" => { self.streamed = false; Some("\n\n".into()) }
                    "result" => {
                        if v.get("is_error").and_then(Value::as_bool) == Some(true) {
                            self.error = v.get("result").and_then(Value::as_str).map(str::to_owned).or(Some("Claude couldn't answer.".into()));
                        }
                        None
                    }
                    _ => None,
                }
            }
            Tool::Codex => {
                let v: Value = serde_json::from_str(line).ok()?;
                // Newer format: {"type":"item.completed","item":{"type":"agent_message","text":…}}
                if let Some(item) = v.get("item") {
                    let kind = item.get("type").and_then(Value::as_str);
                    if v.get("type").and_then(Value::as_str) == Some("item.completed") && matches!(kind, Some("agent_message" | "assistant_message")) {
                        return item.get("text").and_then(Value::as_str).map(|t| format!("{t}\n"));
                    }
                    return None;
                }
                if v.get("type").and_then(Value::as_str) == Some("error") || v.get("type").and_then(Value::as_str) == Some("turn.failed") {
                    self.error = v.pointer("/error/message").or(v.get("message")).and_then(Value::as_str).map(str::to_owned);
                    return None;
                }
                // Older format: {"msg":{"type":"agent_message_delta","delta":…}} / agent_message.
                let msg = v.get("msg")?;
                match msg.get("type").and_then(Value::as_str)? {
                    "agent_message_delta" => { self.codex_delta = true; msg.get("delta").and_then(Value::as_str).map(str::to_owned) }
                    "agent_message" if !self.codex_delta => msg.get("message").and_then(Value::as_str).map(|t| format!("{t}\n")),
                    "agent_message" => { self.codex_delta = false; None }
                    "error" => { self.error = msg.get("message").and_then(Value::as_str).map(str::to_owned); None }
                    _ => None,
                }
            }
        }
    }
}

// ---------- when a tool can't answer ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    /// Not signed in / no API key.
    SignedOut,
    /// Out of its limit or rate-limited.
    OutOfRoom,
    /// The provider no longer supports this way of using it (e.g. Gemini CLI
    /// with a personal Google login since Google moved people to Antigravity).
    Unsupported,
    Other,
}

/// What kind of failure a CLI's error is, so another tool can step in.
pub fn classify(error: &str) -> Failure {
    let e = error.to_lowercase();
    if ["no longer supported", "not supported", "migrate to", "deprecated", "antigravity"].iter().any(|w| e.contains(w)) {
        return Failure::Unsupported;
    }
    if ["api key", "api_key", "apikey", "not logged in", "log in", "login", "sign in", "auth", "credential", "unauthorized", "401", "403"].iter().any(|w| e.contains(w)) {
        Failure::SignedOut
    } else if ["usage limit", "rate limit", "quota", "429", "limit reached", "too many requests", "exceeded"].iter().any(|w| e.contains(w)) {
        Failure::OutOfRoom
    } else {
        Failure::Other
    }
}

/// A plain sentence for the user, with the fix.
pub fn explain_failure(tool: Tool, f: Failure) -> String {
    let name = tool_label(tool);
    match (tool, f) {
        (Tool::Gemini, Failure::SignedOut) => "Gemini isn't signed in. Run `gemini` in a terminal once and choose Login with Google.".into(),
        (Tool::ClaudeCode, Failure::SignedOut) => "Claude Code isn't signed in. Run `claude` in a terminal once and log in.".into(),
        (Tool::Codex, Failure::SignedOut) => "Codex isn't signed in. Run `codex` in a terminal once and sign in with ChatGPT.".into(),
        (Tool::Gemini, Failure::Unsupported) => "Google no longer lets the Gemini CLI use a personal Google login, so Gemini is skipped. To use it, set a free Gemini API key from aistudio.google.com as GEMINI_API_KEY.".into(),
        (_, Failure::Unsupported) => format!("{name} no longer works this way, so it's skipped."),
        (_, Failure::OutOfRoom) => format!("{name} is out of room right now."),
        (_, Failure::Other) => format!("{name} couldn't answer."),
    }
}

// ---------- Council ----------

/// The combined answer: where they agree, where they differ, the best answer.
pub fn council_prompt(question: &str, answers: &[(Tool, String)]) -> String {
    let mut p = String::from(
        "Three AI assistants answered the same question. Compare them for the user.\n\
         Reply in Markdown with exactly these sections:\n\
         **Agree** — what they all say (bullets).\n\
         **Differ** — where they disagree and who's more likely right, with a short reason (bullets; write \"Nothing important\" if none).\n\
         **Best answer** — the single best combined answer to the question.\n\
         Be concise.\n\n",
    );
    p.push_str(&format!("<question>\n{}\n</question>\n", clip(question, 4000)));
    for (t, a) in answers {
        p.push_str(&format!("\n<answer from=\"{}\">\n{}\n</answer>\n", tool_label(*t), clip(a, 6000)));
    }
    p
}

// ---------- pointing ----------

/// A spot to draw an arrow at, in screenshot pixels.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    pub label: String,
}

fn point_instructions(size: Option<(u32, u32)>) -> String {
    let dims = size.map(|(w, h)| format!(" The screenshot is {w}×{h} pixels; (0,0) is its top-left corner.")).unwrap_or_default();
    format!(
        "The user wants to know where to click. Look at the screenshot carefully.{dims} \
         Give short numbered steps. For each step, add a marker on its own line exactly like \
         <point x=\"120\" y=\"340\" label=\"Settings\"/> at the centre of the thing to click, in screenshot pixels. \
         Only add a marker when the thing is visible in the screenshot; for later steps that aren't visible yet, describe them without a marker.\n"
    )
}

/// The markers in an answer, and the answer without them.
pub fn parse_points(answer: &str) -> (Vec<Point>, String) {
    let mut points = Vec::new();
    let mut text = String::with_capacity(answer.len());
    let mut rest = answer;
    while let Some(start) = rest.find("<point") {
        text.push_str(&rest[..start]);
        let Some(end) = rest[start..].find("/>").map(|e| start + e + 2) else { text.push_str(&rest[start..]); rest = ""; break };
        let tag = &rest[start..end];
        let attr = |name: &str| -> Option<String> {
            let k = format!("{name}=\"");
            let i = tag.find(&k)? + k.len();
            Some(tag[i..i + tag[i..].find('"')?].to_owned())
        };
        if let (Some(x), Some(y)) = (attr("x").and_then(|v| v.trim().parse().ok()), attr("y").and_then(|v| v.trim().parse().ok())) {
            points.push(Point { x, y, label: attr("label").unwrap_or_default() });
        }
        rest = &rest[end..];
    }
    text.push_str(rest);
    // Tidy lines left empty by removed markers.
    let cleaned = text.lines().map(str::trim_end).collect::<Vec<_>>().join("\n");
    let mut out = String::new();
    for line in cleaned.split('\n') {
        if line.is_empty() && out.ends_with("\n\n") { continue; }
        out.push_str(line);
        out.push('\n');
    }
    (points, out.trim().to_owned())
}

/// The first fenced code block, for "Run" (a command) or "Copy code".
pub fn first_code_block(answer: &str) -> Option<String> {
    let start = answer.find("```")?;
    let after = &answer[start + 3..];
    let body_start = after.find('\n')? + 1;
    let end = after[body_start..].find("```")?;
    Some(after[body_start..body_start + end].trim_end().to_owned())
}

/// Does this look like a question about where to click?
pub fn wants_pointing(q: &str) -> bool {
    let q = q.to_lowercase();
    ["where do i", "where is", "where's", "where can i", "how do i turn", "how do i find", "how do i open", "how do i change", "show me where", "which button", "where to click"]
        .iter().any(|w| q.contains(w))
}

use crate::model::Tool;
use anyhow::{anyhow, ensure, Result};
use serde_json::Value;
use std::{fs::{self, File}, io::{BufRead, BufReader, Write}, path::{Path, PathBuf}};
use walkdir::WalkDir;

pub fn executable(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    #[cfg(windows)]
    let names = [format!("{name}.exe"), format!("{name}.cmd")];
    #[cfg(not(windows))]
    let names = [name.to_owned()];
    for dir in std::env::split_paths(&path).filter(|p| p.is_absolute()) {
        for name in &names {
            let candidate = dir.join(name);
            let Ok(meta) = candidate.metadata() else { continue };
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if meta.permissions().mode() & 0o111 == 0 { continue; }
            }
            if meta.is_file() { return Some(candidate); }
        }
    }
    None
}

pub fn cli(tool: Tool) -> Option<PathBuf> {
    executable(if tool == Tool::ClaudeCode { "claude" } else { tool.as_str() })
}

#[derive(Default)]
struct Summary {
    cwd: Option<PathBuf>,
    original: String,
    requests: Vec<String>,
    files: Vec<String>,
    errors: Vec<String>,
    done: Vec<String>,
    todos: Vec<String>,
}

fn excerpt(s: &str) -> String {
    s.split_whitespace().take(75).collect::<Vec<_>>().join(" ").chars().take(650).collect()
}

fn recent(list: &mut Vec<String>, s: &str, max: usize) {
    let s = excerpt(s);
    if s.is_empty() { return; }
    list.retain(|item| item != &s);
    list.push(s);
    if list.len() > max { list.remove(0); }
}

fn string<'a>(v: &'a Value, key: &str) -> &'a str { v[key].as_str().unwrap_or("") }

impl Summary {
    fn reply(&mut self, text: &str) {
        recent(&mut self.done, text, 3);
        for line in text.lines() {
            let line = line.trim();
            if let Some(todo) = line.strip_prefix("- [ ] ").or_else(|| line.strip_prefix("TODO: ")) {
                recent(&mut self.todos, todo, 6);
            }
        }
    }

    fn request(&mut self, s: &str) {
        if self.original.is_empty() { self.original = excerpt(s); }
        else { recent(&mut self.requests, s, 3); }
    }

    fn directory(&mut self, v: &Value) {
        if let Some(path) = v["cwd"].as_str() { self.cwd = Some(path.into()); }
    }

    fn claude(&mut self, v: &Value, session: &str) {
        if v["sessionId"].as_str() != Some(session) || v["isSidechain"] == true || v["isMeta"] == true { return; }
        let user = v["type"] == "user";
        if !user && v["type"] != "assistant" { return; }
        self.directory(v);
        let content = &v["message"]["content"];
        if let Some(s) = content.as_str() {
            if user { self.request(s); } else { self.reply(s); }
        }
        for b in content.as_array().into_iter().flatten() {
            match (user, string(b,"type")) {
                (true, "text") => self.request(string(b,"text")),
                (false, "text") => self.reply(string(b,"text")),
                (true, "tool_result") if b["is_error"] == true => {
                    recent(&mut self.errors, string(b,"content"), 3);
                    for part in b["content"].as_array().into_iter().flatten() {
                        recent(&mut self.errors, string(part,"text"), 3);
                    }
                }
                (false, "tool_use") => {
                    let input = &b["input"];
                    match string(b,"name") {
                        "Edit" | "Write" | "MultiEdit" => recent(&mut self.files, string(input,"file_path"), 8),
                        "NotebookEdit" => recent(&mut self.files, string(input,"notebook_path"), 8),
                        "TodoWrite" => {
                            self.todos.clear();
                            for item in input["todos"].as_array().into_iter().flatten().filter(|i| i["status"] != "completed") {
                                recent(&mut self.todos, string(item,"content"), 6);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    fn codex(&mut self, v: &Value) {
        let p = &v["payload"];
        let name = string(p, "name").rsplit('.').next().unwrap_or("");
        match (string(v,"type"), string(p,"type")) {
            ("session_meta" | "turn_context", _) => self.directory(p),
            ("event_msg", "user_message") => self.request(string(p,"message")),
            ("event_msg", "agent_message") => self.reply(string(p,"message")),
            ("response_item", "message") if p["role"] == "assistant" => {
                for b in p["content"].as_array().into_iter().flatten() {
                    self.reply(string(b,"text"));
                }
            }
            ("response_item", "custom_tool_call" | "function_call") if name == "apply_patch" => {
                let args: Value = serde_json::from_str(string(p,"arguments")).unwrap_or_default();
                let patch = p["input"].as_str().or(args["patch"].as_str()).unwrap_or("");
                for line in patch.lines() {
                    for prefix in ["*** Add File: ", "*** Update File: ", "*** Move to: "] {
                        if let Some(path) = line.strip_prefix(prefix) { recent(&mut self.files, path, 8); }
                    }
                }
            }
            ("response_item", "function_call") if name == "update_plan" => {
                if let Ok(args) = serde_json::from_str::<Value>(string(p,"arguments")) {
                    self.todos.clear();
                    for item in args["plan"].as_array().into_iter().flatten().filter(|i| i["status"] != "completed") {
                        recent(&mut self.todos, string(item,"step"), 6);
                    }
                }
            }
            ("event_msg", "patch_apply_end") if p["success"] == true => {
                for (path, change) in p["changes"].as_object().into_iter().flatten() {
                    if matches!(string(change,"type"), "add" | "update") {
                        recent(&mut self.files, change["move_path"].as_str().unwrap_or(path), 8);
                    }
                }
            }
            ("event_msg", "exec_command_end") if p["exit_code"].as_i64().is_some_and(|n| n != 0) => {
                recent(&mut self.errors, string(p,"aggregated_output"), 3);
            }
            ("response_item", "function_call_output") => {
                let out = string(p,"output");
                let structured = serde_json::from_str::<Value>(out).ok();
                let code = structured.as_ref().and_then(|v| v["exit_code"].as_i64().or(v["metadata"]["exit_code"].as_i64()));
                let failed = out.lines().any(|line| ["Process exited with code ", "Exit code: "].iter()
                    .any(|prefix| line.strip_prefix(prefix).and_then(|s| s.trim().parse::<i32>().ok()).is_some_and(|n| n != 0)));
                if failed || code.is_some_and(|n| n != 0) {
                    recent(&mut self.errors, structured.as_ref().and_then(|v| v["output"].as_str()).unwrap_or(out), 3);
                }
            }
            _ => {}
        }
    }

    fn render(self) -> String {
        let mut prompt = String::from("# Session handoff");
        for (title, entries) in [
            ("Original goal", vec![self.original]), ("Latest requests", self.requests),
            ("Files created or edited", self.files), ("Recent errors", self.errors),
            ("Done so far", self.done), ("TODO", self.todos),
        ] {
            prompt.push_str(&format!("\n\n## {title}\n"));
            let mut budget = 220;
            let mut empty = true;
            for entry in entries.iter().filter(|e| !e.is_empty()) {
                let words: Vec<_> = entry.split_whitespace().take(budget).collect();
                budget -= words.len();
                empty = false;
                prompt.push_str(if title == "TODO" { "- [ ] " } else { "- " });
                prompt.push_str(&words.join(" "));
                prompt.push('\n');
                if budget == 0 { break; }
            }
            if empty { prompt.push_str("None recorded.\n"); }
        }
        prompt
    }
}

pub fn prepare(roots: &[PathBuf], source: Tool, session: &str) -> Result<(PathBuf, String)> {
    let suffix = format!("-{session}.jsonl");
    let filename = format!("{session}.jsonl");
    let mut paths = roots.iter().flat_map(|root| WalkDir::new(root).max_depth(if source == Tool::Codex { 5 } else { 2 }))
        .filter_map(Result::ok).filter(|e| e.file_type().is_file())
        .filter(|e| e.file_name().to_str().is_some_and(|n| match source {
            Tool::Codex => n.starts_with("rollout-") && n.ends_with(&suffix),
            Tool::ClaudeCode => n == filename,
            Tool::Gemini => false,
        })).map(|e| e.into_path()).collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    ensure!(paths.len() == 1, "Could not identify the selected session log.");
    let mut summary = Summary::default();
    let mut matched = false;
    for line in BufReader::new(File::open(&paths[0])?).lines() {
        let Ok(v) = serde_json::from_str::<Value>(&line?) else { continue };
        if source == Tool::ClaudeCode { summary.claude(&v, session); }
        else {
            if v["type"] == "session_meta" {
                ensure!(v["payload"]["id"].as_str() == Some(session), "Session log changed.");
                matched = true;
            }
            if matched { summary.codex(&v); }
        }
    }
    let cwd = summary.cwd.take().filter(|p| p.is_absolute() && p.is_dir()).ok_or_else(|| anyhow!("Project folder unavailable."))?;
    Ok((cwd, summary.render()))
}

pub const FILENAME: &str = ".aiuc-handoff.md";

pub fn save(cwd: &Path, prompt: &str) -> Result<()> {
    ensure!(!prompt.trim().is_empty() && prompt.len() <= 100_000 && prompt.split_whitespace().count() < 1500, "Use a nonempty handoff under 1,500 words and 100 KB.");
    let ignore = cwd.join(".gitignore");
    match fs::symlink_metadata(&ignore) {
        Ok(meta) => {
            ensure!(meta.file_type().is_file(), "Cannot update this .gitignore.");
            let contents = fs::read(&ignore)?;
            if !contents.split(|b| *b == b'\n').map(|s| s.strip_suffix(b"\r").unwrap_or(s)).any(|s| s == b"/.aiuc-handoff.md" || s == b".aiuc-handoff.md") {
                let mut file = fs::OpenOptions::new().append(true).open(ignore)?;
                let newline: &[u8] = if contents.windows(2).any(|s| s == b"\r\n") { b"\r\n" } else { b"\n" };
                if !contents.is_empty() && !contents.ends_with(b"\n") { file.write_all(newline)?; }
                file.write_all(b"/.aiuc-handoff.md")?;
                file.write_all(newline)?;
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut file = tempfile::NamedTempFile::new_in(cwd)?;
    file.write_all(prompt.as_bytes())?;
    file.persist(cwd.join(FILENAME))?;
    Ok(())
}

//! Build a clipboard handoff on demand from the selected main conversation.
use anyhow::{anyhow, ensure, Result};
use serde_json::Value;
use std::{fs::File, io::{BufRead, BufReader}, path::PathBuf};
use walkdir::WalkDir;

fn remember(items: &mut Vec<String>, text: Option<&str>, count: usize, width: usize) {
    if let Some(text) = text.map(str::trim).filter(|s| !s.is_empty()) {
        let excerpt: String = text.chars().take(width).collect();
        items.retain(|item| item != &excerpt);
        items.push(excerpt);
        if items.len() > count { items.remove(0); }
    }
}

pub fn prepare(roots: &[PathBuf], session: &str) -> Result<(PathBuf, String)> {
    let filename = format!("{session}.jsonl");
    let mut paths = roots.iter().flat_map(|root| WalkDir::new(root).max_depth(2))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.file_name().to_str() == Some(filename.as_str()))
        .map(|e| e.into_path()).collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    ensure!(paths.len() == 1, "Could not identify the active Claude Code log.");
    let file = File::open(&paths[0]).map_err(|_| anyhow!("Could not open the active Claude Code log."))?;
    let (mut goals, mut files, mut todos, mut errors) = (vec![], vec![], vec![], vec![]);
    let mut cwd = None;
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|_| anyhow!("Could not read the Claude Code log."))?;
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if v["sessionId"].as_str() != Some(session) || v["isMeta"] == true || v["isSidechain"] == true { continue; }
        let user = v["type"] == "user";
        if !user && v["type"] != "assistant" { continue; }
        if let Some(dir) = v["cwd"].as_str() { cwd = Some(PathBuf::from(dir)); }
        let content = &v["message"]["content"];
        if user { remember(&mut goals, content.as_str(), 3, 600); }
        for b in content.as_array().into_iter().flatten() {
            match (user, b["type"].as_str()) {
                (true, Some("text")) => remember(&mut goals, b["text"].as_str(), 3, 600),
                (true, Some("tool_result")) if b["is_error"] == true => {
                    remember(&mut errors, b["content"].as_str(), 3, 300);
                    for part in b["content"].as_array().into_iter().flatten() {
                        remember(&mut errors, part["text"].as_str(), 3, 300);
                    }
                }
                (false, Some("tool_use")) => {
                    let input = &b["input"];
                    remember(&mut files, input["file_path"].as_str(), 8, 200);
                    if b["name"] == "TodoWrite" {
                        todos.clear();
                        for item in input["todos"].as_array().into_iter().flatten() {
                            if item["status"] != "completed" {
                                remember(&mut todos, item["content"].as_str(), 6, 200);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let cwd = cwd.filter(|p| p.is_absolute() && p.is_dir())
        .ok_or_else(|| anyhow!("The session's project folder is unavailable."))?;
    let mut prompt = String::from("Continue this Claude Code session in the current project.");
    for (label, items) in [("Recent user goals", goals), ("Files touched", files), ("Open TODOs", todos), ("Recent errors", errors)] {
        if !items.is_empty() { prompt.push_str(&format!("\n\n{label}\n- {}", items.join("\n- "))); }
    }
    Ok((cwd, prompt))
}

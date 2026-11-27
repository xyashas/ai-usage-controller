//! Incremental log scanner. JSONL files are read from the last byte offset;
//! only complete lines are consumed. Gemini's whole-file JSON is re-parsed when
//! its size or mtime changes (dedup keys make that idempotent).
//!
//! Polling every few seconds is deliberate: it behaves identically on Mac and
//! Windows, survives log rotation, and costs microseconds when nothing changed.

use crate::model::{ParsedItem, Tool};
use crate::parsers::{claude, codex, gemini};
use crate::pricing::PriceTable;
use crate::store::{Cursor, Store};
use anyhow::Result;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

/// Files older than this are skipped on first sight (keeps the first scan fast).
pub const INITIAL_LOOKBACK_DAYS: u64 = 45;

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct ScanReport {
    pub files_seen: usize,
    pub files_read: usize,
    pub events: usize,
    pub gauges: usize,
}

pub fn candidate_files(tool: Tool, roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let cutoff = SystemTime::now() - Duration::from_secs(INITIAL_LOOKBACK_DAYS * 86_400);
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        for entry in WalkDir::new(root).max_depth(6).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            let p = entry.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let ok = match tool {
                Tool::ClaudeCode => name.ends_with(".jsonl"),
                Tool::Codex => name.ends_with(".jsonl") && name.starts_with("rollout-"),
                Tool::Gemini => {
                    name.ends_with(".json")
                        && p.parent().and_then(|d| d.file_name()).and_then(|n| n.to_str()) == Some("chats")
                }
            };
            if !ok {
                continue;
            }
            let recent = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|m| m >= cutoff)
                .unwrap_or(true);
            if recent {
                out.push(p.to_path_buf());
            }
        }
    }
    out
}

fn file_stat(p: &Path) -> Option<(u64, i64)> {
    let m = std::fs::metadata(p).ok()?;
    let mtime = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Some((m.len(), mtime))
}

pub fn scan_tool(store: &mut Store, prices: &PriceTable, tool: Tool, roots: &[PathBuf]) -> Result<ScanReport> {
    let mut rep = ScanReport::default();
    for path in candidate_files(tool, roots) {
        rep.files_seen += 1;
        let key = path.to_string_lossy().to_string();
        let Some((size, mtime)) = file_stat(&path) else {
            continue;
        };
        let prev = store.cursor(&key)?.unwrap_or_default();
        if prev.size == size && prev.mtime == mtime && size > 0 {
            continue;
        }
        rep.files_read += 1;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("session").to_string();
        let mut items: Vec<ParsedItem> = Vec::new();
        let mut cursor = Cursor { offset: prev.offset, size, mtime, ctx: prev.ctx.clone() };

        match tool {
            Tool::Gemini => {
                let Ok(bytes) = std::fs::read(&path) else { continue };
                let content = String::from_utf8_lossy(&bytes);
                items = gemini::parse_file(&content, &stem);
                cursor.offset = size;
            }
            Tool::ClaudeCode | Tool::Codex => {
                let mut start = prev.offset;
                let mut codex_ctx: codex::CodexCtx =
                    prev.ctx.as_deref().and_then(|c| serde_json::from_str(c).ok()).unwrap_or_default();
                if size < start {
                    // truncated or replaced: start over
                    start = 0;
                    codex_ctx = codex::CodexCtx::default();
                }
                let Ok(mut f) = File::open(&path) else { continue };
                if f.seek(SeekFrom::Start(start)).is_err() { continue; }
                let mut buf = Vec::new();
                if f.read_to_end(&mut buf).is_err() { continue; }
                let last_nl = buf.iter().rposition(|b| *b == b'\n');
                let consumed = match last_nl {
                    Some(i) => i + 1,
                    None => 0,
                };
                let text = String::from_utf8_lossy(&buf[..consumed]);
                for line in text.split('\n') {
                    match tool {
                        Tool::ClaudeCode => items.extend(claude::parse_line(line)),
                        Tool::Codex => {
                            if !line.trim().is_empty() {
                                items.extend(codex::parse_line(line, &mut codex_ctx, &stem))
                            }
                        }
                        Tool::Gemini => unreachable!(),
                    }
                }
                cursor.offset = start + consumed as u64;
                // If the trailing partial line is pending, keep size unknown so we re-read next time.
                if (consumed as u64) < buf.len() as u64 {
                    cursor.size = 0;
                }
                if tool == Tool::Codex {
                    cursor.ctx = serde_json::to_string(&codex_ctx).ok();
                }
            }
        }

        let mut events = Vec::new();
        let mut gauges = Vec::new();
        for it in items {
            match it {
                ParsedItem::Event(mut e) => {
                    e.cost_usd = prices.cost(&e);
                    events.push(e);
                }
                ParsedItem::Gauge(g) => gauges.push(g),
            }
        }
        rep.events += events.len();
        rep.gauges += gauges.len();
        if !events.is_empty() {
            store.upsert_events(&events)?;
        }
        if !gauges.is_empty() {
            store.insert_gauges(&gauges)?;
        }
        store.set_cursor(&key, &cursor)?;
    }
    Ok(rep)
}

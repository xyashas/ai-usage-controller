//! One-click stop: send an interrupt (Ctrl-C) to the CLI process running a
//! runaway session. Off by default (settings.alerts.one_click_stop) until the
//! runaway thresholds are proven on beta data.

use crate::model::Tool;
use anyhow::{anyhow, Result};
use std::path::Path;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, UpdateKind};

fn matches_tool(tool: Tool, name: &str, cmd: &str) -> bool {
    let n = name.to_ascii_lowercase();
    let c = cmd.to_ascii_lowercase();
    match tool {
        Tool::ClaudeCode => n == "claude" || n == "claude.exe" || c.contains("@anthropic-ai/claude-code") || c.contains("claude-code/cli"),
        Tool::Codex => n == "codex" || n == "codex.exe" || c.contains("@openai/codex"),
        Tool::Gemini => n == "gemini" || n == "gemini.exe" || c.contains("@google/gemini-cli"),
    }
}

/// Interrupt the process for `tool` whose working directory is `project`.
/// Returns the number of processes signalled.
pub fn interrupt_session(tool: Tool, project: Option<&str>) -> Result<usize> {
    let mut sys = System::new_with_specifics(RefreshKind::nothing().with_processes(
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always).with_cwd(UpdateKind::Always),
    ));
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let mut hits = Vec::new();
    for (pid, p) in sys.processes() {
        let name = p.name().to_string_lossy();
        let cmd = p.cmd().iter().map(|s| s.to_string_lossy()).collect::<Vec<_>>().join(" ");
        if !matches_tool(tool, &name, &cmd) {
            continue;
        }
        if let (Some(proj), Some(cwd)) = (project, p.cwd()) {
            if cwd != Path::new(proj) {
                continue;
            }
        }
        hits.push(*pid);
    }
    if hits.is_empty() {
        return Err(anyhow!("Couldn't find the {} process for this session", tool.label()));
    }
    let mut n = 0;
    for pid in hits {
        if let Some(p) = sys.process(pid) {
            #[cfg(unix)]
            {
                if p.kill_with(sysinfo::Signal::Interrupt).unwrap_or(false) {
                    n += 1;
                }
            }
            #[cfg(not(unix))]
            {
                let _ = p;
                return Err(anyhow!("One-click stop isn't available on Windows yet. Press Esc in the terminal."));
            }
        }
    }
    Ok(n)
}

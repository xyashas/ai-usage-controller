//! Best-effort live gauges. Unofficial: labelled "live (best effort)" in the UI,
//! and the app always keeps the local-log estimate as a fallback.
//!
//! Claude: the same endpoint behind `/usage` in Claude Code, called with the
//! Claude Code OAuth login already on this machine. The token is read, never
//! refreshed or modified, and is only ever sent to api.anthropic.com.

use crate::model::{GaugeSample, Tool};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::time::Duration;

pub const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .http_status_as_error(false)
        .build()
        .into()
}

fn claude_credentials_json() -> Result<Value> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("security")
            .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
            .output()
            .context("reading Claude Code login from Keychain")?;
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            return serde_json::from_str(s.trim()).context("parsing Keychain credentials");
        }
    }
    let path = crate::paths::claude_credentials_file().ok_or_else(|| anyhow!("no home dir"))?;
    let s = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(serde_json::from_str(&s)?)
}

pub fn claude_access_token() -> Result<String> {
    let v = claude_credentials_json()?;
    let o = v.get("claudeAiOauth").unwrap_or(&v);
    if let Some(exp) = o.get("expiresAt").and_then(|e| e.as_i64()) {
        if exp < Utc::now().timestamp_millis() {
            return Err(anyhow!("Claude login expired; open Claude Code once to refresh it"));
        }
    }
    o.get("accessToken")
        .and_then(|t| t.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow!("no Claude Code OAuth login found (API-key users have no plan gauge)"))
}

pub fn parse_claude_usage(v: &Value, now: DateTime<Utc>) -> Vec<GaugeSample> {
    let mut out = Vec::new();
    for (field, window, minutes) in [
        ("five_hour", "five_hour", 300u32),
        ("seven_day", "weekly", 7 * 24 * 60),
        ("seven_day_opus", "weekly_opus", 7 * 24 * 60),
    ] {
        let Some(g) = v.get(field).filter(|g| !g.is_null()) else { continue };
        let Some(pct) = g.get("utilization").and_then(|u| u.as_f64()) else { continue };
        out.push(GaugeSample {
            tool: Tool::ClaudeCode,
            window: window.into(),
            used_pct: pct,
            resets_at: crate::parsers::parse_ts(g.get("resets_at")),
            window_minutes: Some(minutes),
            ts: now,
            source: "claude_oauth".into(),
        });
    }
    out
}

pub fn fetch_claude_gauges(now: DateTime<Utc>) -> Result<Vec<GaugeSample>> {
    let token = claude_access_token()?;
    let mut resp = agent()
        .get(CLAUDE_USAGE_URL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "ai-usage-controller")
        .call()?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(anyhow!("Claude usage gauge returned HTTP {status}"));
    }
    let v: Value = resp.body_mut().read_json()?;
    Ok(parse_claude_usage(&v, now))
}

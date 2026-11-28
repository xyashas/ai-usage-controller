//! GitHub connection: lists the user's repositories so tasks and handoffs can
//! target any of them, cloned here or not (cloud agents work from GitHub).
//!
//! Three ways to connect, easiest first:
//! 1. The GitHub CLI's login (`gh auth token`), if `gh` is installed and signed in.
//! 2. "Sign in with GitHub" (device flow), when the build has an OAuth app's
//!    client id (`AIUC_GITHUB_CLIENT_ID` at build time).
//! 3. A pasted personal access token.
//!
//! The token stays in the local database and is only sent to api.github.com.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

pub const KV_TOKEN: &str = "github_token";
pub const KV_LOGIN: &str = "github_login";
const API: &str = "https://api.github.com";

/// The OAuth app used for "Sign in with GitHub", if this build has one.
pub fn client_id() -> Option<&'static str> {
    option_env!("AIUC_GITHUB_CLIENT_ID").filter(|s| !s.is_empty())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Returns true only for HTTPS URLs targeting `api.github.com`.
pub fn is_api_url(url: &str) -> bool {
    url.starts_with("https://api.github.com/")
}

fn get(token: &str, url: &str) -> Result<Value> {
    anyhow::ensure!(is_api_url(url), "Refusing to send GitHub token outside api.github.com");
    let mut res = agent().get(url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "ai-usage-controller")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call().context("Couldn't reach GitHub")?;
    let status = res.status().as_u16();
    let body: Value = res.body_mut().read_json().unwrap_or(Value::Null);
    match status {
        200 => Ok(body),
        401 => bail!("GitHub didn't accept the token. Connect again."),
        403 | 429 => bail!("GitHub is rate limiting requests. Try again in a minute."),
        s => bail!("GitHub returned {s}"),
    }
}

/// The login the token belongs to; also checks that the token works.
pub fn whoami(token: &str) -> Result<String> {
    get(token, &format!("{API}/user"))?["login"].as_str().map(str::to_owned).ok_or_else(|| anyhow!("GitHub sent no login"))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GhRepo {
    /// owner/name
    pub full_name: String,
    pub html_url: String,
    pub clone_url: String,
    pub private: bool,
    pub default_branch: String,
    pub pushed_at: Option<String>,
}

pub fn parse_repos(v: &Value) -> Vec<GhRepo> {
    v.as_array().into_iter().flatten().filter_map(|r| Some(GhRepo {
        full_name: r["full_name"].as_str()?.to_owned(),
        html_url: r["html_url"].as_str()?.to_owned(),
        clone_url: r["clone_url"].as_str()?.to_owned(),
        private: r["private"].as_bool().unwrap_or(false),
        default_branch: r["default_branch"].as_str().unwrap_or("main").to_owned(),
        pushed_at: r["pushed_at"].as_str().map(str::to_owned),
    })).filter(|r| !r.full_name.is_empty()).collect()
}

/// Your repositories, most recently pushed first (up to 200).
pub fn repos(token: &str) -> Result<Vec<GhRepo>> {
    let mut out = Vec::new();
    for page in 1..=2 {
        let v = get(token, &format!("{API}/user/repos?per_page=100&page={page}&sort=pushed&affiliation=owner,collaborator,organization_member"))?;
        let batch = parse_repos(&v);
        let n = batch.len();
        out.extend(batch);
        if n < 100 { break; }
    }
    Ok(out)
}

/// The GitHub CLI's token, if `gh` is installed and signed in.
pub fn gh_cli_token() -> Option<String> {
    let gh = crate::handoff::executable("gh")?;
    let mut cmd = std::process::Command::new(gh);
    cmd.args(["auth", "token"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd.output().ok()?;
    let token = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !token.is_empty()).then_some(token)
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
}

fn post_form(url: &str, form: &[(&str, &str)]) -> Result<Value> {
    let mut res = agent().post(url).header("Accept", "application/json").send_form(form.iter().copied()).context("Couldn't reach GitHub")?;
    Ok(res.body_mut().read_json().unwrap_or(Value::Null))
}

/// Start "Sign in with GitHub": the user enters `user_code` at `verification_uri`.
pub fn device_start(client_id: &str) -> Result<DeviceCode> {
    let v = post_form("https://github.com/login/device/code", &[("client_id", client_id), ("scope", "repo read:user")])?;
    Ok(DeviceCode {
        device_code: v["device_code"].as_str().ok_or_else(|| anyhow!("GitHub sign-in is unavailable right now"))?.to_owned(),
        user_code: v["user_code"].as_str().unwrap_or_default().to_owned(),
        verification_uri: v["verification_uri"].as_str().unwrap_or("https://github.com/login/device").to_owned(),
        interval: v["interval"].as_u64().unwrap_or(5),
    })
}

/// One poll: `Ok(Some(token))` when approved, `Ok(None)` while waiting.
pub fn device_poll(client_id: &str, device_code: &str) -> Result<Option<String>> {
    let v = post_form("https://github.com/login/oauth/access_token", &[
        ("client_id", client_id), ("device_code", device_code), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
    ])?;
    if let Some(t) = v["access_token"].as_str() { return Ok(Some(t.to_owned())); }
    match v["error"].as_str() {
        Some("authorization_pending") | Some("slow_down") | None => Ok(None),
        Some("expired_token") => bail!("The code expired. Start again."),
        Some("access_denied") => bail!("Sign-in was cancelled."),
        Some(e) => bail!("GitHub sign-in failed: {e}"),
    }
}

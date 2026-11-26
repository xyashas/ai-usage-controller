//! Optional sync. Only counts leave the machine: time, tool, model, token
//! counts, estimated cost and a session id. Never prompts, code, file paths,
//! project names or tool-call signatures.

use crate::settings::SyncSettings;
use crate::store::Store;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncedEvent {
    pub key: String,
    pub tool: String,
    pub ts: i64,
    pub session: String,
    pub model: Option<String>,
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub cost: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RemoteSummary {
    #[serde(default)]
    pub today_cost_all_devices: f64,
    #[serde(default)]
    pub month_cost_all_devices: f64,
    #[serde(default)]
    pub devices: u32,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Build the payload for unsynced events. Exposed so tests can prove nothing private leaks.
pub fn payload(store: &Store, limit: usize) -> Result<Vec<SyncedEvent>> {
    Ok(store
        .unsynced_events(limit)?
        .into_iter()
        .map(|e| SyncedEvent {
            key: e.dedup_key,
            tool: e.tool.as_str().into(),
            ts: e.ts.timestamp_millis(),
            session: e.session_id,
            model: e.model,
            input: e.input_tokens,
            output: e.output_tokens,
            cache_write: e.cache_write_tokens,
            cache_read: e.cache_read_tokens,
            cost: e.cost_usd,
        })
        .collect())
}

/// `meta` carries the device's plan choices and audit results (no usage detail),
/// which the team dashboard uses for the idle-seat and waste reports.
pub fn push(store: &mut Store, s: &SyncSettings, meta: &serde_json::Value) -> Result<usize> {
    let (Some(url), Some(token)) = (&s.server_url, &s.device_token) else {
        return Err(anyhow!("sync is not set up"));
    };
    let mut total = 0;
    loop {
        let batch = payload(store, 500)?;
        if batch.is_empty() && total > 0 {
            break;
        }
        let mut resp = agent()
            .post(&format!("{}/v1/sync", url.trim_end_matches('/')))
            .header("Authorization", &format!("Bearer {token}"))
            .send_json(json!({ "events": batch, "meta": meta }))?;
        if resp.status().as_u16() != 200 {
            let body = resp.body_mut().read_to_string().unwrap_or_default();
            return Err(anyhow!("sync failed: HTTP {} {}", resp.status().as_u16(), body));
        }
        let keys: Vec<String> = batch.iter().map(|e| e.key.clone()).collect();
        store.mark_synced(&keys)?;
        total += keys.len();
        // An empty first batch still goes out once: it's the device heartbeat carrying `meta`.
        if keys.len() < 500 {
            break;
        }
    }
    Ok(total)
}

pub fn pull_summary(s: &SyncSettings) -> Result<RemoteSummary> {
    let (Some(url), Some(token)) = (&s.server_url, &s.device_token) else {
        return Err(anyhow!("sync is not set up"));
    };
    let mut resp = agent()
        .get(&format!("{}/v1/sync/summary", url.trim_end_matches('/')))
        .header("Authorization", &format!("Bearer {token}"))
        .call()?;
    if resp.status().as_u16() != 200 {
        return Err(anyhow!("summary failed: HTTP {}", resp.status().as_u16()));
    }
    Ok(resp.body_mut().read_json()?)
}

/// Join a personal sync group or a team with an invite code.
pub fn join(server_url: &str, invite_code: &str, member_email: &str, device_name: &str) -> Result<SyncSettings> {
    let mut resp = agent()
        .post(&format!("{}/v1/join", server_url.trim_end_matches('/')))
        .send_json(json!({ "invite_code": invite_code, "email": member_email, "device_name": device_name }))?;
    let v: serde_json::Value = resp.body_mut().read_json()?;
    if resp.status().as_u16() != 200 {
        return Err(anyhow!(v.get("error").and_then(|e| e.as_str()).unwrap_or("join failed").to_string()));
    }
    Ok(SyncSettings {
        enabled: true,
        server_url: Some(server_url.to_string()),
        device_token: v.get("device_token").and_then(|s| s.as_str()).map(String::from),
        device_id: v.get("device_id").and_then(|s| s.as_str()).map(String::from),
    })
}

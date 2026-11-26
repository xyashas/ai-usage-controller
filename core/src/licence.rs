//! Licence keys via Lemon Squeezy (merchant of record, handles global tax).
//! Uses Lemon Squeezy's public License API, which needs no secret in the app:
//! https://docs.lemonsqueezy.com/api/license-api
//!
//! Set `AIUC_LS_STORE_ID` at build time to reject keys from other stores.

use crate::entitlements::{LicenceRecord, Tier};
use crate::store::Store;
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::Value;
use std::time::Duration;

const LS_API: &str = "https://api.lemonsqueezy.com/v1/licenses";

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .build()
        .into()
}

fn tier_from_meta(v: &Value) -> Tier {
    let meta = v.get("meta");
    let names = [
        meta.and_then(|m| m.get("product_name")).and_then(|s| s.as_str()).unwrap_or(""),
        meta.and_then(|m| m.get("variant_name")).and_then(|s| s.as_str()).unwrap_or(""),
    ]
    .join(" ")
    .to_ascii_lowercase();
    if names.contains("team") {
        Tier::Team
    } else {
        Tier::Pro
    }
}

fn check_store(v: &Value) -> Result<()> {
    if let Some(expected) = option_env!("AIUC_LS_STORE_ID") {
        let got = v.get("meta").and_then(|m| m.get("store_id")).and_then(|s| s.as_i64()).map(|n| n.to_string());
        if got.as_deref() != Some(expected) {
            return Err(anyhow!("This licence key is not for AI Usage Controller"));
        }
    }
    Ok(())
}

pub fn activate(store: &Store, key: &str, instance_name: &str) -> Result<LicenceRecord> {
    let mut resp = agent()
        .post(&format!("{LS_API}/activate"))
        .header("Accept", "application/json")
        .send_form([("license_key", key), ("instance_name", instance_name)])?;
    let v: Value = resp.body_mut().read_json()?;
    if v.get("activated").and_then(|b| b.as_bool()) != Some(true) {
        let msg = v.get("error").and_then(|e| e.as_str()).unwrap_or("activation failed");
        return Err(anyhow!("{msg}"));
    }
    check_store(&v)?;
    let rec = LicenceRecord {
        key: key.to_string(),
        instance_id: v.get("instance").and_then(|i| i.get("id")).and_then(|s| s.as_str()).map(String::from),
        tier: tier_from_meta(&v),
        last_validated: Utc::now(),
        status: v
            .get("license_key")
            .and_then(|l| l.get("status"))
            .and_then(|s| s.as_str())
            .unwrap_or("active")
            .to_string(),
    };
    store.kv_set("licence", &serde_json::to_string(&rec)?)?;
    Ok(rec)
}

/// Re-validate the stored key. Network errors keep the old record (offline grace applies).
pub fn validate(store: &Store) -> Result<Option<LicenceRecord>> {
    let Some(mut rec) = crate::entitlements::licence(store) else { return Ok(None) };
    let mut form = vec![("license_key", rec.key.clone())];
    if let Some(id) = &rec.instance_id {
        form.push(("instance_id", id.clone()));
    }
    let form_ref: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let mut resp = match agent().post(&format!("{LS_API}/validate")).header("Accept", "application/json").send_form(form_ref) {
        Ok(r) => r,
        Err(_) => return Ok(Some(rec)),
    };
    let v: Value = resp.body_mut().read_json()?;
    if v.get("valid").and_then(|b| b.as_bool()) == Some(true) {
        check_store(&v)?;
        rec.tier = tier_from_meta(&v);
        rec.status = v
            .get("license_key")
            .and_then(|l| l.get("status"))
            .and_then(|s| s.as_str())
            .unwrap_or("active")
            .to_string();
        rec.last_validated = Utc::now();
    } else {
        rec.status = "invalid".into();
    }
    store.kv_set("licence", &serde_json::to_string(&rec)?)?;
    Ok(Some(rec))
}

pub fn deactivate(store: &Store) -> Result<()> {
    if let Some(rec) = crate::entitlements::licence(store) {
        if let Some(id) = &rec.instance_id {
            let _ = agent()
                .post(&format!("{LS_API}/deactivate"))
                .header("Accept", "application/json")
                .send_form([("license_key", rec.key.as_str()), ("instance_id", id.as_str())]);
        }
    }
    store.kv_del("licence")
}

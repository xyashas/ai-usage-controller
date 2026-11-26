//! Free vs Pro vs Team. The meter is free; the savings are paid.

use crate::store::Store;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

pub const TRIAL_DAYS: i64 = 7;
/// How long a validated licence keeps working offline.
pub const OFFLINE_GRACE_DAYS: i64 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Free,
    Trial,
    Pro,
    Team,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LicenceRecord {
    pub key: String,
    pub instance_id: Option<String>,
    pub tier: Tier,
    pub last_validated: DateTime<Utc>,
    pub status: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Entitlements {
    pub tier: Tier,
    pub trial_ends_at: Option<DateTime<Utc>>,
    pub trial_available: bool,
    pub licence_status: Option<String>,
    pub max_devices: u32,
}

impl Entitlements {
    pub fn paid(&self) -> bool {
        !matches!(self.tier, Tier::Free)
    }
}

pub fn licence(store: &Store) -> Option<LicenceRecord> {
    store.kv_get("licence").ok().flatten().and_then(|s| serde_json::from_str(&s).ok())
}

pub fn resolve(store: &Store, now: DateTime<Utc>) -> Entitlements {
    if let Some(l) = licence(store) {
        let fresh = now - l.last_validated <= Duration::days(OFFLINE_GRACE_DAYS);
        if fresh && (l.status == "active" || l.status == "inactive") {
            return Entitlements {
                tier: l.tier,
                trial_ends_at: None,
                trial_available: false,
                licence_status: Some(l.status),
                max_devices: if l.tier == Tier::Team { 3 } else { 2 },
            };
        }
    }
    let trial_start: Option<DateTime<Utc>> = store
        .kv_get("trial_started")
        .ok()
        .flatten()
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&Utc));
    match trial_start {
        Some(s) if now < s + Duration::days(TRIAL_DAYS) => Entitlements {
            tier: Tier::Trial,
            trial_ends_at: Some(s + Duration::days(TRIAL_DAYS)),
            trial_available: false,
            licence_status: None,
            max_devices: 2,
        },
        Some(_) => Entitlements { tier: Tier::Free, trial_ends_at: None, trial_available: false, licence_status: None, max_devices: 1 },
        None => Entitlements { tier: Tier::Free, trial_ends_at: None, trial_available: true, licence_status: None, max_devices: 1 },
    }
}

pub fn start_trial(store: &Store, now: DateTime<Utc>) -> anyhow::Result<()> {
    if store.kv_get("trial_started")?.is_none() {
        store.kv_set("trial_started", &now.to_rfc3339())?;
    }
    Ok(())
}

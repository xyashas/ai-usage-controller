//! GitHub Actions CI watcher for agent branches.
//!
//! Polls every 2 minutes ONLY when GitHub is connected (token in `kv` key
//! `"github_token"`) and `settings.ci_watch` is on. The GitHub token is sent
//! ONLY to `https://api.github.com`. Log tails are kept strictly in memory
//! (never written to disk or `kv`).

use aiuc_core::ci::{self, CiRun};
use aiuc_core::github;
use aiuc_core::repo_handoff as rh;
use aiuc_core::suggest::{Kind, Suggestion};
use aiuc_core::{Engine, Snapshot};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const POLL_INTERVAL: Duration = Duration::from_secs(120);
const LOG_TAIL_LINES: usize = 60;
const MAX_CACHED_LOGS: usize = 20;

#[derive(Default)]
struct CiState {
    runs: Vec<CiRun>,
    logs: HashMap<u64, String>,
    log_order: Vec<u64>,
}

fn state() -> &'static Mutex<CiState> {
    static STATE: OnceLock<Mutex<CiState>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}

#[tauri::command]
pub fn ci_runs() -> Vec<CiRun> {
    state().lock().map(|s| s.runs.clone()).unwrap_or_default()
}

#[tauri::command]
pub fn ci_log(run_id: u64) -> String {
    state()
        .lock()
        .ok()
        .and_then(|s| s.logs.get(&run_id).cloned())
        .unwrap_or_default()
}

fn api_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into()
}

fn plain_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Send an authenticated GET strictly to `https://api.github.com/...`.
fn gh_get_text(token: &str, path: &str) -> Option<(u16, Option<String>, String)> {
    let url = format!("https://api.github.com{path}");
    let mut res = api_agent()
        .get(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "ai-usage-controller")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .ok()?;
    let status = res.status().as_u16();
    let loc = res
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let body = res.body_mut().read_to_string().unwrap_or_default();
    Some((status, loc, body))
}

fn fetch_failed_job_log(token: &str, repo: &str, run_id: u64) -> Option<String> {
    let (status, _, body) = gh_get_text(token, &format!("/repos/{repo}/actions/runs/{run_id}/jobs"))?;
    if status != 200 {
        return None;
    }
    let v: Value = serde_json::from_str(&body).ok()?;
    let jobs = v["jobs"].as_array()?;
    let job_id = jobs
        .iter()
        .find(|j| j["conclusion"].as_str() == Some("failure"))
        .or_else(|| jobs.first())
        .and_then(|j| j["id"].as_u64())?;

    // Request the job log from api.github.com with no auto-redirect so the
    // Bearer token is never forwarded to the pre-signed blob URL.
    let (st, loc, direct_body) = gh_get_text(token, &format!("/repos/{repo}/actions/jobs/{job_id}/logs"))?;
    let raw = if st == 200 {
        direct_body
    } else if (300..400).contains(&st) {
        let redirect_url = loc?;
        let mut res = plain_agent()
            .get(&redirect_url)
            .header("User-Agent", "ai-usage-controller")
            .call()
            .ok()?;
        if res.status().as_u16() != 200 {
            return None;
        }
        res.body_mut().read_to_string().unwrap_or_default()
    } else {
        return None;
    };

    let tail = ci::log_tail(&raw, LOG_TAIL_LINES);
    (!tail.is_empty()).then_some(tail)
}

fn watched_repos_and_branches(
    engine: &Mutex<Engine>,
    last: &Mutex<Option<Snapshot>>,
) -> (Vec<String>, Vec<String>) {
    let mut repos = BTreeSet::new();
    let mut branches = BTreeSet::new();

    if let Ok(roots) = crate::repo_candidates(engine) {
        for r in roots {
            if let Some(url) = rh::status(&r).github {
                if let Some(slug) = ci::owner_repo(&url) {
                    repos.insert(slug);
                }
            }
        }
    }

    if let Ok(guard) = last.lock() {
        if let Some(snap) = guard.as_ref() {
            for a in &snap.agents {
                if let Some(b) = a.branch.as_deref().filter(|b| !b.is_empty()) {
                    branches.insert(b.to_owned());
                }
                if let Some(slug) = ci::owner_repo(&a.repo_name) {
                    repos.insert(slug);
                }
            }
        }
    }

    (repos.into_iter().collect(), branches.into_iter().collect())
}

pub fn spawn(app: AppHandle, engine: Arc<Mutex<Engine>>, last: Arc<Mutex<Option<Snapshot>>>) {
    std::thread::spawn(move || {
        let mut seeded = false;

        loop {
            let (enabled, notifications, token) = {
                match engine.lock() {
                    Ok(eng) => (
                        eng.settings.ci_watch,
                        eng.settings.alerts.notifications,
                        eng.store
                            .kv_get(github::KV_TOKEN)
                            .ok()
                            .flatten()
                            .filter(|t| !t.is_empty()),
                    ),
                    Err(_) => (false, false, None),
                }
            };

            let Some(token) = token.filter(|_| enabled) else {
                seeded = false;
                if let Ok(mut s) = state().lock() {
                    s.runs.clear();
                }
                std::thread::sleep(POLL_INTERVAL);
                continue;
            };

            let (repos, task_branches) = watched_repos_and_branches(&engine, &last);
            let mut fetched = Vec::new();
            for repo in &repos {
                if let Some((200, _, body)) =
                    gh_get_text(&token, &format!("/repos/{repo}/actions/runs?per_page=20"))
                {
                    let runs = ci::parse_runs(&body, repo);
                    fetched.extend(runs.into_iter().filter(|r| ci::watched(r, &task_branches)));
                }
            }
            fetched.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            fetched.truncate(20);

            let newly_failed = {
                let Ok(mut s) = state().lock() else {
                    std::thread::sleep(POLL_INTERVAL);
                    continue;
                };
                let failed = if seeded {
                    ci::newly_failed(&s.runs, &fetched)
                } else {
                    vec![]
                };
                s.runs = fetched;
                seeded = true;
                failed
            };

            for run in newly_failed {
                let tail = fetch_failed_job_log(&token, &run.repo, run.id).unwrap_or_default();
                if let Ok(mut s) = state().lock() {
                    if !tail.is_empty() {
                        s.logs.insert(run.id, tail.clone());
                        s.log_order.retain(|&id| id != run.id);
                        s.log_order.push(run.id);
                        while s.log_order.len() > MAX_CACHED_LOGS {
                            let old = s.log_order.remove(0);
                            s.logs.remove(&old);
                        }
                    }
                }

                if notifications {
                    crate::notify(&app, "CI failed", &format!("CI failed on {}", run.branch));
                }

                let prompt_text = if tail.is_empty() {
                    format!("CI ({}) failed on {} ({}).", run.name, run.repo, run.branch)
                } else {
                    format!(
                        "CI ({}) failed on {} ({}):\n\n{}",
                        run.name, run.repo, run.branch, tail
                    )
                };
                let suggestion = Suggestion {
                    id: format!("ci-fail:{}", run.id),
                    kind: Kind::TestsFailed {
                        task_id: run.id as i64,
                    },
                    title: "CI failed. Send to an agent?".into(),
                    detail: format!("{} · {}", run.repo, run.branch),
                    action: "send_failure".into(),
                    text: Some(prompt_text),
                };
                let _ = app.emit("suggestion", &suggestion);
            }

            std::thread::sleep(POLL_INTERVAL);
        }
    });
}

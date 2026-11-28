use aiuc_core::agent_status::{self as st, Answer, Kind};
use aiuc_core::model::Tool;
use aiuc_core::store::TaskRow;
use aiuc_core::{verify, Engine};
use chrono::{Duration, Utc};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aiuc-test-{name}-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn hook(base: &std::path::Path, v: serde_json::Value, wait_ms: u64) -> Option<String> {
    st::handle_claude(base, &v.to_string(), std::time::Duration::from_millis(wait_ms))
}

#[test]
fn claude_hook_events_become_session_status_without_the_prompt() {
    let base = tmp("events");
    hook(&base, json!({"session_id": "s1", "cwd": "/dev/shop", "hook_event_name": "UserPromptSubmit", "prompt": "SECRET PROMPT"}), 0);
    let s = st::sessions(&base, Utc::now());
    assert_eq!((s.len(), s[0].kind, s[0].cwd.as_str()), (1, Kind::Working, "/dev/shop"));
    let raw = std::fs::read_dir(base.join("sessions")).unwrap().map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap()).collect::<String>();
    assert!(!raw.contains("SECRET"), "the prompt is never written");

    hook(&base, json!({"session_id": "s1", "cwd": "/dev/shop", "hook_event_name": "Notification", "notification_type": "permission_prompt", "message": "x"}), 0);
    assert_eq!(st::sessions(&base, Utc::now())[0].kind, Kind::NeedsApproval);
    hook(&base, json!({"session_id": "s1", "cwd": "/dev/shop", "hook_event_name": "Notification", "notification_type": "auth_success"}), 0);
    assert_eq!(st::sessions(&base, Utc::now())[0].kind, Kind::NeedsApproval, "other notifications are ignored");
    hook(&base, json!({"session_id": "s1", "cwd": "/dev/shop", "hook_event_name": "Stop", "last_assistant_message": "SECRET"}), 0);
    assert_eq!(st::sessions(&base, Utc::now())[0].kind, Kind::Done);
    assert_eq!(hook(&base, json!({"cwd": "/x", "hook_event_name": "Stop"}), 0), None);
    assert_eq!(st::sessions(&base, Utc::now()).len(), 1, "no session id, nothing written");
}

#[test]
fn a_permission_request_waits_for_the_widget() {
    let base = tmp("approve");
    let b = base.clone();
    let answerer = std::thread::spawn(move || {
        for _ in 0..100 {
            if let Some(a) = st::pending(&b, Utc::now()).into_iter().next() {
                assert_eq!((a.tool_name.as_str(), a.summary.as_str(), a.repo_name.as_str()), ("Bash", "npm test", "shop"));
                st::answer(&b, &a.id, Answer::Allow).unwrap();
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    });
    let req = json!({"session_id": "s2", "cwd": "/dev/shop", "hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_input": {"command": "npm test\nmore"}});
    let out = hook(&base, req.clone(), 5000).expect("a decision");
    assert!(answerer.join().unwrap());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PermissionRequest");
    assert_eq!(v["hookSpecificOutput"]["decision"]["behavior"], "allow");
    assert!(st::pending(&base, Utc::now()).is_empty(), "answered requests are removed");
    assert!(st::answer(&base, "gone", Answer::Deny).is_err());

    // No answer: nothing printed, so Claude Code asks in the terminal.
    assert_eq!(hook(&base, req, 300), None);
    assert!(st::pending(&base, Utc::now()).is_empty());
    assert_eq!(st::decision_json(Answer::Ask), None);
    assert!(st::decision_json(Answer::Deny).unwrap().contains("\"deny\""));
}

#[test]
fn summaries_are_one_short_line() {
    assert_eq!(st::summarize("Edit", &json!({"file_path": "/a/b/main.rs"})), "main.rs");
    assert_eq!(st::summarize("WebFetch", &json!({"url": "https://x.dev"})), "https://x.dev");
    let long = st::summarize("Bash", &json!({"command": "x".repeat(400)}));
    assert!(long.chars().count() <= 141 && long.ends_with('…'));
}

#[test]
fn hooks_merge_into_claude_settings_and_come_back_out() {
    let mut s = json!({"model": "opus", "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "node .aiuc/hook.mjs"}]}]}});
    let cmd = st::hook_command(std::path::Path::new(r"C:\Program Files\AIUC\aiuc.exe"));
    assert_eq!(cmd, "\"C:/Program Files/AIUC/aiuc.exe\" --aiuc-hook claude");
    st::merge_claude_hooks(&mut s, &cmd, true);
    st::merge_claude_hooks(&mut s, &cmd, true); // idempotent
    assert_eq!(s["hooks"]["Stop"].as_array().unwrap().len(), 2, "the repo's own Stop hook stays");
    assert_eq!(s["hooks"]["PermissionRequest"].as_array().unwrap().len(), 1);
    assert_eq!(s["hooks"]["Notification"][0]["matcher"], "permission_prompt|idle_prompt|elicitation_dialog|agent_needs_input");
    st::merge_claude_hooks(&mut s, &cmd, false);
    assert!(s["hooks"].get("PermissionRequest").is_none(), "approvals off");
    st::remove_claude_hooks(&mut s);
    assert_eq!(s, json!({"model": "opus", "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "node .aiuc/hook.mjs"}]}]}}));
    let mut bare = json!({});
    st::merge_claude_hooks(&mut bare, &cmd, true);
    st::remove_claude_hooks(&mut bare);
    assert_eq!(bare, json!({}));
}

#[test]
fn codex_notify_is_added_only_when_free() {
    let exe = std::path::Path::new("/Applications/AIUC.app/Contents/MacOS/aiuc");
    let added = st::merge_codex_notify("model = \"o4\"\n\n[profiles.x]\nnotify = 1\n", exe).unwrap();
    assert!(added.starts_with("notify = [\"/Applications/AIUC.app/Contents/MacOS/aiuc\", \"--aiuc-hook\", \"codex\"]\n"));
    assert_eq!(st::merge_codex_notify(&added, exe), None, "already there");
    assert_eq!(st::merge_codex_notify("notify = [\"terminal-notifier\"]\n", exe), None, "someone else's");
    assert_eq!(st::remove_codex_notify(&added).unwrap(), "model = \"o4\"\n\n[profiles.x]\nnotify = 1\n");

    let base = tmp("codex");
    st::handle_codex(&base, &json!({"type": "agent-turn-complete", "thread-id": "t1", "cwd": "/dev/shop", "last-assistant-message": "SECRET"}).to_string());
    let s = st::sessions(&base, Utc::now());
    assert_eq!((s[0].tool, s[0].kind), (Tool::Codex, Kind::Done));
}

#[test]
fn agent_runs_use_what_the_tool_reported() {
    let base = tmp("runs");
    let mut eng = Engine::in_memory().unwrap();
    eng.status_dir = Some(base.clone());
    let now = Utc::now();
    let row = |cwd: &str| TaskRow { created_at: now - Duration::minutes(5), ..TaskRow::new("/dev/shop".into(), cwd.into(), None, Tool::ClaudeCode, "Fix it".into(), Some("terminal".into())) };
    let asking = eng.record_task(&row("/dev/shop.agents/a")).unwrap();
    let done = eng.record_task(&row("/dev/shop.agents/b")).unwrap();
    let queued = eng.record_task(&TaskRow { queued_until: Some(now + Duration::hours(1)), ..row("/dev/shop") }).unwrap();
    let rep = |s: &str, cwd: &str, kind| st::record(&base, &st::SessionStatus { session: s.into(), tool: Tool::ClaudeCode, cwd: cwd.into(), kind, at: now - Duration::seconds(30) }).unwrap();
    rep("a", "/dev/shop.agents/a", Kind::NeedsApproval);
    rep("b", "/dev/shop.agents/b/", Kind::Done);
    let runs = eng.agent_runs(now, &BTreeMap::new()).unwrap();
    let state = |id| runs.iter().find(|r| r.id == id).map(|r| r.state.clone()).unwrap();
    assert_eq!(state(asking), "needs_approval");
    assert_eq!(state(done), "done");
    assert_eq!(state(queued), "queued");
    assert_eq!(eng.store.queued_due(now).unwrap().len(), 0);
    assert_eq!(eng.store.queued_due(now + Duration::hours(2)).unwrap()[0].id, queued);

    eng.store.set_verify(done, "passed", now, Some(12.5)).unwrap();
    let r = eng.agent_runs(now, &BTreeMap::new()).unwrap().into_iter().find(|r| r.id == done).unwrap();
    assert_eq!((r.verify.as_deref(), r.verify_secs), (Some("passed"), Some(12.5)));
    eng.store.start_queued(queued, now, Tool::Codex, "/dev/shop.agents/c", Some("agents/c")).unwrap();
    let q = eng.store.task(queued).unwrap().unwrap();
    assert_eq!((q.queued_until, q.tool, q.branch.as_deref()), (None, Tool::Codex, Some("agents/c")));
}

#[test]
fn verify_detects_and_runs_the_tests() {
    let d = tmp("verify");
    assert_eq!(verify::detect(&d), None);
    std::fs::write(d.join("package.json"), r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#).unwrap();
    assert_eq!(verify::detect(&d), None, "npm's placeholder isn't a test suite");
    std::fs::write(d.join("package.json"), r#"{"scripts":{"test":"vitest run"}}"#).unwrap();
    std::fs::write(d.join("pnpm-lock.yaml"), "").unwrap();
    assert_eq!(verify::detect(&d).as_deref(), Some("pnpm test"));
    std::fs::write(d.join("Cargo.toml"), "").unwrap();
    assert_eq!(verify::detect(&d).as_deref(), Some("cargo test --quiet"));
    std::fs::create_dir_all(d.join(".aiuc")).unwrap();
    std::fs::write(d.join(".aiuc").join("verify"), "# how to test\nmake check\n").unwrap();
    assert_eq!(verify::detect(&d).as_deref(), Some("make check"));

    let ok = verify::run(&d, "echo hello", std::time::Duration::from_secs(30));
    assert_eq!(ok.outcome, verify::Outcome::Passed);
    assert!(ok.tail.contains("hello"));
    assert_eq!(verify::run(&d, "exit 3", std::time::Duration::from_secs(30)).outcome, verify::Outcome::Failed);
}

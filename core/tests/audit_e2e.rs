//! End-to-end audit tests covering:
//! - Real redacted CLI & hook fixtures (StreamReader, handle_claude, handle_codex)
//! - Scanner edge cases (malformed JSON, partial trailing line, huge >1MB line, non-UTF8,
//!   truncated/rotated file, clock skew)
//! - Corrupt SQLite DB & corrupt settings JSON backup + clean recovery
//! - GitHub API URL allowlist (`https://api.github.com/` only)
//! - Sync privacy payload (counts only, no prompts/paths/project names)
//! - Settings round-trip & legacy JSON defaults
//! - Entitlements (trial, expiry, Pro/Team offline grace)
//! - Gemini features edge cases (recap, watch, suggest, polish, autopilot, inbox)

use aiuc_core::agent_status::{self, Kind};
use aiuc_core::ask::StreamReader;
use aiuc_core::autopilot;
use aiuc_core::engine::Engine;
use aiuc_core::entitlements::{self, LicenceRecord, Tier};
use aiuc_core::github;
use aiuc_core::inbox;
use aiuc_core::model::{EventKind, Tool, UsageEvent};
use aiuc_core::polish;
use aiuc_core::recap;
use aiuc_core::scanner::scan_tool;
use aiuc_core::settings::Settings;
use aiuc_core::store::Store;
use aiuc_core::suggest;
use aiuc_core::sync;
use aiuc_core::watch;
use chrono::{Duration, TimeZone, Utc};
use serde_json::Value;
use std::fs;
use std::io::Write;
use tempfile::tempdir;

#[test]
fn fixtures_cli_stream_readers() {
    // 1. Claude Code stream-json
    let claude_fixture = include_str!("fixtures/claude_stream.jsonl");
    let mut r = StreamReader::default();
    let mut out = String::new();
    for line in claude_fixture.lines() {
        if let Some(chunk) = r.line(Tool::ClaudeCode, line) {
            out.push_str(&chunk);
        }
    }
    assert!(r.error.is_none());
    assert_eq!(
        out.trim(),
        "The test failed because the fixture timestamp was stale."
    );

    // 2. Codex exec --json
    let codex_fixture = include_str!("fixtures/codex_exec.jsonl");
    let mut r = StreamReader::default();
    let mut out = String::new();
    for line in codex_fixture.lines() {
        if let Some(chunk) = r.line(Tool::Codex, line) {
            out.push_str(&chunk);
        }
    }
    assert!(r.error.is_none());
    assert_eq!(
        out.trim(),
        "Update the expected date in the assertion to match UTC midnight."
    );

    // 3. Gemini plain text
    let gemini_fixture = include_str!("fixtures/gemini_out.txt");
    let mut r = StreamReader::default();
    let mut out = String::new();
    for line in gemini_fixture.lines() {
        if let Some(chunk) = r.line(Tool::Gemini, line) {
            out.push_str(&chunk);
        }
    }
    assert!(r.error.is_none());
    assert!(out.contains("unused dependency in Cargo.toml"));
}

#[test]
fn fixtures_claude_and_codex_hooks() {
    let dir = tempdir().unwrap();
    let base = dir.path();

    // Codex notify fixture
    let codex_notify = include_str!("fixtures/codex_notify.json");
    agent_status::handle_codex(base, codex_notify);
    let ss = agent_status::sessions(base, Utc::now());
    assert_eq!(ss.len(), 1);
    assert_eq!(ss[0].tool, Tool::Codex);
    assert_eq!(ss[0].session, "codex-thread-99");
    assert_eq!(ss[0].kind, Kind::Done);

    // Claude hooks fixture (UserPromptSubmit -> PermissionRequest -> Notification -> Stop)
    let claude_hooks: Vec<Value> = serde_json::from_str(include_str!("fixtures/claude_hooks.json")).unwrap();
    let expected_kinds = [Kind::Working, Kind::NeedsApproval, Kind::WaitingInput, Kind::Done];
    for (ev, expected) in claude_hooks.iter().zip(expected_kinds) {
        let s = serde_json::to_string(ev).unwrap();
        let _ = agent_status::handle_claude(base, &s, std::time::Duration::ZERO);
        let all = agent_status::sessions(base, Utc::now());
        let claude = all.iter().find(|x| x.tool == Tool::ClaudeCode).unwrap();
        assert_eq!(claude.kind, expected, "hook event {:?} produced wrong kind", ev["hook_event_name"]);
    }
}

#[test]
fn scanner_edge_cases_malformed_partial_huge_non_utf8_rotated_clock_skew() {
    let dir = tempdir().unwrap();
    let claude_dir = dir.path().join("claude");
    let gemini_dir = dir.path().join("gemini").join("chats");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::create_dir_all(&gemini_dir).unwrap();

    let log = claude_dir.join("edge.jsonl");
    let now = Utc::now();
    let future_ts = (now + Duration::hours(48)).to_rfc3339();
    let valid_ts = now.to_rfc3339();
    let huge_pad = "x".repeat(1_200_000); // > 1 MB single line

    {
        let mut f = fs::File::create(&log).unwrap();
        writeln!(f, "{{not valid json").unwrap();
        f.write_all(b"\xff\xfe{\"type\":\"assistant\"}\n").unwrap();
        writeln!(f, "{{\"type\":\"progress\",\"pad\":\"{huge_pad}\"}}").unwrap();
        writeln!(
            f,
            "{{\"type\":\"assistant\",\"timestamp\":\"{future_ts}\",\"sessionId\":\"s1\",\"requestId\":\"r-fut\",\"message\":{{\"id\":\"m-fut\",\"model\":\"claude-sonnet-4-6\",\"usage\":{{\"input_tokens\":10,\"output_tokens\":5}},\"content\":[]}}}}"
        ).unwrap();
        writeln!(
            f,
            "{{\"type\":\"assistant\",\"timestamp\":\"{valid_ts}\",\"sessionId\":\"s1\",\"requestId\":\"r-1\",\"message\":{{\"id\":\"m-1\",\"model\":\"claude-sonnet-4-6\",\"usage\":{{\"input_tokens\":100,\"output_tokens\":50}},\"content\":[]}}}}"
        ).unwrap();
        write!(
            f,
            "{{\"type\":\"assistant\",\"timestamp\":\"{valid_ts}\",\"sessionId\":\"s1\",\"requestId\":\"r-part\",\"message\":{{\"id\":\"m-part\",\"model\":\"claude-sonnet-4-6\",\"usage\":{{\"input_tokens\":77,\"output_tokens\":33}},\"content\":[]}}}}"
        ).unwrap();
    }

    fs::write(gemini_dir.join("session-bad.json"), b"\xff\xfe not json").unwrap();

    let mut store = Store::open_in_memory().unwrap();
    let prices = Engine::in_memory().unwrap().prices;
    let rep1 = scan_tool(&mut store, &prices, Tool::ClaudeCode, std::slice::from_ref(&claude_dir)).unwrap();
    assert_eq!(rep1.events, 2, "valid + future parsed; partial trailing line deferred");
    let grep = scan_tool(&mut store, &prices, Tool::Gemini, &[dir.path().join("gemini")]).unwrap();
    assert_eq!(grep.events, 0);

    {
        let mut f = fs::OpenOptions::new().append(true).open(&log).unwrap();
        writeln!(f).unwrap();
    }
    let rep2 = scan_tool(&mut store, &prices, Tool::ClaudeCode, std::slice::from_ref(&claude_dir)).unwrap();
    assert_eq!(rep2.events, 1, "completed partial line should now be parsed");

    fs::write(
        &log,
        format!(
            "{{\"type\":\"assistant\",\"timestamp\":\"{valid_ts}\",\"sessionId\":\"s2\",\"requestId\":\"r-rot\",\"message\":{{\"id\":\"m-rot\",\"model\":\"claude-sonnet-4-6\",\"usage\":{{\"input_tokens\":42,\"output_tokens\":18}},\"content\":[]}}}}\n"
        ),
    ).unwrap();
    let rep3 = scan_tool(&mut store, &prices, Tool::ClaudeCode, &[claude_dir]).unwrap();
    assert_eq!(rep3.events, 1, "rotated/truncated file should reset offset to 0 and parse");
}

#[test]
fn corrupt_sqlite_db_and_corrupt_settings_recover_cleanly() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("corrupt.db");
    fs::write(&db, b"this is definitely not a valid sqlite database file!").unwrap();

    let (store, recovered) = Store::open_or_recover(&db).unwrap();
    assert!(recovered, "corrupt DB should be detected and recovered");
    let backups: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".corrupt."))
        .collect();
    assert_eq!(backups.len(), 1, "corrupt DB should be backed up before recreating");

    store.kv_set("settings", "{corrupt json").unwrap();
    drop(store);

    let (engine, db_recovered) = Engine::open_or_recover(&db).unwrap();
    assert!(!db_recovered, "DB itself is now healthy");
    assert_eq!(
        engine.store.kv_get("recovered_settings").unwrap().as_deref(),
        Some("1")
    );
    assert_eq!(
        engine.store.kv_get("settings_corrupt_backup").unwrap().as_deref(),
        Some("{corrupt json")
    );
}

#[test]
fn github_api_url_enforcement() {
    assert!(github::is_api_url("https://api.github.com/user"));
    assert!(github::is_api_url("https://api.github.com/user/repos?per_page=100&page=2"));
    assert!(!github::is_api_url("http://api.github.com/user"));
    assert!(!github::is_api_url("https://api.github.com.evil.com/user"));
    assert!(!github::is_api_url("https://api.github.com@evil.com/user"));
    assert!(!github::is_api_url("https://github.com/user"));
    assert!(!github::is_api_url("https://evil.example/api.github.com/user"));
}

#[test]
fn sync_payload_contains_only_counts_never_private_text() {
    let mut store = Store::open_in_memory().unwrap();
    let mut ev = UsageEvent::new(
        Tool::ClaudeCode,
        EventKind::Assistant,
        "cc:priv-1".into(),
        Utc::now(),
        "sess-1".into(),
    );
    ev.project = Some("/Users/secret-user/top-secret-client-repo".into());
    ev.tool_sig = Some("Bash:cat /etc/shadow".into());
    ev.model = Some("claude-sonnet-4-6".into());
    ev.input_tokens = 500;
    ev.output_tokens = 120;
    ev.cost_usd = 0.01;
    store.upsert_events(&[ev]).unwrap();

    let batch = sync::payload(&store, 100).unwrap();
    assert_eq!(batch.len(), 1);
    let json = serde_json::to_string(&batch).unwrap();
    assert!(!json.contains("secret-user"), "project path leaked into sync payload");
    assert!(!json.contains("top-secret-client-repo"), "project name leaked into sync payload");
    assert!(!json.contains("shadow"), "tool signature leaked into sync payload");
    assert!(json.contains("\"input\":500"));
    assert!(json.contains("\"output\":120"));
}

#[test]
fn settings_round_trip_and_legacy_defaults() {
    let old_json = r#"{"onboarded": true, "hide_when_idle": true}"#;
    let parsed: Settings = serde_json::from_str(old_json).unwrap();
    assert!(parsed.onboarded);
    assert!(parsed.hide_when_idle);
    assert!(parsed.show_daily_recap, "new boolean defaults should populate cleanly");
    assert!(parsed.autopilot, "autopilot should default to true");
    assert!(!parsed.polish_auto, "polish_auto should default to false");

    let serialized = serde_json::to_string(&parsed).unwrap();
    let round_trip: Settings = serde_json::from_str(&serialized).unwrap();
    assert_eq!(round_trip.onboarded, parsed.onboarded);
    assert_eq!(round_trip.autopilot, parsed.autopilot);
    assert_eq!(round_trip.show_daily_recap, parsed.show_daily_recap);
}

#[test]
fn entitlements_trial_expiry_and_offline_grace() {
    let store = Store::open_in_memory().unwrap();
    let t0 = Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap();

    let e0 = entitlements::resolve(&store, t0);
    assert_eq!(e0.tier, Tier::Free);
    assert!(e0.trial_available);

    entitlements::start_trial(&store, t0).unwrap();
    let e1 = entitlements::resolve(&store, t0 + Duration::days(3));
    assert_eq!(e1.tier, Tier::Trial);
    assert!(e1.paid());
    let e2 = entitlements::resolve(&store, t0 + Duration::days(8));
    assert_eq!(e2.tier, Tier::Free);
    assert!(!e2.trial_available);

    let lic = LicenceRecord {
        key: "TEST-KEY".into(),
        instance_id: Some("inst-1".into()),
        tier: Tier::Pro,
        last_validated: t0,
        status: "active".into(),
    };
    store.kv_set("licence", &serde_json::to_string(&lic).unwrap()).unwrap();
    assert_eq!(entitlements::resolve(&store, t0 + Duration::days(5)).tier, Tier::Pro);
    assert_eq!(entitlements::resolve(&store, t0 + Duration::days(10)).tier, Tier::Free);
}

#[test]
fn gemini_features_edge_cases_empty_and_boundary_states() {
    let mut engine = Engine::in_memory().unwrap();
    let now = Utc.with_ymd_and_hms(2026, 11, 27, 0, 0, 1).unwrap();
    let snap = engine.snapshot(now).unwrap();

    let daily = recap::build_daily_recap(&engine.store, &snap.tools, 0, false, now).unwrap();
    assert!(daily.is_none(), "empty yesterday with 0 open tasks should suppress morning brief");

    let weekly = recap::build_weekly_recap(&engine.store, &snap.tools, 0, now).unwrap();
    assert_eq!(weekly.handoffs_count, 0);

    let chips = suggest::from_snapshot(&snap, now);
    assert!(chips.len() <= 3);

    let tmp = tempdir().unwrap();
    let ctx = polish::gather(Some(tmp.path()), None);
    assert!(ctx.branch.is_none());
    assert_eq!(polish::clean("Here is your rewritten prompt:\n\"Fix the parser bug\""), "Fix the parser bug");

    assert!(autopilot::decide(&snap, &[], &[Tool::ClaudeCode, Tool::Codex]).is_empty());

    assert!(watch::parse("not a watch at all", &snap, now).is_none());

    let item = inbox::capture(&engine.store, "Remember to check CI", inbox::Source::Voice, Some("VS Code"), now).unwrap();
    let items = inbox::list(&engine.store);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, item.id);
}

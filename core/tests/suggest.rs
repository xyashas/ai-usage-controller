use aiuc_core::entitlements::Tier;
use aiuc_core::model::{EventKind, GaugeSample, Tool, UsageEvent};
use aiuc_core::store::TaskRow;
use aiuc_core::suggest::{self, Kind};
use aiuc_core::Engine;
use chrono::{Duration, Utc};

#[test]
fn looks_like_error_detects_real_errors_and_rejects_normal_prose() {
    // True positives
    assert!(suggest::looks_like_error(
        "Traceback (most recent call last):\n  File \"app.py\", line 42, in main\nValueError: invalid literal"
    ));
    assert!(suggest::looks_like_error(
        "thread 'main' panicked at src/main.rs:18:5:\ncalled `Option::unwrap()` on a `None` value"
    ));
    assert!(suggest::looks_like_error("error[E0308]: mismatched types\n  --> src/lib.rs:10:5"));
    assert!(suggest::looks_like_error("TypeError: Cannot read properties of undefined (reading 'map')"));
    assert!(suggest::looks_like_error("npm ERR! code ELIFECYCLE\nnpm ERR! errno 1"));
    assert!(suggest::looks_like_error("Command failed with exit code 1"));
    assert!(suggest::looks_like_error(
        "    at Object.<anonymous> (/app/src/index.ts:12:5)\n    at Module._compile (node:internal/modules/cjs/loader:1554:14)"
    ));

    // False positives — normal prose must NOT trigger
    assert!(!suggest::looks_like_error("Hi"));
    assert!(!suggest::looks_like_error(
        "There was an error in my thinking about the product roadmap, let's discuss tomorrow."
    ));
    assert!(!suggest::looks_like_error(
        "Please help me write a blog post about how teams handle TypeScript migrations smoothly."
    ));
    assert!(!suggest::looks_like_error(
        "The build succeeded with exit code 0 and all 60 tests passed."
    ));
    assert!(!suggest::looks_like_error(
        "https://github.com/xyashas/ai-usage-controller"
    ));
}

#[test]
fn from_snapshot_suggests_failed_tests_and_limit_mid_task() {
    let mut eng = Engine::in_memory().unwrap();
    eng.set_licence_tier(Tier::Pro).unwrap();
    let now = Utc::now();

    eng.store
        .upsert_events(&[UsageEvent {
            dedup_key: "codex:1".into(),
            tool: Tool::Codex,
            kind: EventKind::Assistant,
            ts: now - Duration::minutes(5),
            session_id: "s1".into(),
            project: Some("/dev/repo".into()),
            model: Some("gpt-5".into()),
            input_tokens: 100,
            output_tokens: 50,
            cache_write_tokens: 0,
            cache_read_tokens: 0,
            cost_usd: 0.1,
            tool_sig: None,
        }])
        .unwrap();

    eng.store
        .insert_gauges(&[
            GaugeSample {
                tool: Tool::ClaudeCode,
                window: "five_hour".into(),
                ts: now - Duration::minutes(1),
                used_pct: 94.0,
                resets_at: Some(now + Duration::hours(2)),
                window_minutes: Some(300),
                source: "test".into(),
            },
            GaugeSample {
                tool: Tool::Codex,
                window: "five_hour".into(),
                ts: now - Duration::minutes(1),
                used_pct: 20.0,
                resets_at: Some(now + Duration::hours(3)),
                window_minutes: Some(300),
                source: "test".into(),
            },
        ])
        .unwrap();

    let mut row = TaskRow::new(
        "/dev/repo".into(),
        "/dev/repo".into(),
        None,
        Tool::ClaudeCode,
        "refactor parser".into(),
        Some("terminal".into()),
    );
    row.created_at = now - Duration::minutes(3);
    let task_id = eng.record_task(&row).unwrap();

    let mut snap = eng.snapshot(now).unwrap();
    snap.agents[0].state = "working".into();
    snap.agents[0].verify = Some("failed".into());
    snap.agents[0].verify_at = Some(now - Duration::minutes(1));

    let suggestions = suggest::from_snapshot(&snap, now);
    assert!(suggestions.iter().all(|s| s.title.chars().count() <= 40));

    let fail = suggestions
        .iter()
        .find(|s| s.kind == Kind::TestsFailed { task_id })
        .expect("should suggest fixing failed tests");
    assert_eq!(fail.action, "send_failure");

    let limit = suggestions
        .iter()
        .find(|s| s.kind == Kind::LimitMidTask { tool: Tool::ClaudeCode, to: Tool::Codex })
        .expect("should suggest handing off to Codex");
    assert_eq!(limit.action, "handoff");
}

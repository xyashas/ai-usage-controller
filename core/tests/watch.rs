use aiuc_core::entitlements::Tier;
use aiuc_core::model::{GaugeSample, Tool};
use aiuc_core::store::TaskRow;
use aiuc_core::watch::{self, Condition};
use aiuc_core::Engine;
use chrono::{Duration, Local, TimeZone, Utc};

fn gauge(tool: Tool, window: &str, ts: chrono::DateTime<Utc>, used_pct: f64, resets_at: Option<chrono::DateTime<Utc>>) -> GaugeSample {
    GaugeSample {
        tool,
        window: window.into(),
        ts,
        used_pct,
        resets_at,
        window_minutes: Some(300),
        source: "test".into(),
    }
}

#[test]
fn parse_and_check_limit_reset_and_below() {
    let mut eng = Engine::in_memory().unwrap();
    eng.set_licence_tier(Tier::Pro).unwrap();
    let now = Utc::now();
    eng.store
        .insert_gauges(&[gauge(Tool::ClaudeCode, "five_hour", now - Duration::minutes(1), 85.0, Some(now + Duration::hours(2)))])
        .unwrap();

    let snap = eng.snapshot(now).unwrap();
    let w_reset = watch::parse("tell me when Claude resets", &snap, now).expect("should parse reset");
    assert_eq!(w_reset.condition, Condition::LimitReset { tool: Tool::ClaudeCode });

    let w_below = watch::parse("when claude is below 50%", &snap, now).expect("should parse below");
    assert_eq!(w_below.condition, Condition::LimitBelow { tool: Tool::ClaudeCode, used_pct: 50.0 });

    // Neither fires at 85%
    assert!(watch::check(&[w_reset.clone(), w_below.clone()], &snap, now).is_empty());

    // After reset (2 hours + 5 min later, gauge shows 0%), both fire
    let later = now + Duration::hours(2) + Duration::minutes(5);
    eng.store
        .insert_gauges(&[gauge(Tool::ClaudeCode, "five_hour", later - Duration::seconds(10), 0.0, Some(later + Duration::hours(5)))])
        .unwrap();
    let snap2 = eng.snapshot(later).unwrap();
    let fired = watch::check(&[w_reset, w_below], &snap2, later);
    assert_eq!(fired.len(), 2);
}

#[test]
fn parse_and_check_agent_done_and_tests_pass() {
    let mut eng = Engine::in_memory().unwrap();
    eng.set_licence_tier(Tier::Pro).unwrap();
    let now = Utc::now();

    let mut row = TaskRow::new(
        "/dev/repo".into(),
        "/dev/repo".into(),
        Some("feat/x".into()),
        Tool::Codex,
        "fix tests".into(),
        Some("terminal".into()),
    );
    row.created_at = now - Duration::minutes(2);
    let task_id = eng.record_task(&row).unwrap();

    let mut snap = eng.snapshot(now).unwrap();
    let w_done = watch::parse("when codex is done", &snap, now).expect("should parse agent done");
    assert_eq!(w_done.condition, Condition::AgentDone { task_id });

    let w_tests = watch::parse("when tests pass", &snap, now).expect("should parse tests pass");
    assert_eq!(w_tests.condition, Condition::TestsPass { task_id });

    // Currently starting -> doesn't fire
    assert!(watch::check(&[w_done.clone(), w_tests.clone()], &snap, now).is_empty());

    // Mark agent done + tests passed
    snap.agents[0].state = "done".into();
    snap.agents[0].verify = Some("passed".into());
    let fired = watch::check(&[w_done, w_tests], &snap, now);
    assert_eq!(fired.len(), 2);
}

#[test]
fn parse_and_check_relative_and_clock_reminders_and_kv_storage() {
    let mut eng = Engine::in_memory().unwrap();
    let now = Local
        .with_ymd_and_hms(2026, 11, 27, 14, 0, 0)
        .earliest()
        .unwrap()
        .with_timezone(&Utc);
    let snap = eng.snapshot(now).unwrap();

    let w_rel = watch::parse("in 20 minutes", &snap, now).expect("should parse relative");
    assert_eq!(w_rel.condition, Condition::At { at: now + Duration::minutes(20) });

    let w_clock = watch::parse("remind me at 3pm to push", &snap, now).expect("should parse 3pm");
    let expected_3pm = Local
        .with_ymd_and_hms(2026, 11, 27, 15, 0, 0)
        .earliest()
        .unwrap()
        .with_timezone(&Utc);
    assert_eq!(w_clock.condition, Condition::At { at: expected_3pm });

    // Persist and reload from kv
    watch::save(&eng.store, &[w_rel.clone(), w_clock.clone()]).unwrap();
    let loaded = watch::load(&eng.store);
    assert_eq!(loaded.len(), 2);

    // At 14:25 only w_rel fires
    let fired1 = watch::check(&loaded, &snap, now + Duration::minutes(25));
    assert_eq!(fired1.len(), 1);
    assert_eq!(fired1[0].0.id, w_rel.id);

    // At 15:01 both fire
    let fired2 = watch::check(&loaded, &snap, now + Duration::minutes(61));
    assert_eq!(fired2.len(), 2);

    // Garbage returns None
    assert!(watch::parse("hello world", &snap, now).is_none());
}

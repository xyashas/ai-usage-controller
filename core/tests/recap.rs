use aiuc_core::entitlements::Tier;
use aiuc_core::model::{EventKind, GaugeSample, Tool, UsageEvent};
use aiuc_core::recap;
use aiuc_core::store::TaskRow;
use aiuc_core::Engine;
use chrono::{Duration, Local, TimeZone, Utc};

fn ev(tool: Tool, id: &str, ts: chrono::DateTime<Utc>, cost: f64) -> UsageEvent {
    UsageEvent {
        dedup_key: format!("{}:{id}", tool.as_str()),
        tool,
        kind: EventKind::Assistant,
        ts,
        session_id: format!("s-{id}"),
        project: Some("/dev/secret-project".into()),
        model: Some("claude-sonnet-4".into()),
        input_tokens: 1000,
        output_tokens: 400,
        cache_write_tokens: 0,
        cache_read_tokens: 0,
        cost_usd: cost,
        tool_sig: None,
    }
}

fn gauge(
    tool: Tool,
    window: &str,
    ts: chrono::DateTime<Utc>,
    used_pct: f64,
    resets_at: Option<chrono::DateTime<Utc>>,
) -> GaugeSample {
    GaugeSample {
        tool,
        window: window.into(),
        ts,
        used_pct,
        resets_at,
        window_minutes: Some(if window == "five_hour" { 300 } else { 10080 }),
        source: "test".into(),
    }
}

fn local_time(y: i32, m: u32, d: u32, h: u32, min: u32) -> chrono::DateTime<Utc> {
    Local
        .with_ymd_and_hms(y, m, d, h, min, 0)
        .earliest()
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn daily_recap_summarizes_yesterday_resets_open_tasks_and_suggestion() {
    let mut eng = Engine::in_memory().unwrap();
    eng.set_licence_tier(Tier::Pro).unwrap();
    // Wednesday 9:00 AM local time
    let now = local_time(2026, 11, 25, 9, 0);
    let yesterday_10am = local_time(2026, 11, 24, 10, 0);
    let yesterday_3pm = local_time(2026, 11, 24, 15, 0);

    // Yesterday's events: 2 Claude requests ($3.50), 1 Codex request ($0.80)
    eng.store
        .upsert_events(&[
            ev(Tool::ClaudeCode, "1", yesterday_10am, 2.0),
            ev(Tool::ClaudeCode, "2", yesterday_3pm, 1.5),
            ev(Tool::Codex, "3", yesterday_3pm, 0.8),
        ])
        .unwrap();

    // Yesterday's peak gauges + today's live gauges with resets later today
    let claude_5h_reset = local_time(2026, 11, 25, 13, 30);
    let claude_wk_reset = local_time(2026, 11, 25, 21, 0);
    let codex_5h_reset = local_time(2026, 11, 25, 14, 0);

    eng.store
        .insert_gauges(&[
            gauge(Tool::ClaudeCode, "five_hour", yesterday_3pm, 82.0, Some(yesterday_3pm + Duration::hours(2))),
            gauge(Tool::ClaudeCode, "weekly", yesterday_3pm, 48.0, Some(claude_wk_reset)),
            gauge(Tool::Codex, "five_hour", yesterday_3pm, 25.0, Some(yesterday_3pm + Duration::hours(3))),
            // Fresh gauges right now: Claude 74% used, Codex 18% used
            gauge(Tool::ClaudeCode, "five_hour", now - Duration::minutes(2), 74.0, Some(claude_5h_reset)),
            gauge(Tool::ClaudeCode, "weekly", now - Duration::minutes(2), 51.0, Some(claude_wk_reset)),
            gauge(Tool::Codex, "five_hour", now - Duration::minutes(2), 18.0, Some(codex_5h_reset)),
        ])
        .unwrap();

    // 2 open tasks yesterday, 1 dismissed yesterday (should not count), 1 open today (should not count)
    for (i, ts) in [(1, yesterday_10am), (2, yesterday_3pm), (3, yesterday_3pm), (4, now - Duration::minutes(10))] {
        let mut row = TaskRow::new(
            "/dev/secret-repo".into(),
            "/dev/secret-repo".into(),
            Some("feat/secret".into()),
            Tool::ClaudeCode,
            format!("secret prompt text {i}"),
            Some("terminal".into()),
        );
        row.created_at = ts;
        let id = eng.record_task(&row).unwrap();
        if i == 3 {
            eng.dismiss_task(id).unwrap();
        }
    }

    let snap = eng.snapshot(now).unwrap();
    let recap = snap.daily_recap.expect("daily recap should be present");
    assert_eq!(recap.date, "2026-11-25");
    assert_eq!(recap.open_tasks_yesterday, 2);
    assert_eq!(recap.suggestion, "Codex has the most room this morning");

    let claude = recap.tools.iter().find(|t| t.tool == Tool::ClaudeCode).unwrap();
    assert_eq!(claude.requests, 2);
    assert!((claude.cost_usd - 3.5).abs() < 1e-6);
    assert_eq!(claude.five_hour_peak_pct, Some(82.0));
    assert_eq!(claude.weekly_peak_pct, Some(48.0));
    assert_eq!(claude.five_hour_resets_at, Some(claude_5h_reset));
    assert_eq!(claude.weekly_resets_at, Some(claude_wk_reset));

    let codex = recap.tools.iter().find(|t| t.tool == Tool::Codex).unwrap();
    assert_eq!(codex.requests, 1);
    assert!((codex.cost_usd - 0.8).abs() < 1e-6);
    assert_eq!(codex.five_hour_peak_pct, Some(25.0));
    assert_eq!(codex.five_hour_resets_at, Some(codex_5h_reset));

    // Privacy check: serialized JSON must never contain prompt text or repo/project names
    let json = serde_json::to_string(&recap).unwrap();
    assert!(!json.contains("secret"));
}

#[test]
fn daily_recap_dismissal_persists_for_day_and_returns_next_day() {
    let mut eng = Engine::in_memory().unwrap();
    let day1 = local_time(2026, 11, 25, 9, 0);
    let day0 = local_time(2026, 11, 24, 14, 0);
    eng.store.upsert_events(&[ev(Tool::ClaudeCode, "1", day0, 1.0)]).unwrap();

    assert!(eng.snapshot(day1).unwrap().daily_recap.is_some());

    eng.dismiss_daily_recap(day1).unwrap();
    assert!(eng.snapshot(day1).unwrap().daily_recap.is_none());
    assert!(eng.snapshot(day1 + Duration::hours(6)).unwrap().daily_recap.is_none());

    // Next day with activity on day1: recap reappears
    eng.store.upsert_events(&[ev(Tool::ClaudeCode, "2", day1 + Duration::hours(1), 2.0)]).unwrap();
    let day2 = local_time(2026, 11, 26, 9, 0);
    assert!(eng.snapshot(day2).unwrap().daily_recap.is_some());
}

#[test]
fn daily_recap_respects_setting_toggle() {
    let mut eng = Engine::in_memory().unwrap();
    let now = local_time(2026, 11, 25, 9, 0);
    eng.store
        .upsert_events(&[ev(Tool::ClaudeCode, "1", now - Duration::hours(20), 1.0)])
        .unwrap();

    eng.settings.show_daily_recap = false;
    assert!(eng.snapshot(now).unwrap().daily_recap.is_none());

    eng.settings.show_daily_recap = true;
    assert!(eng.snapshot(now).unwrap().daily_recap.is_some());
}

#[test]
fn weekly_recap_compares_weeks_busiest_hours_limit_hits_and_handoffs() {
    let mut eng = Engine::in_memory().unwrap();
    eng.set_licence_tier(Tier::Pro).unwrap();
    // Wednesday 2026-11-25 18:00 local (week starts Mon 2026-11-23 00:00 UTC)
    let now = local_time(2026, 11, 25, 18, 0);
    let this_tue_10 = local_time(2026, 11, 24, 10, 15);
    let this_tue_10b = local_time(2026, 11, 24, 10, 45);
    let this_wed_15 = local_time(2026, 11, 25, 15, 10);
    let last_tue_10 = local_time(2026, 11, 17, 10, 0);

    eng.store
        .upsert_events(&[
            // Last week: Claude $10.00 (1 req)
            ev(Tool::ClaudeCode, "lw1", last_tue_10, 10.0),
            // This week: Claude $15.00 (3 reqs: 2 at 10:xx, 1 at 15:xx) -> +50%
            ev(Tool::ClaudeCode, "tw1", this_tue_10, 5.0),
            ev(Tool::ClaudeCode, "tw2", this_tue_10b, 5.0),
            ev(Tool::ClaudeCode, "tw3", this_wed_15, 5.0),
            // This week only: Codex $2.00 (1 req at 10:xx) -> change_pct None
            ev(Tool::Codex, "tw4", this_tue_10, 2.0),
        ])
        .unwrap();

    // Limit hits: 2 polls in the SAME 5h window at 96% and 99% should count as 1 hit;
    // a drop to 10% and a later spike to 97% in a new window counts as a 2nd hit.
    let reset1 = this_tue_10 + Duration::hours(3);
    let reset2 = this_wed_15 + Duration::hours(3);
    eng.store
        .insert_gauges(&[
            gauge(Tool::ClaudeCode, "five_hour", this_tue_10, 96.0, Some(reset1)),
            gauge(Tool::ClaudeCode, "five_hour", this_tue_10 + Duration::minutes(2), 99.0, Some(reset1)),
            gauge(Tool::ClaudeCode, "five_hour", this_wed_15 - Duration::hours(1), 10.0, Some(reset2)),
            gauge(Tool::ClaudeCode, "five_hour", this_wed_15, 97.0, Some(reset2)),
        ])
        .unwrap();

    // 2 handoffs this week, 1 last week (should only count this week's 2)
    recap::record_handoff(&eng.store, last_tue_10).unwrap();
    recap::record_handoff(&eng.store, this_tue_10).unwrap();
    recap::record_handoff(&eng.store, this_wed_15).unwrap();

    let snap = eng.snapshot(now).unwrap();
    let weekly = &snap.weekly_recap;

    assert_eq!(weekly.handoffs_count, 2);
    assert_eq!(weekly.total_limit_hits, 2);

    let claude = weekly.tools.iter().find(|t| t.tool == Tool::ClaudeCode).unwrap();
    assert_eq!(claude.this_week_requests, 3);
    assert_eq!(claude.last_week_requests, 1);
    assert!((claude.this_week_cost_usd - 15.0).abs() < 1e-6);
    assert!((claude.last_week_cost_usd - 10.0).abs() < 1e-6);
    assert!((claude.change_pct.unwrap() - 50.0).abs() < 1e-6);
    assert_eq!(claude.five_hour_hits, 2);
    assert_eq!(claude.weekly_hits, 0);

    let codex = weekly.tools.iter().find(|t| t.tool == Tool::Codex).unwrap();
    assert_eq!(codex.this_week_requests, 1);
    assert_eq!(codex.last_week_requests, 0);
    assert!(codex.change_pct.is_none());

    // Busiest hour should be 10 (3 requests: 2 Claude + 1 Codex), then 15 (1 request)
    assert_eq!(weekly.busiest_hours.len(), 2);
    assert_eq!(weekly.busiest_hours[0].hour, 10);
    assert_eq!(weekly.busiest_hours[0].requests, 3);
    assert_eq!(weekly.busiest_hours[1].hour, 15);
    assert_eq!(weekly.busiest_hours[1].requests, 1);
    assert_eq!(weekly.hourly_requests.len(), 24);
    assert_eq!(weekly.hourly_requests[10], 3);
    assert_eq!(weekly.hourly_requests[15], 1);

    // Privacy check
    let json = serde_json::to_string(weekly).unwrap();
    assert!(!json.contains("secret"));
}

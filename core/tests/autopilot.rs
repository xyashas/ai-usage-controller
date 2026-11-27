use aiuc_core::autopilot::{self, Move};
use aiuc_core::entitlements::Tier;
use aiuc_core::model::{EventKind, GaugeSample, Tool, UsageEvent};
use aiuc_core::store::TaskRow;
use aiuc_core::Engine;
use chrono::{Duration, Utc};

fn seed_event(tool: Tool, id: &str, ts: chrono::DateTime<Utc>) -> UsageEvent {
    UsageEvent {
        dedup_key: format!("{}:{id}", tool.as_str()),
        tool,
        kind: EventKind::Assistant,
        ts,
        session_id: format!("s-{id}"),
        project: Some("/dev/repo".into()),
        model: Some("gpt-5".into()),
        input_tokens: 100,
        output_tokens: 50,
        cache_write_tokens: 0,
        cache_read_tokens: 0,
        cost_usd: 0.1,
        tool_sig: None,
    }
}

fn gauge(tool: Tool, used_pct: f64, ts: chrono::DateTime<Utc>) -> GaugeSample {
    GaugeSample {
        tool,
        window: "five_hour".into(),
        ts,
        used_pct,
        resets_at: Some(ts + Duration::hours(3)),
        window_minutes: Some(300),
        source: "test".into(),
    }
}

#[test]
fn decide_moves_terminal_task_at_97_pct_and_respects_all_guards() {
    let mut eng = Engine::in_memory().unwrap();
    eng.set_licence_tier(Tier::Pro).unwrap();
    let now = Utc::now();

    eng.store
        .upsert_events(&[
            seed_event(Tool::ClaudeCode, "c1", now - Duration::minutes(4)),
            seed_event(Tool::Codex, "x1", now - Duration::minutes(4)),
        ])
        .unwrap();
    eng.store
        .insert_gauges(&[
            gauge(Tool::ClaudeCode, 98.0, now - Duration::minutes(1)),
            gauge(Tool::Codex, 22.0, now - Duration::minutes(1)),
        ])
        .unwrap();

    // Task 1: terminal, working -> should move to Codex
    let mut r1 = TaskRow::new("/dev/repo".into(), "/dev/repo".into(), None, Tool::ClaudeCode, "fix bug".into(), Some("terminal".into()));
    r1.created_at = now - Duration::minutes(3);
    let id1 = eng.record_task(&r1).unwrap();

    // Task 2: web dest -> must NEVER move
    let mut r2 = TaskRow::new("acme/web".into(), "acme/web".into(), None, Tool::ClaudeCode, "cloud task".into(), Some("web".into()));
    r2.created_at = now - Duration::minutes(3);
    let _id2 = eng.record_task(&r2).unwrap();

    // Task 3: terminal, idle 10 min ago -> must NOT move (> 5 min)
    let mut r3 = TaskRow::new("/dev/repo".into(), "/dev/repo".into(), None, Tool::ClaudeCode, "old idle".into(), Some("terminal".into()));
    r3.created_at = now - Duration::minutes(12);
    let id3 = eng.record_task(&r3).unwrap();

    let mut snap = eng.snapshot(now).unwrap();
    for a in &mut snap.agents {
        if a.id == id1 {
            a.state = "working".into();
        } else if a.id == id3 {
            a.state = "idle".into();
            a.last_activity_at = Some(now - Duration::minutes(10));
        } else {
            a.state = "working".into();
        }
    }

    let installed = [Tool::ClaudeCode, Tool::Codex];
    let moves = autopilot::decide(&snap, &[], &installed);
    assert_eq!(
        moves,
        vec![Move {
            task_id: id1,
            from: Tool::ClaudeCode,
            to: Tool::Codex,
            cwd: "/dev/repo".into(),
        }]
    );

    // Already moved -> never move twice
    assert!(autopilot::decide(&snap, &[id1], &installed).is_empty());

    // Target CLI not installed -> no move
    assert!(autopilot::decide(&snap, &[], &[Tool::ClaudeCode]).is_empty());

    // Recent idle (within 5 min) DOES move
    snap.agents.iter_mut().find(|a| a.id == id3).unwrap().last_activity_at = Some(now - Duration::minutes(2));
    let moves2 = autopilot::decide(&snap, &[id1], &installed);
    assert_eq!(moves2.len(), 1);
    assert_eq!(moves2[0].task_id, id3);

    // Persistence & pruning (> 24 h)
    autopilot::record_move(&eng.store, id1, Tool::Codex, now - Duration::hours(25)).unwrap();
    assert!(autopilot::load_moves(&eng.store, now).is_empty());
    autopilot::record_move(&eng.store, id1, Tool::Codex, now).unwrap();
    let loaded = autopilot::load_moves(&eng.store, now);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].task_id, id1);
    assert_eq!(loaded[0].to, Tool::Codex);
}

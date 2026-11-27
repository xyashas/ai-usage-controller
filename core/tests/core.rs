use aiuc_core::advice::{self, AuditAction, AuditInput};
use aiuc_core::entitlements::Tier;
use aiuc_core::metrics::{self, Source};
use aiuc_core::model::{EventKind, GaugeSample, ParsedItem, Tool, UsageEvent};
use aiuc_core::parsers::{claude, codex, gemini};
use aiuc_core::pricing::PriceTable;
use aiuc_core::runaway::{self, RunawayConfig};
use aiuc_core::scanner;
use aiuc_core::store::Store;
use aiuc_core::Engine;
use chrono::{DateTime, Duration, TimeZone, Utc};
use std::io::Write;
use std::path::PathBuf;

fn fixtures(sub: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(sub)
}

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
}

fn t(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

// ---------------- parsers ----------------

#[test]
fn claude_fixture_parses_and_dedups() {
    let mut store = Store::open_in_memory().unwrap();
    let prices = PriceTable::default();
    let rep = scanner::scan_tool(&mut store, &prices, Tool::ClaudeCode, &[fixtures("claude")]).unwrap();
    assert_eq!(rep.files_read, 1);
    let evs = store.events(Some(Tool::ClaudeCode), t("2026-11-26T00:00:00Z"), t("2026-11-27T00:00:00Z")).unwrap();
    let asst: Vec<_> = evs.iter().filter(|e| e.kind == EventKind::Assistant).collect();
    let prompts: Vec<_> = evs.iter().filter(|e| e.kind == EventKind::UserPrompt).collect();
    assert_eq!(asst.len(), 2, "two API responses after dedup; synthetic skipped");
    assert_eq!(prompts.len(), 1, "tool_result and meta lines are not prompts");

    let a = asst.iter().find(|e| e.dedup_key == "cc:msg_01AAA:req_011").unwrap();
    assert_eq!(a.output_tokens, 120, "keeps the largest output across streamed lines");
    assert_eq!(a.cache_write_tokens, 12000);
    approx(a.cost_usd, 0.07802);
    assert_eq!(a.tool_sig.as_deref(), Some("Read:/Users/dev/acme-api/src/auth.ts"));
    assert_eq!(a.project.as_deref(), Some("/Users/dev/acme-api"));

    let b = asst.iter().find(|e| e.dedup_key == "cc:msg_01BBB:req_012").unwrap();
    approx(b.cost_usd, 0.029155);
    assert_eq!(b.tool_sig.as_deref(), Some("Edit:/Users/dev/acme-api/src/auth.ts"));
}

#[test]
fn claude_parser_ignores_garbage() {
    assert!(claude::parse_line("").is_empty());
    assert!(claude::parse_line("{not json").is_empty());
    assert!(claude::parse_line(r#"{"type":"summary","summary":"x"}"#).is_empty());
}

#[test]
fn codex_fixture_parses_tokens_gauges_and_tool_calls() {
    let mut store = Store::open_in_memory().unwrap();
    let prices = PriceTable::default();
    scanner::scan_tool(&mut store, &prices, Tool::Codex, &[fixtures("codex")]).unwrap();
    let evs = store.events(Some(Tool::Codex), t("2026-11-26T00:00:00Z"), t("2026-11-27T00:00:00Z")).unwrap();
    let asst: Vec<_> = evs.iter().filter(|e| e.kind == EventKind::Assistant).collect();
    assert_eq!(asst.len(), 2, "duplicate token_count lines dedup on cumulative total");
    let first = asst[0];
    assert_eq!(first.session_id, "0199a1f2-6a1f-7c3b-9d2e-bbbbbbbbbbbb");
    assert_eq!(first.model.as_deref(), Some("gpt-5-codex"));
    assert_eq!(first.input_tokens, 5000, "cached tokens are subtracted from input");
    assert_eq!(first.cache_read_tokens, 4000);
    approx(first.cost_usd, 0.01275);
    approx(asst[1].cost_usd, 0.007875);

    let calls: Vec<_> = evs.iter().filter(|e| e.kind == EventKind::ToolCall).collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool_sig.as_deref(), Some("shell:bash -lc npm"));
    assert_eq!(evs.iter().filter(|e| e.kind == EventKind::UserPrompt).count(), 1);

    let g = store.latest_gauge(Tool::Codex, "five_hour", t("2026-11-27T00:00:00Z")).unwrap().unwrap();
    assert_eq!(g.used_pct, 15.0);
    assert_eq!(g.resets_at, Some(t("2026-11-26T09:01:30Z") + Duration::seconds(7000)));
    let w = store.latest_gauge(Tool::Codex, "weekly", t("2026-11-27T00:00:00Z")).unwrap().unwrap();
    assert_eq!(w.used_pct, 30.0);
    assert_eq!(w.window_minutes, Some(10080));
    let mid = store.gauges(Tool::Codex, "five_hour", t("2026-11-26T09:00:59Z"), t("2026-11-26T09:01:01Z")).unwrap();
    assert_eq!(mid[0].resets_at, Some(Utc.timestamp_opt(1795600800, 0).unwrap()), "absolute resets_at is supported");
}

#[test]
fn codex_context_survives_between_scans() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rollout-x.jsonl");
    let lines: Vec<String> = std::fs::read_to_string(fixtures("codex").join("rollout-2026-11-26T09-00-00-6a1f.jsonl"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    std::fs::write(&path, format!("{}\n{}\n", lines[0], lines[1])).unwrap();
    let mut store = Store::open_in_memory().unwrap();
    let prices = PriceTable::default();
    scanner::scan_tool(&mut store, &prices, Tool::Codex, &[dir.path().to_path_buf()]).unwrap();
    let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    for l in &lines[2..] {
        writeln!(f, "{l}").unwrap();
    }
    drop(f);
    scanner::scan_tool(&mut store, &prices, Tool::Codex, &[dir.path().to_path_buf()]).unwrap();
    let evs = store.events(Some(Tool::Codex), t("2026-11-26T00:00:00Z"), t("2026-11-27T00:00:00Z")).unwrap();
    let a = evs.iter().find(|e| e.kind == EventKind::Assistant).unwrap();
    assert_eq!(a.session_id, "0199a1f2-6a1f-7c3b-9d2e-bbbbbbbbbbbb", "session id from the first scan is kept");
    assert_eq!(a.model.as_deref(), Some("gpt-5-codex"));
}

#[test]
fn gemini_fixture_counts_requests() {
    let mut store = Store::open_in_memory().unwrap();
    let prices = PriceTable::default();
    scanner::scan_tool(&mut store, &prices, Tool::Gemini, &[fixtures("gemini/tmp")]).unwrap();
    // second scan of the unchanged file is a no-op
    let rep = scanner::scan_tool(&mut store, &prices, Tool::Gemini, &[fixtures("gemini/tmp")]).unwrap();
    assert_eq!(rep.files_read, 0);
    let evs = store.events(Some(Tool::Gemini), t("2026-11-26T00:00:00Z"), t("2026-11-27T00:00:00Z")).unwrap();
    let asst: Vec<_> = evs.iter().filter(|e| e.kind == EventKind::Assistant).collect();
    assert_eq!(asst.len(), 2, "info messages are not requests");
    assert_eq!(asst[0].output_tokens, 1000, "thoughts billed as output");
    assert_eq!(asst[0].cache_read_tokens, 2000);
    approx(asst[0].cost_usd, 0.01812);
    approx(asst[1].cost_usd, 0.00115);
    assert_eq!(asst[0].tool_sig.as_deref(), Some("read_file:/home/dev/proj/README.md"));
}

#[test]
fn incremental_scan_reads_only_complete_new_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let line = |id: &str, ts: &str| {
        format!(
            r#"{{"type":"assistant","sessionId":"s1","timestamp":"{ts}","requestId":"r{id}","message":{{"id":"m{id}","model":"claude-sonnet-4-5","usage":{{"input_tokens":1000,"output_tokens":100}}}}}}"#
        )
    };
    std::fs::write(&path, format!("{}\n{}", line("1", "2026-11-26T10:00:00Z"), &line("2", "2026-11-26T10:01:00Z")[..40])).unwrap();
    let mut store = Store::open_in_memory().unwrap();
    let prices = PriceTable::default();
    scanner::scan_tool(&mut store, &prices, Tool::ClaudeCode, &[dir.path().to_path_buf()]).unwrap();
    let n = |s: &Store| s.events(None, t("2026-11-26T00:00:00Z"), t("2026-11-27T00:00:00Z")).unwrap().len();
    assert_eq!(n(&store), 1, "partial trailing line is not consumed");
    // finish the partial line
    std::fs::write(&path, format!("{}\n{}\n", line("1", "2026-11-26T10:00:00Z"), line("2", "2026-11-26T10:01:00Z"))).unwrap();
    scanner::scan_tool(&mut store, &prices, Tool::ClaudeCode, &[dir.path().to_path_buf()]).unwrap();
    assert_eq!(n(&store), 2);
    // sonnet 4.5: 1000*3 + 100*15 = 4500 / 1e6
    let e = &store.events(None, t("2026-11-26T00:00:00Z"), t("2026-11-27T00:00:00Z")).unwrap()[1];
    approx(e.cost_usd, 0.0045);
}

#[test]
fn pricing_uses_longest_match() {
    let p = PriceTable::default();
    assert_eq!(p.lookup("claude-opus-4-5-20251101").unwrap().pattern, "claude-opus-4-5");
    assert_eq!(p.lookup("claude-opus-4-20250514").unwrap().pattern, "claude-opus-4");
    assert_eq!(p.lookup("claude-sonnet-5-5").unwrap().pattern, "claude-sonnet-5-5");
    assert_eq!(p.lookup("gpt-5-codex").unwrap().pattern, "gpt-5-codex");
    assert_eq!(p.lookup("gemini-2.5-flash-lite").unwrap().pattern, "gemini-2.5-flash-lite");
    assert!(p.lookup("mystery-model").is_none());
    approx(p.cost_for("claude-opus-5-5", 1_000_000, 1_000_000, 0, 0), 24.0);
}

// ---------------- metrics ----------------

fn ev(tool: Tool, kind: EventKind, ts: DateTime<Utc>, session: &str, cost: f64) -> UsageEvent {
    let mut e = UsageEvent::new(tool, kind, format!("{}:{}:{}", session, ts.timestamp_millis(), rand_suffix()), ts, session.into());
    e.cost_usd = cost;
    e.model = Some(match tool {
        Tool::ClaudeCode => "claude-opus-4-5".into(),
        Tool::Codex => "gpt-5-codex".into(),
        Tool::Gemini => "gemini-2.5-pro".into(),
    });
    e
}

fn rand_suffix() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

#[test]
fn five_hour_blocks_follow_claude_rules() {
    let base = t("2026-11-26T09:17:00Z");
    let evs = vec![
        ev(Tool::ClaudeCode, EventKind::Assistant, base, "s", 1.0),
        ev(Tool::ClaudeCode, EventKind::Assistant, base + Duration::hours(2), "s", 2.0),
        ev(Tool::ClaudeCode, EventKind::Assistant, base + Duration::hours(6), "s", 4.0),
    ];
    let blocks = metrics::five_hour_blocks(&evs);
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].0, t("2026-11-26T09:00:00Z"));
    assert_eq!(blocks[0].1, t("2026-11-26T14:00:00Z"));
    assert_eq!(blocks[0].2, 3.0);
    let cur = metrics::current_block(&evs, base + Duration::hours(6) + Duration::minutes(1)).unwrap();
    assert_eq!(cur.0, t("2026-11-26T15:00:00Z"));
}

#[test]
fn time_to_cutoff_rounds_to_five() {
    assert_eq!(metrics::round5(22.4), 20);
    assert_eq!(metrics::round5(23.0), 25);
    assert_eq!(metrics::round5(1.0), 5);
    assert_eq!(metrics::minutes_to_cutoff(40.0, 2.0), Some(20.0));
    assert_eq!(metrics::minutes_to_cutoff(40.0, 0.0), None);
}

#[test]
fn gemini_resets_at_pacific_midnight() {
    let (s, e) = metrics::gemini_day_bounds(t("2026-11-26T20:00:00Z"));
    assert_eq!(s, t("2026-11-26T08:00:00Z")); // PST = UTC-8
    assert_eq!(e, t("2026-11-27T08:00:00Z"));
}

// ---------------- engine ----------------

fn engine_with(events: Vec<UsageEvent>, gauges: Vec<GaugeSample>) -> Engine {
    let mut e = Engine::in_memory().unwrap();
    e.store.upsert_events(&events).unwrap();
    e.store.insert_gauges(&gauges).unwrap();
    e
}

fn codex_gauge(ts: DateTime<Utc>, pct: f64, resets: DateTime<Utc>) -> GaugeSample {
    GaugeSample { tool: Tool::Codex, window: "five_hour".into(), used_pct: pct, resets_at: Some(resets), window_minutes: Some(300), ts, source: "codex_log".into() }
}

#[test]
fn codex_live_gauge_drives_headroom_and_cutoff() {
    let now = t("2026-11-26T12:00:00Z");
    let resets = now + Duration::hours(2);
    let mut events = vec![];
    for i in 0..10 {
        events.push(ev(Tool::Codex, EventKind::Assistant, now - Duration::minutes(10) + Duration::minutes(i), "cx", 0.2));
    }
    let gauges = vec![codex_gauge(now - Duration::minutes(10), 60.0, resets), codex_gauge(now - Duration::seconds(10), 80.0, resets)];
    let mut eng = engine_with(events, gauges);
    eng.set_licence_tier(Tier::Pro).unwrap();
    let s = eng.snapshot(now).unwrap();
    let cx = s.tools.iter().find(|t| t.tool == Tool::Codex).unwrap();
    assert_eq!(cx.source, Source::Live);
    approx(cx.headroom_pct.unwrap(), 20.0);
    assert_eq!(cx.color, "amber");
    // 20 points in ~9.8 minutes ≈ 2.04%/min → 20% left ≈ 9.8 min → rounds to 10
    assert_eq!(cx.minutes_left, Some(10));
    assert!(s.alerts.iter().any(|a| a.kind == "low_headroom" && a.level == "amber" && a.tool == Tool::Codex));
}

#[test]
fn expired_gauge_means_window_reset() {
    let now = t("2026-11-26T12:00:00Z");
    let gauges = vec![codex_gauge(now - Duration::hours(3), 95.0, now - Duration::minutes(5))];
    let events = vec![ev(Tool::Codex, EventKind::Assistant, now - Duration::hours(3), "cx", 1.0)];
    let mut eng = engine_with(events, gauges);
    let s = eng.snapshot(now).unwrap();
    let cx = s.tools.iter().find(|t| t.tool == Tool::Codex).unwrap();
    approx(cx.headroom_pct.unwrap(), 100.0);
}

#[test]
fn claude_estimate_uses_historical_peak_when_no_gauge() {
    let now = t("2026-11-26T12:30:00Z");
    let mut events = vec![];
    // three completed blocks with peak $10
    for (d, c) in [(5, 4.0), (4, 10.0), (3, 6.0)] {
        events.push(ev(Tool::ClaudeCode, EventKind::Assistant, now - Duration::days(d), "old", c));
    }
    // current block: $7.5 used
    events.push(ev(Tool::ClaudeCode, EventKind::Assistant, now - Duration::minutes(20), "cur", 7.5));
    let mut eng = engine_with(events, vec![]);
    eng.settings.live_gauges = false;
    let s = eng.snapshot(now).unwrap();
    let cc = s.tools.iter().find(|t| t.tool == Tool::ClaudeCode).unwrap();
    let w = cc.windows.iter().find(|w| w.key == "five_hour").unwrap();
    assert_eq!(w.source, Source::Estimate);
    approx(w.allowance_usd.unwrap(), 10.0);
    approx(cc.headroom_pct.unwrap(), 25.0);
    assert_eq!(w.resets_at, Some(t("2026-11-26T17:00:00Z")));
}

#[test]
fn new_user_is_learning_not_alarmed() {
    let now = t("2026-11-26T12:30:00Z");
    let events = vec![ev(Tool::ClaudeCode, EventKind::Assistant, now - Duration::minutes(2), "s", 50.0)];
    let mut eng = engine_with(events, vec![]);
    let s = eng.snapshot(now).unwrap();
    let cc = s.tools.iter().find(|t| t.tool == Tool::ClaudeCode).unwrap();
    assert_eq!(cc.source, Source::Learning);
    assert_eq!(cc.color, "grey");
    assert!(s.alerts.is_empty());
}

#[test]
fn gemini_headroom_counts_requests_against_plan() {
    let now = t("2026-11-26T20:00:00Z");
    let mut events = vec![];
    for i in 0..300 {
        events.push(ev(Tool::Gemini, EventKind::Assistant, now - Duration::minutes(300) + Duration::seconds(i * 30), "g", 0.001));
    }
    let mut eng = engine_with(events, vec![]);
    let mut ts = eng.settings.tool(Tool::Gemini);
    ts.plan_id = Some("ai_pro".into());
    eng.settings.tools.insert(Tool::Gemini, ts);
    let s = eng.snapshot(now).unwrap();
    let g = s.tools.iter().find(|t| t.tool == Tool::Gemini).unwrap();
    let w = &g.windows[0];
    assert_eq!(w.used_requests, Some(300));
    assert_eq!(w.allowance_requests, Some(1500));
    approx(g.headroom_pct.unwrap(), 80.0);
    assert_eq!(w.resets_at, Some(t("2026-11-27T08:00:00Z")));
}

#[test]
fn free_tier_shows_one_tool_and_locks_pro_features() {
    let now = t("2026-11-26T12:00:00Z");
    let resets = now + Duration::hours(2);
    let events = vec![
        ev(Tool::Codex, EventKind::Assistant, now - Duration::minutes(1), "cx", 0.5),
        ev(Tool::Gemini, EventKind::Assistant, now - Duration::hours(1), "g", 0.1),
    ];
    let mut eng = engine_with(events, vec![codex_gauge(now - Duration::minutes(1), 50.0, resets)]);
    eng.settings.primary_tool = Some(Tool::Codex);
    let s = eng.snapshot(now).unwrap();
    assert_eq!(s.entitlements.tier, Tier::Free);
    assert!(s.entitlements.trial_available);
    let cx = s.tools.iter().find(|t| t.tool == Tool::Codex).unwrap();
    assert!(!cx.locked);
    assert!(cx.headroom_pct.is_some(), "free gets live headroom");
    assert!(cx.windows.iter().any(|w| w.resets_at.is_some()), "free gets reset timers");
    assert!(cx.minutes_left.is_none(), "time-to-cutoff is Pro");
    assert!(s.tools.iter().find(|t| t.tool == Tool::Gemini).unwrap().locked);
    assert!(s.session_meter.is_none());
    assert!(s.locked.contains(&"runaway".to_string()));

    aiuc_core::entitlements::start_trial(&eng.store, now).unwrap();
    let s = eng.snapshot(now).unwrap();
    assert_eq!(s.entitlements.tier, Tier::Trial);
    assert!(s.session_meter.is_some());
    assert!(s.locked.is_empty());
    let s = eng.snapshot(now + Duration::days(8)).unwrap();
    assert_eq!(s.entitlements.tier, Tier::Free, "trial ends after 7 days");
}

// ---------------- runaway ----------------

fn looping_session(now: DateTime<Utc>, turns: usize, cost_each: f64, repeat: bool) -> Vec<UsageEvent> {
    let mut v = vec![ev(Tool::ClaudeCode, EventKind::UserPrompt, now - Duration::minutes(7), "loop", 0.0)];
    for i in 0..turns {
        let mut e = ev(Tool::ClaudeCode, EventKind::Assistant, now - Duration::minutes(6) + Duration::seconds(i as i64 * 20), "loop", cost_each);
        e.tool_sig = Some(if repeat { format!("Edit:src/a.rs|Bash:cargo test{}", if i % 2 == 0 { "" } else { "" }) } else { format!("Edit:src/file{i}.rs") });
        v.push(e);
    }
    v
}

#[test]
fn runaway_flags_repeating_expensive_loop() {
    let now = t("2026-11-26T12:00:00Z");
    let sess = looping_session(now, 18, 0.25, true);
    let sig = runaway::detect(&sess, 0.5, now, &RunawayConfig::default()).expect("should flag");
    assert!(sig.turns >= 10);
    assert!(sig.message.contains("possible loop"));
    assert!(sig.spend_usd > 2.0);
}

#[test]
fn runaway_ignores_varied_work_and_cheap_work_and_short_runs() {
    let now = t("2026-11-26T12:00:00Z");
    let cfg = RunawayConfig::default();
    assert!(runaway::detect(&looping_session(now, 18, 0.25, false), 0.5, now, &cfg).is_none(), "different files each turn");
    assert!(runaway::detect(&looping_session(now, 18, 0.01, true), 0.5, now, &cfg).is_none(), "no spend spike");
    assert!(runaway::detect(&looping_session(now, 6, 1.0, true), 0.5, now, &cfg).is_none(), "fewer than 10 turns");
    let mut s = looping_session(now, 18, 0.25, true);
    s.insert(10, ev(Tool::ClaudeCode, EventKind::UserPrompt, now - Duration::minutes(3), "loop", 0.0));
    assert!(runaway::detect(&s, 0.5, now, &cfg).is_none(), "user is steering");
}

#[test]
fn runaway_baseline_uses_active_buckets() {
    let now = t("2026-11-26T12:00:00Z");
    let hist = vec![
        ev(Tool::ClaudeCode, EventKind::Assistant, now - Duration::days(1), "h", 1.0),
        ev(Tool::ClaudeCode, EventKind::Assistant, now - Duration::days(2), "h", 3.0),
    ];
    approx(runaway::baseline_5m(&hist, now, 0.5), 2.0);
    approx(runaway::baseline_5m(&[], now, 0.5), 0.5);
}

// ---------------- advice ----------------

#[test]
fn model_nudge_prices_small_opus_tasks_at_sonnet() {
    let prices = PriceTable::default();
    let now = t("2026-11-26T12:00:00Z");
    let mut day = vec![];
    for i in 0..340 {
        let mut e = ev(Tool::ClaudeCode, EventKind::Assistant, now - Duration::seconds(i * 60), "n", 0.0);
        e.model = Some("claude-opus-4-1-20250805".into());
        e.input_tokens = 2000;
        e.output_tokens = 300;
        e.cache_read_tokens = 20_000;
        e.cost_usd = prices.cost(&e);
        day.push(e);
    }
    let n = advice::model_nudge(&day, &prices, "2026-11-26").expect("nudge");
    assert_eq!(n.count, 340);
    assert_eq!(n.to_label, "Sonnet");
    // opus-4-1: 2000*15+300*75+20000*1.5 = 82500; sonnet-4: 2000*3+300*15+20000*0.3 = 16500 → $0.066 each
    approx(n.saving_usd, 340.0 * 0.066);
    assert!(n.message.starts_with("You ran 340 small edits on Opus today. Sonnet would cost about $22 less."), "{}", n.message);

    assert!(advice::model_nudge(&day[..10], &prices, "d").is_none(), "too few to nudge");
}

fn plan(id: &str, price: f64, cap: f64) -> aiuc_core::plans::Plan {
    aiuc_core::plans::Plan { id: id.into(), name: id.into(), price, capacity: cap, metered: false, daily_requests: None }
}

#[test]
fn audit_rules() {
    let tiers = vec![plan("pro", 20.0, 1.0), plan("max5", 100.0, 5.0), plan("max20", 200.0, 20.0)];
    let base = AuditInput {
        tool: Tool::ClaudeCode,
        plan: plan("max20", 200.0, 20.0),
        tiers: tiers.clone(),
        weekly_peaks: vec![Some(10.0), Some(12.0), Some(9.0), Some(11.0)],
        cap_hit_days: vec![0, 0],
        active_days_30: 20,
        data_days: 40,
    };
    let r = advice::audit(&base);
    assert_eq!(r.action, AuditAction::Downgrade, "12% of 20x = 48% of 5x < 60%");
    assert_eq!(r.target_plan_id.as_deref(), Some("max5"));
    approx(r.monthly_saving_usd, 100.0);
    assert_eq!(r.headline, "You used 12% of max20. max5 saves you $100/month.");

    let keep = advice::audit(&AuditInput { weekly_peaks: vec![Some(10.0), Some(20.0), Some(9.0), Some(11.0)], ..base.clone() });
    assert_eq!(keep.action, AuditAction::Keep, "20% of 20x = 80% of 5x");

    let gap = advice::audit(&AuditInput { weekly_peaks: vec![Some(10.0), None, Some(9.0), Some(11.0)], ..base.clone() });
    assert_eq!(gap.action, AuditAction::Keep, "needs 4 full weeks");

    let up = advice::audit(&AuditInput { plan: plan("pro", 20.0, 1.0), cap_hit_days: vec![1, 3], ..base.clone() });
    assert_eq!(up.action, AuditAction::Upgrade);
    assert_eq!(up.target_plan_id.as_deref(), Some("max5"));

    let cancel = advice::audit(&AuditInput { active_days_30: 3, ..base.clone() });
    assert_eq!(cancel.action, AuditAction::Cancel);
    approx(cancel.monthly_saving_usd, 200.0);

    let early = advice::audit(&AuditInput { data_days: 10, ..base });
    assert_eq!(early.action, AuditAction::NotEnoughData);
}

#[test]
fn engine_audit_reads_weekly_gauges() {
    let now = t("2026-11-26T12:00:00Z");
    let this_week = metrics::week_start(now);
    let mut events = vec![];
    let mut gauges = vec![];
    for k in 1..=5 {
        let ws = this_week - Duration::days(7 * k);
        for d in 0..5 {
            events.push(ev(Tool::Codex, EventKind::Assistant, ws + Duration::days(d) + Duration::hours(10), "a", 1.0));
        }
        gauges.push(GaugeSample { tool: Tool::Codex, window: "weekly".into(), used_pct: 5.0, resets_at: None, window_minutes: Some(10080), ts: ws + Duration::days(3), source: "codex_log".into() });
    }
    let mut eng = engine_with(events, gauges);
    let mut ts = eng.settings.tool(Tool::Codex);
    ts.plan_id = Some("pro".into());
    eng.settings.tools.insert(Tool::Codex, ts);
    let r = eng.audit_tool(Tool::Codex, now).unwrap().unwrap();
    assert_eq!(r.action, AuditAction::Downgrade, "{:?}", r);
    assert_eq!(r.target_plan_id.as_deref(), Some("business"));
}

// ---------------- savings + sync privacy ----------------

#[test]
fn savings_counter_sums_ledger_this_month() {
    let now = Utc::now();
    let mut eng = Engine::in_memory().unwrap();
    eng.set_licence_tier(Tier::Pro).unwrap();
    eng.store.add_saving(now, "plan", 100.0, "Max 20x → Max 5x").unwrap();
    eng.store.add_saving(now, "runaway", 12.0, "stopped loop").unwrap();
    eng.store.add_saving(now - Duration::days(70), "plan", 50.0, "old").unwrap();
    let s = eng.snapshot(now).unwrap();
    approx(s.savings_month_usd.unwrap(), 112.0);
}

#[test]
fn sync_payload_contains_no_paths_or_signatures() {
    let mut store = Store::open_in_memory().unwrap();
    let prices = PriceTable::default();
    scanner::scan_tool(&mut store, &prices, Tool::ClaudeCode, &[fixtures("claude")]).unwrap();
    let payload = aiuc_core::sync::payload(&store, 100).unwrap();
    assert_eq!(payload.len(), 2, "only assistant usage events are synced");
    let json = serde_json::to_string(&payload).unwrap();
    assert!(!json.contains("/Users/dev"), "no project paths");
    assert!(!json.contains("auth.ts"), "no file names");
    assert!(!json.contains("Refactor"), "no prompt text");
}

#[test]
fn live_claude_gauge_parsing() {
    let v: serde_json::Value = serde_json::json!({
        "five_hour": {"utilization": 42.0, "resets_at": "2026-11-26T15:00:00Z"},
        "seven_day": {"utilization": 18.5, "resets_at": "2026-11-30T09:00:00Z"},
        "seven_day_opus": null
    });
    let g = aiuc_core::live::parse_claude_usage(&v, t("2026-11-26T12:00:00Z"));
    assert_eq!(g.len(), 2);
    assert_eq!(g[0].window, "five_hour");
    assert_eq!(g[0].used_pct, 42.0);
    assert_eq!(g[1].resets_at, Some(t("2026-11-30T09:00:00Z")));
}

#[test]
fn parsers_report_versions() {
    assert_eq!(claude::PARSER_VERSION, 1);
    assert_eq!(codex::PARSER_VERSION, 1);
    assert_eq!(gemini::PARSER_VERSION, 1);
    let _ = ParsedItem::Event; // public API sanity
}


#[test]
fn claude_web_gauge_shows_plan_usage_without_claude_code_logs() {
    // claude.ai / desktop-app users have no local logs; the web gauge alone must drive the bar.
    let now = t("2026-11-26T12:00:00Z");
    let mut eng = Engine::in_memory().unwrap();
    eng.settings.claude_web = true;
    eng.settings.onboarded = true;
    let before = eng.snapshot(now).unwrap();
    let c = before.tools.iter().find(|x| x.tool == Tool::ClaudeCode).unwrap();
    assert!(c.detected, "a connected claude.ai account counts as detected");
    assert!(c.needs_connect, "no gauge yet, so no real numbers");

    let v = serde_json::json!({
        "five_hour": {"utilization": 73.0, "resets_at": "2026-11-26T14:00:00Z"},
        "seven_day": {"utilization": 30.0, "resets_at": "2026-11-30T09:00:00Z"}
    });
    let g = aiuc_core::live::parse_claude_usage_from(&v, now, "claude_web");
    eng.ingest_live(Tool::ClaudeCode, Ok(g));
    let s = eng.snapshot(now).unwrap();
    let c = s.tools.iter().find(|x| x.tool == Tool::ClaudeCode).unwrap();
    assert_eq!(c.source, Source::Live);
    assert!(!c.needs_connect);
    assert!((c.headroom_pct.unwrap() - 27.0).abs() < 1e-9, "73% used = 27% headroom");
    let five = c.windows.iter().find(|w| w.key == "five_hour").unwrap();
    assert_eq!(five.resets_at, Some(t("2026-11-26T14:00:00Z")));

    let d = eng.diagnostics(now);
    let dc = d.iter().find(|x| x.tool == Tool::ClaudeCode).unwrap();
    assert_eq!(dc.gauge_source.as_deref(), Some("claude_web"));
    assert_eq!(dc.gauge_used_pct, Some(73.0));

    eng.ingest_live(Tool::ClaudeCode, Err("signed out".into()));
    assert_eq!(eng.diagnostics(now)[0].live_error.as_deref(), Some("signed out"));
}

#[test]
fn claude_poll_health_distinguishes_age_network_and_session_expiry() {
    let now = t("2026-11-26T12:00:00Z");
    let mut eng = Engine::in_memory().unwrap();
    eng.settings.claude_web = true;
    let gauge = |ts| aiuc_core::live::parse_claude_usage_from(
        &serde_json::json!({"five_hour": {"utilization": 50, "resets_at": "2026-11-26T14:00:00Z"}}),
        ts, "claude_web");
    let note = |eng: &mut Engine, ts| eng.snapshot(ts).unwrap().tools.into_iter()
        .find(|s| s.tool == Tool::ClaudeCode).unwrap().live_note;
    assert!(!eng.ingest_web(Ok(gauge(now))).unwrap());
    assert_eq!(note(&mut eng, now + Duration::minutes(3)), None);
    assert_eq!(note(&mut eng, now + Duration::minutes(5)).as_deref(), Some("Updated 5 min ago"));
    assert!(!eng.ingest_web(Err("network error".into())).unwrap());
    assert_eq!(note(&mut eng, now).as_deref(), Some("Offline"));
    assert!(eng.ingest_web(Err("signed_out".into())).unwrap());
    eng.live_errors.clear(); // A new process loses transient errors.
    assert_eq!(note(&mut eng, now).as_deref(), Some("Reconnect"));
    assert!(!eng.ingest_web(Err("signed_out".into())).unwrap());
    eng.ingest_web(Ok(gauge(now))).unwrap();
    assert_eq!(note(&mut eng, now), None);
    assert!(eng.ingest_web(Err("signed_out".into())).unwrap());
}

#[test]
fn idle_pill_uses_logged_activity_and_increases_in_web_usage() {
    let now = t("2026-11-26T12:00:00Z");
    let mut eng = engine_with(vec![ev(Tool::Gemini, EventKind::Assistant, now, "session", 0.1)], vec![]);
    let legacy: aiuc_core::settings::Settings = serde_json::from_str("{}").unwrap();
    assert!(legacy.launch_at_login && !legacy.hide_when_idle);
    eng.settings.onboarded = true;
    eng.settings.hide_when_idle = true;
    assert!(!eng.snapshot(now + Duration::minutes(29)).unwrap().idle_hidden);
    let later = now + Duration::minutes(30);
    assert!(eng.snapshot(later).unwrap().idle_hidden);
    eng.settings.claude_web = true;
    for (offset, pct, hidden) in [(0, 10, true), (1, 10, true), (2, 11, false)] {
        let ts = later + Duration::minutes(offset);
        let gauges = aiuc_core::live::parse_claude_usage_from(
            &serde_json::json!({"five_hour":{"utilization":pct,"resets_at":"2026-11-26T15:00:00Z"}}), ts, "claude_web");
        eng.ingest_web(Ok(gauges)).unwrap();
        assert_eq!(eng.snapshot(ts).unwrap().idle_hidden, hidden);
    }
    assert!(eng.snapshot(later + Duration::minutes(32)).unwrap().idle_hidden);
}

#[test]
fn history_tracks_only_the_binding_window_inside_the_last_day() {
    let now = t("2026-11-26T12:00:00Z");
    let reset = now + Duration::hours(1);
    let mut gauges = vec![
        codex_gauge(now - Duration::hours(25), 50.0, reset),
        codex_gauge(now - Duration::hours(2), 93.0, now - Duration::hours(1)),
        codex_gauge(now - Duration::seconds(20), 20.0, reset),
        codex_gauge(now - Duration::seconds(10), 25.0, reset),
    ];
    let mut weekly = codex_gauge(now, 5.0, now + Duration::days(6));
    weekly.window = "weekly".into();
    gauges.push(weekly);
    let mut eng = engine_with(vec![ev(Tool::Codex, EventKind::Assistant, now, "a", 0.1)], gauges);
    eng.settings.primary_tool = Some(Tool::Codex);
    let history = eng.snapshot(now).unwrap().gauge_history;
    assert_eq!(history.len(), 2);
    approx(history[0].used, 93.0);
    approx(history[1].used, 25.0);
    assert_eq!(history[0].reset_at, Some((now - Duration::hours(1)).timestamp_millis()));
}

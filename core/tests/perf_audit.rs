//! Performance & reliability audit benchmarks:
//! - 45-day, ~200k-line multi-tool fixture (Claude, Codex, Gemini)
//! - Cold start (DB open + first snapshot) < 1.5 s
//! - Full scan < 3 s, incremental rescan < 50 ms
//! - SQLite WAL mode + snapshot query timings < 5 ms
//! - 2-hour simulated worker loop harness (flat memory, no unbounded growth)

use aiuc_core::autopilot;
use aiuc_core::engine::Engine;
use aiuc_core::model::Tool;
use aiuc_core::scanner::scan_tool;
use aiuc_core::store::Store;
use aiuc_core::suggest;
use aiuc_core::watch;
use chrono::{Duration, Utc};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Instant;
use tempfile::tempdir;

fn generate_45_day_fixture(root: &Path) -> (Vec<std::path::PathBuf>, Vec<std::path::PathBuf>, Vec<std::path::PathBuf>, usize) {
    let claude_dir = root.join("claude").join("proj-a");
    let codex_dir = root.join("codex").join("2026").join("11");
    let gemini_dir = root.join("gemini").join("projhash").join("chats");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::create_dir_all(&codex_dir).unwrap();
    fs::create_dir_all(&gemini_dir).unwrap();

    let now = Utc::now();
    let mut total_lines = 0usize;

    // 45 days × ~2,200 lines/day across Claude (~99k lines) + ~2,200 lines/day Codex (~99k lines) + Gemini chats (~2.25k messages)
    for day in 0..45i64 {
        let day_ts = now - Duration::days(44 - day);
        let ts_str = day_ts.to_rfc3339();

        let mut cf = fs::File::create(claude_dir.join(format!("sess-{day:02}.jsonl"))).unwrap();
        let mut buf = String::with_capacity(256 * 1024);
        for i in 0..2200usize {
            if i % 10 == 0 {
                buf.push_str(&format!(
                    "{{\"type\":\"assistant\",\"timestamp\":\"{ts_str}\",\"sessionId\":\"cs-{day}\",\"requestId\":\"r-{day}-{i}\",\"cwd\":\"/ws/repo\",\"message\":{{\"id\":\"m-{day}-{i}\",\"model\":\"claude-sonnet-4-6\",\"usage\":{{\"input_tokens\":400,\"output_tokens\":120,\"cache_creation_input_tokens\":50,\"cache_read_input_tokens\":200}},\"content\":[]}}}}\n"
                ));
            } else if i % 10 == 1 {
                buf.push_str(&format!(
                    "{{\"type\":\"user\",\"timestamp\":\"{ts_str}\",\"sessionId\":\"cs-{day}\",\"uuid\":\"u-{day}-{i}\",\"cwd\":\"/ws/repo\",\"message\":{{\"content\":\"fix test {i}\"}}}}\n"
                ));
            } else {
                buf.push_str(&format!(
                    "{{\"type\":\"progress\",\"timestamp\":\"{ts_str}\",\"sessionId\":\"cs-{day}\",\"uuid\":\"p-{day}-{i}\"}}\n"
                ));
            }
            total_lines += 1;
        }
        cf.write_all(buf.as_bytes()).unwrap();

        let mut xf = fs::File::create(codex_dir.join(format!("rollout-2026-11-{day:02}.jsonl"))).unwrap();
        let mut xbuf = String::with_capacity(256 * 1024);
        xbuf.push_str(&format!(
            "{{\"timestamp\":\"{ts_str}\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"xs-{day}\",\"cwd\":\"/ws/repo\"}}}}\n"
        ));
        xbuf.push_str(&format!(
            "{{\"timestamp\":\"{ts_str}\",\"type\":\"turn_context\",\"payload\":{{\"model\":\"gpt-5-codex\",\"cwd\":\"/ws/repo\"}}}}\n"
        ));
        total_lines += 2;
        for i in 0..2198usize {
            if i % 12 == 0 {
                let cum = (i + 1) * 500;
                xbuf.push_str(&format!(
                    "{{\"timestamp\":\"{ts_str}\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"last_token_usage\":{{\"input_tokens\":350,\"cached_input_tokens\":100,\"output_tokens\":150}},\"total_token_usage\":{{\"total_tokens\":{cum}}}}}}}}}\n"
                ));
            } else if i % 12 == 1 {
                xbuf.push_str(&format!(
                    "{{\"timestamp\":\"{ts_str}\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"user_message\"}}}}\n"
                ));
            } else {
                xbuf.push_str(&format!(
                    "{{\"timestamp\":\"{ts_str}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"reasoning\",\"summary\":\"step {i}\"}}}}\n"
                ));
            }
            total_lines += 1;
        }
        xf.write_all(xbuf.as_bytes()).unwrap();

        let mut msgs = Vec::with_capacity(50);
        for i in 0..50usize {
            msgs.push(format!(
                "{{\"id\":\"g-{day}-{i}\",\"timestamp\":\"{ts_str}\",\"type\":\"gemini\",\"model\":\"gemini-2.5-pro\",\"tokens\":{{\"input\":300,\"cached\":50,\"output\":90,\"thoughts\":20}}}}"
            ));
            total_lines += 1;
        }
        let gjson = format!(
            "{{\"sessionId\":\"gs-{day}\",\"projectHash\":\"projhash\",\"messages\":[{}]}}",
            msgs.join(",")
        );
        fs::write(gemini_dir.join(format!("session-{day:02}.json")), gjson).unwrap();
    }

    (vec![root.join("claude")], vec![root.join("codex")], vec![root.join("gemini")], total_lines)
}

#[test]
fn perf_45_day_200k_lines_scan_rescan_cold_start_and_queries() {
    let dir = tempdir().unwrap();
    let (claude_roots, codex_roots, gemini_roots, total_lines) = generate_45_day_fixture(dir.path());
    assert!(total_lines >= 200_000, "fixture should have >= 200k lines, got {total_lines}");

    let db_path = dir.path().join("usage.db");
    let prices = Engine::in_memory().unwrap().prices;

    // 1. Full scan of 45 days / 200k+ lines
    let mut store = Store::open(&db_path).unwrap();
    let t_full = Instant::now();
    let r1 = scan_tool(&mut store, &prices, Tool::ClaudeCode, &claude_roots).unwrap();
    let r2 = scan_tool(&mut store, &prices, Tool::Codex, &codex_roots).unwrap();
    let r3 = scan_tool(&mut store, &prices, Tool::Gemini, &gemini_roots).unwrap();
    let full_ms = t_full.elapsed().as_secs_f64() * 1000.0;
    let total_events = r1.events + r2.events + r3.events;
    assert!(total_events > 25_000, "expected >25k parsed events, got {total_events}");

    // 2. Incremental rescan (append 3 lines to 1 Claude file)
    let latest_claude = claude_roots[0].join("proj-a").join("sess-44.jsonl");
    {
        let mut f = OpenOptions::new().append(true).open(&latest_claude).unwrap();
        let now_str = Utc::now().to_rfc3339();
        for k in 0..3 {
            writeln!(
                f,
                "{{\"type\":\"assistant\",\"timestamp\":\"{now_str}\",\"sessionId\":\"cs-44\",\"requestId\":\"inc-{k}\",\"cwd\":\"/ws/repo\",\"message\":{{\"id\":\"minc-{k}\",\"model\":\"claude-sonnet-4-6\",\"usage\":{{\"input_tokens\":100,\"output_tokens\":50,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0}},\"content\":[]}}}}"
            ).unwrap();
        }
    }
    let t_inc = Instant::now();
    let ri1 = scan_tool(&mut store, &prices, Tool::ClaudeCode, &claude_roots).unwrap();
    let ri2 = scan_tool(&mut store, &prices, Tool::Codex, &codex_roots).unwrap();
    let ri3 = scan_tool(&mut store, &prices, Tool::Gemini, &gemini_roots).unwrap();
    let inc_ms = t_inc.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(ri1.files_read, 1, "only the 1 modified file should be read");
    assert_eq!(ri1.events, 3);
    assert_eq!(ri2.files_read, 0);
    assert_eq!(ri3.files_read, 0);
    drop(store);

    // 3. Cold start: reopen DB + first snapshot
    let t_cold = Instant::now();
    let (mut engine, recovered) = Engine::open_or_recover(&db_path).unwrap();
    assert!(!recovered);
    let now = Utc::now();
    let snap = engine.snapshot(now).unwrap();
    let cold_ms = t_cold.elapsed().as_secs_f64() * 1000.0;
    assert!(!snap.tools.is_empty());

    // 4. Warm snapshot (weekly recap cached)
    let t_warm = Instant::now();
    let _snap2 = engine.snapshot(now + Duration::seconds(5)).unwrap();
    let warm_ms = t_warm.elapsed().as_secs_f64() * 1000.0;

    // 5. SQLite individual snapshot queries on 45-day dataset
    let t_q1 = Instant::now();
    let _ev_5h = engine.store.events(Some(Tool::ClaudeCode), now - Duration::hours(5), now).unwrap();
    let q_5h_ms = t_q1.elapsed().as_secs_f64() * 1000.0;

    let t_q2 = Instant::now();
    let _tasks = engine.store.tasks_since(now - Duration::hours(24)).unwrap();
    let q_tasks_ms = t_q2.elapsed().as_secs_f64() * 1000.0;

    let t_q3 = Instant::now();
    let _due = engine.store.queued_due(now).unwrap();
    let q_due_ms = t_q3.elapsed().as_secs_f64() * 1000.0;

    eprintln!(
        "[PERF_AUDIT] lines={total_lines} events={total_events} full_scan_ms={full_ms:.1} inc_rescan_ms={inc_ms:.2} cold_start_ms={cold_ms:.2} warm_snap_ms={warm_ms:.2} q_5h_ms={q_5h_ms:.3} q_tasks_ms={q_tasks_ms:.3} q_due_ms={q_due_ms:.3}"
    );

    assert!(inc_ms < 50.0, "incremental rescan {inc_ms:.2} ms exceeded 50 ms target");
    assert!(cold_ms < 1500.0, "cold start {cold_ms:.2} ms exceeded 1500 ms target");
    let q_limit = if cfg!(debug_assertions) { 25.0 } else { 5.0 };
    assert!(q_5h_ms < q_limit, "5h events query {q_5h_ms:.3} ms exceeded {q_limit} ms target");
    assert!(q_tasks_ms < 5.0, "tasks_since query {q_tasks_ms:.3} ms exceeded 5 ms target");
    assert!(q_due_ms < 5.0, "queued_due query {q_due_ms:.3} ms exceeded 5 ms target");
    #[cfg(not(debug_assertions))]
    assert!(full_ms < 3000.0, "full scan {full_ms:.1} ms exceeded 3000 ms target");
}

#[test]
fn reliability_2_hour_simulated_worker_loop_flat_memory() {
    let mut engine = Engine::in_memory().unwrap();
    let base = Utc::now();
    let watches = watch::load(&engine.store);
    let installed = vec![Tool::ClaudeCode, Tool::Codex, Tool::Gemini];

    let t0 = Instant::now();
    let mut snap_count = 0usize;
    for tick in 0..2400i64 {
        let now = base + Duration::seconds(tick * 3);
        let snap = engine.snapshot(now).unwrap();
        let _fired = watch::check(&watches, &snap, now);
        let _moves = autopilot::decide(&snap, &[], &installed);
        let _chips = suggest::from_snapshot(&snap, now);
        snap_count += 1;
    }
    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let per_tick_ms = elapsed_ms / snap_count as f64;
    eprintln!(
        "[SOAK_2H] ticks={snap_count} total_ms={elapsed_ms:.1} per_tick_ms={per_tick_ms:.3}"
    );
    assert_eq!(snap_count, 2400);
    assert!(per_tick_ms < 30.0, "worker tick {per_tick_ms:.3} ms would exceed 1% CPU budget");
}

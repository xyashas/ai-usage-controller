# AI Usage Controller — Full Audit Report (`gemini/full-audit`)

**Branch:** `gemini/full-audit` (branched from `origin/claude/ask-the-notch` at `a2f8972`)  
**Toolchains verified:** Rust `1.100.0` (`x86_64-unknown-linux-gnu` + `x86_64-pc-windows-gnu` via MinGW GCC 12), Node `v22.14.0`, Chromium headless via Playwright (`--trace off`).

---

## Top 10 Most Important Findings

1. **Corrupt SQLite DB or corrupt `"settings"` JSON previously caused startup failure (Fixed)** — Added `Store::open_or_recover` (`PRAGMA quick_check(1)` + `.corrupt.<ts>` backup before recreating) and `Engine::open_default_or_recover` (backs up corrupt settings to `settings_corrupt_backup` and notifies the user once on startup). Verified in `core/tests/audit_e2e.rs::corrupt_sqlite_db_and_corrupt_settings_recover_cleanly`.
2. **Missing SQLite indexes + uncached weekly recap on every snapshot tick (Fixed)** — Added `PRAGMA synchronous = NORMAL` and 5 targeted indexes (`events_kind_ts`, `gauges_ts`, `savings_ts`, `tasks_open_created`, `tasks_queued`), plus a 60 s `weekly_recap_cache` in `Engine`. On a 45-day / **200,250-line** (38,610 events) fixture in release mode: full scan **380.8 ms** (< 3 s), incremental rescan **1.07 ms** (< 50 ms), cold start **68.65 ms** (< 1.5 s), warm snapshot **12.50 ms** (5.5× faster), and individual SQLite queries **0.025–2.295 ms** (< 5 ms).
3. **Panic hook missing for headless/tray crashes (Fixed)** — Installed `install_panic_hook()` in `app/src-tauri/src/lib.rs` to append timestamped panic location + payload to `crash.log` in the app data directory before aborting/unwinding.
4. **Unsafe `.unwrap()` calls on floating-point comparisons, timestamps, and external JSON (Fixed)** — Replaced `partial_cmp().unwrap()` with `total_cmp()` across `engine.rs`, `advice.rs`, `plans.rs`, hardened DST-safe bounds in `metrics.rs`, and removed unsafe `.unwrap()` calls in `repo_handoff.rs`, `ci_watch.rs`, and `handoff.rs`.
5. **Multi-monitor, mixed-DPI, top/side taskbar, and negative coordinate placement risk (Fixed)** — Updated `app/src-tauri/src/window.rs::place` to use `Monitor::work_area()` and `PhysicalPosition` so the pill/panel stays visible with top/left/right taskbars, 100–200% DPI scaling, and negative secondary monitor coordinates.
6. **GitHub API token destination enforcement (Fixed)** — Added `github::is_api_url` allowlist (`https://api.github.com/` prefix required) enforced in `core/src/github.rs::get()` and tested against HTTP, subdomain spoof (`api.github.com.evil.com`), and userinfo spoof (`api.github.com@evil.com`).
7. **UI main bundle size & secondary view code splitting (Fixed)** — Split `Paywall`, `AuditCard`, `Onboarding`, and `CheckSetup` via `React.lazy` + `<Suspense>`, and wrapped `UsageHistory` and `DailyRecap` in `React.memo`. Main JS chunk dropped from **380.33 kB** (`116.90 kB` gzip) to **367.28 kB** (`113.68 kB` gzip); panel-open animation measured at **59.9 FPS** (`16.70 ms` median / `16.80 ms` max, zero dropped frames).
8. **Escape from sub-views closed the whole widget instead of returning to Usage panel (Fixed)** — Updated `ui/src/App.tsx` so pressing `Escape` in `settings`, `setup`, `audit`, `paywall`, `handoffs`, `agents`, or `ask` returns cleanly to `"panel"`, and pressing `Escape` again closes the widget.
9. **Every page load logged a 404 `console.error` for `/favicon.ico` + missing `title` tooltips on icon buttons (Fixed)** — Added `<link rel="icon" href="data:," />` in `ui/index.html` and added missing `title` attributes on `.icon-btn` elements in `HandoffHub.tsx`, `HandoffPreview.tsx`, `Watches.tsx`, `SuggestionChip.tsx`, and `DailyRecap.tsx`.
10. **Unused `sha2` dependency warning broke strict `-D warnings` clippy on Linux & Windows (Fixed)** — Removed unused `sha2 = "0.10"` from `core/Cargo.toml`; both `cargo clippy --workspace --all-targets -- -D warnings` and `--target x86_64-pc-windows-gnu -- -D warnings` now pass with **0 warnings**.

---

## Full Audit Results

| area | what you checked | result | file:line | how you verified |
| --- | --- | --- | --- | --- |
| A. Speed — Cold start | Time from `Engine::open_or_recover` to first `snapshot()` on 45-day / 200k-line DB (target < 1.5 s) | OK | `core/src/engine.rs:102`, `core/src/engine.rs:519` | `cargo test --release -p aiuc-core --test perf_audit -- --nocapture`: **68.65 ms** (debug: 152.22 ms) |
| A. Speed — Scanner full & incremental | 45-day, 200,250-line fixture across Claude, Codex, Gemini (targets: full < 3 s, rescan < 50 ms) | fixed | `core/src/scanner.rs:68-166` | Single `file_stat`, non-UTF8 resilience; release `perf_audit`: full scan **380.8 ms**, incremental rescan **1.07 ms** (only 1 modified file read) |
| A. Speed — Worker loop & caching | Snapshot cost, weekly recap recomputation, idle CPU budget (< 1%) | fixed | `core/src/engine.rs:47-128`, `app/src-tauri/src/lib.rs` | Added 60 s `weekly_recap_cache` (warm snap **12.50 ms** vs cold **68.65 ms**); 2h soak (`reliability_2_hour_simulated_worker_loop_flat_memory`) **0.035 ms/tick** (~0.001% CPU at 3 s interval) |
| A. Speed — SQLite WAL & indexes | Journal mode, `synchronous = NORMAL`, indexes for `events`, `gauges`, `savings`, `tasks_since`, `queued_due` (< 5 ms) | fixed | `core/src/store.rs:50-103` | Added 5 indexes + `PRAGMA synchronous = NORMAL`; release `perf_audit` on 38,610 events: `q_5h` **2.295 ms**, `q_tasks` **0.050 ms**, `q_due` **0.025 ms** |
| A. Speed — UI bundle & memoisation | Code-splitting rare views (`Paywall`, `AuditCard`, `Onboarding`, `CheckSetup`) + `React.memo` on heavy panels | fixed | `ui/src/App.tsx:12-20`, `ui/src/components/UsageHistory.tsx`, `ui/src/components/DailyRecap.tsx` | `npm run build`: main chunk **380.33 kB → 367.28 kB** (gzip 116.90 → 113.68 kB) + 4 lazy chunks (`2.01–5.27 kB`) |
| A. Speed — 60 fps panel open | Frame timing & layout thrash during pill → panel open on `vite preview` | OK | `ui/e2e/audit.spec.ts:200` | Playwright rAF trace on `vite preview`: **24 frames, 16.70 ms median, 16.80 ms max, 59.9 FPS** |
| B. Features — Usage parsing | Malformed JSON, partial trailing line, >1 MB line, non-UTF8 bytes, rotated/truncated file, clock skew | fixed | `core/src/scanner.rs:97-142` | `core/tests/audit_e2e.rs::scanner_edge_cases_malformed_partial_huge_non_utf8_rotated_clock_skew` + `core/tests/core.rs` (35 tests pass) |
| B. Features — Limits, windows & DST | 5-hour blocks, Pacific midnight reset, DST-safe bounds, expired gauge reset | fixed | `core/src/metrics.rs`, `core/tests/core.rs` | Replaced unsafe timestamp unwrap in `metrics.rs`; verified via `five_hour_blocks_follow_claude_rules`, `gemini_resets_at_pacific_midnight`, `expired_gauge_means_window_reset` |
| B. Features — Live gauges & alerts | Expiry, offline, session expiry, runaway loop detection, alert dedup | OK | `core/src/live.rs`, `core/src/runaway.rs`, `core/tests/core.rs` | Verified via `claude_poll_health_distinguishes_age_network_and_session_expiry`, `runaway_flags_repeating_expensive_loop`, `live_claude_gauge_parsing` |
| B. Features — Handoffs & dispatcher | Session/repo handoffs, worktree isolation, routing, multi-task splitting, verify runner | fixed | `core/src/repo_handoff.rs`, `app/src-tauri/src/handoff.rs`, `core/tests/dispatch.rs`, `core/tests/repo_handoff.rs` | Removed unsafe unwraps; verified via `core/tests/dispatch.rs` (5 tests), `core/tests/repo_handoff.rs` (6 tests), `aiuc-app` unit tests (3 tests) |
| B. Features — GitHub token & URLs | Token sent strictly to `https://api.github.com/`, never arbitrary URLs | fixed | `core/src/github.rs:35-56` | Enforced `is_api_url` in `github::get()`; verified via `core/tests/audit_e2e.rs::github_api_url_enforcement` |
| B. Features — Gemini features | Daily/weekly recaps, watches, suggestion chips, prompt polish, autopilot, CI watcher, inbox | OK | `core/src/recap.rs`, `watch.rs`, `suggest.rs`, `polish.rs`, `autopilot.rs`, `ci.rs`, `inbox.rs` | Verified via `recap.rs` (4), `watch.rs` (3), `suggest.rs` (2), `polish.rs` (3), `autopilot.rs` (1), `ci.rs` (3), `audit_e2e.rs::gemini_features_edge_cases_empty_and_boundary_states` |
| B. Features — Settings & legacy defaults | Round-trip serialization + old JSON missing newer fields (`show_daily_recap`, `autopilot`, `polish_auto`) | OK | `core/src/settings.rs` | `core/tests/audit_e2e.rs::settings_round_trip_and_legacy_defaults` |
| B. Features — Licence, trial & free tier | 7-day trial, expiry to free tier, Pro offline grace window (7 days) | OK | `core/src/entitlements.rs`, `core/src/licence.rs` | `core/tests/audit_e2e.rs::entitlements_trial_expiry_and_offline_grace` + `free_tier_shows_one_tool_and_locks_pro_features` |
| B. Features — Sync privacy payload | Outbound sync payload contains counts/tokens/cost only; never paths, project names, or tool signatures | OK | `core/src/sync.rs:45-62` | `core/tests/audit_e2e.rs::sync_payload_contains_only_counts_never_private_text` + `sync_payload_contains_no_paths_or_signatures` |
| C. Window — Escape & navigation | Escape from sub-view returns to Usage panel instead of dismissing widget | fixed | `ui/src/App.tsx:116-127` | Playwright E2E `Handoffs & Settings flows + Escape back navigation` |
| C. Window — Multi-monitor, DPI & taskbar | Placement on top/side taskbars, 100–200% DPI scaling, negative secondary monitor coords | fixed | `app/src-tauri/src/window.rs` | Switched to `m.work_area()` + `PhysicalPosition`; verified compilation on Linux and `x86_64-pc-windows-gnu` |
| D. Reliability — 2-hour simulated soak | 2,400 consecutive ticks of `snapshot` + `watch::check` + `autopilot::decide` + `suggest::from_snapshot` | OK | `core/tests/perf_audit.rs:185-207` | Completed in **85.1 ms** (release) with flat memory and zero errors |
| D. Reliability — Panic hook & crash log | Uncaught panic writes timestamp + location + message to `crash.log` in data dir | fixed | `app/src-tauri/src/lib.rs` | Added `install_panic_hook()` at startup; verified Linux + Windows clippy (`-D warnings`) |
| D. Reliability — Corrupt DB & settings | Corrupt SQLite file backed up to `.corrupt.<ts>` and recreated; corrupt settings backed up to `settings_corrupt_backup` | fixed | `core/src/store.rs:104-125`, `core/src/engine.rs:47-60,102-115` | `core/tests/audit_e2e.rs::corrupt_sqlite_db_and_corrupt_settings_recover_cleanly` |
| D. Reliability — Unsafe `.unwrap()` audit | Static audit of all 37 non-test `.unwrap()`/`.expect()` across workspace | fixed | `core/src/engine.rs`, `advice.rs`, `plans.rs`, `metrics.rs`, `repo_handoff.rs`, `app/src-tauri/src/ci_watch.rs`, `handoff.rs` | Replaced all unsafe unwraps in non-Claude files; remaining 4 in Claude-owned files listed under "For Claude" |
| E. Safety Net — Playwright E2E & a11y | All 9 views × 2 platforms + 5 scenarios + 4 buddy overlays + 5 interactive flows + a11y (`aria-label`, `:focus-visible`, contrast ≥ 4.5:1, `prefers-reduced-motion`) | fixed | `ui/e2e/audit.spec.ts`, `ui/playwright.config.ts`, `ui/index.html` | Fixed `/favicon.ico` 404 & missing `.icon-btn` `title` attrs; **34/34 Playwright tests pass** (`--trace off`) |
| E. Safety Net — Redacted CLI & hook fixtures | Real redacted fixtures for Claude stream-json, Codex exec json, Gemini text, Codex notify, Claude hooks | fixed | `core/tests/fixtures/*`, `core/tests/audit_e2e.rs:36-111` | Fed through `ask::StreamReader`, `agent_status::handle_claude`, `agent_status::handle_codex` — all match cleanly |
| F. Cross-platform — Linux & Windows clippy | Zero warnings under `-D warnings` for both Linux and `x86_64-pc-windows-gnu` | fixed | `core/Cargo.toml:24` | Removed unused `sha2` dep; `cargo clippy --workspace --all-targets -- -D warnings` passes on Linux & Windows (0 warnings) |

---

## For Claude (Bugs / Polish in Claude-Owned Files — Not Edited)

1. **`core/src/ask.rs:234,236` — `.unwrap()` in `pick()`**
   - *What:* Line 234 calls `r.used_pct.unwrap()` after `.filter(|r| ... r.used_pct.is_some_and(...))`, and line 236 calls `pinned.unwrap()` after `pinned.is_some_and(...)`.
   - *Why it matters:* Safe today only because of the preceding guard, but fragile under future refactors and flagged by strict unwrap audits.
   - *Likely fix:* Replace `.filter(...).map(...)` on line 233–234 with `.filter_map(|r| r.used_pct.filter(|&u| r.tool != best.tool && u >= 80.0).map(|u| ...))`, and replace line 236 with `else if let Some(p) = pinned.filter(|&p| p != best.tool) { format!("{} is out of room", tool_label(p)) }`.

2. **`core/src/agent_status.rs:251,257` — `.as_object_mut().unwrap()` in `merge_claude_hooks()`**
   - *What:* Lines 251 and 257 call `.as_object_mut().unwrap()` after `if !settings.is_object() { *settings = json!({}); }` and `if !hooks.is_object() { *hooks = json!({}); }`.
   - *Likely fix:* Use `let Some(root) = settings.as_object_mut() else { return };` and `let Some(hooks_obj) = hooks.as_object_mut() else { return };` to eliminate `.unwrap()` completely.

3. **`ui/src/components/AskView.tsx:236` — Back icon button missing `title` attribute**
   - *What:* `<button className="icon-btn" aria-label="Back">` has `aria-label` (passes screen-reader a11y) but omits `title="Back"` unlike all other icon buttons in the app.
   - *Repro:* Open `?view=ask`, hover over the back arrow button — no native tooltip appears.
   - *Likely fix:* Add `title="Back"` to the `<button>` on line 236.

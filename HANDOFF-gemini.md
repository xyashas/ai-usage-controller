# HANDOFF — Gemini (`gemini/daily-weekly-recap`)

## What was built

1. **Daily recap (morning brief)**
   - New `core/src/recap.rs` computes `DailyRecap` using read-only SQLite queries against `Store.conn` (`events`, `gauges`, `tasks` count only) plus the current `ToolStatus` windows.
   - Shows yesterday's usage per tool (requests, API-equivalent cost, peak % of 5-hour/daily and weekly limits — 5-hour always before weekly), today's upcoming limit reset times, count of non-dismissed dispatcher tasks created yesterday (count only, no task text or paths), and one plain suggestion (e.g., `"Codex has the most room this morning"`).
   - Dismissing calls `dismiss_daily_recap` and persists in `kv` under `daily_recap_dismissed:YYYY-MM-DD` (local date), staying dismissed for that day and reappearing automatically the next day.
   - Controlled by `settings.show_daily_recap` (default `true`), exposed in Settings under **Widget → Show daily recap**.
   - New `ui/src/components/DailyRecap.tsx` renders the card at the top of the Usage view (`Expanded.tsx`).

2. **Weekly recap in Usage History**
   - `build_weekly_recap` in `core/src/recap.rs` computes this week vs last week (Monday 00:00 UTC week bounds, matching `metrics::week_start`) per tool: requests, cost, % change, distinct limit-hit episodes (≥95% used, deduplicated per window so 60 s polling doesn't double-count), 24-hour local request distribution + top 3 busiest hours, and completed handoffs count.
   - `launch_handoff` in `app/src-tauri/src/lib.rs` calls `eng.record_handoff(Utc::now())` on success, storing UTC millisecond timestamps only in `kv` key `recap_handoffs` (pruned to 35 days).
   - Added `WeeklyRecapSection` inside `ui/src/components/UsageHistory.tsx` (preserving the existing 24-hour `UsageHistory` sparkline) and rendered it in `Expanded.tsx`.
   - Added realistic mock data for both `daily_recap` and `weekly_recap` (and `dismiss_daily_recap` handler) in `ui/src/api.ts` so both appear in `npm run dev`.

## Files touched

**New files**
- `core/src/recap.rs`
- `core/tests/recap.rs`
- `ui/src/components/DailyRecap.tsx`
- `HANDOFF-gemini.md`

**Minimal additive edits**
- `core/src/lib.rs` (`pub mod recap;`)
- `core/src/settings.rs` (`show_daily_recap: bool`, default `true`)
- `core/src/snapshot.rs` (`daily_recap: Option<DailyRecap>`, `weekly_recap: WeeklyRecap` on `Snapshot`)
- `core/src/engine.rs` (wired `daily_recap` / `weekly_recap` into `snapshot()`, added `dismiss_daily_recap` and `record_handoff`)
- `app/src-tauri/src/lib.rs` (added `dismiss_daily_recap` command, registered in `invoke_handler`, called `record_handoff` in `launch_handoff`)
- `ui/src/types.ts` (added `DailyToolUsage`, `DailyRecap`, `WeeklyToolComparison`, `HourBucket`, `WeeklyRecap`, plus fields on `Snapshot` and `Settings`)
- `ui/src/api.ts` (added `dismissDailyRecap`, `show_daily_recap` in `mockSettings`, mock `daily_recap` & `weekly_recap` in `mockSnapshot`)
- `ui/src/components/UsageHistory.tsx` (added `WeeklyRecapSection` alongside `UsageHistory`)
- `ui/src/components/Expanded.tsx` (rendered `DailyRecap` at top of Usage view and `WeeklyRecapSection` below Today stats)
- `ui/src/components/SettingsView.tsx` (added `"Show daily recap"` toggle in Widget section)

**Untouched (Claude's active areas):** `core/src/dispatch.rs`, `core/src/store.rs`, `app/src-tauri/src/dispatch.rs`, `app/src-tauri/src/handoff.rs`, `app/src-tauri/src/autosend.rs`, `ui/src/components/AgentsView.tsx`, `ui/src/components/Collapsed.tsx`.

## What's tested

- `cargo test --workspace` — all 53 tests pass, including 4 new integration tests in `core/tests/recap.rs` covering:
  - yesterday's per-tool totals, peak %, today's resets, open task count (excluding dismissed/today's tasks), suggestion generation, and JSON privacy assertion (no prompt/project strings)
  - per-day dismissal persistence and next-day reappearance
  - `show_daily_recap` setting toggle
  - weekly comparison (% change + "New" case), busiest local hours + 24-hour buckets, deduplicated limit-hit episodes, and handoff counting
- `cargo clippy --workspace --all-targets` — zero warnings in any touched/new file.
- `cd ui && npm run build` (`tsc -b && vite build`) — clean production build.

## What's left

- Visual/hands-on verification of the Daily Recap card and Weekly Recap section in the Tauri desktop widget on Windows/macOS (and in `npm run dev` browser preview).
- Optional future enhancement: track repo-based/cloud handoffs in `recap::record_handoff` too if a separate cloud-handoff launch path is added later.

## Notes for Claude

- **New `Settings` field:** `show_daily_recap: bool` (serde default `true`). Existing serialized settings without this field deserialize cleanly via `#[serde(default)]`.
- **New `Snapshot` fields:** `daily_recap: Option<crate::recap::DailyRecap>` and `weekly_recap: crate::recap::WeeklyRecap`.
- **New Tauri command:** `dismiss_daily_recap` (takes no arguments; marks today's local date dismissed in `kv`).
- **New `kv` keys used (local only, never synced):**
  - `daily_recap_dismissed:YYYY-MM-DD` → `"1"`
  - `recap_handoffs` → JSON array of UTC epoch millisecond timestamps (pruned to 35 days)
- **Read-only DB access:** `core/src/recap.rs` queries `events`, `gauges`, and `tasks` (`WHERE dismissed = 0` count only) directly on `&store.conn`. If Claude changes the `tasks` schema (e.g. adds a `status` or `completed` column for "verified done"), `query_open_tasks_count` in `core/src/recap.rs` can be updated to exclude completed tasks as well.

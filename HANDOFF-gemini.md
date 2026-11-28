# HANDOFF — Gemini (`gemini/autopilot-ci`)

Built on top of `claude/ask-the-notch` (`95c258f`).

## What was built (Round 3)

1. **Autopilot (automatic mid-task handoff)**
   - `core/src/autopilot.rs`: pure `Move { task_id, from, to, cwd }`, `MovedRecord { task_id, to, at }`, `decide(snap, already_moved, installed) -> Vec<Move>`, plus local-only `kv` helpers `load_moves` / `record_move` under key `"autopilot_moved"` (pruned > 24 h, IDs/timestamps only).
     - Moves a terminal task (`dest == "terminal"`, never app/web) when its tool's 5-hour (or tightest) limit is ≥ 97% used while `"working"`, `"needs_approval"`, or `"idle"` within 5 min, to the installed/enabled/unlocked tool with the most room (< 80% used). Never moves the same task twice.
   - `core/tests/autopilot.rs`: tests 97% threshold, terminal-only guard, recent-idle 5-minute window, installed CLI guard, duplicate-move guard, and 24-hour pruning.
   - `app/src-tauri/src/autopilot.rs`: `pub fn tick(app, engine, snap)` builds the handoff prompt by reusing `repo_handoff::continue_prompt` / `handoff::prepare`, launches the target via `crate::dispatch::launch_task(Some(root), …, Dest::Terminal, false, auto)` in the same folder, records a new `TaskRow::new` (`"Continue: <original text>"`), remembers the move in `"autopilot_moved"`, and notifies `"<From> ran out: <To> took over <repo>"`.
   - Wired `autopilot::tick(&app, &engine, &snap);` in `app/src-tauri/src/lib.rs` right next to `watch::tick(...)`.
   - Exposed `autopilot_moves: Vec<MovedRecord>` on `Snapshot`; `AgentsView.tsx` shows `"Handed to <Tool>"` on moved task rows.
   - Setting: `autopilot: bool` (default `true`), `"Hand off automatically at the limit"` in Settings → Agents.

2. **CI Watcher (GitHub Actions on agent branches)**
   - `core/src/ci.rs`: pure `CiRun`, `parse_runs(json, repo)`, `watched(run, task_branches)` (matches dispatcher task branches or `agents/`, `claude/`, `codex/`, `gemini/` prefixes), `newly_failed(prev, now)`, `log_tail(text, lines)` (strips ANSI codes and leading GitHub ISO timestamps), and `owner_repo`.
   - `core/tests/ci.rs`: JSON fixture tests for `parse_runs`, `watched`, `newly_failed`, `log_tail`, and `owner_repo`.
   - `app/src-tauri/src/ci_watch.rs`: background thread polling every 2 min ONLY when `settings.ci_watch` is `true` and GitHub token (`kv` key `"github_token"`) is present. Watches local repos with a GitHub remote (`repo_handoff::status(...).github`) plus repos of recent agent tasks.
     - Sends the Bearer token ONLY to `https://api.github.com` (uses `max_redirects(0)` on `/actions/jobs/{job_id}/logs` and fetches the pre-signed redirect URL with no `Authorization` header).
     - Keeps failed job log tails strictly IN MEMORY (never stored in SQLite/kv).
     - On a newly failed run: notifies `"CI failed on <branch>"` and emits `"suggestion"` with `Suggestion { title: "CI failed. Send to an agent?", action: "send_failure", detail: "<repo> · <branch>", text: Some(log_tail) }`.
     - Tauri commands: `ci_runs() -> Vec<CiRun>` and `ci_log(run_id: u64) -> String` (registered in `lib.rs`); thread spawned in `lib.rs` `setup`.
   - `ui/src/components/CiRuns.tsx`: `"CI"` section above `<Watches />` in `AgentsView.tsx` showing branch, status dot, relative time, `"Send to agent"` for failures (reusing the existing `"send_failure"` `aiuc-ask` event), and `"Open"` → `html_url`. Mocks added in `ui/src/api.ts`.
   - Setting: `ci_watch: bool` (default `true`), `"Watch CI on agent branches"` in Settings → Agents.

## Files touched (Round 3)

**New files**
- `core/src/autopilot.rs`, `core/tests/autopilot.rs`
- `core/src/ci.rs`, `core/tests/ci.rs`
- `app/src-tauri/src/autopilot.rs`, `app/src-tauri/src/ci_watch.rs`
- `ui/src/components/CiRuns.tsx`

**Minimal additive edits**
- `core/src/lib.rs` (`pub mod autopilot; pub mod ci;`)
- `core/src/settings.rs` (`autopilot: bool`, `ci_watch: bool`)
- `core/src/snapshot.rs` (`autopilot_moves: Vec<crate::autopilot::MovedRecord>`)
- `core/src/suggest.rs` (`text: Option<String>` on `Suggestion`, `#[serde(skip_serializing_if = "Option::is_none", default)]`)
- `core/src/engine.rs` (loads `autopilot_moves` into `Snapshot`)
- `app/src-tauri/src/clipboard_watch.rs` (`text: None` on `Suggestion` literal)
- `app/src-tauri/src/lib.rs` (`mod autopilot; mod ci_watch;`, `autopilot::tick` next to `watch::tick`, registered `ci_watch::{ci_runs, ci_log}`, spawned `ci_watch::spawn`)
- `ui/src/types.ts`, `ui/src/api.ts`, `ui/src/components/SuggestionChip.tsx`, `ui/src/components/AgentsView.tsx`, `ui/src/components/SettingsView.tsx`

**Untouched:** `ask*`, `capture*`, `pointer*`, `voice*`, `guide*`, `Pointer.tsx`, `AskView.tsx`, `Collapsed.tsx`, `Approvals.tsx`, `agents.rs`, `agent_status.rs`, `verify.rs`, `autosend.rs`, `store.rs`.

## Notes for Claude

- Everything is self-wired (`autopilot::tick`, `ci_watch::spawn`, `ci_runs`, `ci_log`) — no extra glue required from Claude.
- `Suggestion` now has an optional in-memory `text?: string | null` field (`#[serde(skip_serializing_if = "Option::is_none", default)]`), and `SuggestionChip.tsx` passes `active.text ?? "Fix the failing tests"` for `action: "send_failure"` so CI failure chips prefill the Agents box with the log tail via `App.tsx`'s existing `"aiuc-ask"` handler.
- New `Settings` fields (serde default safe): `autopilot: bool` (default `true`), `ci_watch: bool` (default `true`).
- New `Snapshot` field: `autopilot_moves: Vec<MovedRecord>`.
- New `kv` key (local only, pruned > 24 h): `"autopilot_moved"`.

## What's tested

- `cargo test --workspace` — **79/79 passed** (including new tests in `core/tests/autopilot.rs` and `core/tests/ci.rs`).
- `cargo clippy --workspace --all-targets` — zero warnings in any touched/new file.
- `cd ui && npm run build` (`tsc -b && vite build`) — clean production build.

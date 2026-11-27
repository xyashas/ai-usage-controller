# HANDOFF — Gemini (`gemini/polish-watch-suggest`)

Built on top of `gemini/daily-weekly-recap` (`64572ee`, which includes Daily & Weekly Recap).

## What was built (Round 2)

1. **Prompt Polish**
   - `core/src/polish.rs`: `PolishContext`, `gather(repo_root, failure_tail)`, `request(raw, ctx)`, `clean(output)`. Reads repo name, current git branch, up to 15 changed/untracked files, optional failing test output tail, and first 40 lines of `AGENTS.md`/`CLAUDE.md`. Strips AI preambles, code fences, and outer quotes.
   - `core/tests/polish.rs`: 3 tests using a temp git repo (branch, 15-file cap, 40-line rules cap, `None` repo, `clean` wrapper stripping).
   - `ui/src/components/PromptPolish.tsx`: exports `PromptPolish`, `PolishButton`, `PolishDiffCard` (highlights newly added words with `"Use it"` / `"Keep mine"`), and `usePromptPolish`. Wired into `AgentsView.tsx` (button next to Run + diff card under the task box).
   - `ui/src/api.ts`: `api.polishPrompt(text, repo)` calling `"polish_prompt"` + browser mock.
   - Setting: `polish_auto: bool` (default `false`), `"Polish tasks before sending"` in Settings → Agents.

2. **Tell Me When / Remind Me (Watches)**
   - `core/src/watch.rs`: `Condition` (`LimitReset`, `LimitBelow`, `AgentDone`, `AgentNeedsYou`, `TestsPass`, `At`), `Watch`, `parse(text, snap, now)`, `check(watches, snap, now)`, plus `load(store)` / `save(store, watches)` using `kv` key `"watches"` (local only, never synced).
   - `core/tests/watch.rs`: 3 tests covering limit reset/below, agent done + tests pass, relative/clock reminders, and `kv` persistence.
   - `app/src-tauri/src/watch.rs`: Tauri commands `add_watch`, `list_watches`, `remove_watch` (registered in `lib.rs` `invoke_handler`), and `pub fn tick(app, engine, snap)` that removes fired watches, emits `"watches-changed"`, and fires desktop notifications.
   - `ui/src/components/Watches.tsx`: `"Watching"` list (inline add input + active list with remove button), rendered at the bottom of `AgentsView.tsx`. `api.addWatch` / `listWatches` / `removeWatch` + mocks.

3. **Speaks Up First (Suggestion Chips)**
   - `core/src/suggest.rs`: `Kind` (`CopiedError`, `TestsFailed`, `LimitMidTask`), `Suggestion` (`title` ≤ 40 chars), `looks_like_error(text)`, `from_snapshot(snap, now)`.
   - `core/tests/suggest.rs`: 2 tests covering error detection (true positives + no false positives on normal prose) and snapshot suggestions (failed tests + ≥90% limit mid-task suggesting roomiest alternative tool).
   - `app/src-tauri/src/clipboard_watch.rs`: polls clipboard text every 1.5 s ONLY when `settings.suggest_chips` is `true`, keeps only a hash in memory (never stores or logs clipboard text), emits `"suggestion"` event when a newly copied error is detected. Spawned in `lib.rs` `setup`.
   - `ui/src/components/SuggestionChip.tsx`: slides down below the pill for 8 s (pauses while hovered), action button + ✕, dispatches `window.dispatchEvent(new CustomEvent("aiuc-ask", { detail }))`. Rendered from `App.tsx`.
   - Setting: `suggest_chips: bool` (default `true`), `"Suggest actions"` in Settings → Agents.

## Files touched (Round 2)

**New files**
- `core/src/polish.rs`, `core/tests/polish.rs`
- `core/src/watch.rs`, `core/tests/watch.rs`
- `core/src/suggest.rs`, `core/tests/suggest.rs`
- `app/src-tauri/src/watch.rs`, `app/src-tauri/src/clipboard_watch.rs`
- `ui/src/components/PromptPolish.tsx`, `ui/src/components/Watches.tsx`, `ui/src/components/SuggestionChip.tsx`

**Minimal additive edits**
- `core/src/lib.rs` (`pub mod polish; pub mod suggest; pub mod watch;`)
- `core/src/settings.rs` (`polish_auto: bool`, `suggest_chips: bool`)
- `core/src/snapshot.rs` (`suggestions: Vec<crate::suggest::Suggestion>`)
- `core/src/engine.rs` (populates `snap.suggestions` when `settings.suggest_chips` is `true`)
- `app/src-tauri/src/lib.rs` (`mod clipboard_watch; mod watch;`, registered `watch::{add_watch, list_watches, remove_watch}`, spawned `clipboard_watch::spawn` in `setup`; `spawn_worker` left untouched)
- `ui/src/types.ts`, `ui/src/api.ts`, `ui/src/components/AgentsView.tsx`, `ui/src/components/SettingsView.tsx`, `ui/src/App.tsx`

**Untouched (Claude's active areas):** `ask*`, `council*`, `pointer*`, `voice*`, `core/src/agent_status.rs`, `core/src/verify.rs`, `core/src/store.rs`, `app/src-tauri/src/agents.rs`, `app/src-tauri/src/lib.rs` worker loop, `Collapsed.tsx`, `Approvals.tsx`.

## Exact contract for Claude

1. **Prompt Polish Tauri command (for Claude to implement & register):**
   - Command name: `polish_prompt` with args `(text: String, repo: Option<String>) -> Result<String, String>`
   - Core helpers to call:
     - `aiuc_core::polish::gather(repo_root: Option<&Path>, failure_tail: Option<&str>) -> aiuc_core::polish::PolishContext`
     - `aiuc_core::polish::request(raw: &str, ctx: &PolishContext) -> String`
     - `aiuc_core::polish::clean(output: &str) -> String`
   - UI component to reuse in Ask box: `import { PromptPolish, PolishButton, PolishDiffCard, usePromptPolish } from "./PromptPolish";`

2. **Watch ticker (one line for Claude to add in `spawn_worker` in `app/src-tauri/src/lib.rs`):**
   - Call `watch::tick(&app, &engine, &snap);` after computing `snap` (and remove `#[allow(dead_code)]` on `watch::tick` if desired).
   - Already registered commands: `watch::add_watch`, `watch::list_watches`, `watch::remove_watch`.
   - Frontend event emitted when a watch fires: `"watches-changed"`.

3. **Suggestion chips → Ask box event (for Claude's Ask box to listen to):**
   - Browser event: `window.addEventListener("aiuc-ask", (e: CustomEvent) => ...)`
   - `e.detail` shapes:
     - `{ action: "ask_fix", text: string }` (copied error)
     - `{ action: "send_failure", text: string, task_id: number }` (tests failed)
     - `{ action: "handoff", text: string, tool: Tool, to: Tool }` (limit ≥90% mid-task)
   - Tauri event from `clipboard_watch.rs`: `"suggestion"` with `aiuc_core::suggest::Suggestion` payload.

4. **New `Settings` fields (serde default safe):**
   - `polish_auto: bool` (default `false`)
   - `suggest_chips: bool` (default `true`)
   - (From Round 1: `show_daily_recap: bool`, default `true`)

## What's tested

- `cargo test --workspace` — **68/68 passed** (including 3 polish tests, 3 watch tests, 2 suggest tests, 4 recap tests, 7 agent_status tests, 35 core tests, 5 dispatch tests, 6 repo_handoff tests, 3 app tests).
- `cargo clippy --workspace --all-targets` — zero warnings in any touched/new file.
- `cd ui && npm run build` (`tsc -b && vite build`) — clean production build.

## What's left

- Claude to wire `polish_prompt` command, `"aiuc-ask"` listener in Ask box, and `watch::tick(&app, &engine, &snap)` in `spawn_worker`.
- Hands-on verification on Windows/macOS of clipboard polling (`clipboard_watch.rs`), chip hover/8 s auto-dismiss, and OS watch notifications.

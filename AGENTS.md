# AGENTS.md — context for coding agents (Codex, Claude Code, etc.)

Read this first. It is the project's memory: what exists, why it is built this
way, what has been verified, and what to build next. Keep it up to date when you
change something important.

## What this is

**AI Usage Controller**: a desktop widget (Mac notch / Windows top-centre pill)
that shows how much of your AI plan limit you have used, warns before you get
cut off, and tells you when you're wasting money. It covers **Claude** (claude.ai,
desktop app and Claude Code share one plan limit), **Codex CLI** and **Gemini CLI**.

Tagline: *Never get cut off mid-task, never overpay for AI.*

Business: Free (meter for 1 tool) / Pro $9/mo or $79/yr / Team $29/seat/mo.
7-day no-card trial. Payments via Lemon Squeezy (licence keys). Owner: solo founder.

## Repo layout

```
core/   Rust library + `aiuc` CLI: log parsers, SQLite store, metrics, alerts,
        advice (nudge/audit), entitlements, licences, sync client, live gauges
app/    Tauri 2 shell (Rust): widget window, hover/click-through, tray, hotkey,
        background worker, claude.ai sign-in window
ui/     React 19 + Vite 7 + Tailwind v4 panels rendered in the Tauri webview
api/    Hono + node:sqlite backend: price/plan tables, waitlist, orgs/invites,
        sync, team dashboard data, Lemon Squeezy webhook (Fly.io config included)
web/    Next.js 16: landing page, week-0 waste calculator, team dashboard
docs/   LOG-FORMATS.md (parsers), RELEASING.md (signing/payments), PRIVACY.md
```

## Build, run, test

Prereqs: Rust stable, Node 22.13+ (api uses `node:sqlite`). Linux needs
`libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev`.

```bash
cargo test -p aiuc-core              # core tests (parsers, metrics, alerts, advice, gating, sync privacy)
npm test --prefix api                # API tests
npm test --prefix web                # calculator tests
npm ci --prefix ui && npm run build --prefix ui
cd app && npx tauri dev              # run the widget (needs the ui deps)
npm run dev --prefix ui              # panels in a browser with mock data
```

Browser mock URL params (ui/src/api.ts): `?scenario=runaway|low|critical|unconnected|connected`,
`?tier=free`, `?onboarding=1`, `?platform=windows|macos`.

CLI: `cargo run -p aiuc-core --bin aiuc -- paths|status [--json]|watch|plan <tool> <plan>|audit|trial`.
Set `AIUC_DATA_DIR=/some/dir` to use a throwaway database.

CI (`.github/workflows/ci.yml`): core tests on Linux/Windows, unsigned Windows
installer artifact (`bundles-windows-latest`), Linux compile check, ui/api/web.
macOS (core tests + universal `.dmg` as `bundles-macos-latest`) runs only via
Actions → CI → Run workflow with "Build for Mac" ticked: Mac minutes cost 10×,
and the owner chose to save them for now.
`release.yml`: tag `v*` → signed draft release (needs secrets, see docs/RELEASING.md).
**Always keep CI green; run the relevant tests before pushing.**

## How it works (key design decisions)

### Data sources
- **Claude plan limits** (5-hour, weekly, weekly Opus):
  1. `app/src-tauri/src/claude_web.rs`: user signs in to claude.ai in an in-app
     window; it is then hidden and parked on `https://claude.ai/api/organizations`.
     Every 60 s we `eval` JS that `fetch`es `/api/organizations/<org>/usage` with
     the webview's own cookies, store the result in `window.__aiuc`, and read it back
     with `eval_with_callback` (the result arrives JSON-encoded twice; see `parse_read`).
     Fallback: user pastes the `sessionKey` cookie, which we set on that webview.
     The login never leaves the webview. Source label `claude_web`.
  2. `core/src/live.rs`: Claude Code OAuth token (macOS Keychain
     `Claude Code-credentials` or `~/.claude/.credentials.json`) →
     `GET https://api.anthropic.com/api/oauth/usage` with `anthropic-beta: oauth-2025-04-20`.
     Only used when claude.ai isn't connected. Token is never refreshed/written.
  Both return `{five_hour, seven_day, seven_day_opus: {utilization 0-100, resets_at}}`.
- **Claude Code logs** `~/.claude/projects/**/*.jsonl` → requests, tokens, cost
  (dedup on `message.id`+`requestId`, keep max tokens).
- **Codex** `~/.codex/sessions/**/rollout-*.jsonl`; `token_count` events carry
  `rate_limits` = Codex's own live gauge.
- **Gemini CLI** `~/.gemini/tmp/*/chats/*.json`; one request per `gemini` message;
  daily quota resets at midnight Pacific.
- Costs are API list-price estimates from `core/data/prices.json` (longest
  substring match on model name). Plan tables in `core/data/plans.json`. Both are
  served by the API and refreshed every 6 h so price changes need no release.

### Engine (`core/src/engine.rs`)
- `snapshot(now)` builds everything the UI shows. Internally numbers are
  **headroom** (% left); the **UI displays % used** (`usedPct` in ui/src/format.ts).
  Alert messages say "% used".
- Allowance per window, in priority: user override → calibrated from live gauge →
  historical peak block/week → otherwise "Learning" (never guess).
- `needs_connect` (Claude only): no fresh live gauge → UI offers "Connect Claude account".
- `detected(Claude)` is also true when claude.ai is connected or gauges exist
  (claude.ai-only users have no logs).
- Runaway (`runaway.rs`): last-5-min spend > 4× normal (floor $0.50), ≥10 agent
  turns without a user message, ≥50% repeated tool signatures. Thresholds are
  untuned guesses — tune with real data.
- Nudge (`advice.rs`): ≥30 responses with ≤1500 output tokens on an expensive
  model in a day and ≥$2 saving vs the cheaper model (rules in prices.json `nudges`).
- Audit: needs 28 days of data; keep / downgrade (<60% for 4 weeks) / upgrade
  (>2 cap days) / cancel (<4 active days).
- Free tier: one tool, fixed alert levels; Pro features listed in `snapshot.locked`.
- `alert_action` takes only an alert **id** and looks it up in the engine's last
  snapshot, so the webview can't choose which process gets interrupted.
- One-click Stop (SIGINT) is unix-only and off by default.

### Widget window (`app/src-tauri/src/window.rs`, `hover.rs`, `ui/src/App.tsx`)
- **One fixed-size transparent window (580×700) pinned top-centre. It never
  resizes.** The pill and panel are one shared "surface" that morphs with CSS
  transitions. (Earlier versions resized the window and raced with a
  ResizeObserver, leaving a huge oval on screen — don't go back to that.)
- The window is click-through except the shape's rectangle. The UI reports the
  **target** rect (not a mid-animation measurement) via `set_hit_rect`;
  `hover.rs` polls the cursor every 40 ms, toggles `set_ignore_cursor_events`,
  and emits `hover` true/false. That event is the single source of truth for
  open-on-hover (140 ms dwell) / close-on-leave (260 ms grace).
- Views other than the main panel (handoffs, settings, onboarding, setup, paywall, audit)
  are "sticky": they don't close on leave, only on click-away (blur) or Esc.
  Blur is ignored while the cursor is over the shape.
- The panel is 520 wide. Content height is measured from a stable wrapper (`innerRef`) and capped at 640.
- Typing needs keyboard focus: inputs call `api.focus()` on focus.
- macOS: window level 25 (above menu bar), all Spaces; notch height from
  `NSScreen.safeAreaInsets`.

### Widget appearance and usage history
- Look: black panel (matches the notch), always dark. Inspired by NotchOwl: each
  tool is a soft tinted tile with dark ink (`ui/src/theme.ts`: Claude clay,
  Codex sky, Gemini lavender; alerts rose/amber). Shared classes live in
  `styles.css` (`tile`, `card`, `btn`, `btn-primary`, `btn-ink`, `seg`, `eyebrow`,
  `field`, `icon-btn`). Use them instead of ad-hoc Tailwind so views stay consistent.
- Top bar (`TopBar.tsx`): name, a Usage / Handoff switch, Check setup, Settings.
- **The 5-hour limit is shown first** (big number) for every tool, the pill and
  the alert; weekly and others are smaller rows under it (`primaryWindow` in
  `Expanded.tsx`). The pill adds a dot when another limit is worse.
- The pill counts to its next percentage and only pulses for red alerts. Reduced
  motion disables transitions, pulses and counting. Hidden layers are inert,
  Tab reaches the visible controls, Enter activates them and Esc returns to the pill.
- The Claude tile shows a 24-hour sparkline of the live gauge only when it is the
  only tool (full width). Tools without gauge history omit it.
- Preview a view directly with `?view=panel|handoffs|settings|setup|paywall|audit|onboarding`.
  Use `?platform=macos&notch=32` for a notched Mac, `?scenario=critical` (local
  handoff in the alert), `?scenario=cloud` (no local session: the alert offers the
  newest repo handoff), and `?scenario=empty|learning|unconnected` for first use.

### Desktop lifecycle
- Launch at login defaults on once onboarding is complete, including existing
  onboarded installations. Settings controls the native autostart registration.
- The tray dot uses the tightest visible limit's alert color. Its tooltip gives
  the tool and percent used. Left-click toggles the panel, right-click opens the menu.
- Claude readings older than three minutes show their age. Failed polls show
  Offline. An expired claude.ai login shows Reconnect and triggers one system
  notification per expiry when notifications are enabled. The expiry marker
  survives restarts and clears after a successful web poll or disconnect.
- Idle hiding is opt-in. After 30 minutes without tool activity, the collapsed
  pill fades and releases its mouse hit area. A logged event, increased Claude
  web usage, or opening the panel with the hotkey or tray brings it back.
- Browser previews also support stale, offline, signed-out and idle scenarios.

## Gotchas already hit (don't repeat)
- **Windows:** creating a webview or touching cookies from a *synchronous* Tauri
  command deadlocks (WebView2, wry#583). Those commands are `async`.
- **Linux:** `set_ignore_cursor_events` before the window is realised panics in tao.
  It's only called from `hover.rs` after the window is visible.
- **Don't spoof a user agent that doesn't match the engine**: claude.ai's
  Cloudflare check fails. Only macOS adds the Safari suffix (Google sign-in needs it).
- Google sign-in may still be refused inside embedded webviews; email-code login
  and session-key paste are the fallbacks.
- Tailwind v4: base styles must live in `@layer base` or they override utilities.
- Engine tests must use `Engine::in_memory()` (it disables filesystem detection;
  otherwise tests pick up the developer's real `~/.claude` logs).
- Windows PowerShell blocks `npm.ps1` by default:
  `Set-ExecutionPolicy -Scope CurrentUser -ExecutionPolicy RemoteSigned`.
- Tool id for Claude is still `claude_code` everywhere (serde/DB); only the label is "Claude".

## Verification status
Verified on the owner's **Windows** PC (v0.1.1):
- claude.ai connection works; the widget matches claude.ai → Settings → Usage (LIVE).
- Hover open/close, pill, panel, red alert, session meter, "time left at this
  pace", Today/Priciest stats all work with real Claude Code use.

GitHub CI passed the desktop lifecycle core tests on Windows, macOS, and Linux,
the UI production build, and the Linux desktop compile check. Tests cover stale
and offline readings, expiry recovery, and idle boundaries. OS login, tray interaction, notifications, and idle wake still
need hands-on verification on Windows and macOS.

At 2× device scale, the browser mock was checked in dark and light mode for the
pill and all six panel views across normal, low, critical, runaway and unconnected
scenarios on Free and Pro. Keyboard and contrast checks used Chromium.

Not yet verified: Mac notch placement on real hardware; Codex and Gemini parsers
against real logs (fixtures only); runaway alert and model nudge in real use;
Antigravity CLI paths (unknown); team sync against a deployed API.

Placeholders to replace before launch: `aiusage.dev` / `api.aiusage.dev`,
Lemon Squeezy checkout URLs (`ui/.env`, `web/.env`), updater pubkey in
`app/src-tauri/tauri.updater.conf.json`, code-signing secrets.

## Roadmap (agreed next steps, in order)
1. **One-click handoff implemented.** A tracked Claude Code or Codex five-hour
   or weekly window at 90% used, or a red low-limit alert, offers installed
   destination CLIs for the most recent source session within two hours. Gemini
   is a destination. The sticky preview reads only the selected local log and
   accepts edits. Native code binds confirmation to that preview's folder and
   destination, saves under 1,500 words to `.aiuc-handoff.md`, lists that file in
   the repo's `.git/info/exclude` (never the tracked `.gitignore`; worktrees and
   subfolders handled), copies to the clipboard, and launches the CLI in that folder.
   CLI lookup (`handoff::executable`) checks PATH plus common install folders
   (Homebrew, npm global, nvm, Volta, bun, ~/.local/bin, %APPDATA%\npm) and, on
   macOS, the login shell — GUI apps there get a minimal PATH. Results are cached
   for 60 s because the snapshot asks every few seconds.
   Windows: the command is passed as `-EncodedCommand` with single-quoted literal
   paths (no env vars: Windows Terminal tabs don't inherit them); Windows Terminal
   first, then a new PowerShell console. macOS uses Terminal.app. Linux uses
   wl-copy/xclip and x-terminal-emulator/xterm.
   Fixture tests cover both source formats. Verified on Windows by the owner: TBD.
   **Test handoff** (Check setup): `Engine::test_handoff_offers` offers the latest
   Claude Code/Codex session from the last 24 h whatever the usage; the preview
   command accepts `test: true` and validates against that list instead of the
   snapshot. Same preview → confirm → launch path as the real handoff.
1b. **Destinations: app, terminal or web** (`app/src-tauri/src/handoff.rs`).
   Claude Code opens the Claude desktop app (its Code tab), Codex the Codex app,
   when installed; otherwise the web (claude.ai/code, chatgpt.com/codex). Gemini
   opens its CLI in a terminal, else gemini.google.com. The preview has an
   "Open in" switch (app / Terminal / Web) listing what exists, best first. The
   prompt always goes to the clipboard; app and web need a paste (no deep link
   takes a prompt). App detection: macOS `/Applications/{Claude,Codex}.app`
   (and ~/Applications), Windows exe under %LOCALAPPDATA% or a registered
   `claude://` / `codex://` scheme (Store installs). Owner to verify on Windows.
1c. **Repo-based handoff for cloud and local** (`core/src/repo_handoff.rs`,
   `core/data/aiuc-hook.mjs`, Handoff tab `ui/src/components/HandoffHub.tsx`).
   The repo is the one place every agent shares, so the handoff lives there:
   **Set up** (per repo, optional commit, never pushes) adds `.aiuc/hook.mjs`, a
   marked rule block in `AGENTS.md`/`CLAUDE.md` (and `GEMINI.md` if present) and
   a Claude Code `Stop` hook in `.claude/settings.json`. After every turn, locally
   and in Claude Code cloud, the hook rewrites the auto section of
   `.aiuc/handoff.md` from the transcript (goal, requests, files, to-dos, last
   reply, errors; secrets redacted; no network) and keeps the agent-written
   **Status** above it. In the cloud it `git add`s the file so it ships with the
   next commit. Codex/Gemini (cloud or local) follow the rule to read it first and
   keep Status current. The widget lists repos from recent session folders plus
   ones added with **+** (settings `repos`), fetches origin (25 s timeout, no
   prompts) to see handoffs pushed by cloud agents, finds the newest handoff on
   any branch (`repo_handoff::latest`), and previews "Continue <repo> on branch X
   in <tool>". When a limit alert has no local session (you were in the cloud),
   the alert offers the newest repo handoff from the last 12 h.
   Next: Codex limits for cloud-only users (ChatGPT account connection,
   feasibility unverified); cloud token counts for the meter.
1d. **Agent dispatcher (Agents tab, Alt+Shift+A).** Type or dictate a request;
   `core/src/dispatch.rs` splits it into tasks with plain rules (newline, `;`, or a
   comma/"and"/"then" followed by a named tool; "fix X and add tests" stays one
   task; "Claude and Codex …" is a list, not a directive) and routes each: a named
   tool wins, otherwise the ready tool (CLI installed) with the most room on its
   5-hour limit, +10 points per task already given to it in the batch so parallel
   work spreads out; ≥90% used goes last. Each task can get its own worktree
   (`<parent>/<repo>.agents/<slug>` on branch `agents/<slug>`, default on for 2+
   tasks) and opens its CLI in a terminal with the task as the first message.
   Every task is written to the local `tasks` table (the outcomes ledger, the base
   for learned routing later). `Engine::agent_runs` matches each task to session
   events by folder and tool: starting / working (activity ≤90 s) / idle ("Your
   turn") / no_activity; with cost and request counts. Shown in the Agents tab and
   as a strip on the Usage panel. Dictation uses the OS: Win+H (sent with
   `keybd_event`) or macOS `startDictation:`; nothing is recorded by the app.
   Next: morning brief / weekly recap (Gemini, branch `gemini/daily-weekly-recap`),
   learned routing.
1e. **Destinations everywhere + GitHub.** App detection on Windows uses
   `Get-StartApps` (cached 5 min) and launches `explorer shell:AppsFolder\<AppID>`,
   so Store installs work; Codex falls back to the **ChatGPT app** (Codex is in its
   sidebar). Each dispatcher task has its own Open in (app / terminal / web), the
   last choice per tool is remembered (`ui/src/prefs.ts`). Terminal tasks start
   together; app/web tasks are copied and opened one at a time ("Copy & open next")
   so each has the clipboard to itself. App/web prompts name the folder/branch or
   GitHub repo. **GitHub** (`core/src/github.rs`): connect with the `gh` CLI login,
   a pasted token, or device-flow "Sign in with GitHub" when the build sets
   `AIUC_GITHUB_CLIENT_ID` (an OAuth app's client id; owner to create). Token in
   the local kv store, sent only to api.github.com. Repos not cloned here appear as
   `github:owner/name` (cloud tasks via web/app; Clone button clones with the token
   and resets the remote to the plain URL). Task rows store `dest`; app/web tasks
   show "Opened" until local logs show activity.
1f. **Auto paste and send** (`app/src-tauri/src/autosend.rs`, setting `auto_send`,
   default on). After opening an app/web page, wait (15 s app / 20 s web) for the
   foreground window whose title or app name contains the tool's keyword (claude /
   codex|chatgpt / gemini; never our own window), let it settle (1.8 s / 3 s),
   re-check, then Ctrl/Cmd+V and Enter (Windows `keybd_event`, macOS System Events,
   which needs Accessibility permission). The ChatGPT app gets paste only (it opens
   on a normal chat, not Codex). If the window never shows, nothing is typed and
   the prompt stays on the clipboard. The dispatcher sends app/web tasks one after
   another while each is delivered; it stops and offers "Copy & open next" otherwise.
1g. **Agent status + approvals from the notch** (`core/src/agent_status.rs`,
   `app/src-tauri/src/agents.rs`, `ui/src/components/Approvals.tsx`). Off until
   the user turns it on (Agents tab card or Settings → Agents), because it edits
   their global tool config: `~/.claude/settings.json` gets hooks on
   UserPromptSubmit, Stop, Notification (permission/idle prompts) and
   PermissionRequest; `~/.codex/config.toml` gets a top-level `notify` (left alone
   if Codex already notifies another program). The hook is this same app binary:
   `main` calls `hook_main()` first, which handles `--aiuc-hook claude|codex` and
   exits before Tauri starts. Our entries are found by the `--aiuc-hook` flag, so
   install/uninstall never touches other hooks, and they are rewritten at startup
   so they follow the app if it moves. Each event writes one status file per
   session in `<data dir>/agents/sessions/` (folder, kind, time; never the prompt
   or reply). `Engine::agent_runs` prefers that report over log timing unless the
   logs show newer activity: states add `needs_approval`, `done`, `queued`.
   **Approvals**: on PermissionRequest the hook writes `agents/pending/<id>.json`
   (tool name + one-line summary such as the command) and waits up to 60 s for
   `agents/answers/<id>.json` from the widget (Allow / Deny / In terminal). No
   answer, or the widget not running (heartbeat file `agents/alive` older than
   20 s), means no output, so Claude Code asks in the terminal as usual. Both files
   are deleted when answered. The panel pops open and the pill pulses amber.
   Unverified: whether Claude Code shows its own prompt while the hook waits.
1h. **Verified done** (`core/src/verify.rs`, setting `verify_done`, default on).
   When a task's agent is done (Stop report, quiet 20 s) or idle, and there was
   activity since the last run (within 2 h), the worker runs the project's tests
   in the task folder: `.aiuc/verify` first line, else cargo / pnpm|yarn|bun|npm
   test (not npm's placeholder) / go / pytest / `make test`. One run at a time, 15
   min timeout (Windows kills the process tree). The task stores only the result
   and seconds; the last 40 lines of output stay in memory for "show". Notifies
   "Codex finished: tests pass".
1i. **Queue at limit reset.** `ToolRoom.resets_at` is the used-up window's reset
   (else the 5-hour one). In the dispatcher a task for a tool at ≥98% shows a
   "Queued · 4:12 PM" chip (on by default, click to send anyway). Queued tasks
   are rows with `queued_until` (and `worktree`); the worker sends due ones when
   the tool shows >5% room: terminal at once, app/web only after 20 s without
   keyboard/mouse input (`autosend::idle_secs`) since they're pasted in. Each row
   offers Send now / Move to <roomiest tool> (`run_queued`).
1j. **Ask the notch** (Claude: `core/src/ask.rs`, `app/src-tauri/src/{ask,capture}.rs`,
   `ui/src/components/{AskView,Markdown,Pointer}.tsx`). Alt+Shift+Space anywhere
   (or the ✦ button): before the widget takes focus, `capture::capture` records the
   front window (HWND / app name), a screenshot of the focused window via `xcap`
   (Win/mac only; scaled to ≤1600 px, temp PNG in `<data>/ask-tmp`, wiped on the
   next ask), the selected text (waits for the hotkey's modifiers to be released,
   sends Ctrl/Cmd+C, reads it, then restores your clipboard) and the clipboard if it
   changed in the last 2 min (a background clock keeps only a sequence number/hash).
   `ask::pick` chooses the installed CLI with most room (a pinned tool wins unless
   ≥98%); `ask::command` runs it headless and read-only: `claude -p --output-format
   stream-json --include-partial-messages --model haiku|opus` (only Read allowed),
   `codex exec --json --sandbox read-only -c model_reasoning_effort=low|high [-i shot] -`,
   `gemini -p "@shot …"` (no `-m`: Google retires model names; the CLI default follows.
   Gemini CLI no longer accepts personal Google logins, so it needs GEMINI_API_KEY and
   `/auth` set to "Use Gemini API key"); the prompt goes on
   stdin, `StreamReader` turns each CLI's output into text deltas (`ask-delta` /
   `ask-done` events). Child processes get `AIUC_ASK=1` so our agent hooks ignore
   them. Questions about limits/agents/tests get `ask::work_brief` (snapshot +
   failed test tails). No CLI at all → opens the web app with the question pasted.
   **Council**: every installed tool answers in parallel (`<id>:<tool>`), then the
   roomiest combines them (Agree / Differ / Best answer). **Pointing**: "where do I…"
   with a screenshot asks for `<point x y label/>` markers; `parse_points` maps them
   to fractions of the captured window and a click-through `pointer` webview window
   (now the `buddy` overlay, see 1k) draws them. Unverified: marker
   accuracy per model. **Actions**: Replace / Paste reply (refocus the original
   window, Ctrl/Cmd+V), Copy, Run in terminal (new shell in the repo, command pasted
   not run), Fix it with an agent / Make it a task (prefills the Agents box), Open in
   the tool's app or web, Listen (Web Speech). "Tell me when…/Remind me…" questions
   go to Gemini's watchers (`add_watch`). History is off by default; the opt-in keeps
   the last 20 Q&A in the webview's localStorage. macOS needs Screen Recording and
   Accessibility permission. **Prompt polish / watchers / suggestion chips** were
   built by Gemini (`core/src/{polish,watch,suggest}.rs`, see HANDOFF-gemini.md);
   `ask::polish_prompt` runs `polish::request` on the roomiest tool (Quick) with the
   repo's newest failed-test tail, `watch::tick` runs every worker tick, and
   "aiuc-ask" window events from chips are handled in App.tsx (fix → Ask,
   send_failure → Agents box, handoff → Handoff tab).
1k. **Hold to talk + on-screen buddy** (Claude: `app/src-tauri/src/{voice,buddy}.rs`,
   `ui/src/components/Buddy.tsx`, `ask::{wants_guide,guide_prompt,parse_guide}`).
   The Ask hotkey now reports press and release: a tap (< 320 ms) opens Ask as
   before; holding starts Windows' own speech recognizer (WinRT
   `SpeechRecognizer` continuous dictation on its own COM thread; needs Settings →
   Privacy → Speech → Online speech recognition, the error says so). Release →
   transcript → `voice_ask`: captures the screen, then either answers in a bubble
   by the cursor (Quick, 1–3 sentences, streamed via `ask-delta` id `voice-…`,
   read aloud by the overlay's Web Speech) or, for "how do I…/guide me…", runs the
   guide loop: screenshot → `guide_prompt` → one step + `<point/>` → the buddy
   pointer flies there → wait for a left click (Windows `GetAsyncKeyState`; 8 s on
   other OSes) → look again, up to 10 steps or `DONE:`. A tap stops a guide. The
   overlay is a click-through `buddy` webview window sized to the cursor's monitor;
   points on a window screenshot are mapped to monitor fractions. The last voice
   turn (3 min) is loaded into the Ask panel on the next tap. `ask::answer` is the
   shared "try the next tool" runner used by Ask, voice and guide. Mac voice later.
2. **Audit from day one.** The scanner already reads 45 days of history; produce
   a plan recommendation on first launch instead of "come back in 4 weeks"
   (be explicit about confidence when history is short).
3. **"Use it or lose it".** Warn when a window will reset with lots unused (the
   queue at reset is done, see 1i).
4. **"What used up my limit?"** Plain-English breakdown of what burned the
   window (session, repeated file reads, cache misses, model choice) with a fix.
5. Team: seat right-sizing report on the dashboard; deploy `api/` (Fly) and `web/` (Vercel).
6. Tune runaway/nudge thresholds on real data.

## Conventions
- Keep UI copy plain and short; show % used; label estimates as "est.".
- The user-confirmed local handoff file and the dispatcher's local `tasks` table
  (task text you typed, kept on this machine, never synced) are the explicit
  persistence exceptions. A pending approval file (tool name + one-line summary)
  exists only until it is answered or times out.
  In usage tracking, never store or sync prompt text, code, file names or
  project names (sync sends counts only — there's a test for this). Keep it that way.
- New core logic gets a test in `core/tests/core.rs`.
- Commit messages: imperative summary line, then bullets of what/why.

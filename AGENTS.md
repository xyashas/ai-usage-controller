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

CI (`.github/workflows/ci.yml`): core tests on Linux/macOS/Windows, unsigned
macOS universal + Windows installers uploaded as artifacts
(`bundles-macos-latest`, `bundles-windows-latest`), Linux compile check, ui/api/web.
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
- **One fixed-size transparent window (420×600) pinned top-centre. It never
  resizes.** The pill and panel are one shared "surface" that morphs with CSS
  transitions. (Earlier versions resized the window and raced with a
  ResizeObserver, leaving a huge oval on screen — don't go back to that.)
- The window is click-through except the shape's rectangle. The UI reports the
  **target** rect (not a mid-animation measurement) via `set_hit_rect`;
  `hover.rs` polls the cursor every 40 ms, toggles `set_ignore_cursor_events`,
  and emits `hover` true/false. That event is the single source of truth for
  open-on-hover (140 ms dwell) / close-on-leave (260 ms grace).
- Views other than the main panel (settings, onboarding, setup, paywall, audit)
  are "sticky": they don't close on leave, only on click-away (blur) or Esc.
  Blur is ignored while the cursor is over the shape.
- Content height is measured from a stable wrapper (`innerRef`) and capped at 560.
- Typing needs keyboard focus: inputs call `api.focus()` on focus.
- macOS: window level 25 (above menu bar), all Spaces; notch height from
  `NSScreen.safeAreaInsets`.

### Widget appearance and usage history
- The native 420×600 window stays fixed. CSS animates the surface to its measured
  view height. The OS selects the panel palette. A collapsed Mac notch stays black
  in both themes and uses a small usage ring. Other pills use a horizontal meter.
- The pill counts to its next percentage and only pulses for red alerts. Reduced
  motion disables transitions, pulses and counting. Hidden layers are inert,
  Tab reaches the visible controls, Enter activates them and Esc returns to the pill.
- The main meter's sparkline uses the last 24 hours of stored samples for its
  binding window after tier gating. Five-minute buckets retain window changes.
  Dashed ticks mark elapsed reset times from the samples. Long gaps are left open.
  Tools without gauge history omit the chart.
- Preview a view directly with `?view=panel|settings|setup|paywall|audit|onboarding`.
  Use `?platform=macos&notch=32` for a notched Mac, and `?scenario=empty|learning`
  to check the first-use states.

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
1. **One-click handoff when about to be cut off.** At ~95% (or on the alert),
   a "Continue in Codex / Gemini" button: read the active Claude Code session from
   its local JSONL (last user goals, files touched, open TODOs, last errors),
   build a concise handoff prompt, copy it to the clipboard and open a terminal in
   the same project folder running the other CLI. Never send log content anywhere
   but the user's own clipboard/terminal. Also the reverse direction.
2. **Audit from day one.** The scanner already reads 45 days of history; produce
   a plan recommendation on first launch instead of "come back in 4 weeks"
   (be explicit about confidence when history is short).
3. **"Use it or lose it" + task queue.** Warn when a window will reset with lots
   unused; let the user queue a task (`claude -p "<task>"` in a folder) that runs
   when the limit resets or overnight.
4. **"What used up my limit?"** Plain-English breakdown of what burned the
   window (session, repeated file reads, cache misses, model choice) with a fix.
5. Team: seat right-sizing report on the dashboard; deploy `api/` (Fly) and `web/` (Vercel).
6. Tune runaway/nudge thresholds on real data.

## Conventions
- Keep UI copy plain and short; show % used; label estimates as "est.".
- Privacy is a feature: never store or sync prompt text, code, file names or
  project names (sync sends counts only — there's a test for this). Keep it that way.
- New core logic gets a test in `core/tests/core.rs`.
- Commit messages: imperative summary line, then bullets of what/why.

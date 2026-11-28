# AI Usage Controller

A Mac and Windows widget that shows how much AI runway you have left, warns you before you get cut off, and tells you when you're wasting money. It covers Claude Code, Codex and Gemini.

**Never get cut off mid-task, never overpay for AI.** The meter is free; the savings are paid.

```
┌──────────────────────── notch ────────────────────────┐
              ▬▬▬▬▬▬▬▬░░░░░░░░░░░░  $3.76
┌──────────────────────────────────────────────┐
│ Claude Code  LIVE                       34%  │
│ ████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ │
│ About 20 min left at this pace  resets 2h12m │
│ ● $4.20 in 6 min, possible loop. Stop? [Stop]│
│ Codex   ████████████████░░░░  76%   ↻ 3h 21m │
│ Gemini  ██████████████████░░  91%  ↻ 10h 00m │
│ THIS SESSION  TODAY     PRICIEST             │
│ $3.76         $20.92    $9.80                │
│ Saved this month $212 est.          Settings │
└──────────────────────────────────────────────┘
```

## Hand work to another AI (local and cloud)

When a limit gets close, carry the work to another tool with its context:
the goal, latest requests, changed files, errors and what's left.

**Where it opens.** Claude Code opens in the Claude desktop app (Code tab), and Codex
in the Codex app, if installed. Otherwise they open on the web (claude.ai/code,
chatgpt.com/codex). Gemini opens its CLI in a terminal. The preview's **Open in**
switch also offers the terminal (for installed CLIs) and the web. The prompt is
always copied to the clipboard; the terminal option also saves it as
`.aiuc-handoff.md` (hidden from git through `.git/info/exclude`) and starts the CLI
already reading it.

**Local sessions.** At 90% of a 5-hour or weekly limit, or on a red alert, the panel
offers your latest Claude Code or Codex session. The **Handoff** tab offers it any time.

**Cloud sessions (and local ones, too).** The repo is the one place every agent shares,
so the handoff lives there. In the Handoff tab, **Set up** a repo once. This adds
`.aiuc/hook.mjs`, a short rule in `AGENTS.md` and `CLAUDE.md`, and a Claude Code hook.
It can commit those changes; it never pushes.
After every Claude Code turn, locally and in Claude Code on the web, the hook rewrites
`.aiuc/handoff.md`. Codex and Gemini read it first and keep its Status current.
The file travels with the branch.
The widget fetches your repos, finds the newest handoff on any branch, and offers
**Continue in Codex / Claude Code / Gemini**. A limit alert does the same when you
were working in the cloud.

The widget prepares handoffs locally and sends nothing to its API. The hook never uses
the network and redacts things that look like keys before writing.

## Run several agents at once

Press **Alt+Shift+A** (⌥⇧A on a Mac) or open the **Agents** tab. Type or dictate
something like *"Have Codex fix the failing checkout test, and Claude update the docs"*.
The widget splits that into tasks and gives each to the tool you named, or to the tool
with the most room left. With **Worktrees** on, each task runs on its own branch in a
separate folder, so the agents don't edit the same files. Each task opens in its own
terminal. The Agents list shows each task as working or waiting for you, with its
cost so far. Task text stays on your computer.

**Know when an agent needs you.** Turn on *agent status* (Agents tab or Settings) and
Claude Code and Codex tell the widget when they finish or are waiting for you. When
Claude Code asks to run a command, the panel opens with **Allow / Deny / In terminal**,
so you can answer from the notch. This adds a hook to `~/.claude/settings.json` and
`~/.codex/config.toml`; only the folder and the event are recorded.

**Verified done.** When an agent finishes, the widget runs the project's tests in its
folder (cargo, npm/pnpm/yarn/bun, go, pytest, `make test`, or the command in
`.aiuc/verify`) and shows *Tests pass* or *Tests fail*, with the output a click away.

**Queue for when the limit resets.** Give a task to a tool that's out of room and it's
held until that limit resets, then sent automatically. Or send it now, or move it to
the tool with the most room.

## What's in v1

| # | Feature | Plan | Where |
|---|---------|------|-------|
| 1 | Live headroom bar (notch on Mac, pill on Windows) | Free (1 tool) | `core/src/engine.rs`, `ui/src/components/Collapsed.tsx` |
| 2 | Time-to-cutoff ("about 20 min left at this pace") | Pro | `core/src/metrics.rs` |
| 3 | Tool switchboard (Claude Code, Codex, Gemini) | Pro | `ui/src/components/Expanded.tsx` |
| 4 | Reset timers (5-hour, weekly, Opus weekly, Gemini daily) | Free | `core/src/metrics.rs` |
| 5 | Runaway alert and stop | Pro | `core/src/runaway.rs`, `core/src/stop.rs` |
| 6 | Live session meter | Pro | `core/src/engine.rs` |
| 7 | Model-cost nudge | Pro | `core/src/advice.rs` |
| 8 | Monthly plan audit | Pro | `core/src/advice.rs`, `ui/src/components/AuditCard.tsx` |
| 9 | Savings counter | Pro | `core/src/store.rs` (savings ledger) |
| 10 | Team dashboard | Team | `web/app/dashboard`, `api/src/app.ts` |

Plus: onboarding, settings, paywall with the user's own projected saving as the headline, 7-day no-card trial, Lemon Squeezy licences, multi-device and team sync (counts only), the week-0 waste calculator, the landing page, the Anthropic Admin API spend view for API-billed teams, and a terminal CLI.

## Repo layout

```
core/   Rust engine: parsers, store, metrics, alerts, advice, licences, sync client, `aiuc` CLI
app/    Tauri 2 shell: notch/pill window, tray, Option/Alt-A, notifications, background loop
ui/     React + Tailwind panels rendered inside the Tauri webview
api/    Hono + SQLite: price/plan tables, waitlist, orgs/invites, sync, team dashboard data, webhooks
web/    Next.js: landing page, week-0 waste calculator, team dashboard
docs/   Log formats (week-1 gate), releasing and signing, privacy
```

## Run it

Prerequisites: Rust (stable), Node 22.13+. On Linux, the Tauri system libraries (`libwebkit2gtk-4.1-dev` and friends).

```bash
# 1. The engine in a terminal (week-1 gate: compare with /usage)
cargo run -p aiuc-core --bin aiuc -- paths
cargo run -p aiuc-core --bin aiuc -- plan claude_code max5
cargo run -p aiuc-core --bin aiuc -- watch
```

```bash
# 2. The desktop widget (hot-reloading UI)
npm ci --prefix ui && npm ci --prefix app
cd app && npx tauri dev
```

```bash
# 3. Just the panels in a browser with mock data
npm run dev --prefix ui        # http://localhost:1420/?scenario=runaway  (also: low, critical, ?tier=free, ?onboarding=1)
```

```bash
# 4. API and website
npm ci --prefix api && npm run dev --prefix api      # :8787
npm ci --prefix web && npm run dev --prefix web      # :3000  (/calculator, /dashboard)
```

Installers: `cd app && npx tauri build` (add `--target universal-apple-darwin` on a Mac). Signed releases come from the `Release` workflow; see [docs/RELEASING.md](docs/RELEASING.md).

## Check that it works

Open the panel, then the pulse icon (**Check setup**). It shows, for each tool, where the numbers come from, how many requests it saw today, when it last updated, and what to compare against:

- **Claude:** claude.ai → Settings → Usage (or `/usage` in Claude Code). If you use claude.ai or the desktop app, press **Connect Claude account** and sign in once; the widget then reads the same plan limits claude.ai shows.
- **Codex:** `/status` inside Codex.
- **Gemini:** `/stats` inside Gemini CLI.

**Copy details** puts a diagnostics report on the clipboard for bug reports.

## Tests

```bash
cargo test -p aiuc-core        # 26 tests: parsers on real-shaped log fixtures, headroom, blocks, cutoff, runaway, nudge, audit, gating, sync privacy
npm test --prefix api          # 6 tests: tables, waitlist, full team flow, personal sync, webhooks, Anthropic cost report
npm test --prefix web          # 10 tests: ccusage parsing and the waste recommendations
```

CI runs the core tests on Linux and Windows and builds the unsigned Windows installer on every pull request. The Mac build (10× the Actions minutes) runs only on request: Actions → CI → Run workflow → tick "Build for Mac".

## How the numbers work

- **Usage** is shown as percent of the limit used, like Claude's and Codex's own screens; the bar shows the tightest window. Sources, best first: the vendor's own gauge (Codex writes `rate_limits` into its logs; Claude's limits come from claude.ai when you connect your account, or from the Claude Code login, best effort), a calibrated allowance learned from past live readings, your own historical peak window. With none of those the tool shows **Learning** instead of guessing.
- **Time to cutoff** uses a rolling 15-minute burn rate, shows only while a session is active, and rounds to 5 minutes.
- **Runaway**: last-5-minute spend > 4× your normal 5-minute average, ≥10 agent turns with no user message, and most turns repeat the same files or tool calls. Alert-only by default; one-click Stop (sends Ctrl-C to that session's process, Mac/Linux) is a setting, off until thresholds are tuned on beta data.
- **Costs** are API-list-price estimates from `core/data/prices.json`. On a subscription they measure allowance used, not money. Savings are always labelled "est."
- Price and plan tables are served by the API at `/v1/prices` and `/v1/plans`; the app refreshes them every 6 hours, so a vendor price change needs no release.

## Privacy

The app reads local log files and keeps everything in a local SQLite database. It never stores prompt text or code. If you turn sync on, only token counts, timestamps, model names and estimated cost leave the machine. See [docs/PRIVACY.md](docs/PRIVACY.md).

## Verification status

Checked in the build environment (Linux):

- Core, API and calculator test suites pass.
- The Claude Code parser was run against real Claude Code session logs, and its request count and cost matched an independent recount of the raw file.
- The Tauri app was built and launched under a virtual display: first-run onboarding, detection, and the live session meter on real data all worked.
- End-to-end sync worked: CLI → running API → team dashboard. The server stored counts only.

Not yet checked; do these in week 1:

- **Mac notch and Windows pill placement.** The window-level and notch code compiles in CI on macOS and Windows but hasn't been run on real hardware.
- **Codex and Gemini parsers.** They're tested against fixtures built from the documented log shapes, not against your own logs yet. Run `aiuc status` on a machine with both installed and compare against `/status` in Codex and `/stats` in Gemini. If a field moved, update the fixture and the parser. This is the week-1 gate in [docs/LOG-FORMATS.md](docs/LOG-FORMATS.md).
- **Claude live gauge.** It uses the same unofficial endpoint as `/usage`. The build machine had no Claude login, so check that the panel shows **LIVE** for Claude.
- **Antigravity CLI.** Its log layout isn't confirmed. Settings accept extra log roots, so it can be added once you know where it writes.

Placeholders to replace before launch: `aiusage.dev` / `api.aiusage.dev`, the Lemon Squeezy checkout URLs (`ui/.env`, `web/.env`), and the updater public key in `app/src-tauri/tauri.updater.conf.json`.

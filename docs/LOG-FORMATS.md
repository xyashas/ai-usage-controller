# Log formats and paths (week-1 gate)

Everything downstream depends on these. Each parser is versioned (`PARSER_VERSION`) and has fixtures in `core/tests/fixtures/`. When a vendor changes a format, the fixture test for that one source breaks and the rest keep working.

## Paths

| Tool | Mac / Linux | Windows | Override |
|------|-------------|---------|----------|
| Claude Code | `~/.claude/projects/<project>/<session>.jsonl`, `~/.config/claude/projects/...` | `%USERPROFILE%\.claude\projects\...` | `CLAUDE_CONFIG_DIR` (comma-separated) |
| Claude login (live gauge) | macOS Keychain item `Claude Code-credentials`; Linux `~/.claude/.credentials.json` | `%USERPROFILE%\.claude\.credentials.json` | |
| Codex CLI | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | `%USERPROFILE%\.codex\sessions\...` | `CODEX_HOME` |
| Gemini CLI | `~/.gemini/tmp/<project-hash>/chats/session-*.json` | `%USERPROFILE%\.gemini\tmp\...` | `GEMINI_DIR` |
| Antigravity CLI | **unconfirmed** | **unconfirmed** | add via Settings → extra roots |

`aiuc paths` prints what exists on the current machine.

## Claude Code (`core/src/parsers/claude.rs`)

- `type: "assistant"` lines carry `message.id`, `requestId`, `message.model`, `message.usage.{input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens}` and `message.content[]`.
- One API response is often written as several lines (one per content block) with the same `message.id` + `requestId`. We dedup on that pair and keep the largest token counts.
- `model: "<synthetic>"` lines are skipped.
- `type: "user"` is a human prompt when `content` is a string or text blocks; arrays of `tool_result` blocks and `isMeta` lines are not prompts.
- Tool-call signatures (`Edit:/path/file.ts`) are kept for loop detection only, and never synced.
- **Checked against real logs** in the build environment. The request count and cost matched an independent recount.

## Claude plan gauge (`core/src/live.rs`)

`GET https://api.anthropic.com/api/oauth/usage` with `Authorization: Bearer <Claude Code OAuth access token>` and `anthropic-beta: oauth-2025-04-20`. Returns `five_hour`, `seven_day`, `seven_day_opus` as `{ utilization (0–100), resets_at }`. Unofficial: polled once a minute, labelled best effort, and the local estimate stays as the fallback. We never refresh or rewrite the token.

## Codex CLI (`core/src/parsers/codex.rs`)

Lines are `{ timestamp, type, payload }`:

- `session_meta` gives `payload.id` and `payload.cwd`.
- `turn_context` gives `payload.model`.
- `event_msg` with `payload.type == "user_message"` is a human prompt.
- `event_msg` with `payload.type == "token_count"`:
  - `payload.info.last_token_usage` holds the per-request tokens; `info.total_token_usage.total_tokens` is the dedup key.
  - `payload.rate_limits.primary|secondary` = `{ used_percent, window_minutes, resets_in_seconds | resets_at }`. This is Codex's own gauge, so Codex headroom is **live**.
- `response_item` with `payload.type == "function_call"` is a tool call.
- OpenAI's `input_tokens` includes cached tokens (we subtract them). `output_tokens` already includes reasoning tokens.
- **To confirm in week 1:** field names on the Codex version your users run (`resets_at` and `resets_in_seconds` are both handled).

## Gemini CLI (`core/src/parsers/gemini.rs`)

- Each session is one JSON file, rewritten as it grows: `{ sessionId, projectHash, messages: [...] }`.
- `type: "gemini"` messages carry `model`, `tokens.{input, output, cached, thoughts, tool, total}` and `toolCalls[]`.
- Each `gemini` message counts as one request against the daily quota. Quotas reset at midnight Pacific.
- Plan limits are in `core/data/plans.json` (free account 1,000/day; Google AI Pro 1,500; Google AI Ultra 2,000; free API key 250) and are served from the backend so they can change without a release.
- **To confirm in week 1:** whether your target users are on Gemini CLI or Antigravity CLI (Google moved Pro and Ultra users on 18 June 2026), and Antigravity's paths and fields.

## Gate checklist

- [ ] `aiuc paths` finds all three tools on a Mac and on a Windows PC.
- [ ] `aiuc status` for Claude matches `/usage` within 5% over three days (set the live gauge on; compare the 5-hour and weekly percentages).
- [ ] Codex headroom matches Codex's own `/status`.
- [ ] Gemini request count matches `/stats`.
- [ ] Add one real (redacted) log sample per tool to `core/tests/fixtures/` and commit.

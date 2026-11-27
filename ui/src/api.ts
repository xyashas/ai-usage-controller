// Bridge to the Rust core. Inside Tauri it calls commands; in a plain browser
// (`npm run dev`) it serves a realistic mock so the panels can be designed fast.
import type { Pref, BuddyState, VoiceTurn, AskContext, AskDone, AskRequest, Alert, ApprovalAnswer, Dest, InstallResult, DeviceCode, DispatchPlan, GithubStatus, Started, Detected, Diagnostics, HandoffOffer, HandoffSession, HandoffPreviewData, Plan, Rect, RepoStatus, Settings, SetupResult, Snapshot, Tool, CiRun, InboxItem, Suggestion, Watch } from "./types";

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(cmd, args);
  }
  return mock(cmd, args) as T;
}

export const api = {
  previewHandoff: (source: Tool, session: string, target: Tool, test = false) => call<HandoffPreviewData>("preview_handoff", { source, session, target, test }),
  previewRepoHandoff: (path: string, target: Tool) => call<HandoffPreviewData>("preview_repo_handoff", { path, target }),
  testHandoffOffers: () => call<HandoffOffer[]>("test_handoff_offers"),
  listHandoffSessions: () => call<HandoffSession[]>("list_handoff_sessions"),
  launchHandoff: (id: number, prompt: string, dest: Dest) => call<string>("launch_handoff", { id, prompt, dest }),
  planDispatch: (text: string, repo: string | null) => call<DispatchPlan>("plan_dispatch", { text, repo }),
  runDispatch: (repo: string, tasks: { text: string; tool: Tool; dest: Dest; queue?: boolean }[], worktrees: boolean) => call<Started[]>("run_dispatch", { repo, tasks, worktrees }),
  runQueued: (id: number, tool?: Tool) => call<string>("run_queued", { id, tool: tool ?? null }),
  setAgentStatus: (on: boolean, approvals?: boolean) => call<InstallResult>("set_agent_status", { on, approvals: approvals ?? null }),
  askContext: (pinned: Tool | null) => call<AskContext>("ask_context", { pinned }),
  askForget: (part: "selection" | "clipboard" | "screenshot" | "all") => call<void>("ask_forget", { part }),
  askStart: (req: AskRequest) => call<void>("ask_start", { req }),
  askCancel: (id: string) => call<void>("ask_cancel", { id }),
  askPasteBack: (text: string) => call<void>("ask_paste_back", { text }),
  askCopy: (text: string) => call<void>("ask_copy", { text }),
  askRun: (command: string) => call<string>("ask_run", { command }),
  askOpenIn: (tool: Tool, text: string) => call<string>("ask_open_in", { tool, text }),
  buddyState: () => call<BuddyState | null>("buddy_state"),
  buddyHide: () => call<void>("buddy_hide"),
  lastVoiceTurn: () => call<VoiceTurn | null>("last_voice_turn"),
  askHidePoints: () => call<void>("ask_hide_points"),
  memoryList: () => call<Pref[]>("memory_list"),
  memoryAdd: (text: string) => call<Pref>("memory_add", { text }),
  memoryRemove: (id: number) => call<void>("memory_remove", { id }),
  memoryClear: () => call<void>("memory_clear"),
  answerApproval: (id: string, answer: ApprovalAnswer) => call<void>("answer_approval", { id, answer }),
  verifyTask: (id: number) => call<void>("verify_task", { id }),
  verifyOutput: (id: number) => call<string | null>("verify_output", { id }),
  githubStatus: () => call<GithubStatus>("github_status"),
  githubConnect: (token?: string) => call<string>("github_connect", { token: token ?? null }),
  githubDeviceStart: () => call<DeviceCode>("github_device_start"),
  githubDevicePoll: (deviceCode: string) => call<string | null>("github_device_poll", { deviceCode }),
  githubDisconnect: () => call<void>("github_disconnect"),
  cloneRepo: (fullName: string) => call<string | null>("clone_repo", { fullName }),
  dismissAgent: (id: number) => call<void>("dismiss_agent", { id }),
  startDictation: () => call<void>("start_dictation"),
  listRepos: (fetch: boolean) => call<RepoStatus[]>("list_repos", { fetch }),
  setupRepo: (path: string, commit: boolean) => call<SetupResult>("setup_repo", { path, commit }),
  addRepo: () => call<string | null>("add_repo"),
  forgetRepo: (path: string) => call<void>("forget_repo", { path }),
  snapshot: () => call<Snapshot>("get_snapshot"),
  settings: () => call<Settings>("get_settings"),
  saveSettings: (settings: Settings) => call<void>("save_settings", { settings }),
  plans: () => call<Record<Tool, Plan[]>>("get_plans"),
  detect: () => call<Detected[]>("detect_tools"),
  setHitRect: (rect: Rect) => call<void>("set_hit_rect", { rect }),
  focus: () => call<void>("focus_widget"),
  connectClaude: () => call<void>("connect_claude"),
  claudeSessionKey: (key: string) => call<void>("claude_session_key", { key }),
  disconnectClaude: () => call<void>("disconnect_claude"),
  refreshNow: () => call<void>("refresh_now"),
  diagnostics: () => call<Diagnostics>("diagnostics"),
  dismiss: (id: string) => call<void>("dismiss_alert", { id }),
  dismissDailyRecap: () => call<void>("dismiss_daily_recap"),
  polishPrompt: (text: string, repo: string | null) => call<string>("polish_prompt", { text, repo }),
  addWatch: (text: string) => call<Watch>("add_watch", { text }),
  listWatches: () => call<Watch[]>("list_watches"),
  removeWatch: (id: string) => call<void>("remove_watch", { id }),
  ciRuns: () => call<CiRun[]>("ci_runs"),
  ciLog: (runId: number) => call<string>("ci_log", { runId }),
  inboxList: () => call<InboxItem[]>("inbox_list"),
  inboxDone: (id: number, done: boolean) => call<InboxItem | null>("inbox_done", { id, done }),
  inboxRemove: (id: number) => call<boolean>("inbox_remove", { id }),
  inboxClearDone: () => call<number>("inbox_clear_done"),
  inboxRemind: (id: number, whenText: string) => call<Watch>("inbox_remind", { id, whenText }),
  inboxToTask: (id: number) => call<string>("inbox_to_task", { id }),
  alertAction: (alert: Alert) => call<string>("alert_action", { id: alert.id }),
  acceptNudge: () => call<void>("accept_nudge"),
  acceptAudit: (tool: Tool) => call<void>("accept_audit", { tool }),
  startTrial: () => call<void>("start_trial"),
  activate: (key: string) => call<string>("activate_licence", { key }),
  deactivate: () => call<void>("deactivate_licence"),
  openUrl: (url: string) => call<void>("open_url", { url }),
  joinSync: (server: string, code: string, email: string) => call<void>("join_sync", { server, code, email }),
  platform: () => call<"macos" | "windows" | "linux">("platform"),
  layout: () => call<{ platform: string; notch_height: number; window: [number, number] } | null>("layout"),
  quit: () => call<void>("quit"),
};

export async function onSnapshot(cb: (s: Snapshot) => void): Promise<() => void> {
  if (inTauri) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<Snapshot>("snapshot", (e) => cb(e.payload));
  }
  const id = setInterval(async () => cb(await api.snapshot()), 3000);
  return () => clearInterval(id);
}

export async function onToggle(cb: () => void): Promise<() => void> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen("toggle", () => cb());
}

/** Cursor entered/left the widget's shape (tracked natively; see hover.rs). */
export async function onHover(cb: (inside: boolean) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<boolean>("hover", (e) => cb(e.payload));
}

/** Streamed pieces of an answer, and the end of it. */
export async function onAsk(
  delta: (id: string, text: string) => void,
  done: (d: AskDone) => void,
  status?: (id: string, s: { tool: Tool; text: string }) => void,
): Promise<() => void> {
  if (inTauri) {
    const { listen } = await import("@tauri-apps/api/event");
    const a = await listen<{ id: string; text: string }>("ask-delta", (e) => delta(e.payload.id, e.payload.text));
    const b = await listen<AskDone>("ask-done", (e) => done(e.payload));
    const c = await listen<{ id: string; tool: Tool; text: string }>("ask-status", (e) => status?.(e.payload.id, { tool: e.payload.tool, text: e.payload.text }));
    return () => { a(); b(); c(); };
  }
  mockAskListeners.push({ delta, done });
  return () => { mockAskListeners = mockAskListeners.filter((l) => l.delta !== delta); };
}

let mockAskListeners: { delta: (id: string, text: string) => void; done: (d: AskDone) => void }[] = [];

export async function onBuddy(cb: (b: BuddyState) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<BuddyState>("buddy", (e) => cb(e.payload));
}

export async function onSuggestion(cb: (s: Suggestion) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<Suggestion>("suggestion", (e) => cb(e.payload));
}

export async function onInboxChanged(cb: () => void): Promise<() => void> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen("inbox-changed", () => cb());
}

export async function onWatchesChanged(cb: () => void): Promise<() => void> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen("watches-changed", () => cb());
}

export async function onOpenView(cb: (v: string) => void): Promise<() => void> {
  if (!inTauri) {
    const preview = new URLSearchParams(location.search).get("view");
    if (preview && ["panel", "settings", "setup", "audit", "paywall", "onboarding", "handoffs", "agents", "ask", "inbox"].includes(preview)) cb(preview);
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>("open-view", (e) => cb(e.payload));
}

// ---------------- browser mock ----------------

const now = () => new Date();
const inMin = (m: number) => new Date(Date.now() + m * 60_000).toISOString();
let mockTier: Snapshot["entitlements"]["tier"] = (new URLSearchParams(location.search).get("tier") as never) || "pro";
const mockScenario = new URLSearchParams(location.search).get("scenario") || "normal";
let mockSettings: Settings = {
  onboarded: new URLSearchParams(location.search).get("onboarding") !== "1",
  tools: {
    claude_code: { enabled: true, plan_id: "max5", five_hour_allowance_usd: null, weekly_allowance_usd: null, extra_roots: [] },
    codex: { enabled: true, plan_id: "plus", five_hour_allowance_usd: null, weekly_allowance_usd: null, extra_roots: [] },
    gemini: { enabled: true, plan_id: "ai_pro", five_hour_allowance_usd: null, weekly_allowance_usd: null, extra_roots: [] },
  },
  primary_tool: "claude_code",
  alerts: { amber_pct: 20, red_pct: 5, runaway_alerts: true, one_click_stop: false, notifications: true },
  launch_at_login: true,
  hide_when_idle: false,
  hotkey: "Alt+A",
  sync: { enabled: false, server_url: null, device_token: null, device_id: null },
  live_gauges: true,
  claude_web: mockScenario === "connected",
  api_base: "https://api.aiusage.dev",
  auto_send: true,
  agent_status: mockScenario !== "unconnected",
  approvals: true,
  verify_done: true,
  show_daily_recap: true,
  ask_screenshots: true,
  ask_memory: true,
  polish_auto: false,
  suggest_chips: true,
  autopilot: true,
  ci_watch: true,
};
const dismissed = new Set<string>();
let mockInbox: InboxItem[] = mockScenario === "empty" ? [] : [
  { id: 201, text: "Check why the checkout coupon test fails when tax is zero", source: "voice", app: "Chrome", at: inMin(-18), remind_at: null, done: false },
  { id: 202, text: "Add rate-limit retry with backoff on the Lemon Squeezy webhook", source: "ask", app: "VS Code", at: inMin(-95), remind_at: inMin(180), done: false },
  { id: 203, text: "Update release notes for v0.1.2", source: "selection", app: "Linear", at: inMin(-360), remind_at: null, done: true },
];
let mockWatches: Watch[] = [
  { id: "w-1", condition: { kind: "limit_reset", tool: "claude_code" }, label: "When Claude resets", created_at: inMin(-15) },
];
let mockGithub: string | null = null;
const extra = { queued_until: null, verify: null, verify_secs: null, verify_at: null } as const;
const mockAgents: Snapshot["agents"] = mockScenario === "empty" ? [] : [
  { ...extra, id: 5, tool: "claude_code", text: "Add rate limiting to the coupon endpoint", repo_name: "acme-api", branch: "agents/rate-limit-coupons", cwd: "", started_at: inMin(-3), state: "queued", dest: "terminal", cost_usd: 0, requests: 0, last_activity_at: null, queued_until: inMin(132) },
  { ...extra, id: 4, tool: "claude_code", text: "Migrate the orders table to the new schema", repo_name: "acme-api", branch: "agents/migrate-orders", cwd: "", started_at: inMin(-9), state: "needs_approval", dest: "terminal", cost_usd: 0.61, requests: 9, last_activity_at: inMin(0) },
  { ...extra, id: 3, tool: "codex", text: "Fix the failing checkout test", repo_name: "acme-web", branch: "agents/fix-failing-checkout-test", cwd: "", started_at: inMin(-6), state: "working", dest: "terminal", cost_usd: 0.42, requests: 18, last_activity_at: inMin(0) },
  { ...extra, id: 2, tool: "claude_code", text: "Update the docs for the new coupon API", repo_name: "acme-web", branch: "agents/update-docs-new-coupon-api", cwd: "", started_at: inMin(-26), state: "done", dest: "terminal", cost_usd: 1.18, requests: 26, last_activity_at: inMin(-2), verify: "passed", verify_secs: 41, verify_at: inMin(-1) },
  { ...extra, id: 6, tool: "codex", text: "Remove the legacy cart code", repo_name: "acme-web", branch: "agents/remove-legacy-cart", cwd: "", started_at: inMin(-40), state: "done", dest: "terminal", cost_usd: 0.88, requests: 14, last_activity_at: inMin(-12), verify: "failed", verify_secs: 63, verify_at: inMin(-11) },
  { ...extra, id: 1, tool: "gemini", text: "Summarise open issues labelled bug", repo_name: "acme-api", branch: null, cwd: "", started_at: inMin(-1), state: "opened", dest: "web", cost_usd: 0, requests: 0, last_activity_at: null },
];
const mockApprovals: Snapshot["approvals"] = mockScenario === "approval" ? [
  { id: "a1", tool: "claude_code", session: "s", cwd: "/dev/acme-api.agents/migrate-orders", repo_name: "migrate-orders", tool_name: "Bash", summary: "npx prisma migrate dev --name orders_v2", at: inMin(0) },
] : [];
const mockRepos: RepoStatus[] = mockScenario === "empty" ? [] : [
  { id: "/Users/you/dev/acme-web", local: true, root: "/Users/you/dev/acme-web", name: "acme/web", set_up: true, current_branch: "main", github: "https://github.com/acme/web",
    latest: { branch: "claude/fix-login", updated_at: Date.now() / 1000 - 760, origin: "commit", content: "# Handoff" } },
  { id: "/Users/you/dev/acme-api", local: true, root: "/Users/you/dev/acme-api", name: "acme/api", set_up: false, current_branch: "main", github: "https://github.com/acme/api", latest: null },
];
const mockRemote: RepoStatus[] = ["acme/mobile", "acme/infra", "you/dotfiles"].map((n) => ({ id: `github:${n}`, local: false, root: "", name: n, set_up: false, current_branch: "main", github: `https://github.com/${n}`, latest: null }));

function mockSnapshot(): Snapshot {
  const stamp = Date.now();
  const t = stamp / 1000;
  const claudeLeft = mockScenario === "low" ? 14 : mockScenario === "critical" || mockScenario === "cloud" ? 4 : 38 - ((t / 20) % 6);
  const paid = mockTier !== "free";
  const tool = (tool: Tool, label: string, left: number, extra: Partial<Snapshot["tools"][0]> = {}): Snapshot["tools"][0] => ({
    tool, label, detected: true, enabled: true, plan_id: null, plan_name: null,
    headroom_pct: left, color: left <= 5 ? "red" : left <= 20 ? "amber" : "green", source: "live",
    windows: [], minutes_left: null, active: false, today_cost_usd: 0, today_requests: 0, burn_usd_per_min: 0, locked: false, reconnect: false, live_note: null, live_error: null, needs_connect: false, ...extra,
  });
  const tools = [
    tool("claude_code", "Claude", mockScenario === "unconnected" ? 100 : claudeLeft, {
      reconnect: mockScenario === "signed-out",
      live_note: ({ stale: "Updated 5 min ago", offline: "Offline", "signed-out": "Reconnect" } as Record<string, string>)[mockScenario ?? ""] ?? null,
      ...(mockScenario === "unconnected" ? { needs_connect: true, source: "estimate" as const } : {}),
      plan_name: "Claude Max 5x", active: true, minutes_left: paid ? 20 : null, today_cost_usd: 18.42, today_requests: 212, burn_usd_per_min: 0.31,
      windows: mockScenario === "unconnected" ? [] : [
        { key: "five_hour", label: "5-hour", used_pct: 100 - claudeLeft, headroom_pct: claudeLeft, resets_at: inMin(132), source: "live", used_usd: null, allowance_usd: null, used_requests: null, allowance_requests: null },
        { key: "weekly", label: "Weekly", used_pct: 41, headroom_pct: 59, resets_at: inMin(60 * 52), source: "live", used_usd: null, allowance_usd: null, used_requests: null, allowance_requests: null },
      ],
    }),
    tool("codex", "Codex", 76, {
      plan_name: "ChatGPT Plus", today_cost_usd: 2.1, today_requests: 31, locked: !paid,
      windows: [{ key: "five_hour", label: "5-hour", used_pct: 24, headroom_pct: 76, resets_at: inMin(201), source: "live", used_usd: null, allowance_usd: null, used_requests: null, allowance_requests: null }],
    }),
    tool("gemini", "Gemini", 91, {
      plan_name: "Google AI Pro", source: "estimate", today_cost_usd: 0.4, today_requests: 134, locked: !paid,
      windows: [{ key: "daily", label: "Daily", used_pct: 9, headroom_pct: 91, resets_at: inMin(600), source: "estimate", used_usd: null, allowance_usd: null, used_requests: 134, allowance_requests: 1500 }],
    }),
  ];
  if (mockScenario === "empty") tools.length = 0;
  if (mockScenario === "learning" || mockScenario === "unconnected") {
    Object.assign(tools[0], { today_requests: 0, headroom_pct: null, windows: [], minutes_left: null, source: "learning" });
  }
  const alerts: Alert[] = [];
  if (mockScenario === "runaway" && paid)
    alerts.push({ id: "run:mock", level: "red", kind: "runaway", tool: "claude_code", message: "$4.20 in 6 min, possible loop. Stop?", action: { kind: "stop", label: "Stop", target_tool: null }, session_id: "s", project: "/dev/acme", burn_usd_per_min: 0.7 });
  if (claudeLeft <= 20)
    alerts.push({ id: `low:${claudeLeft <= 5 ? "red" : "amber"}`, level: claudeLeft <= 5 ? "red" : "amber", kind: "low_headroom", tool: "claude_code", message: `Claude 5-hour limit ${Math.round(100 - claudeLeft)}% used, about 20 min left at this pace`, action: paid ? { kind: "switch", label: "Switch to Codex", target_tool: "codex" } : { kind: "dismiss", label: "Dismiss", target_tool: null }, session_id: null, project: null, burn_usd_per_min: null });
  return {
    agents: mockAgents.filter((a) => !dismissed.has(`agent:${a.id}`)),
    approvals: mockApprovals.filter((a) => !dismissed.has(`approval:${a.id}`)),
    handoffs: mockScenario === "critical" ? [{ source: "claude_code", session: "s", targets: ["codex", "gemini"] }] : [],
    gauge_history: ["empty", "learning", "unconnected"].includes(mockScenario) ? [] : Array.from({length: 97}, (_, i) => ({
      at: stamp - (96 - i) * 900000, used: i === 96 ? 100 - claudeLeft : (i % 20) * 4,
      reset_at: stamp + (Math.floor(i / 20) * 20 + 20 - 96) * 900000,
    })),
    idle_hidden: mockSettings.hide_when_idle && mockScenario === "idle",
    generated_at: now().toISOString(),
    onboarded: mockSettings.onboarded,
    entitlements: { tier: mockTier, trial_ends_at: mockTier === "trial" ? inMin(60 * 24 * 5) : null, trial_available: mockTier === "free", licence_status: null, max_devices: 2 },
    active_tool: "claude_code",
    tools,
    today: { cost_usd: tools.length ? 20.92 : 0, sessions: 7, top_session: paid ? { tool: "claude_code", session_id: "s", project_name: "acme-api", cost_usd: 9.8 } : null },
    session_meter: paid && tools.length ? { tool: "claude_code", session_id: "s", project_name: "acme-api", cost_usd: 3.62 + ((t / 3) % 10) * 0.02, started_at: inMin(-38), minutes: 38, burn_usd_per_min: 0.31 } : null,
    alerts: alerts.filter((a) => !dismissed.has(a.id)),
    nudge: tools.length && paid && !dismissed.has("nudge") ? { id: "n", tool: "claude_code", count: 340, from_label: "Opus", to_label: "Sonnet", to_model: "claude-sonnet-5-5", actual_usd: 20, alternative_usd: 8, saving_usd: 12, message: "You ran 340 small edits on Opus today. Sonnet would cost about $12 less." } : null,
    savings_month_usd: paid ? 212 : null,
    audit: tools.length && paid ? [{ tool: "claude_code", current_plan: "Claude Max 20x", current_price: 200, action: "downgrade", target_plan_id: "max5", target_plan: "Claude Max 5x", monthly_saving_usd: 100, peak_used_pct: 31, headline: "You used 31% of Claude Max 20x. Claude Max 5x saves you $100/month.", reasoning: ["Peak weekly use would have been 52% of Claude Max 5x (threshold: under 60% for 4 weeks)."] }] : [],
    projected_saving_usd: 112,
    locked: paid ? [] : ["time_to_cutoff", "switchboard", "runaway", "session_meter", "model_nudge", "plan_audit", "savings", "custom_alerts"],
    remote: null,
    daily_recap: tools.length && mockSettings.show_daily_recap && !dismissed.has("daily_recap") ? {
      date: now().toISOString().slice(0, 10),
      tools: [
        { tool: "claude_code", label: "Claude", requests: 184, cost_usd: 14.6, five_hour_peak_pct: 84, weekly_peak_pct: 38, five_hour_resets_at: inMin(132), weekly_resets_at: null },
        ...(paid ? [
          { tool: "codex" as Tool, label: "Codex", requests: 42, cost_usd: 2.9, five_hour_peak_pct: 28, weekly_peak_pct: 14, five_hour_resets_at: inMin(201), weekly_resets_at: null },
          { tool: "gemini" as Tool, label: "Gemini", requests: 96, cost_usd: 0.35, five_hour_peak_pct: 6, weekly_peak_pct: null, five_hour_resets_at: inMin(600), weekly_resets_at: null },
        ] : []),
      ],
      open_tasks_yesterday: 2,
      suggestion: paid ? "Codex has the most room this morning" : "Claude has room to start your day",
    } : null,
    weekly_recap: tools.length ? {
      tools: [
        { tool: "claude_code", label: "Claude", this_week_requests: 812, last_week_requests: 640, this_week_cost_usd: 64.2, last_week_cost_usd: 51.8, change_pct: 23.9, five_hour_hits: 2, weekly_hits: 0 },
        ...(paid ? [
          { tool: "codex" as Tool, label: "Codex", this_week_requests: 194, last_week_requests: 220, this_week_cost_usd: 12.4, last_week_cost_usd: 14.6, change_pct: -15.1, five_hour_hits: 0, weekly_hits: 0 },
          { tool: "gemini" as Tool, label: "Gemini", this_week_requests: 410, last_week_requests: 0, this_week_cost_usd: 1.2, last_week_cost_usd: 0, change_pct: null, five_hour_hits: 0, weekly_hits: 0 },
        ] : []),
      ],
      busiest_hours: [{ hour: 10, requests: 218 }, { hour: 15, requests: 184 }, { hour: 11, requests: 162 }],
      hourly_requests: [6, 2, 0, 0, 0, 0, 4, 18, 54, 112, 218, 162, 95, 118, 140, 184, 132, 86, 48, 24, 16, 12, 8, 4],
      handoffs_count: 3,
      total_limit_hits: 2,
    } : { tools: [], busiest_hours: [], hourly_requests: Array(24).fill(0), handoffs_count: 0, total_limit_hits: 0 },
    suggestions: mockSettings.suggest_chips && mockScenario === "critical" ? [
      { id: "limit-mid-task:claude_code:codex", kind: { kind: "limit_mid_task", tool: "claude_code", to: "codex" }, title: "Hand off to Codex?", detail: "Claude at 96% mid-task", action: "handoff" },
    ] : [],
    autopilot_moves: mockScenario === "critical" ? [{ task_id: 2, to: "codex", at: inMin(-3) }] : [],
  };
}

const mockPlans: Record<Tool, Plan[]> = {
  claude_code: [
    { id: "pro", name: "Claude Pro", price: 20, capacity: 1 },
    { id: "max5", name: "Claude Max 5x", price: 100, capacity: 5 },
    { id: "max20", name: "Claude Max 20x", price: 200, capacity: 20 },
    { id: "api", name: "API (pay as you go)", price: 0, capacity: 0, metered: true },
  ],
  codex: [
    { id: "plus", name: "ChatGPT Plus", price: 20, capacity: 1 },
    { id: "business", name: "ChatGPT Business", price: 30, capacity: 1 },
    { id: "pro", name: "ChatGPT Pro", price: 200, capacity: 6 },
    { id: "api", name: "API (pay as you go)", price: 0, capacity: 0, metered: true },
  ],
  gemini: [
    { id: "free", name: "Free Google account", price: 0, capacity: 1000, daily_requests: 1000 },
    { id: "ai_pro", name: "Google AI Pro", price: 19.99, capacity: 1500, daily_requests: 1500 },
    { id: "ai_ultra", name: "Google AI Ultra", price: 249.99, capacity: 2000, daily_requests: 2000 },
    { id: "api_free", name: "Free API key", price: 0, capacity: 250, daily_requests: 250 },
  ],
};

const mockAnswers: Record<string, string> = {
  fix: "**Cause:** `cart.summary` is `undefined` when the cart is empty, so reading `.total` throws.\n\nGuard it in `applyCoupon`:\n\n```ts\nconst total = cart.summary?.total ?? 0;\nif (!total) return cart;\n```\n\nThen add a test for an empty cart:\n\n```bash\nnpm test -- checkout/total\n```",
  work: "- **Codex** is fixing the checkout test (6 min, working).\n- **Claude** finished the coupon API docs. Tests pass.\n- **Claude** is waiting for your OK to run a migration in acme-api.\n\nClaude has **34%** left and resets at **4:12 PM**, so start the big refactor on **Codex** (24% used).",
  rewrite: "Thanks for the quick turnaround. I've reviewed the changes and they look good. Could you add a test for the empty-cart case before we merge?",
};
let mockPrefs: Pref[] = [
  { id: 1, text: "I mostly write TypeScript and React.", at: "2026-11-20T10:00:00Z" },
  { id: 2, text: "Keep answers short and casual.", at: "2026-11-24T10:00:00Z" },
];

function mockAnswer(req: AskRequest) {
  const key = req.action === "fix" || /fix|error|wrong/i.test(req.question) ? "fix" : req.action === "rewrite" || req.action === "professional" || req.action === "shorter" ? "rewrite" : "work";
  const run = (id: string, tool: Tool, text: string, reason: string, after = 0) => {
    const words = text.split(/(?<= )/);
    words.forEach((w, i) => setTimeout(() => mockAskListeners.forEach((l) => l.delta(id, w)), after + i * 18));
    setTimeout(() => mockAskListeners.forEach((l) => l.done({ id, tool, reason, error: null, points: [], text, opened_web: false })), after + words.length * 18 + 50);
  };
  if (req.council) {
    run(`${req.id}:claude_code`, "claude_code", mockAnswers[key], "");
    run(`${req.id}:codex`, "codex", mockAnswers[key].replace("Guard it", "Check it"), "");
    run(`${req.id}:gemini`, "gemini", "Add a null check before reading `total`.", "");
    run(`${req.id}:combined`, "codex", "**Agree**\n- The crash is `summary` being undefined for empty carts.\n\n**Differ**\n- Gemini only adds a null check; Claude and Codex also return early. Returning early is safer.\n\n**Best answer**\nUse `cart.summary?.total ?? 0` and return the cart unchanged when it's 0, plus an empty-cart test.", "combined by Codex", 1600);
    return;
  }
  run(req.id, req.tool ?? "codex", mockAnswers[key], req.tool ? "your pick" : "Claude is at 96%");
}

function mock(cmd: string, args?: Record<string, unknown>): unknown {
  switch (cmd) {
    case "test_handoff_offers": return [{ source: "claude_code", session: "sess-checkout", targets: ["codex", "gemini"] }];
    case "list_handoff_sessions": return mockScenario === "empty" ? [] : [
      { source: "claude_code", session: "sess-checkout", project: "acme-web", cwd: "/Users/me/acme-web", title: "Fix checkout coupon rounding before tax and add unit tests", updated_at: Math.floor(Date.now() / 1000) - 420, targets: ["codex", "gemini"] },
      { source: "codex", session: "sess-voice", project: "ai-usage-controller", cwd: "/Users/me/ai-usage-controller", title: "Add local Whisper fallback and Windows mic diagnostics", updated_at: Math.floor(Date.now() / 1000) - 1800, targets: ["claude_code", "gemini"] },
      { source: "claude_code", session: "sess-auth", project: "acme-web", cwd: "/Users/me/acme-web", title: "Refactor OAuth token refresh retry loop", updated_at: Math.floor(Date.now() / 1000) - 7200, targets: ["codex", "gemini"] },
    ];
    case "preview_handoff": {
      const sess = String(args?.session ?? "");
      if (sess === "sess-voice") {
        return {
          id: 2, place: "ai-usage-controller",
          prompt: "# Session handoff\n\n## Original goal\n- Add local Whisper fallback and Windows mic diagnostics\n\n## Files created or edited\n- app/src-tauri/src/voice.rs\n- app/src-tauri/src/whisper_local.rs\n\n## TODO\n- [ ] Verify end-to-end on Windows 11",
          destinations: [{ dest: "app", label: "Claude app" }, { dest: "terminal", label: "Terminal" }, { dest: "web", label: "Web" }],
        };
      }
      return {
        id: 1, place: "acme-web",
        prompt: "# Session handoff\n\n## Original goal\n- Fix checkout coupon rounding before tax and add unit tests\n\n## Files created or edited\n- src/checkout/total.ts\n- src/checkout/total.test.ts\n\n## TODO\n- [ ] Run the checkout tests",
        destinations: (args!.target as Tool) === "gemini" ? [{ dest: "terminal", label: "Terminal" }, { dest: "web", label: "Web" }] : [{ dest: "app", label: (args!.target as Tool) === "codex" ? "Codex app" : "Claude app" }, { dest: "terminal", label: "Terminal" }, { dest: "web", label: "Web" }],
      };
    }
    case "preview_repo_handoff": return {
      id: 1, place: cmd === "preview_handoff" ? "acme-api" : "acme/web · claude/fix-login",
      prompt: "# Session handoff\n\n## Original goal\nFix the checkout flow so coupons apply before tax.\n\n## Files created or edited\n- src/checkout/total.ts\n- src/checkout/total.test.ts\n\n## TODO\n- [ ] Run the checkout tests\n- [ ] Update the changelog",
      destinations: (args!.target as Tool) === "gemini" ? [{ dest: "terminal", label: "Terminal" }, { dest: "web", label: "Web" }] : [{ dest: "app", label: (args!.target as Tool) === "codex" ? "Codex app" : "Claude app" }, { dest: "terminal", label: "Terminal" }, { dest: "web", label: "Web" }],
    };
    case "launch_handoff": return args!.dest === "terminal" ? "Codex is starting in a terminal." : "Prompt copied. Paste it into Codex in the Codex app.";
    case "list_repos": return mockGithub ? [...mockRepos, ...mockRemote] : mockRepos;
    case "plan_dispatch": {
      const rooms = [
        { tool: "claude_code", ready: true, used_pct: mockScenario === "critical" ? 100 : 64, active: true, resets_at: inMin(132) },
        { tool: "codex", ready: true, used_pct: 24, active: false, resets_at: inMin(201) },
        { tool: "gemini", ready: true, used_pct: 9, active: false, resets_at: inMin(600) },
      ] as const;
      const names: Record<string, Tool> = { claude: "claude_code", codex: "codex", gemini: "gemini" };
      const parts = String(args!.text).split(/\n|;|,?\s+(?:and|then)\s+(?=(?:have |ask |use )?(?:claude|codex|gemini)\b)/i).map((p) => p.trim()).filter(Boolean);
      let turn = 0;
      const tasks = parts.map((p) => {
        const m = p.match(/^(?:please\s+)?(?:have|ask|use|let|tell)?\s*(claude(?: code)?|codex|gemini)[:,]?\s+(?:to\s+)?(.*)$/i);
        const tool = m ? names[m[1].toLowerCase().split(" ")[0]] : (["gemini", "codex"] as Tool[])[turn++ % 2];
        const text = (m ? m[2] : p).replace(/^./, (c) => c.toUpperCase());
        return { text, tool, explicit: !!m, reason: m ? `You asked for ${tool === "claude_code" ? "Claude Code" : tool === "codex" ? "Codex" : "Gemini"}` : `Most room: ${tool === "gemini" ? 9 : 24}% used` };
      });
      const local = !String(args!.repo ?? "").startsWith("github:");
      const app = (label: string) => [{ dest: "app", label }, ...(local ? [{ dest: "terminal", label: "Terminal" }] : []), { dest: "web", label: "Web" }];
      return { tasks, rooms, dests: { claude_code: app("Claude app"), codex: app("ChatGPT app"), gemini: [...(local ? [{ dest: "terminal", label: "Terminal" }] : []), { dest: "web", label: "Web" }] } };
    }
    case "run_dispatch": return (args!.tasks as { text: string; tool: Tool; dest: Dest; queue?: boolean }[]).map((t) => ({ ...t, branch: args!.worktrees && t.dest !== "web" && !t.queue ? "agents/x" : null, message: t.queue ? "Queued. Sends when Claude Code resets at 4:12 PM" : t.dest === "terminal" ? "Started in a terminal" : t.dest === "app" ? "Sent in the app" : "Sent in the browser", delivered: true, error: null }));
    case "ask_context": return {
      window: "checkout.ts — acme-web — Visual Studio Code", app: "Visual Studio Code",
      selection: mockScenario === "empty" || mockScenario === "copied" ? null : "TypeError: Cannot read properties of undefined (reading 'total')\n    at applyCoupon (src/checkout/total.ts:42:18)",
      clipboard: mockScenario === "copied" ? "TypeError: Cannot read properties of undefined (reading 'total')\n    at applyCoupon (src/checkout/total.ts:42:18)" : null,
      clipboard_image: false,
      screenshot: mockScenario === "copied" ? null : "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='160' height='100'><rect width='160' height='100' fill='%231e1e1e'/><rect x='0' y='0' width='34' height='100' fill='%23252526'/><rect x='44' y='14' width='90' height='6' rx='2' fill='%23569cd6'/><rect x='44' y='28' width='70' height='6' rx='2' fill='%239cdcfe'/><rect x='44' y='42' width='100' height='6' rx='2' fill='%23f44747'/></svg>",
      repo: "acme-web", pick: { tool: (args!.pinned as Tool) ?? "claude_code", reason: args!.pinned ? "your pick" : "" }, installed: ["claude_code", "codex", "gemini"],
    };
    case "ask_start": {
      const req = args!.req as AskRequest;
      const m = /^(?:please )?remember (?:that )?(.+)/i.exec(req.question.trim());
      if (m && !req.history.length) {
        const text = m[1].replace(/^./, (c) => c.toUpperCase()).replace(/\.?$/, ".");
        mockPrefs.push({ id: Date.now(), text, at: new Date().toISOString() });
        setTimeout(() => mockAskListeners.forEach((l) => l.done({ id: req.id, tool: null, reason: "noted", error: null, points: [], text: `Got it. I'll remember: ${text}`, opened_web: false })), 250);
        return null;
      }
      mockAnswer(req); return null;
    }
    case "buddy_state": {
      const m = new URLSearchParams(location.search).get("buddy");
      if (!m) return null;
      const base = { said: "how do I turn off notifications in Slack", text: "", cursor: [0.42, 0.3], spots: [], step: 1, id: "", speak: "" };
      if (m === "guide") return { ...base, mode: "guide", text: "Click your profile picture in the top right.", spots: [{ fx: 0.82, fy: 0.12, label: "Profile" }], step: 2 };
      if (m === "listening") return { ...base, mode: "listening", said: "how do I turn off notifications" };
      if (m === "starting") return { ...base, mode: "starting", said: "" };
      if (m === "thinking") return { ...base, mode: "thinking", said: "what's the capital of Australia" };
      return { ...base, mode: "answer", said: "what's the capital of Australia", text: "Canberra. Sydney is bigger, but Canberra has been the capital since 1913." };
    }
    case "last_voice_turn": return null;
    case "memory_list": return mockPrefs;
    case "memory_add": { const p = { id: Date.now(), text: String(args!.text).replace(/^./, (c) => c.toUpperCase()).replace(/\.?$/, "."), at: new Date().toISOString() }; mockPrefs.push(p); return p; }
    case "memory_remove": mockPrefs = mockPrefs.filter((p) => p.id !== args!.id); return null;
    case "memory_clear": mockPrefs = []; return null;
    case "ask_run": return "Typed into a new terminal. Check it, then press Enter.";
    case "ask_open_in": return "Sent to Claude Code in the Claude app";
    case "answer_approval": dismissed.add(`approval:${args!.id}`); return null;
    case "run_queued": { const a = mockAgents.find((x) => x.id === args!.id); if (a) { a.state = "starting"; a.queued_until = null; if (args!.tool) a.tool = args!.tool as Tool; } return "Started in a terminal"; }
    case "verify_task": { const a = mockAgents.find((x) => x.id === args!.id); if (a) a.verify = "running"; return null; }
    case "verify_output": return "$ npm test\n\n  FAIL  src/cart/legacy.test.ts\n  ● removes old cart › keeps saved items\n\n    expect(received).toHaveLength(expected)\n    Expected length: 2\n    Received length: 0\n\nTests: 1 failed, 46 passed, 47 total";
    case "set_agent_status": mockSettings.agent_status = !!args!.on; return { claude: true, codex: true, codex_busy: false };
    case "github_status": return { login: mockGithub, cli: !mockGithub, device_flow: true };
    case "github_connect": mockGithub = "you"; return "you";
    case "github_device_start": return { device_code: "d", user_code: "WDJB-MJHT", verification_uri: "https://github.com/login/device", interval: 5 };
    case "github_device_poll": mockGithub = "you"; return "you";
    case "github_disconnect": mockGithub = null; return null;
    case "clone_repo": return null;
    case "dismiss_agent": dismissed.add(`agent:${args!.id}`); return null;
    case "setup_repo": { const r = mockRepos.find((x) => x.root === args!.path); if (r) r.set_up = true; return { root: args!.path, changed: [".aiuc/hook.mjs", "AGENTS.md", "CLAUDE.md", ".claude/settings.json"], committed: !!args!.commit }; }
    case "add_repo": return null;
    case "get_snapshot": return mockSnapshot();
    case "get_settings": return structuredClone(mockSettings);
    case "save_settings": mockSettings = args!.settings as Settings; return null;
    case "get_plans": return mockPlans;
    case "detect_tools": return [
      { tool: "claude_code", label: "Claude", found: true, roots: ["~/.claude/projects"] },
      { tool: "codex", label: "Codex", found: true, roots: ["~/.codex/sessions"] },
      { tool: "gemini", label: "Gemini", found: false, roots: [] },
    ];
    case "dismiss_alert": dismissed.add(args!.id as string); return null;
    case "dismiss_daily_recap": dismissed.add("daily_recap"); return null;
    case "polish_prompt": {
      const raw = String(args!.text ?? "").trim();
      const repoLabel = args!.repo ? ` in ${String(args!.repo).split(/[\\/]/).pop()}` : "";
      return `${raw}${repoLabel}. Keep edits scoped to the affected files, follow AGENTS.md conventions, and verify the relevant tests and build pass before finishing.`;
    }
    case "list_watches": return [...mockWatches];
    case "add_watch": {
      const label = String(args!.text ?? "").trim();
      if (!label) throw new Error("Couldn't understand that watch.");
      const w: Watch = { id: `w-${Date.now()}`, condition: { kind: "at", at: inMin(20) }, label, created_at: now().toISOString() };
      mockWatches = [...mockWatches, w];
      return w;
    }
    case "remove_watch": mockWatches = mockWatches.filter((w) => w.id !== args!.id); return null;
    case "ci_runs": return mockScenario === "empty" ? [] : [
      { id: 901, repo: "acme/web", branch: "agents/fix-failing-checkout-test", name: "CI", status: "completed", conclusion: "failure", html_url: "https://github.com/acme/web/actions/runs/901", updated_at: inMin(-4) },
      { id: 902, repo: "acme/web", branch: "gemini/autopilot-ci", name: "CI", status: "completed", conclusion: "success", html_url: "https://github.com/acme/web/actions/runs/902", updated_at: inMin(-18) },
    ];
    case "ci_log": return "test checkout::coupon_before_tax ... FAILED\nassertion `left == right` failed\n  left: 108.0\n right: 97.2";
    case "accept_nudge": dismissed.add("nudge"); return null;
    case "alert_action": dismissed.add(args!.id as string); return "ok";
    case "start_trial": mockTier = "trial"; return null;
    case "activate_licence": mockTier = "pro"; return "pro";
    case "layout": return { notch_height: Number(new URLSearchParams(location.search).get("notch")) || 0 };
    case "platform": return new URLSearchParams(location.search).get("platform") || (navigator.userAgent.includes("Windows") ? "windows" : "macos");
    case "connect_claude": mockSettings.claude_web = true; return null;
    case "disconnect_claude": mockSettings.claude_web = false; return null;
    case "diagnostics": return {
      app_version: "0.1.1", platform: "windows", data_dir: "C:/Users/you/AppData/Roaming/ai-usage-controller", claude_web: mockSettings.claude_web, claude_code_login: false, signing_in: false,
      tools: [
        { tool: "claude_code", label: "Claude", folders_found: ["C:/Users/you/.claude/projects"], folders_checked: ["C:/Users/you/.claude/projects", "C:/Users/you/.config/claude/projects"], requests_total: 1840, requests_today: 212, last_request_at: inMin(-2), gauge_source: mockSettings.claude_web ? "claude_web" : null, gauge_at: mockSettings.claude_web ? inMin(-1) : null, gauge_used_pct: mockSettings.claude_web ? 62 : null, live_error: null },
        { tool: "codex", label: "Codex", folders_found: ["C:/Users/you/.codex/sessions"], folders_checked: ["C:/Users/you/.codex/sessions"], requests_total: 120, requests_today: 31, last_request_at: inMin(-90), gauge_source: "codex_log", gauge_at: inMin(-90), gauge_used_pct: 24, live_error: null },
        { tool: "gemini", label: "Gemini", folders_found: [], folders_checked: ["C:/Users/you/.gemini/tmp"], requests_total: 0, requests_today: 0, last_request_at: null, gauge_source: null, gauge_at: null, gauge_used_pct: null, live_error: null },
      ],
    };
    case "inbox_list": return [...mockInbox];
    case "inbox_done": {
      const it = mockInbox.find((x) => x.id === args!.id);
      if (it) it.done = !!args!.done;
      return it ? { ...it } : null;
    }
    case "inbox_remove": {
      const before = mockInbox.length;
      mockInbox = mockInbox.filter((x) => x.id !== args!.id);
      return mockInbox.length < before;
    }
    case "inbox_clear_done": {
      const before = mockInbox.length;
      mockInbox = mockInbox.filter((x) => !x.done);
      return before - mockInbox.length;
    }
    case "inbox_remind": {
      const it = mockInbox.find((x) => x.id === args!.id);
      const at = String(args!.whenText ?? "").includes("tomorrow") ? inMin(60 * 16) : inMin(180);
      if (it) it.remind_at = at;
      const w: Watch = { id: `w-inbox-${Date.now()}`, condition: { kind: "at", at }, label: it?.text.slice(0, 60) ?? "Reminder", created_at: now().toISOString() };
      mockWatches = [...mockWatches, w];
      return w;
    }
    case "inbox_to_task": {
      const it = mockInbox.find((x) => x.id === args!.id);
      return it?.text ?? "";
    }
    default: return null;
  }
}

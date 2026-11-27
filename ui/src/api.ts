// Bridge to the Rust core. Inside Tauri it calls commands; in a plain browser
// (`npm run dev`) it serves a realistic mock so the panels can be designed fast.
import type { Alert, Dest, Detected, Diagnostics, HandoffOffer, HandoffPreviewData, Plan, Rect, RepoStatus, Settings, SetupResult, Snapshot, Tool } from "./types";

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
  launchHandoff: (id: number, prompt: string, dest: Dest) => call<string>("launch_handoff", { id, prompt, dest }),
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

export async function onOpenView(cb: (v: string) => void): Promise<() => void> {
  if (!inTauri) {
    const preview = new URLSearchParams(location.search).get("view");
    if (preview && ["panel", "settings", "setup", "audit", "paywall", "onboarding", "handoffs"].includes(preview)) cb(preview);
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
};
const dismissed = new Set<string>();
const mockRepos: RepoStatus[] = mockScenario === "empty" ? [] : [
  { root: "/Users/you/dev/acme-web", name: "acme/web", set_up: true, current_branch: "main", github: "https://github.com/acme/web",
    latest: { branch: "claude/fix-login", updated_at: Date.now() / 1000 - 760, origin: "commit", content: "# Handoff" } },
  { root: "/Users/you/dev/acme-api", name: "acme/api", set_up: false, current_branch: "main", github: "https://github.com/acme/api", latest: null },
];

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

function mock(cmd: string, args?: Record<string, unknown>): unknown {
  switch (cmd) {
    case "test_handoff_offers": return [{ source: "claude_code", session: "s", targets: ["codex", "gemini"] }];
    case "preview_handoff":
    case "preview_repo_handoff": return {
      id: 1, place: cmd === "preview_handoff" ? "acme-api" : "acme/web · claude/fix-login",
      prompt: "# Session handoff\n\n## Original goal\nFix the checkout flow so coupons apply before tax.\n\n## Files created or edited\n- src/checkout/total.ts\n- src/checkout/total.test.ts\n\n## TODO\n- [ ] Run the checkout tests\n- [ ] Update the changelog",
      destinations: (args!.target as Tool) === "gemini" ? [{ dest: "terminal", label: "Terminal" }, { dest: "web", label: "Web" }] : [{ dest: "app", label: (args!.target as Tool) === "codex" ? "Codex app" : "Claude app" }, { dest: "terminal", label: "Terminal" }, { dest: "web", label: "Web" }],
    };
    case "launch_handoff": return args!.dest === "terminal" ? "Codex is starting in a terminal." : "Prompt copied. Paste it into Codex in the Codex app.";
    case "list_repos": return mockRepos;
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
    default: return null;
  }
}

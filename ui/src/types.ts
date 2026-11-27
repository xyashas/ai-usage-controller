// Mirrors core/src/snapshot.rs (serde snake_case).
export type Tool = "claude_code" | "codex" | "gemini";
export type Source = "live" | "estimate" | "learning";
export type Tier = "free" | "trial" | "pro" | "team";
export type Color = "green" | "amber" | "red" | "grey";

export interface WindowStatus {
  key: string;
  label: string;
  used_pct: number;
  headroom_pct: number;
  resets_at: string | null;
  source: Source;
  used_usd: number | null;
  allowance_usd: number | null;
  used_requests: number | null;
  allowance_requests: number | null;
}

export interface ToolStatus {
  tool: Tool;
  label: string;
  detected: boolean;
  enabled: boolean;
  plan_id: string | null;
  plan_name: string | null;
  headroom_pct: number | null;
  color: Color;
  source: Source;
  windows: WindowStatus[];
  minutes_left: number | null;
  active: boolean;
  today_cost_usd: number;
  today_requests: number;
  burn_usd_per_min: number;
  locked: boolean;
  reconnect: boolean;
  live_note: string | null;
  live_error: string | null;
  needs_connect: boolean;
}

export interface AlertAction { kind: "stop" | "switch" | "dismiss" | "upgrade"; label: string; target_tool: Tool | null }
export interface Alert {
  id: string;
  level: "amber" | "red";
  kind: "low_headroom" | "runaway";
  tool: Tool;
  message: string;
  action: AlertAction;
  session_id: string | null;
  project: string | null;
  burn_usd_per_min: number | null;
}

export interface Nudge { id: string; tool: Tool; count: number; from_label: string; to_label: string; to_model: string; actual_usd: number; alternative_usd: number; saving_usd: number; message: string }
export type AuditAction = "keep" | "downgrade" | "upgrade" | "cancel" | "not_enough_data";
export interface AuditResult { tool: Tool; current_plan: string; current_price: number; action: AuditAction; target_plan_id: string | null; target_plan: string | null; monthly_saving_usd: number; peak_used_pct: number | null; headline: string; reasoning: string[] }

export interface Entitlements { tier: Tier; trial_ends_at: string | null; trial_available: boolean; licence_status: string | null; max_devices: number }

export interface HandoffOffer { source: Tool; session: string; targets: Tool[] }

export interface Snapshot {
  agents: AgentRun[];
  approvals: Approval[];
  handoffs: HandoffOffer[];
  gauge_history: { at: number; used: number; reset_at: number | null }[];
  idle_hidden: boolean;
  generated_at: string;
  onboarded: boolean;
  entitlements: Entitlements;
  active_tool: Tool | null;
  tools: ToolStatus[];
  today: { cost_usd: number; sessions: number; top_session: { tool: Tool; session_id: string; project_name: string | null; cost_usd: number } | null };
  session_meter: { tool: Tool; session_id: string; project_name: string | null; cost_usd: number; started_at: string; minutes: number; burn_usd_per_min: number } | null;
  alerts: Alert[];
  nudge: Nudge | null;
  savings_month_usd: number | null;
  audit: AuditResult[];
  projected_saving_usd: number;
  locked: string[];
  remote: { today_cost_all_devices: number; month_cost_all_devices: number; devices: number } | null;
  daily_recap: DailyRecap | null;
  weekly_recap: WeeklyRecap;
  suggestions: Suggestion[];
  autopilot_moves: AutopilotMove[];
}

export interface Plan { id: string; name: string; price: number; capacity: number; metered?: boolean; daily_requests?: number | null }
export interface ToolSettings { enabled: boolean; plan_id: string | null; five_hour_allowance_usd: number | null; weekly_allowance_usd: number | null; extra_roots: string[] }
export interface Settings {
  onboarded: boolean;
  tools: Record<Tool, ToolSettings>;
  primary_tool: Tool | null;
  alerts: { amber_pct: number; red_pct: number; runaway_alerts: boolean; one_click_stop: boolean; notifications: boolean };
  launch_at_login: boolean;
  hide_when_idle: boolean;
  hotkey: string;
  sync: { enabled: boolean; server_url: string | null; device_token: string | null; device_id: string | null };
  live_gauges: boolean;
  claude_web: boolean;
  api_base: string;
  auto_send: boolean;
  agent_status: boolean;
  approvals: boolean;
  verify_done: boolean;
  ask_screenshots: boolean;
  show_daily_recap: boolean;
  polish_auto: boolean;
  suggest_chips: boolean;
  autopilot: boolean;
  ci_watch: boolean;
}
export interface Detected { tool: Tool; label: string; found: boolean; roots: string[] }
export type Mode = "collapsed" | "expanded";

export interface ToolDiagnostics {
  tool: Tool;
  label: string;
  folders_found: string[];
  folders_checked: string[];
  requests_total: number;
  requests_today: number;
  last_request_at: string | null;
  gauge_source: string | null;
  gauge_at: string | null;
  gauge_used_pct: number | null;
  live_error: string | null;
}
export interface Diagnostics {
  app_version: string;
  platform: string;
  data_dir: string;
  claude_web: boolean;
  claude_code_login: boolean;
  signing_in: boolean;
  tools: ToolDiagnostics[];
}
export interface Rect { x: number; y: number; w: number; h: number }

export type Dest = "app" | "terminal" | "web";
export interface DestOption { dest: Dest; label: string }
export interface HandoffPreviewData { id: number; prompt: string; destinations: DestOption[]; place: string }
export interface RepoLatest { branch: string; updated_at: number; origin: "commit" | "working tree"; content: string }
export interface RepoStatus { id: string; local: boolean; root: string; name: string; set_up: boolean; current_branch: string; github: string | null; latest: RepoLatest | null }
export interface SetupResult { root: string; changed: string[]; committed: boolean }

export type AgentState = "starting" | "working" | "idle" | "done" | "needs_approval" | "no_activity" | "opened" | "queued";
export type VerifyState = "running" | "passed" | "failed" | "timed_out" | "error";
export interface AgentRun {
  id: number; tool: Tool; text: string; repo_name: string; branch: string | null; cwd: string; started_at: string; state: AgentState;
  dest: Dest | null; cost_usd: number; requests: number; last_activity_at: string | null;
  queued_until: string | null; verify: VerifyState | null; verify_secs: number | null; verify_at: string | null;
}
/** A tool call waiting for your OK. The summary is shown, never stored. */
export interface Approval { id: string; tool: Tool; session: string; cwd: string; repo_name: string; tool_name: string; summary: string; at: string }
export type ApprovalAnswer = "allow" | "deny" | "ask";
export interface InstallResult { claude: boolean; codex: boolean; codex_busy: boolean }
export interface Routed { text: string; tool: Tool | null; explicit: boolean; reason: string }
export interface ToolRoom { tool: Tool; ready: boolean; used_pct: number | null; active: boolean; resets_at: string | null }
export interface DispatchPlan { tasks: Routed[]; rooms: ToolRoom[]; dests: Record<Tool, DestOption[]> }
export interface Started { text: string; tool: Tool; dest: Dest; branch: string | null; message: string | null; delivered: boolean; error: string | null }
export interface GithubStatus { login: string | null; cli: boolean; device_flow: boolean }
export interface DeviceCode { device_code: string; user_code: string; verification_uri: string; interval: number }

export interface DailyToolUsage {
  tool: Tool;
  label: string;
  requests: number;
  cost_usd: number;
  five_hour_peak_pct: number | null;
  weekly_peak_pct: number | null;
  five_hour_resets_at: string | null;
  weekly_resets_at: string | null;
}
export interface DailyRecap {
  date: string;
  tools: DailyToolUsage[];
  open_tasks_yesterday: number;
  suggestion: string;
}
export interface WeeklyToolComparison {
  tool: Tool;
  label: string;
  this_week_requests: number;
  last_week_requests: number;
  this_week_cost_usd: number;
  last_week_cost_usd: number;
  change_pct: number | null;
  five_hour_hits: number;
  weekly_hits: number;
}
export interface HourBucket { hour: number; requests: number }
export interface WeeklyRecap {
  tools: WeeklyToolComparison[];
  busiest_hours: HourBucket[];
  hourly_requests: number[];
  handoffs_count: number;
  total_limit_hits: number;
}

// ---------- Ask the notch ----------
export type AskAction = "ask" | "explain" | "fix" | "find_bug" | "rewrite" | "shorter" | "professional" | "translate" | "reply" | "summarize" | "point";
export type AskMode = "quick" | "deep";
export interface AskPick { tool: Tool; reason: string }
export interface AskContext {
  window: string | null; app: string | null; selection: string | null; clipboard: string | null; clipboard_image: boolean;
  screenshot: string | null; repo: string | null; pick: AskPick | null; installed: Tool[];
}
export interface AskTurn { question: string; answer: string }
export interface AskPoint { x: number; y: number; label: string }
export interface AskDone { id: string; tool: Tool | null; reason: string; error: string | null; points: AskPoint[]; text: string; opened_web: boolean }
export interface AskRequest { id: string; question: string; action: AskAction; mode: AskMode; tool: Tool | null; history: AskTurn[]; council: boolean }
export type WatchCondition =
  | { kind: "limit_reset"; tool: Tool }
  | { kind: "limit_below"; tool: Tool; used_pct: number }
  | { kind: "agent_done"; task_id: number }
  | { kind: "agent_needs_you"; task_id: number | null }
  | { kind: "tests_pass"; task_id: number }
  | { kind: "at"; at: string };
export interface Watch { id: string; condition: WatchCondition; label: string; created_at: string }

export type SuggestionKind =
  | { kind: "copied_error" }
  | { kind: "tests_failed"; task_id: number }
  | { kind: "limit_mid_task"; tool: Tool; to: Tool };
export interface Suggestion {
  id: string;
  kind: SuggestionKind;
  title: string;
  detail: string;
  action: "ask_fix" | "send_failure" | "handoff";
  text?: string | null;
}

export interface AutopilotMove { task_id: number; to: Tool; at: string }
export interface CiRun {
  id: number;
  repo: string;
  branch: string;
  name: string;
  status: string;
  conclusion: string | null;
  html_url: string;
  updated_at: string;
}

// ---------- the on-screen buddy ----------
export interface BuddySpot { fx: number; fy: number; label: string }
export interface BuddyState {
  mode: "listening" | "thinking" | "answer" | "guide" | "done" | "error" | "hide";
  said: string; text: string; cursor: [number, number]; spots: BuddySpot[]; step: number; id: string; speak: string;
}
export interface VoiceTurn { question: string; answer: string; tool: Tool | null; reason: string }

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
  live_error: string | null;
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

export interface Snapshot {
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
}

export interface Plan { id: string; name: string; price: number; capacity: number; metered?: boolean; daily_requests?: number | null }
export interface ToolSettings { enabled: boolean; plan_id: string | null; five_hour_allowance_usd: number | null; weekly_allowance_usd: number | null; extra_roots: string[] }
export interface Settings {
  onboarded: boolean;
  tools: Record<Tool, ToolSettings>;
  primary_tool: Tool | null;
  alerts: { amber_pct: number; red_pct: number; runaway_alerts: boolean; one_click_stop: boolean; notifications: boolean };
  hotkey: string;
  sync: { enabled: boolean; server_url: string | null; device_token: string | null; device_id: string | null };
  live_gauges: boolean;
  api_base: string;
}
export interface Detected { tool: Tool; label: string; found: boolean; roots: string[] }
export type Mode = "collapsed" | "expanded";

import type { Color } from "./types";

export const usd = (n: number | null | undefined, digits = 2) => {
  if (n == null || !Number.isFinite(n)) return "—";
  const v = Math.abs(n) < 0.5 * 10 ** -digits ? 0 : n; // never "$-0.00"
  return `$${v.toLocaleString(undefined, { minimumFractionDigits: digits, maximumFractionDigits: digits })}`;
};

/** Percent of the limit used (what Claude's /usage and claude.ai show). */
export const usedPct = (headroom: number | null | undefined) => (headroom == null ? null : Math.max(0, Math.min(100, 100 - headroom)));

/** Short countdown: "2h 10m", "45m", "3d". */
export function countdown(iso: string | null): string | null {
  if (!iso) return null;
  const mins = Math.max(0, Math.round((new Date(iso).getTime() - Date.now()) / 60000));
  if (mins < 60) return `${mins}m`;
  const h = Math.floor(mins / 60);
  if (h < 48) return `${h}h ${String(mins % 60).padStart(2, "0")}m`;
  return `${Math.round(h / 24)}d`;
}

export function ago(iso: string | null): string {
  if (!iso) return "never";
  const s = Math.max(0, Math.round((Date.now() - new Date(iso).getTime()) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86400)} days ago`;
}

export const usd0 = (n: number | null | undefined) => usd(n, 0);

export function until(iso: string | null): string {
  if (!iso) return "starts when you next use it";
  const mins = Math.max(0, Math.round((new Date(iso).getTime() - Date.now()) / 60000));
  if (mins < 60) return `resets in ${mins}m`;
  const h = Math.floor(mins / 60);
  if (h < 48) return `resets in ${h}h ${String(mins % 60).padStart(2, "0")}m`;
  return `resets in ${Math.round(h / 24)}d`;
}

export const barColor: Record<Color, string> = {
  green: "bg-ok",
  amber: "bg-warn",
  red: "bg-bad",
  grey: "bg-fg/25",
};

export const textColor: Record<Color, string> = {
  green: "text-ok",
  amber: "text-warn",
  red: "text-bad",
  grey: "text-mute",
};

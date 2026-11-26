import type { Color } from "./types";

export const usd = (n: number | null | undefined, digits = 2) =>
  n == null ? "—" : `$${n.toLocaleString(undefined, { minimumFractionDigits: digits, maximumFractionDigits: digits })}`;

export const usd0 = (n: number | null | undefined) => usd(n, 0);

export function until(iso: string | null): string {
  if (!iso) return "resets on next use";
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
  grey: "bg-mute/50",
};

export const textColor: Record<Color, string> = {
  green: "text-ok",
  amber: "text-warn",
  red: "text-bad",
  grey: "text-mute",
};

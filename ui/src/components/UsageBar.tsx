import type { Color } from "../types";
import { barColor } from "../format";

/** Fills up as the limit is used, like Claude's own usage bars. */
export function UsageBar({ used, color, size = "md" }: { used: number | null; color: Color; size?: "sm" | "md" | "lg" }) {
  const w = used == null ? 0 : Math.max(used > 0 ? 1.5 : 0, Math.min(100, used));
  const h = size === "lg" ? "h-2" : size === "md" ? "h-1.5" : "h-1";
  return (
    <div className={`w-full overflow-hidden rounded-full bg-white/[0.08] ${h}`}>
      <div className={`h-full rounded-full transition-[width] duration-500 ease-out ${barColor[color]}`} style={{ width: `${w}%` }} />
    </div>
  );
}

import type { Color } from "../types";
import { barColor } from "../format";

export function HeadroomBar({ pct, color, thin = false }: { pct: number | null; color: Color; thin?: boolean }) {
  const w = pct == null ? 0 : Math.max(2, Math.min(100, pct));
  return (
    <div className={`w-full overflow-hidden rounded-full bg-white/10 ${thin ? "h-1" : "h-1.5"}`}>
      <div className={`h-full rounded-full transition-[width] duration-150 ${barColor[color]}`} style={{ width: `${w}%` }} />
    </div>
  );
}

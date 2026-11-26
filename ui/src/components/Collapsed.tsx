import type { Snapshot } from "../types";
import { HeadroomBar } from "./HeadroomBar";
import { usd } from "../format";

// Mac: a thin bar living inside the notch. Windows: a small rounded pill at top centre.
export function Collapsed({ snap, platform, notch = 0, onOpen }: { snap: Snapshot; platform: string; notch?: number; onOpen: () => void }) {
  const t = snap.tools.find((x) => x.tool === snap.active_tool) ?? snap.tools.find((x) => x.detected);
  const alert = snap.alerts[0];
  const glow = alert ? (alert.level === "red" ? "pulse-red" : "glow-amber") : "";
  const cost = snap.session_meter?.cost_usd ?? snap.today.cost_usd;
  const shape = platform === "macos" ? "rounded-b-2xl rounded-t-none" : "rounded-full";
  return (
    <div className="flex h-full w-full items-start justify-center" onMouseEnter={onOpen}>
      <button
        onClick={onOpen}
        className={`flex h-full w-full items-center gap-2.5 bg-black px-4 ${shape} ${glow}`}
        style={notch ? { paddingTop: notch } : undefined}
        title={t ? `${t.label}: ${t.headroom_pct?.toFixed(0) ?? "?"}% left` : "AI Usage Controller"}
      >
        <div className="flex-1">
          <HeadroomBar pct={t?.headroom_pct ?? null} color={t?.color ?? "grey"} thin />
        </div>
        <span className="tabular text-[11px] font-medium text-white/85">{usd(cost)}</span>
      </button>
    </div>
  );
}

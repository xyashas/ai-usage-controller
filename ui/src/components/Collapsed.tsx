import type { Snapshot } from "../types";
import { api } from "../api";
import { UsageBar } from "./UsageBar";
import { countdown, textColor, usedPct } from "../format";

// The resting state: one glanceable line. Percent used of the tightest limit,
// a small bar, and when that limit resets.
export function Collapsed({ snap, onOpen }: { snap: Snapshot; onOpen: () => void }) {
  const t = snap.tools.find((x) => x.tool === snap.active_tool) ?? snap.tools.find((x) => x.detected && x.enabled);
  const binding = t ? [...t.windows].filter((w) => w.source !== "learning").sort((a, b) => a.headroom_pct - b.headroom_pct)[0] : undefined;
  const used = usedPct(t?.headroom_pct);
  const reset = countdown(binding?.resets_at ?? null);
  const claude = snap.tools.find((x) => x.tool === "claude_code" && x.enabled);
  const note = claude?.live_note;
  const setup = !snap.onboarded;
  const connect = t?.needs_connect && (t.today_requests === 0 || used == null);

  return (
    <button title={note ? `Claude: ${note}` : undefined} onClick={claude?.reconnect ? () => { api.connectClaude().catch(onOpen); } : onOpen} className="flex h-full w-full items-center gap-2.5 px-4 text-left" aria-label={claude?.reconnect ? "Reconnect Claude account" : "Open AI usage panel"}>
      {setup || !t ? (
        <span className="flex-1 text-center text-[11.5px] font-medium text-white/70">{setup ? "Finish setup" : "No AI tools found"}</span>
      ) : note ? (
        <><span className="text-[11.5px] text-white/75">Claude</span><span className="flex-1 text-right text-[11px] text-white/55">{note}</span></>
      ) : connect ? (
        <>
          <span className="text-[11.5px] font-medium text-white/80">{t.label}</span>
          <span className="flex-1 text-right text-[11px] text-white/45">Connect for limits</span>
        </>
      ) : (
        <>
          <span className="max-w-[52px] truncate text-[11px] font-medium text-white/55">{t.label}</span>
          <div className="flex-1">
            <UsageBar used={used} color={t.color} size="sm" />
          </div>
          <span className={`tabular text-[12px] font-semibold ${used == null ? "text-white/45" : t.color === "green" ? "text-white" : textColor[t.color]}`}>{used == null ? "—" : `${Math.round(used)}%`}</span>
          {reset && <span className="tabular text-[10.5px] text-white/40">{reset}</span>}
        </>
      )}
    </button>
  );
}

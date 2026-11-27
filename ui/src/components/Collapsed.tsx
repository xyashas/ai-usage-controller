import { useEffect, useRef, useState } from "react";
import type { Snapshot } from "../types";
import { api } from "../api";
import { UsageBar } from "./UsageBar";
import { countdown, textColor, usedPct } from "../format";

// The resting state: one glanceable line. Percent used of the tightest limit,
// a small bar, and when that limit resets.
function useCount(value: number | null) {
  const target = value == null ? null : Math.round(value);
  const [shown, setShown] = useState(target);
  const current = useRef(target);
  useEffect(() => {
    const from = current.current;
    const motion = matchMedia("(prefers-reduced-motion: reduce)");
    if (target == null || from == null || motion.matches) {
      current.current = target;
      setShown(target);
      return;
    }
    let frame: number;
    const start = performance.now();
    function tick(time: number) {
      const progress = motion.matches ? 1 : Math.min(1, (time - start) / 320);
      current.current = Math.round(from! + (target! - from!) * (1 - (1 - progress) ** 3));
      setShown(current.current);
      if (progress < 1) frame = requestAnimationFrame(tick);
    }
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [target]);
  return shown;
}

export function Collapsed({ snap, onOpen, notch }: { snap: Snapshot; onOpen: () => void; notch: boolean }) {
  const t = snap.tools.find((x) => x.tool === snap.active_tool) ?? snap.tools.find((x) => x.detected && x.enabled);
  const binding = t ? [...t.windows].filter((w) => w.source !== "learning").sort((a, b) => a.headroom_pct - b.headroom_pct)[0] : undefined;
  const used = usedPct(t?.headroom_pct);
  const shown = useCount(used);
  const reset = countdown(binding?.resets_at ?? null);
  const claude = snap.tools.find((x) => x.tool === "claude_code" && x.enabled);
  const note = claude?.live_note;
  const setup = !snap.onboarded;
  const connect = t?.needs_connect && (t.today_requests === 0 || used == null);

  return (
    <button title={note ? `Claude: ${note}` : undefined} onClick={claude?.reconnect ? () => { api.connectClaude().catch(onOpen); } : onOpen} className="flex h-full w-full items-center gap-2 px-4 text-left" aria-label={claude?.reconnect ? "Reconnect Claude account" : "Open AI usage panel"}>
      {setup || !t ? (
        <span className="flex-1 text-center text-[11.5px] font-medium text-muted">{setup ? "Finish setup" : "No AI tools found"}</span>
      ) : note ? (
        <><span className="text-[11.5px] text-fg">Claude</span><span className="flex-1 text-right text-[11px] text-muted">{note}</span></>
      ) : connect ? (
        <>
          <span className="text-[11.5px] font-medium text-fg">{t.label}</span>
          <span className="flex-1 text-right text-[11px] text-muted">Connect for limits</span>
        </>
      ) : (
        <>
          <span className="max-w-[52px] truncate text-[11px] font-medium text-muted">{t.label}</span>
          {notch ? <svg aria-hidden="true" viewBox="0 0 16 16" className={`ml-auto h-4 w-4 ${textColor[t.color]}`}>
            <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" opacity=".25" strokeWidth="2" />
            <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeWidth="2" pathLength="100" strokeDasharray={`${shown ?? 0} 100`} transform="rotate(-90 8 8)" />
          </svg> : <div className="flex-1">
            <UsageBar used={used} color={t.color} size="sm" />
          </div>}
          <span className={`tabular text-[12px] font-semibold ${used == null ? "text-muted" : t.color === "green" ? "text-fg" : textColor[t.color]}`}>{used == null ? "—" : `${shown ?? Math.round(used)}%`}</span>
          {reset && <span className="tabular text-[10.5px] text-muted">{reset}</span>}
        </>
      )}
    </button>
  );
}

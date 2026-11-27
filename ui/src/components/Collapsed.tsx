import { useEffect, useRef, useState } from "react";
import type { Snapshot } from "../types";
import { api } from "../api";
import { UsageBar } from "./UsageBar";
import { countdown, textColor, usedPct } from "../format";
import { primaryWindow } from "./Expanded";

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
  // The 5-hour limit first, like the panel. If another limit is tighter, its
  // colour still shows through the dot.
  const binding = t ? primaryWindow(t) : undefined;
  const used = usedPct(binding ? binding.headroom_pct : t?.headroom_pct);
  const own = binding ? (binding.headroom_pct <= 5 ? "red" : binding.headroom_pct <= 20 ? "amber" : "green") : t?.color ?? "grey";
  const otherWorse = !!t && own !== t.color && t.color !== "green" && t.color !== "grey";
  const shown = useCount(used);
  const reset = countdown(binding?.resets_at ?? null);
  const claude = snap.tools.find((x) => x.tool === "claude_code" && x.enabled);
  const note = claude?.live_note;
  const setup = !snap.onboarded;
  const connect = t?.needs_connect && (t.today_requests === 0 || used == null);
  // Agents come first when one needs you; otherwise a small count of the busy ones.
  const asking = snap.approvals[0];
  const waiting = snap.agents.filter((a) => a.state === "needs_approval").length;
  const busy = snap.agents.filter((a) => a.state === "working" || a.state === "starting").length;

  return (
    <button title={note ? `Claude: ${note}` : t ? t.label : undefined} onClick={claude?.reconnect && !asking ? () => { api.connectClaude().catch(onOpen); } : onOpen} className="flex h-full w-full items-center gap-2 px-4 text-left" aria-label={asking ? "An agent needs your OK. Open the panel" : waiting ? `${waiting} agent${waiting > 1 ? "s" : ""} waiting for you. Open the panel` : claude?.reconnect ? "Reconnect Claude account" : "Open AI usage panel"}>
      {asking ? (
        <span className="pill-swap flex min-w-0 flex-1 items-center justify-center gap-2">
          <span className="live-dot h-1.5 w-1.5 shrink-0 rounded-full bg-warn" />
          <span className="truncate text-[11.5px] font-medium text-fg">{`${asking.tool === "codex" ? "Codex" : asking.tool === "gemini" ? "Gemini" : "Claude"} needs your OK`}</span>
        </span>
      ) : setup || !t ? (
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
          {waiting > 0 && <span title={`${waiting} agent${waiting > 1 ? "s" : ""} waiting for you`} className="flex items-center gap-1 text-[10.5px] font-semibold text-warn"><span className="live-dot h-1.5 w-1.5 rounded-full bg-warn" />{waiting}</span>}
          {!waiting && busy > 0 && <span title={`${busy} agent${busy > 1 ? "s" : ""} working`} className="flex items-center gap-1 text-[10.5px] font-semibold text-ok"><span className="live-dot h-1.5 w-1.5 rounded-full bg-ok" />{busy}</span>}
          {!(waiting || busy) && <span className="max-w-[52px] truncate text-[11px] font-medium text-muted">{t.label}</span>}
          {notch ? <svg aria-hidden="true" viewBox="0 0 16 16" className={`ml-auto h-4 w-4 ${textColor[own]}`}>
            <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" opacity=".25" strokeWidth="2" />
            <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeWidth="2" pathLength="100" strokeDasharray={`${shown ?? 0} 100`} transform="rotate(-90 8 8)" />
          </svg> : <div className="min-w-[28px] flex-1">
            <UsageBar used={used} color={own} size="sm" />
          </div>}
          <span className={`tabular shrink-0 text-[12px] font-semibold ${used == null ? "text-muted" : own === "green" ? "text-fg" : textColor[own]}`}>{used == null ? "—" : `${shown ?? Math.round(used)}%`}</span>
          {otherWorse && <span title={`Another ${t.label} limit is ${t.color === "red" ? "used up" : "running low"}`} className={`h-1.5 w-1.5 rounded-full ${t.color === "red" ? "bg-bad" : "bg-warn"}`} />}
          {reset && <span className="tabular shrink-0 whitespace-nowrap text-[10.5px] text-muted">{reset}</span>}
        </>
      )}
    </button>
  );
}

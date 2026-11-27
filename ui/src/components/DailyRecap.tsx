import { useState } from "react";
import type { DailyRecap as DailyRecapData } from "../types";
import { ToolMark } from "./ToolMark";
import { tint } from "../theme";
import { usd } from "../format";

function clockTime(iso: string): string {
  const d = new Date(iso);
  return d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

/** Morning brief card shown at the top of Usage on first open each day. */
export function DailyRecap({ recap, onDismiss }: { recap: DailyRecapData; onDismiss: () => void }) {
  const resets = recap.tools
    .flatMap((t) => {
      const out: { key: string; label: string; at: string }[] = [];
      if (t.five_hour_resets_at) {
        const win = t.tool === "gemini" ? "daily" : "5h";
        out.push({ key: `${t.tool}:5h`, label: `${t.label} ${win}`, at: clockTime(t.five_hour_resets_at) });
      }
      if (t.weekly_resets_at) {
        out.push({ key: `${t.tool}:wk`, label: `${t.label} weekly`, at: clockTime(t.weekly_resets_at) });
      }
      return out;
    });

  const [open, setOpen] = useState(false);
  const cost = recap.tools.reduce((n, t) => n + t.cost_usd, 0);
  const reqs = recap.tools.reduce((n, t) => n + t.requests, 0);

  return (
    <section className="tile fade-in !py-2.5" aria-label="Morning brief">
      <div className="flex items-center gap-2.5">
        <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-[9px] bg-[#fbbf24]/15 text-[#fbbf24]" aria-hidden>
          <svg viewBox="0 0 20 20" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round"><circle cx="10" cy="11" r="3.2" /><path d="M10 3.5v1.6M4.2 6.2l1.1 1.1M15.8 6.2l-1.1 1.1M3 15.5h14" /></svg>
        </span>
        <button type="button" onClick={() => setOpen((o) => !o)} aria-expanded={open} className="min-w-0 flex-1 text-left">
          <p className="truncate text-[12.5px] font-medium leading-snug text-fg">{recap.suggestion}</p>
          <p className="tabular truncate text-[11px] text-muted">
            Yesterday {usd(cost)} · {reqs} requests
            {recap.open_tasks_yesterday > 0 && <span className="text-warn"> · {recap.open_tasks_yesterday} open {recap.open_tasks_yesterday === 1 ? "task" : "tasks"}</span>}
          </p>
        </button>
        <button type="button" onClick={() => setOpen((o) => !o)} aria-label={open ? "Hide details" : "Show details"} className="icon-btn shrink-0">
          <svg viewBox="0 0 16 16" className={`h-3.5 w-3.5 transition-transform duration-200 ${open ? "rotate-180" : ""}`} fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"><path d="M4 6l4 4 4-4" /></svg>
        </button>
        <button type="button" onClick={onDismiss} aria-label="Dismiss morning brief" title="Dismiss for today" className="icon-btn shrink-0 text-[15px] leading-none">×</button>
      </div>

      <div className={`grid transition-[grid-template-rows,opacity] duration-300 ease-out ${open ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0"}`}>
        <div className="min-h-0 overflow-hidden">
          <div className="space-y-1.5 pt-2.5">
            {recap.tools.map((t) => {
              const c = tint[t.tool];
              const peaks = [
                t.five_hour_peak_pct != null ? `${t.tool === "gemini" ? "daily" : "5h"} ${Math.round(t.five_hour_peak_pct)}%` : null,
                t.weekly_peak_pct != null ? `wk ${Math.round(t.weekly_peak_pct)}%` : null,
              ].filter(Boolean).join(" · ");
              return (
                <div key={t.tool} className="flex items-center justify-between gap-2 text-[12px]">
                  <span className="flex min-w-0 items-center gap-1.5 font-medium">
                    <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-md" style={{ background: c.bg, color: c.ink }}>
                      <ToolMark tool={t.tool} />
                    </span>
                    <span className="truncate">{t.label}</span>
                  </span>
                  <span className="tabular flex items-center gap-2 text-[11.5px] text-muted">
                    <span>{t.requests} req</span>
                    <span className="font-medium text-fg">{usd(t.cost_usd)}</span>
                    {peaks && <span className="opacity-75">peak {peaks}</span>}
                  </span>
                </div>
              );
            })}
            {resets.length > 0 && (
              <p className="tabular border-t border-white/[0.06] pt-2 text-[11px] text-muted">
                Resets today: {resets.map((r, i) => (
                  <span key={r.key}>{i > 0 ? " · " : ""}<span className="text-fg/85">{r.label}</span> {r.at}</span>
                ))}
              </p>
            )}
          </div>
        </div>
      </div>
    </section>
  );
}

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

  return (
    <section className="tile fade-in space-y-2.5" aria-label="Daily recap">
      <div className="flex items-center justify-between gap-2">
        <div>
          <span className="eyebrow">Morning brief</span>
          <p className="mt-0.5 text-[13px] font-medium leading-snug text-fg">{recap.suggestion}</p>
        </div>
        <button
          type="button"
          onClick={onDismiss}
          aria-label="Dismiss daily recap"
          title="Dismiss for today"
          className="icon-btn shrink-0 text-[15px] leading-none"
        >
          ×
        </button>
      </div>

      {recap.tools.length > 0 && (
        <div className="space-y-1.5">
          <div className="eyebrow !text-[9.5px]">Yesterday</div>
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
                  {peaks && <span className="opacity-75">{peaks}</span>}
                </span>
              </div>
            );
          })}
        </div>
      )}

      {(resets.length > 0 || recap.open_tasks_yesterday > 0) && (
        <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 border-t border-white/[0.06] pt-2 text-[11px] text-muted">
          {resets.length > 0 && (
            <span className="tabular">
              Resets today: {resets.map((r, i) => (
                <span key={r.key}>
                  {i > 0 ? " · " : ""}
                  <span className="text-fg/85">{r.label}</span> {r.at}
                </span>
              ))}
            </span>
          )}
          {recap.open_tasks_yesterday > 0 && (
            <span className="tabular text-warn">
              {recap.open_tasks_yesterday} open {recap.open_tasks_yesterday === 1 ? "task" : "tasks"} from yesterday
            </span>
          )}
        </div>
      )}
    </section>
  );
}

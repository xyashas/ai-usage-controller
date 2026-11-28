import { memo } from "react";
import type { Snapshot, WeeklyRecap } from "../types";
import { ToolMark } from "./ToolMark";
import { tint } from "../theme";
import { usd } from "../format";

const DAY = 86_400_000;

/** The last 24 hours of the live gauge, as a soft area under a line. */
export const UsageHistory = memo(function UsageHistory({ points, end, color = "currentColor" }: { points: Snapshot["gauge_history"]; end: number; color?: string }) {
  const recent = points.filter((p) => p.at > end - DAY);
  if (recent.length < 2) return null;
  const x = (at: number) => Math.max(0, Math.min(320, (320 * (at - end + DAY)) / DAY));
  const y = (used: number) => 30 - (28 * Math.min(100, Math.max(0, used))) / 100;
  const segments: { at: number; used: number }[][] = [];
  recent.forEach((p, i) => {
    const prev = recent[i - 1];
    if (!prev || p.at - prev.at > 1_800_000 || (prev.reset_at != null && prev.reset_at > prev.at && prev.reset_at <= p.at)) segments.push([]);
    segments[segments.length - 1].push(p);
  });
  const last = recent[recent.length - 1];
  return (
    <figure className="space-y-1">
      <svg role="img" aria-label={`Usage over the past 24 hours, latest ${Math.round(last.used)}% used`} viewBox="0 0 320 32" preserveAspectRatio="none" className="block h-8 w-full">
        {segments.filter((s) => s.length > 1).map((s, i) => {
          const line = s.map((p) => `${x(p.at)},${y(p.used)}`).join(" ");
          return (
            <g key={i}>
              <polygon points={`${x(s[0].at)},31 ${line} ${x(s[s.length - 1].at)},31`} fill={color} opacity="0.16" />
              <polyline points={line} fill="none" stroke={color} strokeWidth="1.6" strokeLinejoin="round" strokeLinecap="round" vectorEffect="non-scaling-stroke" />
            </g>
          );
        })}
      </svg>
      <figcaption className="flex justify-between text-[10px] opacity-45"><span>24h ago</span><span>now</span></figcaption>
    </figure>
  );
});

function formatHour(h: number): string {
  const hour12 = h % 12 === 0 ? 12 : h % 12;
  return `${hour12}${h < 12 ? "a" : "p"}`;
}

function deltaLabel(pct: number | null, hasThisWeek: boolean): { text: string; cls: string } {
  if (pct == null) return hasThisWeek ? { text: "New", cls: "text-accent" } : { text: "—", cls: "text-muted" };
  const rounded = Math.round(pct);
  if (rounded > 0) return { text: `+${rounded}%`, cls: "text-warn" };
  if (rounded < 0) return { text: `${rounded}%`, cls: "text-ok" };
  return { text: "0%", cls: "text-muted" };
}

/** Glanceable weekly recap: this week vs last week, busiest hours, limit hits & handoffs. */
export const WeeklyRecapSection = memo(function WeeklyRecapSection({ recap }: { recap: WeeklyRecap }) {
  const hasData = recap.tools.length > 0 || recap.handoffs_count > 0 || recap.total_limit_hits > 0;
  if (!hasData) return null;

  const maxHour = Math.max(1, ...recap.hourly_requests);
  const peakSet = new Set(recap.busiest_hours.map((b) => b.hour));

  return (
    <section className="tile fade-in space-y-2.5" aria-label="Weekly recap">
      <div className="flex items-baseline justify-between gap-2">
        <span className="eyebrow">This week vs last</span>
        <span className="tabular text-[11px] text-muted">
          {recap.total_limit_hits} limit {recap.total_limit_hits === 1 ? "hit" : "hits"} · {recap.handoffs_count} {recap.handoffs_count === 1 ? "handoff" : "handoffs"}
        </span>
      </div>

      {recap.tools.length > 0 && (
        <div className="space-y-1.5">
          {recap.tools.map((t) => {
            const c = tint[t.tool];
            const delta = deltaLabel(t.change_pct, t.this_week_requests > 0);
            const hits = [
              t.five_hour_hits > 0 ? `${t.tool === "gemini" ? "daily" : "5h"} ×${t.five_hour_hits}` : null,
              t.weekly_hits > 0 ? `wk ×${t.weekly_hits}` : null,
            ].filter(Boolean).join(" · ");
            return (
              <div key={t.tool} className="flex items-center justify-between gap-2 text-[12px]">
                <span className="flex min-w-0 items-center gap-1.5 font-medium">
                  <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-md" style={{ background: c.bg, color: c.ink }}>
                    <ToolMark tool={t.tool} />
                  </span>
                  <span className="truncate">{t.label}</span>
                  {hits && <span className="tabular rounded-full bg-white/[0.07] px-1.5 py-0.5 text-[10px] text-muted">{hits}</span>}
                </span>
                <span className="tabular flex items-center gap-2 text-[11.5px]">
                  <span className="text-muted">{t.this_week_requests} req</span>
                  <span className="font-medium text-fg">{usd(t.this_week_cost_usd)}</span>
                  <span className={`w-10 text-right font-semibold ${delta.cls}`}>{delta.text}</span>
                </span>
              </div>
            );
          })}
        </div>
      )}

      {recap.busiest_hours.length > 0 && (
        <div className="space-y-1 border-t border-white/[0.06] pt-2">
          <div className="flex items-center justify-between text-[11px]">
            <span className="text-muted">Busiest hours</span>
            <span className="tabular font-medium text-fg/90">
              {recap.busiest_hours.map((b) => formatHour(b.hour)).join(" · ")}
            </span>
          </div>
          <div className="flex h-5 items-end gap-[2px]" aria-hidden="true">
            {recap.hourly_requests.slice(0, 24).map((count, h) => {
              const pct = count > 0 ? Math.max(16, Math.round((count / maxHour) * 100)) : 8;
              const isPeak = peakSet.has(h);
              return (
                <span
                  key={h}
                  title={`${formatHour(h)}: ${count} req`}
                  className="flex-1 rounded-sm transition-all"
                  style={{
                    height: `${pct}%`,
                    background: isPeak ? "#f5f5f7" : count > 0 ? "rgba(255,255,255,0.28)" : "rgba(255,255,255,0.06)",
                  }}
                />
              );
            })}
          </div>
        </div>
      )}
    </section>
  );
});

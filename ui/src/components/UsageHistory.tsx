import type { Snapshot } from "../types";

const DAY = 86_400_000;

/** The last 24 hours of the live gauge, as a soft area under a line. */
export function UsageHistory({ points, end, color = "currentColor" }: { points: Snapshot["gauge_history"]; end: number; color?: string }) {
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
}

import type { Snapshot } from "../types";

const DAY = 86_400_000;

export function UsageHistory({ points, end }: { points: Snapshot["gauge_history"]; end: number }) {
  if (!points.length) return null;
  const x = (at: number) => Math.max(1, Math.min(319, 1 + 318 * (at - end + DAY) / DAY));
  const y = (used: number) => 32 - 30 * Math.min(100, Math.max(0, used)) / 100;
  const resets = [...new Set(points.map(p => p.reset_at).filter((t): t is number => t != null && t > end - DAY && t <= end))];
  const segments: string[][] = [];
  points.forEach((p, i) => {
    const prev = points[i - 1];
    if (!prev || p.at - prev.at > 1_800_000 || (prev.reset_at != null && prev.reset_at > prev.at && prev.reset_at <= p.at)) segments.push([]);
    segments[segments.length - 1].push(`${x(p.at)},${y(p.used)}`);
  });
  const last = points[points.length - 1];
  return <figure className="space-y-2">
    <figcaption className="flex justify-between text-[11px] text-muted"><span>Last 24 hours</span>{resets.length > 0 && <span>{resets.length} {resets.length === 1 ? "reset" : "resets"}</span>}</figcaption>
    <svg role="img" aria-label={`Usage over the past 24 hours, latest sample ${Math.round(last.used)}% used. ${resets.length} resets.`} viewBox="0 0 320 34" preserveAspectRatio="none" className="block h-8 w-full text-muted">
      <path d="M0 32H320" stroke="currentColor" opacity=".25" />
      {resets.map(t => <line key={t} x1={x(t)} x2={x(t)} y1="1" y2="32" stroke="currentColor" strokeDasharray="2 3"><title>{new Date(t).toLocaleTimeString()} reset</title></line>)}
      {segments.map((segment, i) => <polyline key={i} points={(segment.length === 1 ? [segment[0], segment[0]] : segment).join(" ")} fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" strokeLinecap="round" />)}
      <circle cx={x(last.at)} cy={y(last.used)} r="2" fill="currentColor" />
    </svg>
  </figure>;
}

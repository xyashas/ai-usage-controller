import { useEffect, useState } from "react";
import type { CiRun } from "../types";
import { api } from "../api";
import { ago } from "../format";

function dotStyle(run: CiRun): { color: string; pulse: boolean } {
  if (run.conclusion === "success") return { color: "#4ade80", pulse: false };
  if (run.conclusion === "failure") return { color: "#f87171", pulse: false };
  if (run.status === "in_progress" || run.status === "queued") return { color: "#fbbf24", pulse: true };
  return { color: "rgba(255,255,255,0.35)", pulse: false };
}

/** GitHub Actions CI runs on agent branches, shown above Watches in AgentsView. */
export function CiRuns() {
  const [runs, setRuns] = useState<CiRun[]>([]);
  const [sending, setSending] = useState<number | null>(null);

  useEffect(() => {
    const load = () => api.ciRuns().then(setRuns, () => {});
    load();
    const id = setInterval(load, 30_000);
    return () => clearInterval(id);
  }, []);

  if (runs.length === 0) return null;

  async function sendToAgent(run: CiRun) {
    setSending(run.id);
    try {
      const log = await api.ciLog(run.id).catch(() => "");
      const text = log.trim()
        ? `CI (${run.name}) failed on ${run.repo} (${run.branch}):\n\n${log.trim()}`
        : `CI (${run.name}) failed on ${run.repo} (${run.branch}).`;
      window.dispatchEvent(new CustomEvent("aiuc-ask", { detail: { action: "send_failure", text } }));
    } finally {
      setSending(null);
    }
  }

  return (
    <section className="space-y-1.5 pt-1" aria-label="CI">
      <div className="flex items-center justify-between px-1">
        <p className="eyebrow">CI</p>
        <span className="tabular text-[11px] text-mute">{runs.length}</span>
      </div>
      <div className="tile !p-1">
        {runs.slice(0, 6).map((r) => {
          const dot = dotStyle(r);
          const failed = r.conclusion === "failure";
          return (
            <div key={r.id} className="flex items-center gap-2.5 rounded-2xl px-3 py-2 hover:bg-white/[0.04]">
              <span
                className={`inline-block h-1.5 w-1.5 shrink-0 rounded-full ${dot.pulse ? "live-dot" : ""}`}
                style={{ background: dot.color }}
              />
              <div className="min-w-0 flex-1">
                <p className="truncate text-[12px] font-medium text-fg/95">{r.branch}</p>
                <p className="truncate text-[10.5px] text-muted">
                  {r.name} · {ago(r.updated_at)}
                </p>
              </div>
              <div className="flex shrink-0 items-center gap-1">
                {failed && (
                  <button
                    type="button"
                    disabled={sending === r.id}
                    onClick={() => sendToAgent(r)}
                    className="btn !px-2.5 !py-1 text-[11px]"
                  >
                    {sending === r.id ? "Loading…" : "Send to agent"}
                  </button>
                )}
                {r.html_url && (
                  <button
                    type="button"
                    onClick={() => api.openUrl(r.html_url)}
                    className="btn btn-ghost !px-2 !py-1 text-[11px]"
                  >
                    Open
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}

import type { Snapshot } from "../types";
import { api } from "../api";
import { usd } from "../format";

const LABEL: Record<string, string> = {
  claude_code: "Claude Code",
  codex: "Codex",
  gemini: "Gemini",
};

// Opens on the 1st of each month: what you used, what you paid, one recommendation.
export function AuditCard({ snap, onBack, refresh }: { snap: Snapshot; onBack: () => void; refresh: () => void }) {
  const paid = snap.audit.reduce((s, a) => s + a.current_price, 0);
  const best = [...snap.audit].sort((a, b) => b.monthly_saving_usd - a.monthly_saving_usd)[0];
  return (
    <div className="flex flex-col gap-3.5 p-5">
      <button onClick={onBack} className="self-start text-[12px] text-white/50 hover:text-white">← Back</button>
      <div className="text-[11px] uppercase tracking-wide text-white/50">Monthly plan audit</div>
      {best ? (
        <>
          <h2 className="text-[18px] font-semibold leading-snug">{best.headline}</h2>
          <div className="space-y-1.5">
            {snap.audit.map((a) => (
              <div key={a.tool} className="flex justify-between text-[12.5px]">
                <span className="text-white/80">{LABEL[a.tool]} · {a.current_plan}</span>
                <span className="tabular text-white/50">{a.peak_used_pct != null ? `${Math.round(a.peak_used_pct)}% peak · ` : ""}{usd(a.current_price, 0)}/mo</span>
              </div>
            ))}
            <div className="flex justify-between border-t border-line pt-1.5 text-[12.5px]">
              <span>You pay</span>
              <span className="tabular font-semibold">{usd(paid, 0)}/mo</span>
            </div>
          </div>
          <ul className="space-y-1 text-[11.5px] text-white/50">
            {best.reasoning.map((r) => <li key={r}>· {r}</li>)}
          </ul>
          {(best.action === "downgrade" || best.action === "cancel" || best.action === "upgrade") && (
            <button
              onClick={async () => { await api.acceptAudit(best.tool); refresh(); onBack(); }}
              className="rounded-full bg-accent py-2.5 text-[14px] font-semibold text-black"
            >
              {best.action === "cancel" ? `I cancelled ${LABEL[best.tool]}` : `I switched to ${best.target_plan}`}
            </button>
          )}
          <p className="text-[11px] text-white/50">You make the change with the vendor; we record the estimated saving.</p>
        </>
      ) : (
        <p className="text-[13px] text-white/50">Pick your plans in Settings to get a monthly audit.</p>
      )}
    </div>
  );
}

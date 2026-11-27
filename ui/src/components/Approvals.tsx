import { useState } from "react";
import type { Approval, ApprovalAnswer } from "../types";
import { api } from "../api";
import { tint, toolName } from "../theme";
import { ToolMark } from "./ToolMark";

/** Tool calls waiting for your OK, answered right from the notch. */
export function Approvals({ items, refresh }: { items: Approval[]; refresh: () => void }) {
  const [busy, setBusy] = useState<string | null>(null);
  const [err, setErr] = useState("");
  if (!items.length) return null;

  async function answer(a: Approval, ans: ApprovalAnswer) {
    setBusy(a.id);
    setErr("");
    try { await api.answerApproval(a.id, ans); }
    catch (e) { setErr(String(e)); }
    finally { setBusy(null); refresh(); }
  }

  return (
    <div className="space-y-2">
      {items.map((a) => {
        const c = tint[a.tool];
        return (
          <section key={a.id} className="card fade-in space-y-3" style={{ background: c.bg, color: c.ink }} role="alertdialog" aria-label={`${toolName[a.tool]} needs your OK`}>
            <div className="flex items-center gap-1.5 text-[12.5px] font-semibold">
              <ToolMark tool={a.tool} />
              <span className="flex-1">{toolName[a.tool]} wants to use {a.tool_name}</span>
              <span className="text-[11px] font-medium opacity-60">{a.repo_name}</span>
            </div>
            {a.summary && (
              <p className="select-text truncate rounded-xl bg-black/[0.07] px-3 py-2 font-mono text-[11.5px]" title={a.summary}>{a.summary}</p>
            )}
            <div className="flex gap-1.5">
              <button disabled={busy === a.id} className="btn btn-ink grow !py-2" onClick={() => answer(a, "allow")}>Allow</button>
              <button disabled={busy === a.id} className="btn grow !py-2" style={{ background: "rgba(0,0,0,0.08)", color: c.ink }} onClick={() => answer(a, "deny")}>Deny</button>
              <button disabled={busy === a.id} className="rounded-full px-3 text-[11.5px] font-medium opacity-70 hover:opacity-100" onClick={() => answer(a, "ask")} title="Let it ask in the terminal instead">In terminal</button>
            </div>
          </section>
        );
      })}
      {err && <p className="px-1 text-[11.5px] text-bad">{err}</p>}
    </div>
  );
}

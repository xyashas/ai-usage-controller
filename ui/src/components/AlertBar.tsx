import type { Alert } from "../types";
import { api } from "../api";
import { useState } from "react";

// Every alert has exactly one button that does something.
export function AlertBar({ alert, onDone }: { alert: Alert; onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const red = alert.level === "red";
  async function act() {
    setBusy(true);
    try {
      if (alert.action.kind === "dismiss") await api.dismiss(alert.id);
      else {
        const r = await api.alertAction(alert);
        if (r && r !== "ok") setMsg(r);
      }
      onDone();
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className={`flex items-center gap-3 rounded-2xl px-3.5 py-3 ${red ? "bg-bad/15 ring-1 ring-bad/40" : "bg-warn/10 ring-1 ring-warn/30"}`}>
      <span className={`h-2 w-2 shrink-0 rounded-full ${red ? "bg-bad pulse-red" : "bg-warn"}`} />
      <p className="flex-1 text-[12.5px] leading-snug text-white/90">{msg ?? alert.message}</p>
      <button
        disabled={busy}
        onClick={act}
        className={`shrink-0 rounded-full px-3 py-1 text-[12px] font-semibold text-black ${red ? "bg-bad" : "bg-warn"} disabled:opacity-50`}
      >
        {alert.action.label}
      </button>
    </div>
  );
}

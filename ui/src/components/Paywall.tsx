import { BackIcon } from "./icons";
import { useState } from "react";
import type { Snapshot } from "../types";
import { api } from "../api";
import { usd } from "../format";

const CHECKOUT_ANNUAL = import.meta.env.VITE_CHECKOUT_ANNUAL_URL ?? "https://aiusage.lemonsqueezy.com/buy/pro-annual";
const CHECKOUT_MONTHLY = import.meta.env.VITE_CHECKOUT_MONTHLY_URL ?? "https://aiusage.lemonsqueezy.com/buy/pro-monthly";

// The headline is always the user's own projected saving.
export function Paywall({ snap, onBack, refresh }: { snap: Snapshot; onBack: () => void; refresh: () => void }) {
  const [key, setKey] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const saving = snap.projected_saving_usd;
  const headline = saving >= 1 ? `You could save about ${usd(saving, 0)} a month.` : "Never get cut off mid-task.";

  async function activate() {
    setBusy(true);
    setErr(null);
    try {
      await api.activate(key.trim());
      refresh();
      onBack();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex flex-col gap-3 p-3">
      <button onClick={onBack} className="btn btn-ghost self-start !pl-1.5"><BackIcon /> Back</button>
      <h2 className="text-[19px] font-semibold leading-tight">{headline}</h2>
      <ul className="space-y-2.5 text-[13px] text-fg">
        {["Time left before cutoff, at your pace", "All tools side by side, with \u201cswitch to\u201d advice", "Runaway-loop alerts and a live session meter", "Model nudges and a monthly plan audit"].map((f) => (
          <li key={f} className="flex items-start gap-2.5">
            <span className="mt-[1px] flex h-4 w-4 shrink-0 items-center justify-center rounded-full bg-[#34c759]/15 text-[#34c759]">
              <svg viewBox="0 0 12 12" className="h-2.5 w-2.5" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><path d="M2.5 6.2l2.2 2.2 4.8-4.9" /></svg>
            </span>
            {f}
          </li>
        ))}
      </ul>

      {snap.entitlements.trial_available && (
        <button onClick={async () => { await api.startTrial(); refresh(); onBack(); }} className="btn btn-primary w-full !py-2.5 text-[13px]">
          Start 7-day free trial · no card
        </button>
      )}
      <div className="grid grid-cols-2 gap-2">
        <button onClick={() => api.openUrl(CHECKOUT_ANNUAL)} className="rounded-2xl bg-white/[0.12] px-3.5 py-3 text-left ring-1 ring-white/30">
          <div className="text-[13px] font-semibold">$79 / year</div>
          <div className="text-[11px] text-ok">2 months free</div>
        </button>
        <button onClick={() => api.openUrl(CHECKOUT_MONTHLY)} className="rounded-2xl bg-[#141416] px-3.5 py-3 text-left ring-1 ring-white/[0.06] hover:bg-[#1b1b1e]">
          <div className="text-[13px] font-semibold">$9 / month</div>
          <div className="text-[11px] text-muted">cancel anytime</div>
        </button>
      </div>

      <div className="space-y-2">
        <div className="flex gap-2">
          <input onFocus={() => api.focus()} value={key} onChange={(e) => setKey(e.target.value)} placeholder="Have a licence key?" className="field min-w-0 flex-1" />
          <button disabled={!key || busy} onClick={activate} className="btn">Activate</button>
        </div>
        {err && <p className="text-[11.5px] text-bad">{err}</p>}
        <p className="text-[11px] text-muted">14-day refunds, no questions. Savings are estimates.</p>
      </div>
    </div>
  );
}

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
    <div className="fade-in flex h-full flex-col gap-3 rounded-[22px] bg-panel/95 p-5 ring-1 ring-line">
      <button onClick={onBack} className="self-start text-[12px] text-mute hover:text-white">← Back</button>
      <h2 className="text-[19px] font-semibold leading-tight">{headline}</h2>
      <ul className="space-y-1.5 text-[13px] text-white/85">
        <li>⏱ Time left before cutoff, at your pace</li>
        <li>🔀 All tools side by side, with "switch to" advice</li>
        <li>🛑 Runaway-loop alerts and a live session meter</li>
        <li>💡 Model nudges and a monthly plan audit</li>
      </ul>

      {snap.entitlements.trial_available && (
        <button onClick={async () => { await api.startTrial(); refresh(); onBack(); }} className="rounded-xl bg-accent py-2.5 text-[14px] font-semibold text-black">
          Start 7-day free trial · no card
        </button>
      )}
      <div className="grid grid-cols-2 gap-2">
        <button onClick={() => api.openUrl(CHECKOUT_ANNUAL)} className="rounded-xl bg-white/10 px-3 py-2.5 text-left ring-1 ring-accent/50 hover:bg-white/15">
          <div className="text-[13px] font-semibold">$79 / year</div>
          <div className="text-[11px] text-ok">2 months free</div>
        </button>
        <button onClick={() => api.openUrl(CHECKOUT_MONTHLY)} className="rounded-xl bg-white/5 px-3 py-2.5 text-left ring-1 ring-line hover:bg-white/10">
          <div className="text-[13px] font-semibold">$9 / month</div>
          <div className="text-[11px] text-mute">cancel anytime</div>
        </button>
      </div>

      <div className="mt-auto space-y-1.5">
        <div className="flex gap-2">
          <input value={key} onChange={(e) => setKey(e.target.value)} placeholder="Have a licence key?" className="min-w-0 flex-1 rounded-lg bg-black/40 px-3 py-2 text-[12.5px] outline-none ring-1 ring-line focus:ring-accent" />
          <button disabled={!key || busy} onClick={activate} className="rounded-lg bg-white/10 px-3 text-[12.5px] disabled:opacity-40">Activate</button>
        </div>
        {err && <p className="text-[11.5px] text-bad">{err}</p>}
        <p className="text-[11px] text-mute">14-day refunds, no questions. Savings are estimates.</p>
      </div>
    </div>
  );
}

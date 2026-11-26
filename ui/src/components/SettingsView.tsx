import { useEffect, useState } from "react";
import type { Plan, Settings, Snapshot, Tool } from "../types";
import { api } from "../api";

const TOOLS: { tool: Tool; label: string }[] = [
  { tool: "claude_code", label: "Claude Code" },
  { tool: "codex", label: "Codex" },
  { tool: "gemini", label: "Gemini" },
];

export function SettingsView({ snap, onBack, onPaywall }: { snap: Snapshot; onBack: () => void; onPaywall: () => void }) {
  const [s, setS] = useState<Settings | null>(null);
  const [plans, setPlans] = useState<Record<Tool, Plan[]> | null>(null);
  const [saved, setSaved] = useState(false);
  const [invite, setInvite] = useState({ server: "", code: "", email: "" });
  const [syncMsg, setSyncMsg] = useState<string | null>(null);
  const paid = snap.entitlements.tier !== "free";

  useEffect(() => {
    api.settings().then(setS);
    api.plans().then(setPlans);
  }, []);
  if (!s || !plans) return null;

  const update = (next: Settings) => {
    setS(next);
    setSaved(false);
  };
  const tool = (t: Tool, patch: Partial<Settings["tools"][Tool]>) => update({ ...s, tools: { ...s.tools, [t]: { ...s.tools[t], ...patch } } });

  async function save() {
    await api.saveSettings(s!);
    setSaved(true);
  }

  async function join() {
    setSyncMsg(null);
    try {
      await api.joinSync(invite.server, invite.code, invite.email);
      setSyncMsg("Joined. This device now syncs counts only.");
      setS(await api.settings());
    } catch (e) {
      setSyncMsg(String(e));
    }
  }

  return (
    <div className="fade-in flex h-full flex-col gap-3 overflow-y-auto rounded-[22px] bg-panel/95 p-4 ring-1 ring-line">
      <div className="flex items-center justify-between">
        <button onClick={onBack} className="text-[12px] text-mute hover:text-white">← Back</button>
        <span className="text-[11px] uppercase tracking-wide text-mute">{snap.entitlements.tier}</span>
      </div>

      <Section title="Tools & plans">
        {TOOLS.map(({ tool: t, label }) => (
          <div key={t} className="flex items-center gap-2">
            <input type="checkbox" checked={s.tools[t].enabled} onChange={(e) => tool(t, { enabled: e.target.checked })} />
            <span className="w-24 text-[13px]">{label}</span>
            <select value={s.tools[t].plan_id ?? ""} onChange={(e) => tool(t, { plan_id: e.target.value || null })} className="min-w-0 flex-1 rounded-md bg-black/40 px-2 py-1 text-[12px] ring-1 ring-line">
              <option value="">Not sure</option>
              {plans[t].map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
            </select>
          </div>
        ))}
        {!paid && (
          <label className="flex items-center gap-2 text-[12px] text-mute">
            Free tracks one tool:
            <select value={s.primary_tool ?? ""} onChange={(e) => update({ ...s, primary_tool: (e.target.value || null) as Tool | null })} className="rounded-md bg-black/40 px-2 py-1 ring-1 ring-line">
              {TOOLS.map((t) => <option key={t.tool} value={t.tool}>{t.label}</option>)}
            </select>
          </label>
        )}
      </Section>

      <Section title="Alerts">
        <Row label="Amber at % left">
          <input type="number" min={1} max={90} disabled={!paid} value={s.alerts.amber_pct} onChange={(e) => update({ ...s, alerts: { ...s.alerts, amber_pct: Number(e.target.value) } })} className="w-16 rounded-md bg-black/40 px-2 py-1 text-right text-[12px] ring-1 ring-line disabled:opacity-40" />
        </Row>
        <Row label="Red at % left">
          <input type="number" min={1} max={50} disabled={!paid} value={s.alerts.red_pct} onChange={(e) => update({ ...s, alerts: { ...s.alerts, red_pct: Number(e.target.value) } })} className="w-16 rounded-md bg-black/40 px-2 py-1 text-right text-[12px] ring-1 ring-line disabled:opacity-40" />
        </Row>
        <Toggle label="Runaway-loop alerts" on={s.alerts.runaway_alerts} set={(v) => update({ ...s, alerts: { ...s.alerts, runaway_alerts: v } })} />
        <Toggle label="One-click Stop (beta)" on={s.alerts.one_click_stop} set={(v) => update({ ...s, alerts: { ...s.alerts, one_click_stop: v } })} />
        <Toggle label="System notifications" on={s.alerts.notifications} set={(v) => update({ ...s, alerts: { ...s.alerts, notifications: v } })} />
        <Toggle label="Read Claude's live gauge (best effort)" on={s.live_gauges} set={(v) => update({ ...s, live_gauges: v })} />
        {!paid && <button onClick={onPaywall} className="text-left text-[12px] text-accent">Custom alert levels are Pro →</button>}
      </Section>

      <Section title="Hotkey">
        <Row label="Open panel">
          <input value={s.hotkey} onChange={(e) => update({ ...s, hotkey: e.target.value })} className="w-28 rounded-md bg-black/40 px-2 py-1 text-right text-[12px] ring-1 ring-line" />
        </Row>
      </Section>

      <Section title="Sync & team">
        <p className="text-[11.5px] text-mute">Only token counts, timestamps and model names are sent. Never prompts, code or file names.</p>
        {s.sync.enabled ? (
          <div className="flex items-center justify-between text-[12.5px]">
            <span className="text-ok">Syncing to {s.sync.server_url}</span>
            <button onClick={() => update({ ...s, sync: { ...s.sync, enabled: false } })} className="text-mute hover:text-white">Turn off</button>
          </div>
        ) : (
          <div className="space-y-1.5">
            <input placeholder="Server (https://api.aiusage.dev)" value={invite.server} onChange={(e) => setInvite({ ...invite, server: e.target.value })} className="w-full rounded-md bg-black/40 px-2 py-1 text-[12px] ring-1 ring-line" />
            <div className="flex gap-1.5">
              <input placeholder="Invite code" value={invite.code} onChange={(e) => setInvite({ ...invite, code: e.target.value })} className="min-w-0 flex-1 rounded-md bg-black/40 px-2 py-1 text-[12px] ring-1 ring-line" />
              <input placeholder="Your email" value={invite.email} onChange={(e) => setInvite({ ...invite, email: e.target.value })} className="min-w-0 flex-1 rounded-md bg-black/40 px-2 py-1 text-[12px] ring-1 ring-line" />
            </div>
            <button disabled={!invite.server || !invite.code} onClick={join} className="w-full rounded-md bg-white/10 py-1.5 text-[12px] disabled:opacity-40">Join team or sync group</button>
            {syncMsg && <p className="text-[11.5px] text-mute">{syncMsg}</p>}
          </div>
        )}
      </Section>

      <Section title="Licence">
        {paid && snap.entitlements.tier !== "trial" ? (
          <button onClick={async () => { await api.deactivate(); }} className="text-left text-[12px] text-mute hover:text-white">Deactivate on this device</button>
        ) : (
          <button onClick={onPaywall} className="text-left text-[12px] text-accent">Upgrade or enter a licence key →</button>
        )}
      </Section>

      <div className="sticky bottom-0 mt-auto flex gap-2 bg-panel pt-2">
        <button onClick={save} className="flex-1 rounded-xl bg-accent py-2 text-[13px] font-semibold text-black">{saved ? "Saved ✓" : "Save"}</button>
        <button onClick={() => api.quit()} className="rounded-xl bg-white/5 px-3 text-[12px] text-mute hover:text-white">Quit</button>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-2 border-t border-line pt-2.5 first-of-type:border-0">
      <h3 className="text-[11px] uppercase tracking-wide text-mute">{title}</h3>
      {children}
    </section>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between text-[12.5px]">
      <span>{label}</span>
      {children}
    </div>
  );
}

function Toggle({ label, on, set }: { label: string; on: boolean; set: (v: boolean) => void }) {
  return (
    <button onClick={() => set(!on)} className="flex w-full items-center justify-between text-[12.5px]">
      <span>{label}</span>
      <span className={`flex h-4 w-7 items-center rounded-full px-0.5 transition-colors duration-150 ${on ? "bg-accent" : "bg-white/15"}`}>
        <span className={`h-3 w-3 rounded-full bg-white transition-transform duration-150 ${on ? "translate-x-3" : ""}`} />
      </span>
    </button>
  );
}

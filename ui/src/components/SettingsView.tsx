import { ToolMark } from "./ToolMark";
import { useEffect, useState } from "react";
import type { Plan, Settings, Snapshot, Tool } from "../types";
import { api } from "../api";
import { BackIcon } from "./icons";

const TOOLS: { tool: Tool; label: string }[] = [
  { tool: "claude_code", label: "Claude" },
  { tool: "codex", label: "Codex" },
  { tool: "gemini", label: "Gemini" },
];

export function SettingsView({ snap, onBack, onPaywall, onSetup }: { snap: Snapshot; onBack: () => void; onPaywall: () => void; onSetup: () => void }) {
  const [s, setS] = useState<Settings | null>(null);
  const [plans, setPlans] = useState<Record<Tool, Plan[]> | null>(null);
  const [saveError, setSaveError] = useState("");
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
    setSaveError("");
    try { await api.saveSettings(s!); setSaved(true); }
    catch (e) { setSaveError(String(e)); }
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
    <div className="flex flex-col gap-3 p-3">
      <div className="flex items-center justify-between">
        <button onClick={onBack} className="btn btn-ghost !pl-1.5"><BackIcon /> Back</button>
        <span className="rounded-full bg-white/[0.08] px-2.5 py-0.5 text-[10.5px] font-semibold uppercase tracking-wider text-muted">{snap.entitlements.tier}</span>
      </div>

      <Section title="Claude account">
        {s.claude_web ? (
          <div className="flex items-center justify-between text-[12.5px]">
            <span className="text-ok">Connected to claude.ai</span>
            <button
              onClick={async () => { await api.disconnectClaude(); setS(await api.settings()); }}
              className="text-muted hover:text-fg"
            >
              Disconnect
            </button>
          </div>
        ) : (
          <div className="space-y-2">
            <p className="text-[11.5px] leading-relaxed text-muted">Shows the same plan limits as claude.ai. Needed if you use claude.ai or the Claude desktop app.</p>
            <button onClick={() => api.connectClaude()} className="btn btn-primary w-full">Connect Claude account</button>
          </div>
        )}
        <button onClick={onSetup} className="text-left text-[12px] text-fg underline-offset-2 hover:underline">Check setup and test the numbers →</button>
      </Section>

      <Section title="Agents">
        <Toggle label="Agent status" on={s.agent_status} set={async (v) => {
          setSaveError("");
          try { await api.setAgentStatus(v, s.approvals); setS({ ...s, agent_status: v }); }
          catch (e) { setSaveError(String(e)); }
        }} />
        <p className="text-[11px] leading-relaxed text-muted">Claude Code and Codex tell the widget when they finish or need you. Adds a hook to their settings; only the folder and the event are recorded.</p>
        {s.agent_status && <Toggle label="Approve Claude Code's requests from the widget" on={s.approvals} set={(v) => update({ ...s, approvals: v })} />}
        <Toggle label="Run tests when an agent finishes" on={s.verify_done} set={(v) => update({ ...s, verify_done: v })} />
        <p className="text-[11px] leading-relaxed text-muted">Runs the project's tests in the task's folder (cargo, npm, pnpm, go, pytest, make, or the command in <code className="text-fg">.aiuc/verify</code>) and shows whether they pass.</p>
        <Toggle label="Paste and send tasks automatically" on={s.auto_send} set={(v) => update({ ...s, auto_send: v })} />
        <p className="text-[11px] leading-relaxed text-muted">
          When a task or handoff opens in an app or the web, the widget pastes it and presses Enter once that window is in front. It never types into any other window.
          {navigator.userAgent.includes("Mac") ? " On a Mac, allow AI Usage Controller under System Settings → Privacy & Security → Accessibility." : ""}
        </p>
      </Section>

      <Section title="Tools & plans">
        {TOOLS.map(({ tool: t, label }) => (
          <div key={t} className="flex items-center gap-2">
            <input aria-label={`Track ${label}`} onFocus={() => api.focus()} type="checkbox" checked={s.tools[t].enabled} onChange={(e) => tool(t, { enabled: e.target.checked })} />
            <span className="flex w-24 items-center gap-2 text-[13px]"><ToolMark tool={t} />{label}</span>
            <select aria-label={`${label} plan`} value={s.tools[t].plan_id ?? ""} onChange={(e) => tool(t, { plan_id: e.target.value || null })} className="min-w-0 flex-1 rounded-[10px] bg-white/[0.07] px-2.5 py-1.5 outline-none text-[12px] ring-1 ring-white/[0.06] focus:ring-white/25">
              <option value="">Not sure</option>
              {plans[t].map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
            </select>
          </div>
        ))}
        {!paid && (
          <label className="flex items-center gap-2 text-[12px] text-muted">
            Free tracks one tool:
            <select value={s.primary_tool ?? ""} onChange={(e) => update({ ...s, primary_tool: (e.target.value || null) as Tool | null })} className="rounded-[10px] bg-white/[0.07] px-2.5 py-1.5 outline-none ring-1 ring-white/[0.06] focus:ring-white/25">
              {TOOLS.map((t) => <option key={t.tool} value={t.tool}>{t.label}</option>)}
            </select>
          </label>
        )}
      </Section>

      <Section title="Alerts">
        <Row label="Amber warning at % used">
          <input onFocus={() => api.focus()} type="number" min={10} max={99} disabled={!paid} value={100 - s.alerts.amber_pct} onChange={(e) => update({ ...s, alerts: { ...s.alerts, amber_pct: 100 - Number(e.target.value) } })} className="w-16 rounded-[10px] bg-white/[0.07] px-2.5 py-1.5 outline-none text-right text-[12px] ring-1 ring-white/[0.06] focus:ring-white/25 disabled:opacity-40" />
        </Row>
        <Row label="Red warning at % used">
          <input onFocus={() => api.focus()} type="number" min={50} max={99} disabled={!paid} value={100 - s.alerts.red_pct} onChange={(e) => update({ ...s, alerts: { ...s.alerts, red_pct: 100 - Number(e.target.value) } })} className="w-16 rounded-[10px] bg-white/[0.07] px-2.5 py-1.5 outline-none text-right text-[12px] ring-1 ring-white/[0.06] focus:ring-white/25 disabled:opacity-40" />
        </Row>
        <Toggle label="Runaway-loop alerts" on={s.alerts.runaway_alerts} set={(v) => update({ ...s, alerts: { ...s.alerts, runaway_alerts: v } })} />
        <Toggle label="One-click Stop (beta)" on={s.alerts.one_click_stop} set={(v) => update({ ...s, alerts: { ...s.alerts, one_click_stop: v } })} />
        <Toggle label="System notifications" on={s.alerts.notifications} set={(v) => update({ ...s, alerts: { ...s.alerts, notifications: v } })} />
        <Toggle label="Also read limits via the Claude Code login" on={s.live_gauges} set={(v) => update({ ...s, live_gauges: v })} />
        {!paid && <button onClick={onPaywall} className="text-left text-[12px] text-fg underline-offset-2">Custom alert levels are Pro →</button>}
      </Section>

      <Section title="Widget">
        <Toggle label="Launch at login" on={s.launch_at_login} set={(v) => update({ ...s, launch_at_login: v })} />
        <Toggle label="Hide pill when no AI tool has been used for 30 min" on={s.hide_when_idle} set={(v) => update({ ...s, hide_when_idle: v })} />
        {saveError && <p role="alert" className="text-xs text-bad">{saveError}</p>}

        <Row label="Open panel">
          <input onFocus={() => api.focus()} value={s.hotkey} onChange={(e) => update({ ...s, hotkey: e.target.value })} className="w-28 rounded-[10px] bg-white/[0.07] px-2.5 py-1.5 outline-none text-right text-[12px] ring-1 ring-white/[0.06] focus:ring-white/25" />
        </Row>
      </Section>

      <Section title="Sync & team">
        <p className="text-[11.5px] text-muted">Only token counts, timestamps and model names are sent. Never prompts, code or file names.</p>
        {s.sync.enabled ? (
          <div className="flex items-center justify-between text-[12.5px]">
            <span className="text-ok">Syncing to {s.sync.server_url}</span>
            <button onClick={() => update({ ...s, sync: { ...s.sync, enabled: false } })} className="text-muted hover:text-fg">Turn off</button>
          </div>
        ) : (
          <div className="space-y-2">
            <input onFocus={() => api.focus()} placeholder="Server (https://api.aiusage.dev)" value={invite.server} onChange={(e) => setInvite({ ...invite, server: e.target.value })} className="w-full rounded-[10px] bg-white/[0.07] px-2.5 py-1.5 outline-none text-[12px] ring-1 ring-white/[0.06] focus:ring-white/25" />
            <div className="flex gap-1.5">
              <input onFocus={() => api.focus()} placeholder="Invite code" value={invite.code} onChange={(e) => setInvite({ ...invite, code: e.target.value })} className="min-w-0 flex-1 rounded-[10px] bg-white/[0.07] px-2.5 py-1.5 outline-none text-[12px] ring-1 ring-white/[0.06] focus:ring-white/25" />
              <input onFocus={() => api.focus()} placeholder="Your email" value={invite.email} onChange={(e) => setInvite({ ...invite, email: e.target.value })} className="min-w-0 flex-1 rounded-[10px] bg-white/[0.07] px-2.5 py-1.5 outline-none text-[12px] ring-1 ring-white/[0.06] focus:ring-white/25" />
            </div>
            <button disabled={!invite.server || !invite.code} onClick={join} className="btn w-full">Join team or sync group</button>
            {syncMsg && <p className="text-[11.5px] text-muted">{syncMsg}</p>}
          </div>
        )}
      </Section>

      <Section title="Licence">
        {paid && snap.entitlements.tier !== "trial" ? (
          <button onClick={async () => { await api.deactivate(); }} className="text-left text-[12px] text-muted hover:text-fg">Deactivate on this device</button>
        ) : (
          <button onClick={onPaywall} className="text-left text-[12px] text-fg underline-offset-2">Upgrade or enter a licence key →</button>
        )}
      </Section>

      <div className="sticky bottom-0 -mx-3 -mb-3 mt-auto flex gap-2 bg-black px-3 pb-3 pt-3">
        <button onClick={save} className="btn btn-primary flex-1 !py-2">{saved ? "Saved ✓" : "Save"}</button>
        <button onClick={() => api.quit()} className="btn btn-ghost">Quit</button>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-1.5">
      <h3 className="eyebrow px-1">{title}</h3>
      <div className="tile space-y-2.5">{children}</div>
    </section>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="flex items-center justify-between text-[12.5px]">
      <span>{label}</span>
      {children}
    </label>
  );
}

function Toggle({ label, on, set }: { label: string; on: boolean; set: (v: boolean) => void }) {
  return (
    <button role="switch" aria-checked={on} onClick={() => set(!on)} className="flex w-full items-center justify-between text-[12.5px]">
      <span>{label}</span>
      <span className={`flex h-[18px] w-8 shrink-0 items-center rounded-full px-0.5 transition-colors duration-150 ${on ? "bg-[#34c759]" : "bg-white/15"}`}>
        <span className={`h-[14px] w-[14px] rounded-full bg-white shadow transition-transform duration-200 ${on ? "translate-x-[14px]" : ""}`} />
      </span>
    </button>
  );
}

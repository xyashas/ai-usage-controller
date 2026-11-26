import { useEffect, useState } from "react";
import type { Detected, Plan, Settings, Tool } from "../types";
import { api } from "../api";

// Detect installed tools → pick a plan for each → choose alert levels. Under 60 seconds.
export function Onboarding({ onDone }: { onDone: () => void }) {
  const [step, setStep] = useState(0);
  const [found, setFound] = useState<Detected[]>([]);
  const [plans, setPlans] = useState<Record<Tool, Plan[]> | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);

  useEffect(() => {
    Promise.all([api.detect(), api.plans(), api.settings()]).then(([d, p, s]) => {
      setFound(d);
      setPlans(p);
      setSettings(s);
    });
  }, []);

  if (!settings || !plans) return <Shell><p className="text-mute">Looking for your AI tools…</p></Shell>;
  const tools = found.filter((d) => d.found);

  const setPlan = (tool: Tool, plan_id: string) =>
    setSettings({ ...settings, tools: { ...settings.tools, [tool]: { ...settings.tools[tool], plan_id } } });

  async function finish() {
    const s: Settings = {
      ...settings!,
      onboarded: true,
      primary_tool: settings!.primary_tool ?? tools[0]?.tool ?? "claude_code",
    };
    for (const d of found) s.tools[d.tool] = { ...s.tools[d.tool], enabled: d.found };
    await api.saveSettings(s);
    onDone();
  }

  return (
    <Shell>
      <div className="flex gap-1">
        {[0, 1, 2].map((i) => <div key={i} className={`h-1 flex-1 rounded-full ${i <= step ? "bg-accent" : "bg-white/10"}`} />)}
      </div>

      {step === 0 && (
        <>
          <h2 className="text-[18px] font-semibold">Found on this {navigator.userAgent.includes("Mac") ? "Mac" : navigator.userAgent.includes("Windows") ? "PC" : "computer"}</h2>
          <div className="space-y-2">
            {found.map((d) => (
              <div key={d.tool} className="flex items-center justify-between rounded-xl bg-white/5 px-3 py-2.5">
                <span className="text-[13.5px]">{d.label}</span>
                <span className={`text-[12px] ${d.found ? "text-ok" : "text-mute"}`}>{d.found ? "✓ found" : "not installed"}</span>
              </div>
            ))}
          </div>
          <p className="text-[11.5px] text-mute">We read usage counts from local logs. Prompts and code never leave your machine.</p>
          <button onClick={() => setStep(1)} className="mt-auto rounded-xl bg-accent py-2.5 font-semibold text-black">Continue</button>
        </>
      )}

      {step === 1 && (
        <>
          <h2 className="text-[18px] font-semibold">Which plan are you on?</h2>
          <div className="space-y-3 overflow-y-auto">
            {(tools.length ? tools : found).map((d) => (
              <label key={d.tool} className="block space-y-1">
                <span className="text-[12px] text-mute">{d.label}</span>
                <select
                  value={settings.tools[d.tool]?.plan_id ?? ""}
                  onChange={(e) => setPlan(d.tool, e.target.value)}
                  className="w-full rounded-lg bg-black/40 px-3 py-2 text-[13px] outline-none ring-1 ring-line focus:ring-accent"
                >
                  <option value="">Not sure</option>
                  {plans[d.tool].map((p) => (
                    <option key={p.id} value={p.id}>{p.name}{p.price ? ` · $${p.price}/mo` : ""}</option>
                  ))}
                </select>
              </label>
            ))}
          </div>
          <button onClick={() => setStep(2)} className="mt-auto rounded-xl bg-accent py-2.5 font-semibold text-black">Continue</button>
        </>
      )}

      {step === 2 && (
        <>
          <h2 className="text-[18px] font-semibold">When should we warn you?</h2>
          <div className="space-y-2">
            {[
              { label: "Balanced", amber: 20, red: 5, hint: "Amber at 20% left, red at 5%" },
              { label: "Early", amber: 35, red: 10, hint: "Amber at 35% left, red at 10%" },
              { label: "Late", amber: 10, red: 3, hint: "Amber at 10% left, red at 3%" },
            ].map((o) => {
              const on = settings.alerts.amber_pct === o.amber && settings.alerts.red_pct === o.red;
              return (
                <button
                  key={o.label}
                  onClick={() => setSettings({ ...settings, alerts: { ...settings.alerts, amber_pct: o.amber, red_pct: o.red } })}
                  className={`w-full rounded-xl px-3 py-2.5 text-left ring-1 ${on ? "bg-accent/15 ring-accent" : "bg-white/5 ring-line"}`}
                >
                  <div className="text-[13.5px] font-medium">{o.label}</div>
                  <div className="text-[11.5px] text-mute">{o.hint}</div>
                </button>
              );
            })}
          </div>
          <p className="text-[11.5px] text-mute">Custom levels are part of Pro; Free uses Balanced.</p>
          <button onClick={finish} className="mt-auto rounded-xl bg-accent py-2.5 font-semibold text-black">Done</button>
        </>
      )}
    </Shell>
  );
}

function Shell({ children }: { children: React.ReactNode }) {
  return <div className="fade-in flex h-full flex-col gap-4 rounded-[22px] bg-panel/95 p-5 ring-1 ring-line">{children}</div>;
}

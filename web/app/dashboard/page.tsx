"use client";

import { useCallback, useEffect, useState } from "react";
import { API_URL } from "@/lib/config";

interface Seat {
  member_id: string;
  email: string;
  devices: number;
  last_active: number | null;
  idle: boolean;
  api_equivalent_30d: number;
  requests_30d: number;
  tools: { tool: string; requests: number; api_equivalent: number }[];
  subscriptions: { tool: string; plan: string | null; price: number }[];
  subscription_cost: number;
  recommendations: { tool: string; action: string; monthly_saving_usd: number; current_plan: string }[];
  monthly_waste: number;
}
interface Dashboard {
  org: { name: string; invite_code: string; seats_paid: number; subscription_status: string; has_anthropic_key: boolean };
  totals: { members: number; idle_seats: number; subscription_cost: number; monthly_waste: number; api_equivalent_30d: number; our_price: number };
  seats: Seat[];
  by_model: { model: string; cost: number; requests: number }[];
}

const usd = (n: number, d = 0) => `$${n.toLocaleString(undefined, { minimumFractionDigits: d, maximumFractionDigits: d })}`;
const TOOL: Record<string, string> = { claude_code: "Claude Code", codex: "Codex", gemini: "Gemini" };
const ago = (ts: number | null) => {
  if (!ts) return "never";
  const d = Math.floor((Date.now() - ts) / 86_400_000);
  return d === 0 ? "today" : d === 1 ? "yesterday" : `${d} days ago`;
};
const KEY = "aiuc_admin_token";

export default function DashboardPage() {
  const [token, setToken] = useState<string | null>(null);
  const [data, setData] = useState<Dashboard | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [created, setCreated] = useState<{ admin_token: string; invite_code: string } | null>(null);

  useEffect(() => {
    const fromUrl = new URLSearchParams(location.hash.slice(1)).get("token");
    let saved: string | null = null;
    try {
      saved = localStorage.getItem(KEY);
    } catch {}
    setToken(fromUrl ?? saved);
  }, []);

  const load = useCallback(async (t: string) => {
    setErr(null);
    const r = await fetch(`${API_URL}/v1/admin/dashboard`, { headers: { authorization: `Bearer ${t}` } }).catch(() => null);
    if (!r) return setErr(`Can't reach the API at ${API_URL}.`);
    if (r.status === 401) return setErr("That admin token isn't valid.");
    const j = (await r.json()) as Dashboard;
    setData(j);
    try {
      localStorage.setItem(KEY, t);
    } catch {}
  }, []);

  useEffect(() => {
    if (token) load(token);
  }, [token, load]);

  if (!token || (!data && err)) return <SignIn onToken={setToken} err={err} onCreated={(c) => { setCreated(c); setToken(c.admin_token); }} />;
  if (!data) return <main className="mx-auto max-w-5xl px-4 py-10 text-mute sm:px-6">Loading…</main>;

  const t = data.totals;
  return (
    <main className="mx-auto max-w-5xl space-y-8 px-4 pb-16 sm:px-6">
      {created && (
        <div className="rounded-2xl bg-warn/10 p-4 text-sm ring-1 ring-warn/40">
          Save your admin token somewhere safe; it's shown once: <code className="break-all text-white">{created.admin_token}</code>
        </div>
      )}
      <div className="flex flex-wrap items-end justify-between gap-4 pt-4">
        <div>
          <h1 className="text-3xl font-semibold tracking-tight">{data.org.name}</h1>
          <p className="mt-1 text-sm text-mute">
            Invite code <code className="rounded bg-white/10 px-1.5 py-0.5 text-white">{data.org.invite_code}</code> · paste it in the widget under Settings → Sync & team.
          </p>
        </div>
        <button onClick={() => load(token)} className="rounded-xl bg-white/10 px-4 py-2 text-sm">Refresh</button>
      </div>

      <section className="grid gap-3 sm:grid-cols-4">
        <Big label="Monthly waste (est.)" value={usd(t.monthly_waste)} tone={t.monthly_waste > 0 ? "text-warn" : "text-ok"} />
        <Big label="Idle seats" value={`${t.idle_seats} of ${t.members}`} tone={t.idle_seats ? "text-warn" : ""} />
        <Big label="AI subscriptions" value={`${usd(t.subscription_cost)}/mo`} />
        <Big label="Usage at API prices, 30d" value={usd(t.api_equivalent_30d)} />
      </section>

      <section className="overflow-x-auto rounded-2xl bg-panel ring-1 ring-line">
        <table className="w-full min-w-[720px] text-left text-sm">
          <thead className="text-xs uppercase tracking-wide text-mute">
            <tr className="border-b border-line">
              <th className="p-3 font-medium">Seat</th>
              <th className="p-3 font-medium">Last active</th>
              <th className="p-3 font-medium">Plans</th>
              <th className="p-3 text-right font-medium">30d at API prices</th>
              <th className="p-3 font-medium">Recommendation</th>
              <th className="p-3 text-right font-medium">Waste / mo</th>
            </tr>
          </thead>
          <tbody>
            {data.seats.map((s) => (
              <tr key={s.member_id} className="border-b border-line/60 last:border-0">
                <td className="p-3">
                  <div>{s.email}</div>
                  <div className="text-xs text-mute">{s.devices} device{s.devices === 1 ? "" : "s"} · {s.tools.map((x) => TOOL[x.tool]).join(", ") || "no usage"}</div>
                </td>
                <td className={`p-3 ${s.idle ? "text-warn" : "text-mute"}`}>{s.idle ? `Idle · ${ago(s.last_active)}` : ago(s.last_active)}</td>
                <td className="p-3 text-mute">{s.subscriptions.map((x) => `${x.plan} ($${x.price})`).join(", ") || "—"}</td>
                <td className="tabular p-3 text-right">{usd(s.api_equivalent_30d, 2)}</td>
                <td className="p-3 text-mute">
                  {s.idle ? "Cancel or reassign this seat" : s.recommendations.map((r) => `${TOOL[r.tool]}: ${r.action}${r.monthly_saving_usd ? ` (save ${usd(r.monthly_saving_usd)})` : ""}`).join("; ") || "Plan fits"}
                </td>
                <td className={`tabular p-3 text-right font-medium ${s.monthly_waste > 0 ? "text-warn" : "text-mute"}`}>{usd(s.monthly_waste)}</td>
              </tr>
            ))}
            {!data.seats.length && (
              <tr><td colSpan={6} className="p-6 text-center text-mute">No seats yet. Share the invite code with your team.</td></tr>
            )}
          </tbody>
        </table>
      </section>

      <section className="grid gap-6 md:grid-cols-2">
        <div className="rounded-2xl bg-panel p-5 ring-1 ring-line">
          <h2 className="font-semibold">Spend by model, 30 days</h2>
          <div className="mt-4 space-y-2.5">
            {data.by_model.map((m) => {
              const max = data.by_model[0]?.cost || 1;
              return (
                <div key={m.model} className="text-sm">
                  <div className="flex justify-between"><span className="text-white/85">{m.model}</span><span className="tabular text-mute">{usd(m.cost, 2)}</span></div>
                  <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-white/10"><div className="h-full rounded-full bg-accent" style={{ width: `${(m.cost / max) * 100}%` }} /></div>
                </div>
              );
            })}
            {!data.by_model.length && <p className="text-sm text-mute">No usage synced yet.</p>}
          </div>
        </div>
        <AnthropicSpend token={token} hasKey={data.org.has_anthropic_key} onSaved={() => load(token)} />
      </section>
      <p className="text-xs text-mute">Usage at API prices is an estimate from token counts. On subscriptions it measures how much of the allowance was used, not money spent. Waste = idle seats' subscriptions + unused tools + plan downgrades the widget recommends.</p>
    </main>
  );
}

function Big({ label, value, tone = "" }: { label: string; value: string; tone?: string }) {
  return (
    <div className="rounded-2xl bg-panel p-4 ring-1 ring-line">
      <div className="text-xs text-mute">{label}</div>
      <div className={`tabular mt-1 text-2xl font-semibold ${tone}`}>{value}</div>
    </div>
  );
}

function AnthropicSpend({ token, hasKey, onSaved }: { token: string; hasKey: boolean; onSaved: () => void }) {
  const [key, setKey] = useState("");
  const [spend, setSpend] = useState<{ total_usd: number; days: { date: string; usd: number }[] } | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    if (!hasKey) return;
    fetch(`${API_URL}/v1/admin/api-spend`, { headers: { authorization: `Bearer ${token}` } })
      .then(async (r) => (r.ok ? setSpend(await r.json()) : setMsg((await r.json()).error)))
      .catch(() => setMsg("Couldn't load API spend."));
  }, [hasKey, token]);

  async function save(e: React.FormEvent) {
    e.preventDefault();
    const r = await fetch(`${API_URL}/v1/admin/anthropic-key`, { method: "PUT", headers: { authorization: `Bearer ${token}`, "content-type": "application/json" }, body: JSON.stringify({ admin_key: key }) });
    const j = await r.json();
    setMsg(r.ok ? "Saved. Stored encrypted; only sent to Anthropic." : j.error);
    if (r.ok) {
      setKey("");
      onSaved();
    }
  }

  return (
    <div className="rounded-2xl bg-panel p-5 ring-1 ring-line">
      <h2 className="font-semibold">Anthropic API spend</h2>
      {spend ? (
        <>
          <div className="tabular mt-3 text-3xl font-semibold">{usd(spend.total_usd, 2)}</div>
          <div className="text-xs text-mute">last 30 days, from the Admin API cost report</div>
          <div className="mt-4 flex h-16 items-end gap-[3px]">
            {spend.days.map((d) => {
              const max = Math.max(...spend.days.map((x) => x.usd), 1);
              return <div key={d.date} title={`${d.date}: ${usd(d.usd, 2)}`} className="flex-1 rounded-t bg-accent/70" style={{ height: `${Math.max(2, (d.usd / max) * 100)}%` }} />;
            })}
          </div>
        </>
      ) : (
        <p className="mt-2 text-sm text-mute">For teams billed through the Anthropic API. Paste an Admin key (sk-ant-admin…) from the Claude Console.</p>
      )}
      <form onSubmit={save} className="mt-4 flex gap-2">
        <input value={key} onChange={(e) => setKey(e.target.value)} type="password" placeholder={hasKey ? "Replace admin key" : "sk-ant-admin…"} className="min-w-0 flex-1 rounded-xl bg-black/40 px-3 py-2 text-sm ring-1 ring-line" />
        <button className="rounded-xl bg-white/10 px-4 text-sm">Save</button>
      </form>
      {msg && <p className="mt-2 text-xs text-mute">{msg}</p>}
    </div>
  );
}

function SignIn({ onToken, err, onCreated }: { onToken: (t: string) => void; err: string | null; onCreated: (c: { admin_token: string; invite_code: string }) => void }) {
  const [t, setT] = useState("");
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [cErr, setCErr] = useState<string | null>(null);

  async function create(e: React.FormEvent) {
    e.preventDefault();
    const r = await fetch(`${API_URL}/v1/orgs`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ name, email, kind: "team" }) }).catch(() => null);
    if (!r) return setCErr(`Can't reach the API at ${API_URL}.`);
    const j = await r.json();
    if (!r.ok) return setCErr(j.error);
    onCreated(j);
  }

  return (
    <main className="mx-auto grid max-w-3xl gap-6 px-4 py-10 sm:px-6 md:grid-cols-2">
      <form onSubmit={(e) => { e.preventDefault(); onToken(t.trim()); }} className="rounded-2xl bg-panel p-6 ring-1 ring-line">
        <h1 className="text-xl font-semibold">Team dashboard</h1>
        <p className="mt-1 text-sm text-mute">Sign in with your team's admin token.</p>
        <input value={t} onChange={(e) => setT(e.target.value)} placeholder="Admin token" className="mt-4 w-full rounded-xl bg-black/40 px-3 py-2.5 text-sm ring-1 ring-line" />
        <button className="mt-3 w-full rounded-xl bg-accent py-2.5 font-semibold text-black">Open dashboard</button>
        {err && <p className="mt-2 text-sm text-bad">{err}</p>}
      </form>
      <form onSubmit={create} className="rounded-2xl bg-panel p-6 ring-1 ring-line">
        <h2 className="text-xl font-semibold">New team</h2>
        <p className="mt-1 text-sm text-mute">Free 14-day waste report, then $29 a seat.</p>
        <input value={name} onChange={(e) => setName(e.target.value)} required placeholder="Team name" className="mt-4 w-full rounded-xl bg-black/40 px-3 py-2.5 text-sm ring-1 ring-line" />
        <input value={email} onChange={(e) => setEmail(e.target.value)} required type="email" placeholder="Your work email" className="mt-2 w-full rounded-xl bg-black/40 px-3 py-2.5 text-sm ring-1 ring-line" />
        <button className="mt-3 w-full rounded-xl bg-white/10 py-2.5 font-semibold">Create team</button>
        {cErr && <p className="mt-2 text-sm text-bad">{cErr}</p>}
      </form>
    </main>
  );
}

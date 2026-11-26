"use client";

import { useMemo, useState } from "react";
import { buildReport, parseCcusage, PLANS, type LimitHits, type Report } from "@/lib/calc";
import { API_URL, REPORT_CHECKOUT_URL } from "@/lib/config";

const usd = (n: number) => `$${n.toLocaleString(undefined, { maximumFractionDigits: 0 })}`;

export default function Calculator() {
  const [text, setText] = useState("");
  const [plan, setPlan] = useState("max20");
  const [hits, setHits] = useState<LimitHits>("never");
  const [email, setEmail] = useState("");
  const [joined, setJoined] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const usage = useMemo(() => parseCcusage(text), [text]);
  const plans = PLANS[usage?.vendor ?? "claude"];
  const planId = plans.some((p) => p.id === plan) ? plan : plans[plans.length - 1].id;
  const report: Report | null = useMemo(() => (usage && usage.days.length ? buildReport(usage, planId, hits) : null), [usage, planId, hits]);

  async function join(e: React.FormEvent) {
    e.preventDefault();
    setErr(null);
    try {
      const r = await fetch(`${API_URL}/v1/waitlist`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          email,
          source: "calculator",
          report: report && { plan: report.current.id, waste: Math.round(report.monthlyWaste), api: Math.round(report.apiEquivalentMonthly), rec: report.recommended.kind },
        }),
      });
      const j = await r.json();
      if (!r.ok) throw new Error(j.error ?? "something went wrong");
      setJoined(`You're #${j.position} on the list. We'll email you when the widget is ready.`);
    } catch (e) {
      setErr(String((e as Error).message));
    }
  }

  return (
    <main className="mx-auto max-w-3xl px-4 pb-16 sm:px-6">
      <h1 className="mt-6 text-3xl font-semibold tracking-tight sm:text-4xl">Are you overpaying for AI?</h1>
      <p className="mt-3 text-mute">
        Run <code className="rounded bg-white/10 px-1.5 py-0.5 text-white">npx ccusage@latest daily --json</code> (or <code className="rounded bg-white/10 px-1.5 py-0.5 text-white">npx @ccusage/codex@latest daily --json</code>), paste the output, and pick your plan. Everything runs in your browser.
      </p>

      <textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder='{"daily":[{"date":"2026-11-01","totalCost":12.34, ...}]}  — or paste the table'
        className="mt-6 h-44 w-full rounded-2xl bg-panel p-4 font-mono text-xs outline-none ring-1 ring-line focus:ring-accent"
      />
      {text && !usage && <p className="mt-2 text-sm text-bad">Couldn't read that. Paste the JSON from <code>ccusage daily --json</code> or the printed table.</p>}

      <div className="mt-4 grid gap-3 sm:grid-cols-2">
        <label className="block text-sm">
          <span className="text-mute">Your plan</span>
          <select value={planId} onChange={(e) => setPlan(e.target.value)} className="mt-1 w-full rounded-xl bg-panel px-3 py-2.5 ring-1 ring-line">
            {plans.map((p) => <option key={p.id} value={p.id}>{p.name} · ${p.price}/mo</option>)}
          </select>
        </label>
        <label className="block text-sm">
          <span className="text-mute">How often do you hit the limit?</span>
          <select value={hits} onChange={(e) => setHits(e.target.value as LimitHits)} className="mt-1 w-full rounded-xl bg-panel px-3 py-2.5 ring-1 ring-line">
            <option value="never">Never</option>
            <option value="sometimes">Sometimes</option>
            <option value="often">Every week or more</option>
          </select>
        </label>
      </div>

      {report && (
        <section className="mt-8 rounded-2xl bg-panel p-6 ring-1 ring-line">
          <div className="text-xs uppercase tracking-wide text-mute">Your report · estimate</div>
          <h2 className={`mt-2 text-2xl font-semibold leading-snug ${report.monthlyWaste >= 1 ? "text-warn" : "text-ok"}`}>{report.headline}</h2>
          <div className="mt-5 grid grid-cols-3 gap-3 text-sm">
            <Stat label="You pay" value={`${usd(report.current.price)}/mo`} />
            <Stat label="Usage at API prices" value={`${usd(report.apiEquivalentMonthly)}/mo`} />
            <Stat label="Wasted" value={`${usd(report.monthlyWaste)}/mo`} />
          </div>
          <ul className="mt-5 space-y-1 text-sm text-mute">{report.reasons.map((r) => <li key={r}>· {r}</li>)}</ul>

          <div className="mt-6 grid gap-3 sm:grid-cols-2">
            <a href={REPORT_CHECKOUT_URL} className="rounded-xl bg-accent px-4 py-3 text-center font-semibold text-black">Full report with day-by-day plan · $9</a>
            <form onSubmit={join} className="flex gap-2">
              <input value={email} onChange={(e) => setEmail(e.target.value)} type="email" required placeholder="you@company.com" className="min-w-0 flex-1 rounded-xl bg-black/40 px-3 py-3 text-sm ring-1 ring-line" />
              <button className="rounded-xl bg-white/10 px-4 text-sm font-medium">Get the widget</button>
            </form>
          </div>
          {joined && <p className="mt-3 text-sm text-ok">{joined}</p>}
          {err && <p className="mt-3 text-sm text-bad">{err}</p>}
        </section>
      )}
    </main>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl bg-black/30 p-3">
      <div className="text-xs text-mute">{label}</div>
      <div className="tabular mt-0.5 text-lg font-semibold">{value}</div>
    </div>
  );
}

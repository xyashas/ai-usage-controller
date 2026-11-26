import Link from "next/link";
import { DOWNLOAD_URL, PRO_CHECKOUT_URL, TEAM_CHECKOUT_URL } from "@/lib/config";

function WidgetMock() {
  return (
    <div className="mx-auto w-full max-w-[380px]">
      <div className="mx-auto mb-3 flex h-8 w-52 items-center gap-2.5 rounded-b-2xl bg-black px-4 ring-1 ring-line">
        <div className="h-1 flex-1 overflow-hidden rounded-full bg-white/10"><div className="h-full w-[34%] rounded-full bg-ok" /></div>
        <span className="tabular text-[11px] text-white/85">$3.76</span>
      </div>
      <div className="space-y-3.5 rounded-[22px] bg-panel p-4 text-left ring-1 ring-line">
        <div className="flex items-baseline justify-between">
          <span className="font-semibold">Claude Code <span className="ml-1 rounded bg-white/5 px-1.5 py-0.5 text-[10px] uppercase text-mute">live</span></span>
          <span className="tabular text-2xl font-semibold text-ok">34%</span>
        </div>
        <div className="h-1.5 overflow-hidden rounded-full bg-white/10"><div className="h-full w-[34%] rounded-full bg-ok" /></div>
        <div className="flex justify-between text-xs text-mute"><span className="text-white/90">About 20 min left at this pace</span><span>resets in 2h 12m</span></div>
        <div className="flex items-center gap-3 rounded-xl bg-bad/15 px-3 py-2.5 text-[12.5px] ring-1 ring-bad/40">
          <span className="h-2 w-2 rounded-full bg-bad" />
          <span className="flex-1">$4.20 in 6 min, possible loop. Stop?</span>
          <span className="rounded-lg bg-bad px-2.5 py-1 text-xs font-semibold text-black">Stop</span>
        </div>
        {[
          ["Codex", 76, "3h 21m"],
          ["Gemini", 91, "10h 00m"],
        ].map(([n, p, r]) => (
          <div key={n} className="flex items-center gap-3 text-[13px]">
            <span className="w-20 text-white/80">{n}</span>
            <div className="h-1 flex-1 overflow-hidden rounded-full bg-white/10"><div className="h-full rounded-full bg-ok" style={{ width: `${p}%` }} /></div>
            <span className="tabular w-10 text-right text-xs text-ok">{p}%</span>
            <span className="tabular w-16 text-right text-[11px] text-mute">↻ {r}</span>
          </div>
        ))}
        <div className="flex justify-between border-t border-line pt-3 text-xs text-mute">
          <span>Saved this month <b className="text-ok">$212</b> est.</span>
          <span>Settings</span>
        </div>
      </div>
    </div>
  );
}

const features = [
  ["Live headroom bar", "A thin bar in the notch or a pill on Windows: green, amber, red."],
  ["Time to cutoff", "“About 22 min left at this pace,” from a 15-minute rolling burn rate."],
  ["Tool switchboard", "Claude Code, Codex and Gemini side by side, with reset timers."],
  ["Runaway alerts", "“$4.20 in 6 min, possible loop.” Spot an agent stuck on the same files."],
  ["Model-cost nudge", "“340 small edits on Opus today. Sonnet would cost about $12 less.”"],
  ["Monthly plan audit", "“You used 31% of Max 20x. Max 5x saves you $100/month.”"],
];

export default function Home() {
  return (
    <main className="mx-auto max-w-5xl px-4 sm:px-6">
      <section className="grid items-center gap-12 py-12 md:grid-cols-2 md:py-20">
        <div>
          <h1 className="text-4xl font-semibold leading-tight tracking-tight sm:text-5xl">Never get cut off mid-task. Never overpay for AI.</h1>
          <p className="mt-5 text-lg text-mute">
            A tiny Mac and Windows widget for Claude Code, Codex and Gemini. It shows the runway you have left, warns you before you hit the wall, and tells you when you're wasting money.
          </p>
          <div className="mt-8 flex flex-wrap gap-3">
            <a href={DOWNLOAD_URL} className="rounded-xl bg-accent px-5 py-3 font-semibold text-black">Download free</a>
            <Link href="/calculator" className="rounded-xl bg-white/5 px-5 py-3 font-medium ring-1 ring-line hover:bg-white/10">Am I overpaying? →</Link>
          </div>
          <p className="mt-4 text-sm text-mute">Free tracks one tool. Reads local logs; prompts and code never leave your machine.</p>
        </div>
        <WidgetMock />
      </section>

      <section className="grid gap-4 py-10 sm:grid-cols-2 lg:grid-cols-3">
        {features.map(([t, d]) => (
          <div key={t} className="rounded-2xl bg-panel p-5 ring-1 ring-line">
            <h3 className="font-semibold">{t}</h3>
            <p className="mt-1.5 text-sm text-mute">{d}</p>
          </div>
        ))}
      </section>

      <section className="py-10">
        <h2 className="text-2xl font-semibold">Free trackers look back. This looks forward.</h2>
        <p className="mt-3 max-w-2xl text-mute">
          Tools like ccusage report what you already spent. AI Usage Controller tells you how long you have left, which tool to switch to, when an agent is looping, and which plan you should be on.
        </p>
      </section>

      <section id="pricing" className="grid gap-4 py-12 md:grid-cols-3">
        {[
          { name: "Free", price: "$0", sub: "forever", items: ["One tool", "Live headroom bar", "Reset timers", "Amber and red alerts"], cta: ["Download", DOWNLOAD_URL] },
          { name: "Pro", price: "$79/yr", sub: "or $9/mo · 7-day trial, no card", items: ["All tools + switchboard", "Time to cutoff", "Runaway alerts + session meter", "Model nudges + monthly plan audit", "Savings counter · 2 devices"], cta: ["Get Pro", PRO_CHECKOUT_URL], hi: true },
          { name: "Team", price: "$29", sub: "per seat / month · 3-seat minimum", items: ["Everything in Pro", "Team dashboard", "Idle-seat and waste report", "Anthropic Admin API spend", "Priority support"], cta: ["Start a team", TEAM_CHECKOUT_URL] },
        ].map((p) => (
          <div key={p.name} className={`flex flex-col rounded-2xl p-6 ring-1 ${p.hi ? "bg-accent/10 ring-accent/50" : "bg-panel ring-line"}`}>
            <h3 className="font-semibold">{p.name}</h3>
            <div className="mt-2 text-3xl font-semibold">{p.price}</div>
            <div className="text-sm text-mute">{p.sub}</div>
            <ul className="mt-5 flex-1 space-y-2 text-sm text-white/85">{p.items.map((i) => <li key={i}>✓ {i}</li>)}</ul>
            <a href={p.cta[1]} className={`mt-6 rounded-xl py-2.5 text-center font-semibold ${p.hi ? "bg-accent text-black" : "bg-white/10"}`}>{p.cta[0]}</a>
          </div>
        ))}
      </section>
      <p className="pb-6 text-sm text-mute">Refunds within 14 days, no questions. Savings figures are estimates and always labelled as such.</p>
    </main>
  );
}

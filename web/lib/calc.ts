// Week-0 validation calculator: paste ccusage output, pick your plan, get back
// "you're wasting $X, here's the plan to be on". Runs entirely in the browser.

export type Vendor = "claude" | "codex";

export interface Day {
  date: string; // YYYY-MM-DD
  cost: number; // API-equivalent USD
}

export interface ParsedUsage {
  days: Day[];
  source: "daily-json" | "monthly-json" | "table" | "session-json";
  vendor: Vendor;
}

export interface PlanOption {
  id: string;
  name: string;
  price: number;
}

export const PLANS: Record<Vendor, PlanOption[]> = {
  claude: [
    { id: "pro", name: "Claude Pro", price: 20 },
    { id: "max5", name: "Claude Max 5x", price: 100 },
    { id: "max20", name: "Claude Max 20x", price: 200 },
  ],
  codex: [
    { id: "plus", name: "ChatGPT Plus", price: 20 },
    { id: "pro", name: "ChatGPT Pro", price: 200 },
  ],
};

const num = (v: unknown) => (typeof v === "number" && Number.isFinite(v) ? v : Number(v) || 0);

/** Accepts `ccusage daily --json`, `ccusage monthly --json`, `ccusage session --json`, or the printed table. */
export function parseCcusage(text: string): ParsedUsage | null {
  const t = text.trim();
  if (!t) return null;
  const vendor: Vendor = /gpt-|codex/i.test(t) && !/claude-/i.test(t) ? "codex" : "claude";
  if (t.startsWith("{") || t.startsWith("[")) {
    try {
      const j = JSON.parse(t);
      if (Array.isArray(j?.daily)) {
        return { vendor, source: "daily-json", days: j.daily.map((d: Record<string, unknown>) => ({ date: String(d.date ?? d.day ?? "").slice(0, 10), cost: num(d.totalCost ?? d.costUSD ?? d.cost) })).filter((d: Day) => d.date) };
      }
      if (Array.isArray(j?.monthly)) {
        // Spread each month evenly across its days so the 30-day projection still works.
        const days: Day[] = [];
        for (const m of j.monthly as Record<string, unknown>[]) {
          const month = String(m.month ?? "");
          const [y, mo] = month.split("-").map(Number);
          if (!y || !mo) continue;
          const n = new Date(Date.UTC(y, mo, 0)).getUTCDate();
          const cost = num(m.totalCost ?? m.costUSD ?? m.cost);
          for (let d = 1; d <= n; d++) days.push({ date: `${month}-${String(d).padStart(2, "0")}`, cost: cost / n });
        }
        return { vendor, source: "monthly-json", days };
      }
      if (Array.isArray(j?.sessions)) {
        const byDay = new Map<string, number>();
        for (const s of j.sessions as Record<string, unknown>[]) {
          const date = String(s.lastActivity ?? s.date ?? "").slice(0, 10);
          if (date) byDay.set(date, (byDay.get(date) ?? 0) + num(s.totalCost ?? s.costUSD));
        }
        return { vendor, source: "session-json", days: [...byDay].map(([date, cost]) => ({ date, cost })) };
      }
    } catch {
      /* fall through to table parsing */
    }
  }
  // Table: one row per day with a date and a "$12.34" total as the last money value.
  const days: Day[] = [];
  for (const line of t.split(/\r?\n/)) {
    const date = line.match(/(20\d\d-\d\d-\d\d)/)?.[1];
    if (!date || /total/i.test(line)) continue;
    const money = [...line.matchAll(/\$\s?([\d,]+(?:\.\d+)?)/g)].map((m) => Number(m[1].replace(/,/g, "")));
    if (money.length) days.push({ date, cost: money[money.length - 1] });
  }
  return days.length ? { vendor, source: "table", days } : null;
}

export type LimitHits = "never" | "sometimes" | "often";

export interface Report {
  vendor: Vendor;
  daysCovered: number;
  activeDays: number;
  apiEquivalentMonthly: number;
  peakDay: Day | null;
  current: PlanOption;
  recommended: { kind: "plan"; plan: PlanOption } | { kind: "api"; monthly: number } | { kind: "keep" } | { kind: "upgrade"; plan: PlanOption };
  monthlyWaste: number;
  headline: string;
  reasons: string[];
}

/**
 * Recommendation rules (deliberately conservative; savings are labelled estimates):
 * - Subscriptions give more API-equivalent value than their price. So if your usage at
 *   API prices is below a cheaper tier's price, that tier covers you.
 * - If your usage at API prices is below even the cheapest tier, pay-as-you-go is cheaper.
 * - Hitting limits often means the plan is too small, never too big.
 * - Opened on fewer than 4 days in 30: you may not need it at all.
 */
export function buildReport(usage: ParsedUsage, currentPlanId: string, hits: LimitHits): Report {
  const plans = PLANS[usage.vendor];
  const current = plans.find((p) => p.id === currentPlanId) ?? plans[0];
  const sorted = [...usage.days].sort((a, b) => a.date.localeCompare(b.date));
  const first = sorted[0]?.date;
  const last = sorted[sorted.length - 1]?.date;
  const span = first && last ? Math.round((Date.parse(last) - Date.parse(first)) / 86_400_000) + 1 : 0;
  const daysCovered = Math.max(span, 1);
  const total = sorted.reduce((s, d) => s + d.cost, 0);
  const apiMonthly = (total / daysCovered) * 30;
  const activeDays = sorted.filter((d) => d.cost > 0.01).length;
  const peakDay = sorted.reduce<Day | null>((m, d) => (!m || d.cost > m.cost ? d : m), null);
  const reasons: string[] = [
    `${activeDays} active days in the ${daysCovered} days you pasted.`,
    `At API list prices this usage costs about $${apiMonthly.toFixed(0)} a month.`,
  ];
  const r = (n: number) => Math.round(n);

  let recommended: Report["recommended"] = { kind: "keep" };
  let waste = 0;
  let headline: string;

  const perMonthActive = (activeDays / daysCovered) * 30;
  if (hits === "often") {
    const up = plans.find((p) => p.price > current.price);
    if (up) {
      recommended = { kind: "upgrade", plan: up };
      headline = `You're hitting ${current.name} limits often. ${up.name} stops the cut-offs for $${r(up.price - current.price)} more.`;
      reasons.push("Hitting limits weekly or more means the plan is too small.");
    } else {
      headline = `You're on the biggest ${current.name.split(" ")[0]} plan; spread work across tools to avoid cut-offs.`;
    }
  } else if (daysCovered >= 14 && perMonthActive < 4) {
    recommended = { kind: "api", monthly: apiMonthly };
    waste = Math.max(0, current.price - apiMonthly);
    headline = `You used it on ${activeDays} days. Pay-as-you-go would cost about $${r(apiMonthly)} instead of $${r(current.price)}.`;
    reasons.push("Opened on fewer than 4 days a month.");
  } else if (apiMonthly < plans[0].price && hits === "never") {
    recommended = { kind: "api", monthly: apiMonthly };
    waste = Math.max(0, current.price - apiMonthly);
    headline = waste >= 1 ? `You're wasting about $${r(waste)} a month. Pay-as-you-go would cost about $${r(apiMonthly)}.` : `${current.name} fits how you work.`;
    reasons.push(`Your usage is below the price of the cheapest plan ($${plans[0].price}).`);
  } else {
    const cheaper = plans.filter((p) => p.price < current.price && apiMonthly <= p.price);
    const best = cheaper.sort((a, b) => a.price - b.price)[0];
    if (best && hits === "never") {
      recommended = { kind: "plan", plan: best };
      waste = current.price - best.price;
      headline = `You're wasting about $${r(waste)} a month. ${best.name} covers your usage.`;
      reasons.push(`Your usage at API prices ($${r(apiMonthly)}) is below ${best.name}'s price, and you never hit limits.`);
    } else {
      headline = `${current.name} fits how you work.`;
      reasons.push(hits === "sometimes" ? "You hit limits sometimes, so a smaller plan would cut you off more." : "A cheaper plan wouldn't cover your usage.");
    }
  }

  return { vendor: usage.vendor, daysCovered, activeDays, apiEquivalentMonthly: apiMonthly, peakDay, current, recommended, monthlyWaste: waste, headline, reasons };
}

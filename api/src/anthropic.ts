// Anthropic Admin API cost report, for teams billed through the API rather than
// Claude subscriptions. Needs an Admin key (sk-ant-admin...), which an org admin
// pastes into the dashboard; it is stored encrypted and only sent to Anthropic.

export interface DailyCost {
  date: string;
  usd: number;
  by_description: Record<string, number>;
}

const BASE = "https://api.anthropic.com/v1/organizations/cost_report";

/**
 * Fetch daily costs for [from, to). Amounts come back as decimal strings in the
 * lowest currency unit (cents), so we divide by 100.
 */
export async function costReport(adminKey: string, from: Date, to: Date, fetchImpl: typeof fetch = fetch): Promise<DailyCost[]> {
  const out: DailyCost[] = [];
  let page: string | undefined;
  for (let i = 0; i < 20; i++) {
    const u = new URL(BASE);
    u.searchParams.set("starting_at", from.toISOString());
    u.searchParams.set("ending_at", to.toISOString());
    u.searchParams.set("bucket_width", "1d");
    u.searchParams.append("group_by[]", "description");
    if (page) u.searchParams.set("page", page);
    const r = await fetchImpl(u, {
      headers: { "x-api-key": adminKey, "anthropic-version": "2023-06-01", "user-agent": "ai-usage-controller" },
    });
    if (!r.ok) throw new Error(`Anthropic cost report: HTTP ${r.status} ${await r.text()}`);
    const body = (await r.json()) as {
      data: { starting_at: string; results: { amount: string; description?: string | null }[] }[];
      has_more?: boolean;
      next_page?: string | null;
    };
    for (const bucket of body.data ?? []) {
      const day: DailyCost = { date: bucket.starting_at.slice(0, 10), usd: 0, by_description: {} };
      for (const r of bucket.results ?? []) {
        const usd = Number(r.amount) / 100;
        if (!Number.isFinite(usd)) continue;
        day.usd += usd;
        const k = r.description ?? "other";
        day.by_description[k] = (day.by_description[k] ?? 0) + usd;
      }
      out.push(day);
    }
    if (!body.has_more || !body.next_page) break;
    page = body.next_page;
  }
  return out;
}

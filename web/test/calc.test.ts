import { test } from "node:test";
import assert from "node:assert/strict";
import { buildReport, parseCcusage } from "../lib/calc.ts";

const daily = (costs: number[], start = "2026-10-01", model = "claude-opus-4-5") =>
  JSON.stringify({
    daily: costs.map((c, i) => {
      const d = new Date(Date.parse(start) + i * 86_400_000).toISOString().slice(0, 10);
      return { date: d, inputTokens: 1, outputTokens: 1, totalCost: c, modelsUsed: [model] };
    }),
    totals: { totalCost: costs.reduce((a, b) => a + b, 0) },
  });

test("parses ccusage daily json", () => {
  const u = parseCcusage(daily([1, 2, 3]))!;
  assert.equal(u.source, "daily-json");
  assert.equal(u.vendor, "claude");
  assert.deepEqual(u.days.map((d) => d.cost), [1, 2, 3]);
});

test("parses ccusage monthly json by spreading across days", () => {
  const u = parseCcusage(JSON.stringify({ monthly: [{ month: "2026-11", totalCost: 300 }] }))!;
  assert.equal(u.days.length, 30);
  assert.ok(Math.abs(u.days.reduce((s, d) => s + d.cost, 0) - 300) < 1e-9);
});

test("parses the printed ccusage table and skips the total row", () => {
  const table = `
┌────────────┬──────────────────┬──────────┬──────────┬────────────┐
│ Date       │ Models           │    Input │   Output │ Cost (USD) │
├────────────┼──────────────────┼──────────┼──────────┼────────────┤
│ 2026-11-20 │ - opus-4-5       │    1,234 │   56,789 │     $12.34 │
│ 2026-11-21 │ - sonnet-4-5     │      999 │    1,000 │  $1,002.50 │
├────────────┼──────────────────┼──────────┼──────────┼────────────┤
│ Total      │                  │    2,233 │   57,789 │  $1,014.84 │
└────────────┴──────────────────┴──────────┴──────────┴────────────┘`;
  const u = parseCcusage(table)!;
  assert.equal(u.source, "table");
  assert.deepEqual(u.days, [
    { date: "2026-11-20", cost: 12.34 },
    { date: "2026-11-21", cost: 1002.5 },
  ]);
});

test("detects codex output", () => {
  assert.equal(parseCcusage(daily([1], "2026-10-01", "gpt-5-codex"))!.vendor, "codex");
});

test("rejects junk", () => {
  assert.equal(parseCcusage("hello"), null);
  assert.equal(parseCcusage(""), null);
});

test("light user on Max 20x is told to drop to the cheapest plan that covers them", () => {
  // ~$2/day for 30 days = $60/mo API-equivalent; Max 5x ($100) covers it
  const u = parseCcusage(daily(Array(30).fill(2)))!;
  const r = buildReport(u, "max20", "never");
  assert.equal(r.recommended.kind, "plan");
  assert.equal(r.recommended.kind === "plan" && r.recommended.plan.id, "max5");
  assert.equal(r.monthlyWaste, 100);
  assert.match(r.headline, /wasting about \$100 a month/);
});

test("very light user is pointed at pay-as-you-go", () => {
  const u = parseCcusage(daily(Array(30).fill(0.3)))!; // $9/mo
  const r = buildReport(u, "pro", "never");
  assert.equal(r.recommended.kind, "api");
  assert.equal(Math.round(r.monthlyWaste), 11);
});

test("rarely-used subscription", () => {
  const costs = Array(30).fill(0);
  costs[3] = 40;
  costs[20] = 30;
  const r = buildReport(parseCcusage(daily(costs))!, "max5", "never");
  assert.equal(r.recommended.kind, "api");
  assert.match(r.headline, /on 2 days/);
});

test("heavy user hitting limits is told to upgrade, never downgrade", () => {
  const r = buildReport(parseCcusage(daily(Array(30).fill(30)))!, "pro", "often");
  assert.equal(r.recommended.kind, "upgrade");
  assert.equal(r.monthlyWaste, 0);
});

test("sometimes hitting limits keeps the plan even if usage looks small", () => {
  const r = buildReport(parseCcusage(daily(Array(30).fill(2)))!, "max20", "sometimes");
  assert.equal(r.recommended.kind, "keep");
  assert.equal(r.monthlyWaste, 0);
});

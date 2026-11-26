import { test } from "node:test";
import assert from "node:assert/strict";
import { createHmac } from "node:crypto";
import { createApp } from "../src/app.ts";
import { openDb } from "../src/db.ts";

function setup(extra: Record<string, unknown> = {}) {
  const db = openDb(":memory:");
  const app = createApp(db, { SECRET_KEY: "test-secret", ADMIN_SECRET: "admin", LEMONSQUEEZY_WEBHOOK_SECRET: "whsec", ...extra });
  const req = (method: string, path: string, body?: unknown, token?: string, headers: Record<string, string> = {}) =>
    app.request(path, {
      method,
      headers: { "content-type": "application/json", ...(token ? { authorization: `Bearer ${token}` } : {}), ...headers },
      body: body === undefined ? undefined : typeof body === "string" ? body : JSON.stringify(body),
    });
  return { db, app, req };
}

const ev = (key: string, ts: number, cost: number, tool = "claude_code", model = "claude-opus-4-5") => ({
  key, tool, ts, session: "s1", model, input: 100, output: 50, cache_write: 0, cache_read: 1000, cost,
});

test("serves price and plan tables, owner can override them", async () => {
  const { req } = setup();
  const prices = await (await req("GET", "/v1/prices")).json();
  assert.ok(prices.models.length > 10);
  const plans = await (await req("GET", "/v1/plans")).json();
  assert.equal(plans.tools.claude_code[0].id, "pro");

  assert.equal((await req("PUT", "/v1/admin/tables/prices", { version: "x", models: [] })).status, 403);
  const ok = await req("PUT", "/v1/admin/tables/prices", { version: "2027-01-01", models: [] }, undefined, { "x-admin-secret": "admin" });
  assert.equal(ok.status, 200);
  assert.equal((await (await req("GET", "/v1/prices")).json()).version, "2027-01-01");
  const bad = await req("PUT", "/v1/admin/tables/plans", { version: "x" }, undefined, { "x-admin-secret": "admin" });
  assert.equal(bad.status, 400);
});

test("waitlist validates and dedups emails", async () => {
  const { req } = setup();
  assert.equal((await req("POST", "/v1/waitlist", { email: "nope" })).status, 400);
  const a = await (await req("POST", "/v1/waitlist", { email: "A@x.dev", source: "calculator", report: { waste: 100 } })).json();
  const b = await (await req("POST", "/v1/waitlist", { email: "a@x.dev" })).json();
  assert.equal(a.position, 1);
  assert.equal(b.position, 1);
});

test("team flow: create, join, sync counts, dashboard shows idle seats and waste", async () => {
  const { req } = setup();
  const org = await (await req("POST", "/v1/orgs", { name: "Acme", email: "cto@acme.dev" })).json();
  assert.match(org.invite_code, /^[A-Z2-9]{4}-[A-Z2-9]{4}$/);

  assert.equal((await req("POST", "/v1/join", { invite_code: "NOPE-NOPE", email: "a@acme.dev" })).status, 404);
  const alice = await (await req("POST", "/v1/join", { invite_code: org.invite_code.toLowerCase(), email: "alice@acme.dev", device_name: "mbp" })).json();
  const bob = await (await req("POST", "/v1/join", { invite_code: org.invite_code, email: "bob@acme.dev", device_name: "win" })).json();
  const alice2 = await (await req("POST", "/v1/join", { invite_code: org.invite_code, email: "alice@acme.dev", device_name: "imac" })).json();
  assert.equal(alice.member_id, alice2.member_id, "same person, second device");

  const now = Date.now();
  const meta = { plans: { claude_code: "max20", codex: "plus", gemini: null }, audit: [{ tool: "claude_code", action: "downgrade", current_plan: "Claude Max 20x", monthly_saving_usd: 100 }] };
  const r = await req("POST", "/v1/sync", { events: [ev("k1", now - 60_000, 1.5), ev("k2", now - 30_000, 2.5), { key: 1, tool: "evil" }], meta }, alice.device_token);
  assert.deepEqual(await r.json(), { accepted: 2 });
  // re-sync same key updates, doesn't duplicate
  await req("POST", "/v1/sync", { events: [ev("k2", now - 30_000, 3.0)] }, alice.device_token);
  await req("POST", "/v1/sync", { events: [ev("k1", now - 10_000, 0.5)] }, alice2.device_token);
  // bob: subscribed to Max 5x but hasn't used anything in 10 days
  await req("POST", "/v1/sync", { events: [ev("b1", now - 10 * 86_400_000, 4.0)], meta: { plans: { claude_code: "max5" }, audit: [] } }, bob.device_token);

  assert.equal((await req("POST", "/v1/sync", { events: [] }, "bad-token")).status, 401);

  const summary = await (await req("GET", "/v1/sync/summary", undefined, alice.device_token)).json();
  assert.equal(summary.devices, 2);
  assert.ok(Math.abs(summary.month_cost_all_devices - 5.0) < 1e-9, String(summary.month_cost_all_devices));

  assert.equal((await req("GET", "/v1/admin/dashboard", undefined, alice.device_token)).status, 401, "device token is not an admin token");
  const dash = await (await req("GET", "/v1/admin/dashboard", undefined, org.admin_token)).json();
  assert.equal(dash.totals.members, 2);
  assert.equal(dash.totals.idle_seats, 1);
  const a = dash.seats.find((s: { email: string }) => s.email === "alice@acme.dev");
  const b = dash.seats.find((s: { email: string }) => s.email === "bob@acme.dev");
  assert.equal(a.devices, 2);
  assert.ok(Math.abs(a.api_equivalent_30d - 5.0) < 1e-9, `alice cost ${a.api_equivalent_30d}`);
  assert.equal(a.subscription_cost, 220);
  // alice: Codex Plus unused in 30 days ($20) + downgrade advice ($100)
  assert.equal(a.monthly_waste, 120);
  assert.equal(b.idle, true);
  assert.equal(b.monthly_waste, 100, "idle seat wastes its whole subscription");
  assert.equal(dash.totals.monthly_waste, 220);
  assert.equal(dash.totals.our_price, 87, "3-seat minimum at $29");

  const rot = await (await req("POST", "/v1/admin/invite/rotate", undefined, org.admin_token)).json();
  assert.notEqual(rot.invite_code, org.invite_code);
  assert.equal((await req("POST", "/v1/join", { invite_code: org.invite_code, email: "eve@x.dev" })).status, 404);

  const del = await (await req("DELETE", `/v1/admin/members/${b.member_id}`, undefined, org.admin_token)).json();
  assert.equal(del.removed, 1);
  assert.equal((await req("POST", "/v1/sync", { events: [] }, bob.device_token)).status, 401, "removed member's device is revoked");
});

test("personal sync groups map every device to the owner", async () => {
  const { req } = setup();
  const org = await (await req("POST", "/v1/orgs", { email: "me@x.dev", kind: "personal" })).json();
  const d1 = await (await req("POST", "/v1/join", { invite_code: org.invite_code, device_name: "laptop" })).json();
  const d2 = await (await req("POST", "/v1/join", { invite_code: org.invite_code, device_name: "desktop" })).json();
  assert.equal(d1.member_id, d2.member_id);
});

test("lemon squeezy webhook: signature required, updates seats once", async () => {
  const { req, db } = setup();
  const org = await (await req("POST", "/v1/orgs", { email: "cto@acme.dev" })).json();
  const body = JSON.stringify({
    meta: { event_name: "subscription_created", custom_data: { org_id: org.org_id } },
    data: { id: "1", attributes: { status: "active", first_subscription_item: { quantity: 5 } } },
  });
  assert.equal((await req("POST", "/v1/webhooks/lemonsqueezy", body, undefined, { "x-signature": "bad" })).status, 401);
  const sig = createHmac("sha256", "whsec").update(body).digest("hex");
  const ok = await req("POST", "/v1/webhooks/lemonsqueezy", body, undefined, { "x-signature": sig });
  assert.equal(ok.status, 200);
  const dup = await (await req("POST", "/v1/webhooks/lemonsqueezy", body, undefined, { "x-signature": sig })).json();
  assert.equal(dup.duplicate, true);
  const row = db.prepare("SELECT seats_paid, subscription_status FROM orgs WHERE id = ?").get(org.org_id) as { seats_paid: number; subscription_status: string };
  assert.deepEqual({ ...row }, { seats_paid: 5, subscription_status: "active" });
});

test("anthropic admin key is stored encrypted and used for the cost report", async () => {
  const calls: string[] = [];
  const fakeFetch = (async (url: URL, init: RequestInit) => {
    calls.push(String(url));
    assert.equal((init.headers as Record<string, string>)["x-api-key"], "sk-ant-admin01-abc");
    return new Response(
      JSON.stringify({
        data: [
          { starting_at: "2026-11-24T00:00:00Z", results: [{ amount: "1250.5", description: "Claude Opus 5.5 usage" }, { amount: "300", description: null }] },
          { starting_at: "2026-11-25T00:00:00Z", results: [{ amount: "99", description: "Claude Opus 5.5 usage" }] },
        ],
        has_more: false,
      }),
      { status: 200 },
    );
  }) as unknown as typeof fetch;
  const { req, db } = setup({ fetch: fakeFetch });
  const org = await (await req("POST", "/v1/orgs", { email: "cto@acme.dev" })).json();
  assert.equal((await req("PUT", "/v1/admin/anthropic-key", { admin_key: "sk-ant-api03-nope" }, org.admin_token)).status, 400);
  assert.equal((await req("PUT", "/v1/admin/anthropic-key", { admin_key: "sk-ant-admin01-abc" }, org.admin_token)).status, 200);
  const stored = (db.prepare("SELECT anthropic_admin_key_enc AS k FROM orgs").get() as { k: string }).k;
  assert.ok(!stored.includes("sk-ant"), "not stored in plain text");
  const spend = await (await req("GET", "/v1/admin/api-spend", undefined, org.admin_token)).json();
  assert.equal(spend.days.length, 2);
  assert.ok(Math.abs(spend.total_usd - 16.495) < 1e-9, String(spend.total_usd));
  assert.ok(calls[0].includes("bucket_width=1d"));
});

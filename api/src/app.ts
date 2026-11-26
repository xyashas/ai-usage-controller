import { Hono, type Context } from "hono";
import { cors } from "hono/cors";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { DB } from "./db.js";
import { decrypt, encrypt, hash, id, inviteCode, token, verifyLemonSqueezy } from "./crypto.js";
import { costReport } from "./anthropic.js";

export interface Env {
  /** Encrypts secrets at rest. Required in production. */
  SECRET_KEY: string;
  /** Lets the owner replace the price/plan tables without a release. */
  ADMIN_SECRET?: string;
  LEMONSQUEEZY_WEBHOOK_SECRET?: string;
  /** Origins allowed to call the API from a browser (the dashboard and landing page). */
  WEB_ORIGINS?: string[];
  TABLES_DIR?: string;
  fetch?: typeof fetch;
}

type Plan = { id: string; name: string; price: number; capacity: number; metered?: boolean };
type PlanTable = { version: string; tools: Record<string, Plan[]> };

const here = dirname(fileURLToPath(import.meta.url));
const DAY = 86_400_000;
const SEAT_PRICE = 29;
const MIN_SEATS = 3;
const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

interface SyncEvent {
  key: string;
  tool: string;
  ts: number;
  session: string;
  model?: string | null;
  input: number;
  output: number;
  cache_write: number;
  cache_read: number;
  cost: number;
}

function bearer(c: Context): string | null {
  const h = c.req.header("authorization") ?? "";
  return h.startsWith("Bearer ") ? h.slice(7).trim() : null;
}

const n = (v: unknown) => (typeof v === "number" && Number.isFinite(v) && v >= 0 ? v : 0);

export function createApp(db: DB, env: Env) {
  const app = new Hono();
  const tablesDir = env.TABLES_DIR ?? resolve(here, "../../core/data");
  const fileTable = (name: "prices" | "plans") => readFileSync(resolve(tablesDir, `${name}.json`), "utf8");
  const table = (name: "prices" | "plans"): string => {
    const row = db.prepare("SELECT body FROM tables_override WHERE name = ?").get(name) as { body: string } | undefined;
    return row?.body ?? fileTable(name);
  };
  const plans = (): PlanTable => JSON.parse(table("plans"));

  app.use("/v1/*", cors({ origin: env.WEB_ORIGINS ?? ["http://localhost:3000"], allowHeaders: ["authorization", "content-type"], allowMethods: ["GET", "POST", "PUT", "DELETE"] }));
  app.onError((e, c) => {
    console.error(e);
    return c.json({ error: "internal error" }, 500);
  });

  app.get("/health", (c) => c.json({ ok: true }));

  // ---------- price & plan tables (the app pulls these every 6 hours) ----------
  app.get("/v1/prices", (c) => c.body(table("prices"), 200, { "content-type": "application/json", "cache-control": "public, max-age=3600" }));
  app.get("/v1/plans", (c) => c.body(table("plans"), 200, { "content-type": "application/json", "cache-control": "public, max-age=3600" }));
  app.put("/v1/admin/tables/:name", async (c) => {
    if (!env.ADMIN_SECRET || c.req.header("x-admin-secret") !== env.ADMIN_SECRET) return c.json({ error: "forbidden" }, 403);
    const name = c.req.param("name");
    if (name !== "prices" && name !== "plans") return c.json({ error: "unknown table" }, 404);
    const body = await c.req.text();
    let parsed: { version?: unknown; models?: unknown; tools?: unknown };
    try {
      parsed = JSON.parse(body);
    } catch {
      return c.json({ error: "invalid JSON" }, 400);
    }
    const ok = typeof parsed.version === "string" && (name === "prices" ? Array.isArray(parsed.models) : typeof parsed.tools === "object");
    if (!ok) return c.json({ error: `not a ${name} table` }, 400);
    db.prepare("INSERT OR REPLACE INTO tables_override (name, body, updated_at) VALUES (?, ?, ?)").run(name, body, Date.now());
    return c.json({ ok: true, version: parsed.version });
  });

  // ---------- week-0 validation: waitlist ----------
  app.post("/v1/waitlist", async (c) => {
    const b = (await c.req.json().catch(() => ({}))) as { email?: string; source?: string; report?: unknown };
    const email = (b.email ?? "").trim().toLowerCase();
    if (!EMAIL.test(email) || email.length > 254) return c.json({ error: "enter a valid email" }, 400);
    const report = b.report ? JSON.stringify(b.report).slice(0, 4000) : null;
    db.prepare("INSERT INTO waitlist (email, source, report, created_at) VALUES (?, ?, ?, ?) ON CONFLICT(email) DO UPDATE SET report = COALESCE(excluded.report, report)").run(
      email,
      (b.source ?? "").slice(0, 64),
      report,
      Date.now(),
    );
    const count = (db.prepare("SELECT COUNT(*) AS n FROM waitlist").get() as { n: number }).n;
    return c.json({ ok: true, position: count });
  });

  // ---------- orgs: a team, or a personal sync group for one person's devices ----------
  app.post("/v1/orgs", async (c) => {
    const b = (await c.req.json().catch(() => ({}))) as { name?: string; email?: string; kind?: string };
    const email = (b.email ?? "").trim().toLowerCase();
    const kind = b.kind === "personal" ? "personal" : "team";
    if (!EMAIL.test(email)) return c.json({ error: "enter a valid email" }, 400);
    const orgId = id();
    const admin = token();
    const code = inviteCode();
    db.prepare("INSERT INTO orgs (id, name, kind, owner_email, admin_token_hash, invite_code, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)").run(
      orgId,
      (b.name ?? (kind === "team" ? "My team" : "My devices")).slice(0, 80),
      kind,
      email,
      hash(admin),
      code,
      Date.now(),
    );
    return c.json({ org_id: orgId, admin_token: admin, invite_code: code });
  });

  app.post("/v1/join", async (c) => {
    const b = (await c.req.json().catch(() => ({}))) as { invite_code?: string; email?: string; device_name?: string };
    const code = (b.invite_code ?? "").trim().toUpperCase();
    const email = (b.email ?? "").trim().toLowerCase();
    const org = db.prepare("SELECT id, kind, owner_email FROM orgs WHERE invite_code = ?").get(code) as { id: string; kind: string; owner_email: string } | undefined;
    if (!org) return c.json({ error: "invite code not found" }, 404);
    const memberEmail = org.kind === "personal" ? org.owner_email : email;
    if (!EMAIL.test(memberEmail)) return c.json({ error: "enter a valid email" }, 400);
    let member = db.prepare("SELECT id FROM members WHERE org_id = ? AND email = ?").get(org.id, memberEmail) as { id: string } | undefined;
    if (!member) {
      member = { id: id() };
      db.prepare("INSERT INTO members (id, org_id, email, created_at) VALUES (?, ?, ?, ?)").run(member.id, org.id, memberEmail, Date.now());
    }
    const deviceId = id();
    const deviceToken = token();
    db.prepare("INSERT INTO devices (id, member_id, name, token_hash, created_at) VALUES (?, ?, ?, ?, ?)").run(
      deviceId,
      member.id,
      (b.device_name ?? "device").slice(0, 80),
      hash(deviceToken),
      Date.now(),
    );
    return c.json({ device_id: deviceId, device_token: deviceToken, member_id: member.id, org_kind: org.kind });
  });

  const device = (c: Context) => {
    const t = bearer(c);
    if (!t) return null;
    return db
      .prepare("SELECT d.id AS device_id, d.member_id, m.org_id FROM devices d JOIN members m ON m.id = d.member_id WHERE d.token_hash = ?")
      .get(hash(t)) as { device_id: string; member_id: string; org_id: string } | undefined;
  };

  // ---------- sync: counts only ----------
  app.post("/v1/sync", async (c) => {
    const d = device(c);
    if (!d) return c.json({ error: "unauthorized" }, 401);
    const b = (await c.req.json().catch(() => ({}))) as { events?: SyncEvent[]; meta?: unknown };
    const events = Array.isArray(b.events) ? b.events.slice(0, 1000) : [];
    const ins = db.prepare(
      `INSERT INTO events (device_id, key, tool, ts, session, model, input, output, cache_write, cache_read, cost)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT(device_id, key) DO UPDATE SET input = excluded.input, output = excluded.output,
         cache_write = excluded.cache_write, cache_read = excluded.cache_read, cost = excluded.cost`,
    );
    let accepted = 0;
    db.exec("BEGIN");
    try {
      for (const e of events) {
        if (typeof e?.key !== "string" || !["claude_code", "codex", "gemini"].includes(e.tool) || !Number.isFinite(e.ts)) continue;
        ins.run(d.device_id, e.key.slice(0, 200), e.tool, Math.trunc(e.ts), String(e.session ?? "").slice(0, 100), e.model ? String(e.model).slice(0, 80) : null, n(e.input), n(e.output), n(e.cache_write), n(e.cache_read), n(e.cost));
        accepted++;
      }
      db.exec("COMMIT");
    } catch (err) {
      db.exec("ROLLBACK");
      throw err;
    }
    const meta = b.meta && typeof b.meta === "object" ? JSON.stringify(b.meta).slice(0, 8000) : null;
    db.prepare("UPDATE devices SET last_seen = ?, meta = COALESCE(?, meta) WHERE id = ?").run(Date.now(), meta, d.device_id);
    return c.json({ accepted });
  });

  app.get("/v1/sync/summary", (c) => {
    const d = device(c);
    if (!d) return c.json({ error: "unauthorized" }, 401);
    const now = Date.now();
    const sum = (from: number) =>
      (db.prepare("SELECT COALESCE(SUM(e.cost), 0) AS s FROM events e JOIN devices d ON d.id = e.device_id WHERE d.member_id = ? AND e.ts >= ?").get(d.member_id, from) as { s: number }).s;
    const devices = (db.prepare("SELECT COUNT(*) AS n FROM devices WHERE member_id = ?").get(d.member_id) as { n: number }).n;
    const start = new Date(now);
    start.setUTCHours(0, 0, 0, 0);
    const month = new Date(Date.UTC(start.getUTCFullYear(), start.getUTCMonth(), 1)).getTime();
    return c.json({ today_cost_all_devices: sum(start.getTime()), month_cost_all_devices: sum(month), devices });
  });

  // ---------- team dashboard ----------
  const adminOrg = (c: Context) => {
    const t = bearer(c);
    if (!t) return null;
    return db.prepare("SELECT * FROM orgs WHERE admin_token_hash = ?").get(hash(t)) as
      | { id: string; name: string; kind: string; invite_code: string; seats_paid: number; subscription_status: string; anthropic_admin_key_enc: string | null; created_at: number }
      | undefined;
  };

  app.get("/v1/admin/dashboard", (c) => {
    const org = adminOrg(c);
    if (!org) return c.json({ error: "unauthorized" }, 401);
    const now = Date.now();
    const table = plans();
    const price = (tool: string, planId: string | null | undefined) => {
      const p = table.tools[tool]?.find((x) => x.id === planId);
      return p && !p.metered ? p.price : 0;
    };
    const planName = (tool: string, planId: string | null | undefined) => table.tools[tool]?.find((x) => x.id === planId)?.name ?? null;

    const members = db.prepare("SELECT id, email, created_at FROM members WHERE org_id = ? ORDER BY email").all(org.id) as { id: string; email: string; created_at: number }[];
    const seats = members.map((m) => {
      const devs = db.prepare("SELECT name, meta, last_seen FROM devices WHERE member_id = ?").all(m.id) as { name: string; meta: string | null; last_seen: number | null }[];
      const stats = db
        .prepare(
          `SELECT e.tool, COUNT(*) AS requests, SUM(e.cost) AS cost, MAX(e.ts) AS last_ts,
                  SUM(CASE WHEN e.ts >= ? THEN e.cost ELSE 0 END) AS cost_7d
           FROM events e JOIN devices d ON d.id = e.device_id
           WHERE d.member_id = ? AND e.ts >= ? GROUP BY e.tool`,
        )
        .all(now - 7 * DAY, m.id, now - 30 * DAY) as { tool: string; requests: number; cost: number; last_ts: number; cost_7d: number }[];
      // Latest plan choices / audit across this person's devices.
      let planIds: Record<string, string | null> = {};
      let audit: { tool: string; action: string; monthly_saving_usd: number; current_plan: string }[] = [];
      const latest = devs.filter((d) => d.meta).sort((a, b) => (b.last_seen ?? 0) - (a.last_seen ?? 0))[0];
      if (latest?.meta) {
        try {
          const meta = JSON.parse(latest.meta);
          planIds = meta.plans ?? {};
          audit = Array.isArray(meta.audit) ? meta.audit : [];
        } catch {
          /* ignore malformed meta */
        }
      }
      const lastActive = stats.reduce((mx, s) => Math.max(mx, s.last_ts ?? 0), 0) || null;
      const idle = !lastActive || now - lastActive > 7 * DAY;
      const subscriptions = Object.entries(planIds)
        .filter(([, p]) => p)
        .map(([tool, p]) => ({ tool, plan_id: p, plan: planName(tool, p), price: price(tool, p) }))
        .filter((s) => s.price > 0);
      const subscriptionCost = subscriptions.reduce((s, x) => s + x.price, 0);
      const unusedTools = subscriptions.filter((s) => !stats.some((x) => x.tool === s.tool && now - x.last_ts <= 30 * DAY));
      const auditSavings = audit.filter((a) => a.action === "downgrade" || a.action === "cancel").reduce((s, a) => s + n(a.monthly_saving_usd), 0);
      const waste = idle ? subscriptionCost : unusedTools.reduce((s, x) => s + x.price, 0) + auditSavings;
      return {
        member_id: m.id,
        email: m.email,
        devices: devs.length,
        last_seen: devs.reduce((mx, d) => Math.max(mx, d.last_seen ?? 0), 0) || null,
        last_active: lastActive,
        idle,
        api_equivalent_30d: stats.reduce((s, x) => s + x.cost, 0),
        api_equivalent_7d: stats.reduce((s, x) => s + x.cost_7d, 0),
        requests_30d: stats.reduce((s, x) => s + x.requests, 0),
        tools: stats.map((s) => ({ tool: s.tool, requests: s.requests, api_equivalent: s.cost, last_ts: s.last_ts })),
        subscriptions,
        subscription_cost: subscriptionCost,
        recommendations: audit.filter((a) => a.action !== "keep" && a.action !== "not_enough_data"),
        monthly_waste: waste,
      };
    });

    const byModel = db
      .prepare(
        `SELECT COALESCE(e.model, 'unknown') AS model, SUM(e.cost) AS cost, COUNT(*) AS requests
         FROM events e JOIN devices d ON d.id = e.device_id JOIN members m ON m.id = d.member_id
         WHERE m.org_id = ? AND e.ts >= ? GROUP BY 1 ORDER BY cost DESC LIMIT 8`,
      )
      .all(org.id, now - 30 * DAY);
    const daily = db
      .prepare(
        `SELECT (e.ts / 86400000) * 86400000 AS day, SUM(e.cost) AS cost
         FROM events e JOIN devices d ON d.id = e.device_id JOIN members m ON m.id = d.member_id
         WHERE m.org_id = ? AND e.ts >= ? GROUP BY 1 ORDER BY 1`,
      )
      .all(org.id, now - 30 * DAY);

    const seatCount = Math.max(MIN_SEATS, members.length);
    return c.json({
      org: { id: org.id, name: org.name, kind: org.kind, invite_code: org.invite_code, seats_paid: org.seats_paid, subscription_status: org.subscription_status, has_anthropic_key: !!org.anthropic_admin_key_enc },
      totals: {
        members: members.length,
        idle_seats: seats.filter((s) => s.idle).length,
        subscription_cost: seats.reduce((s, x) => s + x.subscription_cost, 0),
        monthly_waste: seats.reduce((s, x) => s + x.monthly_waste, 0),
        api_equivalent_30d: seats.reduce((s, x) => s + x.api_equivalent_30d, 0),
        our_price: seatCount * SEAT_PRICE,
      },
      seats,
      by_model: byModel,
      daily,
      generated_at: now,
    });
  });

  app.post("/v1/admin/invite/rotate", (c) => {
    const org = adminOrg(c);
    if (!org) return c.json({ error: "unauthorized" }, 401);
    const code = inviteCode();
    db.prepare("UPDATE orgs SET invite_code = ? WHERE id = ?").run(code, org.id);
    return c.json({ invite_code: code });
  });

  app.delete("/v1/admin/members/:id", (c) => {
    const org = adminOrg(c);
    if (!org) return c.json({ error: "unauthorized" }, 401);
    const r = db.prepare("DELETE FROM members WHERE id = ? AND org_id = ?").run(c.req.param("id"), org.id);
    return c.json({ removed: r.changes });
  });

  // ---------- Anthropic Admin API (API-billed teams) ----------
  app.put("/v1/admin/anthropic-key", async (c) => {
    const org = adminOrg(c);
    if (!org) return c.json({ error: "unauthorized" }, 401);
    const b = (await c.req.json().catch(() => ({}))) as { admin_key?: string };
    const key = (b.admin_key ?? "").trim();
    if (key === "") {
      db.prepare("UPDATE orgs SET anthropic_admin_key_enc = NULL WHERE id = ?").run(org.id);
      return c.json({ ok: true, removed: true });
    }
    if (!key.startsWith("sk-ant-admin")) return c.json({ error: "that isn't an Anthropic Admin key (sk-ant-admin…)" }, 400);
    db.prepare("UPDATE orgs SET anthropic_admin_key_enc = ? WHERE id = ?").run(encrypt(key, env.SECRET_KEY), org.id);
    return c.json({ ok: true });
  });

  app.get("/v1/admin/api-spend", async (c) => {
    const org = adminOrg(c);
    if (!org) return c.json({ error: "unauthorized" }, 401);
    if (!org.anthropic_admin_key_enc) return c.json({ error: "no Anthropic admin key saved" }, 404);
    const key = decrypt(org.anthropic_admin_key_enc, env.SECRET_KEY);
    const to = new Date();
    const from = new Date(to.getTime() - 30 * DAY);
    try {
      const days = await costReport(key, from, to, env.fetch ?? fetch);
      return c.json({ days, total_usd: days.reduce((s, d) => s + d.usd, 0) });
    } catch (e) {
      return c.json({ error: String((e as Error).message) }, 502);
    }
  });

  // ---------- payments: Lemon Squeezy webhooks ----------
  app.post("/v1/webhooks/lemonsqueezy", async (c) => {
    const raw = await c.req.text();
    if (!env.LEMONSQUEEZY_WEBHOOK_SECRET || !verifyLemonSqueezy(raw, c.req.header("x-signature"), env.LEMONSQUEEZY_WEBHOOK_SECRET)) {
      return c.json({ error: "bad signature" }, 401);
    }
    const body = JSON.parse(raw) as {
      meta?: { event_name?: string; custom_data?: { org_id?: string } };
      data?: { id?: string; attributes?: { status?: string; first_subscription_item?: { quantity?: number } } };
    };
    const event = body.meta?.event_name ?? "unknown";
    const dedupe = hash(raw);
    const seen = db.prepare("SELECT 1 FROM webhook_log WHERE id = ?").get(dedupe);
    if (seen) return c.json({ ok: true, duplicate: true });
    db.prepare("INSERT INTO webhook_log (id, event, received_at) VALUES (?, ?, ?)").run(dedupe, event, Date.now());

    const orgId = body.meta?.custom_data?.org_id;
    if (orgId && event.startsWith("subscription_")) {
      const status = body.data?.attributes?.status ?? "active";
      const qty = body.data?.attributes?.first_subscription_item?.quantity;
      db.prepare("UPDATE orgs SET subscription_status = ?, seats_paid = COALESCE(?, seats_paid) WHERE id = ?").run(status, typeof qty === "number" ? qty : null, orgId);
    }
    return c.json({ ok: true });
  });

  return app;
}

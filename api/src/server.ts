import { serve } from "@hono/node-server";
import { createApp } from "./app.js";
import { openDb } from "./db.js";

const SECRET_KEY = process.env.SECRET_KEY;
if (!SECRET_KEY && process.env.NODE_ENV === "production") {
  console.error("SECRET_KEY is required in production");
  process.exit(1);
}

const db = openDb(process.env.DATABASE_PATH ?? "aiuc.db");
const app = createApp(db, {
  SECRET_KEY: SECRET_KEY ?? "dev-only-secret",
  ADMIN_SECRET: process.env.ADMIN_SECRET,
  LEMONSQUEEZY_WEBHOOK_SECRET: process.env.LEMONSQUEEZY_WEBHOOK_SECRET,
  WEB_ORIGINS: process.env.WEB_ORIGINS?.split(",").map((s) => s.trim()).filter(Boolean),
  TABLES_DIR: process.env.TABLES_DIR,
});

const port = Number(process.env.PORT ?? 8787);
serve({ fetch: app.fetch, port }, () => console.log(`aiuc api listening on :${port}`));

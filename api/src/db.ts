// SQLite via Node's built-in driver: one file, no service to run. The schema is
// plain SQL so it moves to Postgres (e.g. Supabase) without changes to the API.
import { DatabaseSync } from "node:sqlite";

export type DB = DatabaseSync;

const SCHEMA = `
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS orgs (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('team', 'personal')),
  owner_email TEXT NOT NULL,
  admin_token_hash TEXT NOT NULL,
  invite_code TEXT NOT NULL UNIQUE,
  seats_paid INTEGER NOT NULL DEFAULT 0,
  subscription_status TEXT NOT NULL DEFAULT 'trial',
  anthropic_admin_key_enc TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS members (
  id TEXT PRIMARY KEY,
  org_id TEXT NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
  email TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  UNIQUE (org_id, email)
);

CREATE TABLE IF NOT EXISTS devices (
  id TEXT PRIMARY KEY,
  member_id TEXT NOT NULL REFERENCES members(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  meta TEXT,
  last_seen INTEGER,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS events (
  device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  tool TEXT NOT NULL,
  ts INTEGER NOT NULL,
  session TEXT NOT NULL,
  model TEXT,
  input INTEGER NOT NULL,
  output INTEGER NOT NULL,
  cache_write INTEGER NOT NULL,
  cache_read INTEGER NOT NULL,
  cost REAL NOT NULL,
  PRIMARY KEY (device_id, key)
);
CREATE INDEX IF NOT EXISTS events_ts ON events(ts);

CREATE TABLE IF NOT EXISTS waitlist (
  email TEXT PRIMARY KEY,
  source TEXT,
  report TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS tables_override (
  name TEXT PRIMARY KEY,
  body TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS webhook_log (
  id TEXT PRIMARY KEY,
  event TEXT NOT NULL,
  received_at INTEGER NOT NULL
);
`;

export function openDb(path: string): DB {
  const db = new DatabaseSync(path);
  db.exec(SCHEMA);
  return db;
}

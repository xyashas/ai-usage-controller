use crate::model::{EventKind, GaugeSample, Tool, UsageEvent};
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub struct Store {
    pub conn: Connection,
}

#[derive(Clone, Debug, Default)]
pub struct Cursor {
    pub offset: u64,
    pub size: u64,
    pub mtime: i64,
    pub ctx: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct SavingsEntry {
    pub ts: DateTime<Utc>,
    pub kind: String,
    pub amount_usd: f64,
    pub note: String,
}

fn ms(t: DateTime<Utc>) -> i64 {
    t.timestamp_millis()
}
fn from_ms(v: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(v).unwrap_or_default()
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS events (
  dedup_key   TEXT PRIMARY KEY,
  tool        TEXT NOT NULL,
  kind        TEXT NOT NULL,
  ts          INTEGER NOT NULL,
  session_id  TEXT NOT NULL,
  project     TEXT,
  model       TEXT,
  input       INTEGER NOT NULL DEFAULT 0,
  output      INTEGER NOT NULL DEFAULT 0,
  cache_write INTEGER NOT NULL DEFAULT 0,
  cache_read  INTEGER NOT NULL DEFAULT 0,
  cost        REAL NOT NULL DEFAULT 0,
  tool_sig    TEXT,
  synced      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS events_tool_ts ON events(tool, ts);
CREATE INDEX IF NOT EXISTS events_session ON events(session_id, ts);
CREATE INDEX IF NOT EXISTS events_synced ON events(synced);
CREATE TABLE IF NOT EXISTS gauges (
  tool TEXT NOT NULL, window TEXT NOT NULL, ts INTEGER NOT NULL,
  used_pct REAL NOT NULL, resets_at INTEGER, window_minutes INTEGER, source TEXT NOT NULL,
  PRIMARY KEY (tool, window, ts)
);
CREATE TABLE IF NOT EXISTS cursors (
  path TEXT PRIMARY KEY, offset INTEGER NOT NULL, size INTEGER NOT NULL, mtime INTEGER NOT NULL, ctx TEXT
);
CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- Tasks started from the dispatcher: the local outcomes ledger. Task text is
-- kept on this machine only (never synced), like the handoff file.
CREATE TABLE IF NOT EXISTS tasks (
  id INTEGER PRIMARY KEY AUTOINCREMENT, created_at INTEGER NOT NULL, repo TEXT NOT NULL,
  cwd TEXT NOT NULL, branch TEXT, tool TEXT NOT NULL, text TEXT NOT NULL,
  dismissed INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS savings (
  id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, kind TEXT NOT NULL, amount REAL NOT NULL, note TEXT NOT NULL
);
"#;

/// Columns added after a table first shipped. Errors mean "already there".
fn migrate(conn: &Connection) {
    for col in ["dest TEXT", "queued_until INTEGER", "worktree INTEGER NOT NULL DEFAULT 0", "verify TEXT", "verify_at INTEGER", "verify_secs REAL"] {
        let _ = conn.execute(&format!("ALTER TABLE tasks ADD COLUMN {col}"), []);
    }
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn);
        Ok(Store { conn })
    }

    pub fn open_in_memory() -> Result<Store> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn);
        Ok(Store { conn })
    }

    /// Insert or merge an event. Claude writes the same response on several lines;
    /// on conflict we keep the largest token counts and the first tool signature.
    pub fn upsert_events(&mut self, events: &[UsageEvent]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut n = 0;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO events (dedup_key, tool, kind, ts, session_id, project, model, input, output, cache_write, cache_read, cost, tool_sig)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(dedup_key) DO UPDATE SET
                   input = MAX(input, excluded.input),
                   output = MAX(output, excluded.output),
                   cache_write = MAX(cache_write, excluded.cache_write),
                   cache_read = MAX(cache_read, excluded.cache_read),
                   cost = MAX(cost, excluded.cost),
                   tool_sig = CASE WHEN tool_sig IS NULL THEN excluded.tool_sig
                                   WHEN excluded.tool_sig IS NULL OR instr(tool_sig, excluded.tool_sig) > 0 THEN tool_sig
                                   ELSE tool_sig || '|' || excluded.tool_sig END,
                   synced = CASE WHEN output < excluded.output OR input < excluded.input THEN 0 ELSE synced END",
            )?;
            for e in events {
                n += stmt.execute(params![
                    e.dedup_key,
                    e.tool.as_str(),
                    e.kind.as_str(),
                    ms(e.ts),
                    e.session_id,
                    e.project,
                    e.model,
                    e.input_tokens as i64,
                    e.output_tokens as i64,
                    e.cache_write_tokens as i64,
                    e.cache_read_tokens as i64,
                    e.cost_usd,
                    e.tool_sig,
                ])?;
            }
        }
        tx.commit()?;
        Ok(n)
    }

    pub fn insert_gauges(&mut self, gauges: &[GaugeSample]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO gauges (tool, window, ts, used_pct, resets_at, window_minutes, source) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            )?;
            for g in gauges {
                stmt.execute(params![
                    g.tool.as_str(),
                    g.window,
                    ms(g.ts),
                    g.used_pct,
                    g.resets_at.map(ms),
                    g.window_minutes,
                    g.source
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn row_to_event(r: &rusqlite::Row) -> rusqlite::Result<UsageEvent> {
        Ok(UsageEvent {
            dedup_key: r.get(0)?,
            tool: Tool::parse(&r.get::<_, String>(1)?).unwrap_or(Tool::ClaudeCode),
            kind: EventKind::parse(&r.get::<_, String>(2)?),
            ts: from_ms(r.get(3)?),
            session_id: r.get(4)?,
            project: r.get(5)?,
            model: r.get(6)?,
            input_tokens: r.get::<_, i64>(7)? as u64,
            output_tokens: r.get::<_, i64>(8)? as u64,
            cache_write_tokens: r.get::<_, i64>(9)? as u64,
            cache_read_tokens: r.get::<_, i64>(10)? as u64,
            cost_usd: r.get(11)?,
            tool_sig: r.get(12)?,
        })
    }

    const EVENT_COLS: &'static str =
        "dedup_key, tool, kind, ts, session_id, project, model, input, output, cache_write, cache_read, cost, tool_sig";

    /// Events for a tool (or all tools) in [from, to), oldest first.
    pub fn events(&self, tool: Option<Tool>, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<UsageEvent>> {
        let sql = format!(
            "SELECT {} FROM events WHERE ts >= ?1 AND ts < ?2 AND (?3 IS NULL OR tool = ?3) ORDER BY ts ASC",
            Self::EVENT_COLS
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![ms(from), ms(to), tool.map(|t| t.as_str())], Self::row_to_event)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn unsynced_events(&self, limit: usize) -> Result<Vec<UsageEvent>> {
        let sql = format!(
            "SELECT {} FROM events WHERE synced = 0 AND kind = 'assistant' ORDER BY ts ASC LIMIT ?1",
            Self::EVENT_COLS
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![limit as i64], Self::row_to_event)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn mark_synced(&mut self, keys: &[String]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("UPDATE events SET synced = 1 WHERE dedup_key = ?1")?;
            for k in keys {
                stmt.execute(params![k])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn first_event_ts(&self, tool: Option<Tool>) -> Result<Option<DateTime<Utc>>> {
        let v: Option<i64> = self.conn.query_row(
            "SELECT MIN(ts) FROM events WHERE (?1 IS NULL OR tool = ?1)",
            params![tool.map(|t| t.as_str())],
            |r| r.get(0),
        )?;
        Ok(v.map(from_ms))
    }

    /// (number of AI requests, newest event time) for the diagnostics screen.
    pub fn event_stats(&self, tool: Tool) -> Result<(u64, Option<DateTime<Utc>>)> {
        let (n, last): (i64, Option<i64>) = self.conn.query_row(
            "SELECT COUNT(*), MAX(ts) FROM events WHERE tool = ?1 AND kind = 'assistant'",
            params![tool.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((n as u64, last.map(from_ms)))
    }

    pub fn has_gauges(&self, tool: Tool) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM (SELECT 1 FROM gauges WHERE tool = ?1 LIMIT 1)",
            params![tool.as_str()],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn has_events(&self, tool: Tool) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM (SELECT 1 FROM events WHERE tool = ?1 LIMIT 1)",
            params![tool.as_str()],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn gauges(&self, tool: Tool, window: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<GaugeSample>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT tool, window, ts, used_pct, resets_at, window_minutes, source FROM gauges
             WHERE tool = ?1 AND window = ?2 AND ts >= ?3 AND ts < ?4 ORDER BY ts ASC",
        )?;
        let rows = stmt.query_map(params![tool.as_str(), window, ms(from), ms(to)], |r| {
            Ok(GaugeSample {
                tool: Tool::parse(&r.get::<_, String>(0)?).unwrap_or(Tool::ClaudeCode),
                window: r.get(1)?,
                ts: from_ms(r.get(2)?),
                used_pct: r.get(3)?,
                resets_at: r.get::<_, Option<i64>>(4)?.map(from_ms),
                window_minutes: r.get(5)?,
                source: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn latest_gauge(&self, tool: Tool, window: &str, before: DateTime<Utc>) -> Result<Option<GaugeSample>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT tool, window, ts, used_pct, resets_at, window_minutes, source FROM gauges
             WHERE tool = ?1 AND window = ?2 AND ts <= ?3 ORDER BY ts DESC LIMIT 1",
        )?;
        Ok(stmt
            .query_row(params![tool.as_str(), window, ms(before)], |r| {
                Ok(GaugeSample {
                    tool: Tool::parse(&r.get::<_, String>(0)?).unwrap_or(Tool::ClaudeCode),
                    window: r.get(1)?,
                    ts: from_ms(r.get(2)?),
                    used_pct: r.get(3)?,
                    resets_at: r.get::<_, Option<i64>>(4)?.map(from_ms),
                    window_minutes: r.get(5)?,
                    source: r.get(6)?,
                })
            })
            .optional()?)
    }

    pub fn cursor(&self, path: &str) -> Result<Option<Cursor>> {
        Ok(self
            .conn
            .query_row(
                "SELECT offset, size, mtime, ctx FROM cursors WHERE path = ?1",
                params![path],
                |r| {
                    Ok(Cursor {
                        offset: r.get::<_, i64>(0)? as u64,
                        size: r.get::<_, i64>(1)? as u64,
                        mtime: r.get(2)?,
                        ctx: r.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn set_cursor(&self, path: &str, c: &Cursor) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO cursors (path, offset, size, mtime, ctx) VALUES (?1,?2,?3,?4,?5)",
            params![path, c.offset as i64, c.size as i64, c.mtime, c.ctx],
        )?;
        Ok(())
    }

    pub fn kv_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM kv WHERE key = ?1", params![key], |r| r.get(0))
            .optional()?)
    }

    pub fn kv_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute("INSERT OR REPLACE INTO kv (key, value) VALUES (?1, ?2)", params![key, value])?;
        Ok(())
    }

    pub fn kv_del(&self, key: &str) -> Result<()> {
        self.conn.execute("DELETE FROM kv WHERE key = ?1", params![key])?;
        Ok(())
    }

    pub fn insert_task(&self, t: &TaskRow) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO tasks (created_at, repo, cwd, branch, tool, text, dest, queued_until, worktree) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![ms(t.created_at), t.repo, t.cwd, t.branch, t.tool.as_str(), t.text, t.dest, t.queued_until.map(ms), t.worktree],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Tasks started (or queued) since `from` that weren't dismissed, newest first.
    pub fn tasks_since(&self, from: DateTime<Utc>) -> Result<Vec<TaskRow>> {
        self.tasks_where("created_at >= ?1 AND dismissed = 0", ms(from))
    }

    /// Queued tasks whose time has come.
    pub fn queued_due(&self, now: DateTime<Utc>) -> Result<Vec<TaskRow>> {
        self.tasks_where("queued_until IS NOT NULL AND queued_until <= ?1 AND dismissed = 0", ms(now))
    }

    pub fn task(&self, id: i64) -> Result<Option<TaskRow>> {
        Ok(self.tasks_where("id = ?1", id)?.into_iter().next())
    }

    fn tasks_where(&self, cond: &str, arg: i64) -> Result<Vec<TaskRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, created_at, repo, cwd, branch, tool, text, dest, queued_until, worktree, verify, verify_at, verify_secs FROM tasks WHERE {cond} ORDER BY created_at DESC, id DESC"
        ))?;
        let rows = stmt.query_map(params![arg], |r| {
            let tool: String = r.get(5)?;
            Ok(TaskRow {
                id: r.get(0)?,
                created_at: from_ms(r.get(1)?),
                repo: r.get(2)?,
                cwd: r.get(3)?,
                branch: r.get(4)?,
                tool: Tool::parse(&tool).unwrap_or(Tool::ClaudeCode),
                text: r.get(6)?,
                dest: r.get(7)?,
                queued_until: r.get::<_, Option<i64>>(8)?.map(from_ms),
                worktree: r.get::<_, Option<bool>>(9)?.unwrap_or(false),
                verify: r.get(10)?,
                verify_at: r.get::<_, Option<i64>>(11)?.map(from_ms),
                verify_secs: r.get(12)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// A queued task was sent: it starts now, in `cwd`.
    pub fn start_queued(&self, id: i64, now: DateTime<Utc>, tool: Tool, cwd: &str, branch: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE tasks SET queued_until = NULL, created_at = ?2, tool = ?3, cwd = ?4, branch = ?5 WHERE id = ?1",
            params![id, ms(now), tool.as_str(), cwd, branch],
        )?;
        Ok(())
    }

    pub fn set_task_dest(&self, id: i64, dest: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE tasks SET dest = ?2 WHERE id = ?1", params![id, dest])?;
        Ok(())
    }

    /// Record a test run: "running", "passed", "failed", "timed_out" or "error".
    pub fn set_verify(&self, id: i64, status: &str, at: DateTime<Utc>, secs: Option<f64>) -> Result<()> {
        self.conn.execute("UPDATE tasks SET verify = ?2, verify_at = ?3, verify_secs = ?4 WHERE id = ?1", params![id, status, ms(at), secs])?;
        Ok(())
    }

    pub fn dismiss_task(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE tasks SET dismissed = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn add_saving(&self, ts: DateTime<Utc>, kind: &str, amount: f64, note: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO savings (ts, kind, amount, note) VALUES (?1,?2,?3,?4)",
            params![ms(ts), kind, amount, note],
        )?;
        Ok(())
    }

    pub fn savings_since(&self, from: DateTime<Utc>) -> Result<Vec<SavingsEntry>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT ts, kind, amount, note FROM savings WHERE ts >= ?1 ORDER BY ts ASC")?;
        let rows = stmt.query_map(params![ms(from)], |r| {
            Ok(SavingsEntry { ts: from_ms(r.get(0)?), kind: r.get(1)?, amount_usd: r.get(2)?, note: r.get(3)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

/// A task started from the dispatcher.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRow {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub repo: String,
    pub cwd: String,
    pub branch: Option<String>,
    pub tool: Tool,
    pub text: String,
    /// "terminal", "app" or "web".
    pub dest: Option<String>,
    /// Waiting for this tool's limit to reset; sent then.
    pub queued_until: Option<DateTime<Utc>>,
    /// Run in its own worktree (used when a queued task is sent).
    pub worktree: bool,
    /// Last test run: "running", "passed", "failed", "timed_out" or "error".
    pub verify: Option<String>,
    pub verify_at: Option<DateTime<Utc>>,
    pub verify_secs: Option<f64>,
}

impl TaskRow {
    pub fn new(repo: String, cwd: String, branch: Option<String>, tool: Tool, text: String, dest: Option<String>) -> TaskRow {
        TaskRow { id: 0, created_at: Utc::now(), repo, cwd, branch, tool, text, dest, queued_until: None, worktree: false, verify: None, verify_at: None, verify_secs: None }
    }
}

use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{params, Connection};

use crate::model::{Agent, CostSource, Tokens, UsageEvent};

/// Computed cost for an event (None = price unknown). Supplied by the pricing module.
pub type CostFn<'a> = &'a dyn Fn(&UsageEvent) -> Option<f64>;

#[derive(Clone, Debug, PartialEq)]
pub struct FileCursor {
    pub path: String,
    pub size: u64,
    pub mtime: i64,
    pub offset: u64,
}

/// One stored event as read back for aggregation (`cost_usd` None = price unknown).
#[derive(Clone, Debug, PartialEq)]
pub struct StoredEvent {
    pub ts: DateTime<Utc>,
    pub agent: Agent,
    pub model: String,
    pub cwd: String,
    pub session_id: String,
    pub tokens: Tokens,
    pub cost_usd: Option<f64>,
}

/// First event of a session: when it started and in which folder.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionStart {
    pub agent: Agent,
    pub session_id: String,
    pub start: DateTime<Utc>,
    pub cwd: String,
}

/// Timestamps are stored as UTC epoch milliseconds (INTEGER): sorts and range-queries via the `ts` index.
pub struct Store {
    conn: Connection,
}

const SCHEMA_V1: &str = "
CREATE TABLE events (
    id INTEGER PRIMARY KEY,
    dedup_key TEXT NOT NULL UNIQUE,
    ts INTEGER NOT NULL,
    agent TEXT NOT NULL,
    model TEXT NOT NULL,
    cwd TEXT NOT NULL,
    session_id TEXT NOT NULL,
    input INTEGER NOT NULL,
    output INTEGER NOT NULL,
    cache_read INTEGER NOT NULL,
    cache_write_5m INTEGER NOT NULL,
    cache_write_1h INTEGER NOT NULL,
    reasoning INTEGER NOT NULL,
    cost_usd REAL NULL,
    cost_source TEXT NOT NULL
);
CREATE INDEX idx_events_ts ON events(ts);
CREATE INDEX idx_events_agent ON events(agent);
CREATE INDEX idx_events_session ON events(session_id);
CREATE TABLE file_cursors (
    path TEXT PRIMARY KEY,
    size INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    offset INTEGER NOT NULL
);
CREATE TABLE watermarks (source TEXT PRIMARY KEY, value INTEGER NOT NULL);
CREATE TABLE quota_samples (
    agent TEXT NOT NULL,
    window_id TEXT NOT NULL,
    ts INTEGER NOT NULL,
    used_pct REAL NOT NULL
);
CREATE INDEX idx_quota_lookup ON quota_samples(agent, window_id, ts);
";

fn agent_str(a: Agent) -> &'static str {
    match a {
        Agent::ClaudeCode => "cc",
        Agent::Codex => "cx",
        Agent::OpenCode => "oc",
        Agent::Hermes => "hm",
        Agent::Cursor => "cr",
    }
}

fn parse_agent(s: &str) -> Agent {
    match s {
        "cx" => Agent::Codex,
        "oc" => Agent::OpenCode,
        "hm" => Agent::Hermes,
        "cr" => Agent::Cursor,
        _ => Agent::ClaudeCode,
    }
}

fn source_str(s: CostSource) -> &'static str {
    match s {
        CostSource::Computed => "computed",
        CostSource::Reported => "reported",
        CostSource::Unknown => "unknown",
    }
}

fn resolve_cost(reported: Option<f64>, computed: impl FnOnce() -> Option<f64>) -> (Option<f64>, CostSource) {
    if let Some(c) = reported {
        return (Some(c), CostSource::Reported);
    }
    match computed() {
        Some(c) => (Some(c), CostSource::Computed),
        None => (None, CostSource::Unknown),
    }
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(&format!("BEGIN;{SCHEMA_V1}PRAGMA user_version = 1;COMMIT;"))?;
        }
        Ok(Store { conn })
    }

    /// Returns the number of newly inserted rows; events whose `dedup_key` already exists are ignored.
    pub fn insert_events(&self, evs: &[UsageEvent], cost: CostFn) -> Result<usize> {
        self.write_events(evs, cost, false)
    }

    /// Replaces any existing events with the same `dedup_key` (delete + insert in one transaction).
    /// Use for sources whose rows are updated in place (Hermes, OpenCode).
    pub fn upsert_events(&self, evs: &[UsageEvent], cost: CostFn) -> Result<usize> {
        self.write_events(evs, cost, true)
    }

    fn write_events(&self, evs: &[UsageEvent], cost: CostFn, replace: bool) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let mut inserted = 0;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO events (dedup_key, ts, agent, model, cwd, session_id, input, output,
                 cache_read, cache_write_5m, cache_write_1h, reasoning, cost_usd, cost_source)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            )?;
            let mut del = tx.prepare("DELETE FROM events WHERE dedup_key=?1")?;
            for e in evs {
                if replace {
                    del.execute([&e.dedup_key])?;
                }
                let (c, src) = resolve_cost(e.reported_cost_usd, || cost(e));
                let t = &e.tokens;
                inserted += stmt.execute(params![
                    e.dedup_key,
                    e.ts.timestamp_millis(),
                    agent_str(e.agent),
                    e.model,
                    e.cwd,
                    e.session_id,
                    t.input as i64,
                    t.output as i64,
                    t.cache_read as i64,
                    t.cache_write_5m as i64,
                    t.cache_write_1h as i64,
                    t.reasoning as i64,
                    c,
                    source_str(src),
                ])?;
            }
        }
        tx.commit()?;
        Ok(inserted)
    }

    /// Re-prices every non-reported event (e.g. after the price table changes).
    pub fn recompute_costs(&self, cost: CostFn) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut sel = tx.prepare(
                "SELECT id, ts, agent, model, cwd, session_id, input, output, cache_read, cache_write_5m,
                 cache_write_1h, reasoning, dedup_key FROM events WHERE cost_source != 'reported'",
            )?;
            let mut upd = tx.prepare("UPDATE events SET cost_usd=?1, cost_source=?2 WHERE id=?3")?;
            let rows = sel.query_map([], |r| {
                let agent: String = r.get(2)?;
                let ev = UsageEvent {
                    ts: Utc.timestamp_millis_opt(r.get(1)?).single().unwrap_or_default(),
                    agent: parse_agent(&agent),
                    model: r.get(3)?,
                    cwd: r.get(4)?,
                    session_id: r.get(5)?,
                    tokens: Tokens {
                        input: r.get::<_, i64>(6)? as u64,
                        output: r.get::<_, i64>(7)? as u64,
                        cache_read: r.get::<_, i64>(8)? as u64,
                        cache_write_5m: r.get::<_, i64>(9)? as u64,
                        cache_write_1h: r.get::<_, i64>(10)? as u64,
                        reasoning: r.get::<_, i64>(11)? as u64,
                    },
                    reported_cost_usd: None,
                    dedup_key: r.get(12)?,
                };
                Ok((r.get::<_, i64>(0)?, ev))
            })?;
            for row in rows {
                let (id, ev) = row?;
                let (c, src) = resolve_cost(None, || cost(&ev));
                upd.execute(params![c, source_str(src), id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_cursor(&self, path: &str) -> Result<Option<FileCursor>> {
        let mut stmt = self.conn.prepare("SELECT path, size, mtime, offset FROM file_cursors WHERE path=?1")?;
        let mut rows = stmt.query_map([path], |r| {
            Ok(FileCursor {
                path: r.get(0)?,
                size: r.get::<_, i64>(1)? as u64,
                mtime: r.get(2)?,
                offset: r.get::<_, i64>(3)? as u64,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn set_cursor(&self, c: &FileCursor) -> Result<()> {
        self.conn.execute(
            "INSERT INTO file_cursors (path, size, mtime, offset) VALUES (?1,?2,?3,?4)
             ON CONFLICT(path) DO UPDATE SET size=excluded.size, mtime=excluded.mtime, offset=excluded.offset",
            params![c.path, c.size as i64, c.mtime, c.offset as i64],
        )?;
        Ok(())
    }

    /// Removes one event by its `dedup_key` (no-op when absent); used to upsert changed rows.
    pub fn delete_by_dedup_key(&self, key: &str) -> Result<()> {
        self.conn.execute("DELETE FROM events WHERE dedup_key=?1", [key])?;
        Ok(())
    }

    /// 0 when the source has no watermark yet.
    pub fn get_watermark(&self, source: &str) -> Result<i64> {
        let mut stmt = self.conn.prepare("SELECT value FROM watermarks WHERE source=?1")?;
        let mut rows = stmt.query_map([source], |r| r.get::<_, i64>(0))?;
        Ok(rows.next().transpose()?.unwrap_or(0))
    }

    pub fn set_watermark(&self, source: &str, v: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO watermarks (source, value) VALUES (?1,?2)
             ON CONFLICT(source) DO UPDATE SET value=excluded.value",
            params![source, v],
        )?;
        Ok(())
    }

    /// Keys of notifications already sent. Kept in `watermarks` under a `sent:` prefix; the value is the
    /// reset time (ms) after which the key is useless.
    pub fn sent_keys(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT substr(source, 6) FROM watermarks WHERE source LIKE 'sent:%'")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn mark_sent(&self, key: &str, expires: DateTime<Utc>) -> Result<()> {
        self.set_watermark(&format!("sent:{key}"), expires.timestamp_millis())
    }

    /// Forgets sent keys whose window reset before `before`.
    pub fn prune_sent(&self, before: DateTime<Utc>) -> Result<()> {
        self.conn.execute("DELETE FROM watermarks WHERE source LIKE 'sent:%' AND value < ?1", params![before.timestamp_millis()])?;
        Ok(())
    }

    pub fn add_quota_sample(&self, agent: Agent, window_id: &str, ts: DateTime<Utc>, used_pct: f64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO quota_samples (agent, window_id, ts, used_pct) VALUES (?1,?2,?3,?4)",
            params![agent_str(agent), window_id, ts.timestamp_millis(), used_pct],
        )?;
        Ok(())
    }

    /// Samples with `ts >= since`, oldest first.
    pub fn quota_samples(&self, agent: Agent, window_id: &str, since: DateTime<Utc>) -> Result<Vec<(DateTime<Utc>, f64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT ts, used_pct FROM quota_samples WHERE agent=?1 AND window_id=?2 AND ts>=?3 ORDER BY ts",
        )?;
        let rows = stmt.query_map(params![agent_str(agent), window_id, since.timestamp_millis()], |r| {
            Ok((Utc.timestamp_millis_opt(r.get(0)?).single().unwrap_or_default(), r.get::<_, f64>(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Events with `from <= ts < to`, oldest first.
    pub fn events_between(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<StoredEvent>> {
        self.query_events("ts >= ?1 AND ts < ?2", params![from.timestamp_millis(), to.timestamp_millis()])
    }

    /// Every event of one session, oldest first.
    pub fn session_events(&self, agent: Agent, session_id: &str) -> Result<Vec<StoredEvent>> {
        self.query_events("agent = ?1 AND session_id = ?2", params![agent_str(agent), session_id])
    }

    fn query_events(&self, filter: &str, args: impl rusqlite::Params) -> Result<Vec<StoredEvent>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT ts, agent, model, cwd, session_id, input, output, cache_read, cache_write_5m,
             cache_write_1h, reasoning, cost_usd FROM events WHERE {filter} ORDER BY ts"
        ))?;
        let rows = stmt.query_map(args, |r| {
            Ok(StoredEvent {
                ts: Utc.timestamp_millis_opt(r.get(0)?).single().unwrap_or_default(),
                agent: parse_agent(&r.get::<_, String>(1)?),
                model: r.get(2)?,
                cwd: r.get(3)?,
                session_id: r.get(4)?,
                tokens: Tokens {
                    input: r.get::<_, i64>(5)? as u64,
                    output: r.get::<_, i64>(6)? as u64,
                    cache_read: r.get::<_, i64>(7)? as u64,
                    cache_write_5m: r.get::<_, i64>(8)? as u64,
                    cache_write_1h: r.get::<_, i64>(9)? as u64,
                    reasoning: r.get::<_, i64>(10)? as u64,
                },
                cost_usd: r.get(11)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// One row per (agent, session_id): first timestamp and the cwd of that first event
    /// (SQLite takes bare columns from the row that holds the MIN).
    pub fn session_starts(&self) -> Result<Vec<SessionStart>> {
        let mut stmt = self.conn.prepare("SELECT agent, session_id, MIN(ts), cwd FROM events GROUP BY agent, session_id")?;
        let rows = stmt.query_map([], |r| {
            Ok(SessionStart {
                agent: parse_agent(&r.get::<_, String>(0)?),
                session_id: r.get(1)?,
                start: Utc.timestamp_millis_opt(r.get(2)?).single().unwrap_or_default(),
                cwd: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use chrono::{TimeZone, Utc};

    fn ev(key: &str, model: &str, reported: Option<f64>) -> UsageEvent {
        UsageEvent {
            ts: Utc.timestamp_millis_opt(1_700_000_000_123).unwrap(),
            agent: Agent::OpenCode,
            model: model.into(),
            cwd: "/tmp/p".into(),
            session_id: "s1".into(),
            tokens: Tokens { input: 10, output: 20, cache_read: 3, cache_write_5m: 4, cache_write_1h: 5, reasoning: 6 },
            reported_cost_usd: reported,
            dedup_key: key.into(),
        }
    }

    fn row(s: &Store, key: &str) -> (Option<f64>, String, i64, i64) {
        s.conn
            .query_row(
                "SELECT cost_usd, cost_source, input, ts FROM events WHERE dedup_key=?1",
                [key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap()
    }

    #[test]
    fn duplicate_dedup_key_is_ignored() {
        let s = Store::open_in_memory().unwrap();
        let n = s.insert_events(&[ev("k", "m", None), ev("k", "m", None)], &|_| Some(1.0)).unwrap();
        assert_eq!(n, 1);
        let c: i64 = s.conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)).unwrap();
        assert_eq!(c, 1);
        assert_eq!(s.insert_events(&[ev("k", "m", None)], &|_| Some(1.0)).unwrap(), 0);
    }

    #[test]
    fn cursor_roundtrip() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.get_cursor("/a").unwrap(), None);
        let c = FileCursor { path: "/a".into(), size: 100, mtime: -5, offset: 42 };
        s.set_cursor(&c).unwrap();
        assert_eq!(s.get_cursor("/a").unwrap(), Some(c.clone()));
        let c2 = FileCursor { offset: 99, ..c };
        s.set_cursor(&c2).unwrap();
        assert_eq!(s.get_cursor("/a").unwrap(), Some(c2));
    }

    #[test]
    fn reported_cost_wins_over_computed() {
        let s = Store::open_in_memory().unwrap();
        s.insert_events(&[ev("k", "m", Some(0.4))], &|_| Some(1.0)).unwrap();
        let (cost, src, input, ts) = row(&s, "k");
        assert_eq!(cost, Some(0.4));
        assert_eq!(src, "reported");
        assert_eq!(input, 10);
        assert_eq!(ts, 1_700_000_000_123);
    }

    #[test]
    fn computed_cost_used_without_report() {
        let s = Store::open_in_memory().unwrap();
        s.insert_events(&[ev("k", "m", None)], &|_| Some(1.0)).unwrap();
        let (cost, src, _, _) = row(&s, "k");
        assert_eq!((cost, src.as_str()), (Some(1.0), "computed"));
    }

    #[test]
    fn unknown_model_stores_null_cost() {
        let s = Store::open_in_memory().unwrap();
        let f = |e: &UsageEvent| if e.model == "big-pickle" { None } else { Some(1.0) };
        s.insert_events(&[ev("k", "big-pickle", None)], &f).unwrap();
        let (cost, src, input, _) = row(&s, "k");
        assert_eq!(cost, None);
        assert_eq!(src, "unknown");
        assert_eq!(input, 10);
    }

    #[test]
    fn recompute_skips_reported_and_fills_unknown() {
        let s = Store::open_in_memory().unwrap();
        s.insert_events(&[ev("r", "m", Some(0.4)), ev("u", "m", None), ev("c", "m", None)], &|e| {
            if e.dedup_key == "c" { Some(1.0) } else { None }
        })
        .unwrap();
        s.recompute_costs(&|_| Some(2.0)).unwrap();
        assert_eq!(row(&s, "r").0, Some(0.4));
        assert_eq!(row(&s, "u").0, Some(2.0));
        assert_eq!(row(&s, "u").1, "computed");
        assert_eq!(row(&s, "c").0, Some(2.0));
        s.recompute_costs(&|_| None).unwrap();
        assert_eq!(row(&s, "c"), (None, "unknown".to_string(), 10, 1_700_000_000_123));
        assert_eq!(row(&s, "r").1, "reported");
    }

    #[test]
    fn watermark_defaults_to_zero_and_roundtrips() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.get_watermark("oc").unwrap(), 0);
        s.set_watermark("oc", 77).unwrap();
        s.set_watermark("oc", 88).unwrap();
        assert_eq!(s.get_watermark("oc").unwrap(), 88);
    }

    #[test]
    fn sent_keys_persist_and_old_ones_are_pruned_without_touching_watermarks() {
        let s = Store::open_in_memory().unwrap();
        let t = |m| Utc.timestamp_millis_opt(m).unwrap();
        s.set_watermark("oc", 5).unwrap();
        s.mark_sent("cx:five_hour:100", t(1000)).unwrap();
        s.mark_sent("cx:five_hour:200", t(5000)).unwrap();
        s.mark_sent("cx:five_hour:200", t(5000)).unwrap();
        assert_eq!(s.sent_keys().unwrap().len(), 2);
        assert!(s.sent_keys().unwrap().contains("cx:five_hour:100"));
        s.prune_sent(t(2000)).unwrap();
        assert_eq!(s.sent_keys().unwrap(), ["cx:five_hour:200".to_string()].into());
        assert_eq!(s.get_watermark("oc").unwrap(), 5);
    }

    #[test]
    fn quota_samples_filtered_by_since_and_ordered() {
        let s = Store::open_in_memory().unwrap();
        let t = |m| Utc.timestamp_millis_opt(m).unwrap();
        s.add_quota_sample(Agent::ClaudeCode, "5h", t(3000), 30.0).unwrap();
        s.add_quota_sample(Agent::ClaudeCode, "5h", t(1000), 10.0).unwrap();
        s.add_quota_sample(Agent::ClaudeCode, "7d", t(2000), 99.0).unwrap();
        s.add_quota_sample(Agent::Codex, "5h", t(2000), 55.0).unwrap();
        let v = s.quota_samples(Agent::ClaudeCode, "5h", t(2000)).unwrap();
        assert_eq!(v, vec![(t(3000), 30.0)]);
        let all = s.quota_samples(Agent::ClaudeCode, "5h", t(0)).unwrap();
        assert_eq!(all, vec![(t(1000), 10.0), (t(3000), 30.0)]);
    }

    #[test]
    fn file_backed_db_persists_and_sets_schema_version() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("db.sqlite");
        Store::open(&p).unwrap().set_watermark("x", 5).unwrap();
        let s = Store::open(&p).unwrap();
        assert_eq!(s.get_watermark("x").unwrap(), 5);
        let v: i64 = s.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 1);
    }

    #[test]
    fn delete_by_dedup_key_removes_only_that_event() {
        let s = Store::open_in_memory().unwrap();
        s.insert_events(&[ev("a", "m", None), ev("b", "m", None)], &|_| None).unwrap();
        s.delete_by_dedup_key("a").unwrap();
        s.delete_by_dedup_key("missing").unwrap();
        let n: i64 = s.conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let b: i64 = s.conn.query_row("SELECT COUNT(*) FROM events WHERE dedup_key='b'", [], |r| r.get(0)).unwrap();
        assert_eq!(b, 1);
    }

    #[test]
    fn upsert_events_replaces_same_key_and_insert_ignores() {
        let s = Store::open_in_memory().unwrap();
        s.insert_events(&[ev("a", "m", None)], &|_| None).unwrap();
        let mut bigger = ev("a", "m", None);
        bigger.tokens.input = 999;
        assert_eq!(s.insert_events(&[bigger.clone()], &|_| None).unwrap(), 0);
        assert_eq!(row(&s, "a").2, 10);
        assert_eq!(s.upsert_events(&[bigger, ev("b", "m", None)], &|_| None).unwrap(), 2);
        assert_eq!(row(&s, "a").2, 999);
        let n: i64 = s.conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
    }

    fn at(key: &str, ms: i64, agent: Agent, session: &str, cwd: &str) -> UsageEvent {
        UsageEvent {
            ts: Utc.timestamp_millis_opt(ms).unwrap(),
            agent,
            cwd: cwd.into(),
            session_id: session.into(),
            ..ev(key, "m", None)
        }
    }

    #[test]
    fn agent_codes_map_both_ways() {
        for a in crate::model::STACK_ORDER {
            assert_eq!(parse_agent(agent_str(a)), a);
        }
        assert_eq!(agent_str(Agent::Cursor), "cr");
        assert_eq!(parse_agent("cr"), Agent::Cursor);
    }

    #[test]
    fn events_between_is_half_open_and_oldest_first() {
        let s = Store::open_in_memory().unwrap();
        let evs = [
            at("c", 3000, Agent::Codex, "s", "/p"),
            at("a", 1000, Agent::ClaudeCode, "s", "/p"),
            at("b", 2000, Agent::Hermes, "s", "/p"),
            at("d", 4000, Agent::OpenCode, "s", "/p"),
        ];
        s.insert_events(&evs, &|e| if e.dedup_key == "b" { Some(0.5) } else { None }).unwrap();
        let t = |m| Utc.timestamp_millis_opt(m).unwrap();
        let got = s.events_between(t(1000), t(4000)).unwrap();
        let ts: Vec<i64> = got.iter().map(|e| e.ts.timestamp_millis()).collect();
        assert_eq!(ts, vec![1000, 2000, 3000]);
        let b = &got[1];
        assert_eq!(b.agent, Agent::Hermes);
        assert_eq!(b.cost_usd, Some(0.5));
        assert_eq!((b.model.as_str(), b.cwd.as_str(), b.session_id.as_str()), ("m", "/p", "s"));
        assert_eq!(b.tokens, Tokens { input: 10, output: 20, cache_read: 3, cache_write_5m: 4, cache_write_1h: 5, reasoning: 6 });
        assert_eq!(got[0].cost_usd, None);
        assert_eq!(got[2].agent, Agent::Codex);
    }

    #[test]
    fn session_events_filter_by_agent_and_session() {
        let s = Store::open_in_memory().unwrap();
        let evs = [
            at("a2", 2000, Agent::ClaudeCode, "s1", "/p"),
            at("a1", 1000, Agent::ClaudeCode, "s1", "/p"),
            at("b", 1500, Agent::ClaudeCode, "s2", "/p"),
            at("c", 1500, Agent::Codex, "s1", "/p"),
        ];
        s.insert_events(&evs, &|_| None).unwrap();
        let got = s.session_events(Agent::ClaudeCode, "s1").unwrap();
        let ts: Vec<i64> = got.iter().map(|e| e.ts.timestamp_millis()).collect();
        assert_eq!(ts, vec![1000, 2000]);
        assert!(s.session_events(Agent::Hermes, "s1").unwrap().is_empty());
    }

    #[test]
    fn session_starts_take_first_event_time_and_cwd() {
        let s = Store::open_in_memory().unwrap();
        let evs = [
            at("a2", 2000, Agent::ClaudeCode, "s1", "/later"),
            at("a1", 1000, Agent::ClaudeCode, "s1", "/first"),
            at("a3", 3000, Agent::ClaudeCode, "s1", "/last"),
            at("c", 500, Agent::Codex, "s1", "/cx"),
        ];
        s.insert_events(&evs, &|_| None).unwrap();
        let mut got = s.session_starts().unwrap();
        got.sort_by_key(|x| x.start);
        let t = |m| Utc.timestamp_millis_opt(m).unwrap();
        assert_eq!(
            got,
            vec![
                SessionStart { agent: Agent::Codex, session_id: "s1".into(), start: t(500), cwd: "/cx".into() },
                SessionStart { agent: Agent::ClaudeCode, session_id: "s1".into(), start: t(1000), cwd: "/first".into() },
            ]
        );
    }
}

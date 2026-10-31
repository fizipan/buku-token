use std::path::Path;

use anyhow::Result;
use chrono::DateTime;

use super::ReadBatch;
use crate::model::{Agent, Tokens, UsageEvent};

/// Reads Hermes `session_model_usage` rows whose `last_seen` (ms) is above `watermark`.
/// A row whose `last_seen` grows is returned again with the same `dedup_key`, so the caller
/// must persist with `Store::upsert_events`, not `insert_events`. Malformed rows (NULL or
/// wrong-typed `session_id`/`model`) are skipped and counted; the watermark still advances.
pub fn read_since(db: &Path, watermark: i64) -> Result<ReadBatch> {
    let mut batch = ReadBatch { watermark, ..Default::default() };
    let Some(conn) = super::open_read_only(db)? else { return Ok(batch) };
    let mut stmt = conn.prepare(
        "SELECT u.session_id, u.model, u.task, u.input_tokens, u.output_tokens, u.cache_read_tokens,
                u.cache_write_tokens, u.reasoning_tokens, u.estimated_cost_usd, u.actual_cost_usd,
                CAST(ROUND(u.last_seen * 1000) AS INTEGER) AS last_ms,
                COALESCE(NULLIF(s.git_repo_root, ''), NULLIF(s.cwd, ''), '')
         FROM session_model_usage u LEFT JOIN sessions s ON s.id = u.session_id
         WHERE last_ms > ?1
         ORDER BY last_ms",
    )?;
    // Wrong-typed values read as absent instead of failing the whole scan.
    let tok = |r: &rusqlite::Row, i: usize| r.get::<_, Option<i64>>(i).ok().flatten().unwrap_or(0).max(0) as u64;
    let cost = |r: &rusqlite::Row, i: usize| r.get::<_, Option<f64>>(i).ok().flatten().unwrap_or(0.0);
    let text = |r: &rusqlite::Row, i: usize| r.get::<_, Option<String>>(i).ok().flatten();
    let mut rows = stmt.query([watermark])?;
    while let Some(r) = rows.next()? {
        let last_ms: i64 = r.get(10)?;
        batch.watermark = batch.watermark.max(last_ms);
        let (Some(session_id), Some(model)) = (text(r, 0), text(r, 1)) else {
            batch.skipped += 1;
            continue;
        };
        let task = text(r, 2);
        batch.events.push(UsageEvent {
            ts: DateTime::from_timestamp_millis(last_ms).unwrap_or_default(),
            agent: Agent::Hermes,
            dedup_key: format!("hm:{session_id}:{model}:{}", task.as_deref().unwrap_or("")),
            model,
            cwd: text(r, 11).unwrap_or_default(),
            session_id,
            tokens: Tokens {
                input: tok(r, 3),
                output: tok(r, 4),
                cache_read: tok(r, 5),
                cache_write_5m: tok(r, 6),
                cache_write_1h: 0,
                reasoning: tok(r, 7),
            },
            reported_cost_usd: [cost(r, 9), cost(r, 8)].into_iter().find(|c| *c > 0.0),
        });
    }
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

    fn make_db(dir: &Path) -> std::path::PathBuf {
        let p = dir.join("state.db");
        Connection::open(&p)
            .unwrap()
            .execute_batch(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, cwd TEXT, git_repo_root TEXT, started_at REAL);
                 CREATE TABLE session_model_usage (
                   session_id TEXT, model TEXT, billing_provider TEXT, billing_base_url TEXT, billing_mode TEXT,
                   task TEXT, api_call_count INTEGER, input_tokens INTEGER, output_tokens INTEGER,
                   cache_read_tokens INTEGER, cache_write_tokens INTEGER, reasoning_tokens INTEGER,
                   estimated_cost_usd REAL, actual_cost_usd REAL, cost_status TEXT, cost_source TEXT,
                   first_seen REAL, last_seen REAL);",
            )
            .unwrap();
        p
    }

    fn session(c: &Connection, id: &str, cwd: Option<&str>, root: Option<&str>) {
        c.execute("INSERT INTO sessions VALUES (?1,?2,?3,1.0)", params![id, cwd, root]).unwrap();
    }

    #[allow(clippy::too_many_arguments)]
    fn usage(c: &Connection, sid: &str, task: Option<&str>, est: f64, act: Option<f64>, last: f64) {
        c.execute(
            "INSERT INTO session_model_usage VALUES (?1,'gpt-x','p','u','m',?2,3,100,50,20,7,9,?3,?4,'s','src',?5,?5)",
            params![sid, task, est, act, last],
        )
        .unwrap();
    }

    #[test]
    fn hermes_prefers_actual_then_estimated_cost() {
        let dir = tempfile::tempdir().unwrap();
        let p = make_db(dir.path());
        let c = Connection::open(&p).unwrap();
        for s in ["a", "b", "c"] {
            session(&c, s, Some("/w"), None);
        }
        usage(&c, "a", None, 0.5, Some(0.25), 1791698778.984166);
        usage(&c, "b", Some("approval"), 0.5, Some(0.0), 1791698779.0);
        usage(&c, "c", Some("title_generation"), 0.0, Some(0.0), 1791698780.0);
        drop(c);
        let ReadBatch { events: evs, watermark: wm, .. } = read_since(&p, 0).unwrap();
        assert_eq!(evs.len(), 3);
        let by = |k: &str| evs.iter().find(|e| e.dedup_key == k).unwrap();
        assert_eq!(by("hm:a:gpt-x:").reported_cost_usd, Some(0.25));
        assert_eq!(by("hm:b:gpt-x:approval").reported_cost_usd, Some(0.5));
        assert_eq!(by("hm:c:gpt-x:title_generation").reported_cost_usd, None);
        assert_eq!(wm, 1791698780000);
        let e = by("hm:a:gpt-x:");
        assert_eq!(e.agent, Agent::Hermes);
        assert_eq!(e.ts.timestamp_millis(), 1791698778984);
        assert_eq!(
            e.tokens,
            Tokens { input: 100, output: 50, cache_read: 20, cache_write_5m: 7, cache_write_1h: 0, reasoning: 9 }
        );
    }

    #[test]
    fn hermes_uses_git_root_over_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let p = make_db(dir.path());
        let c = Connection::open(&p).unwrap();
        session(&c, "a", Some("/repo/sub"), Some("/repo"));
        session(&c, "b", Some("/only/cwd"), Some(""));
        session(&c, "c", None, None);
        for s in ["a", "b", "c", "orphan"] {
            usage(&c, s, None, 0.0, None, 1791698778.0);
        }
        drop(c);
        let evs = read_since(&p, 0).unwrap().events;
        let cwd = |sid: &str| evs.iter().find(|e| e.session_id == sid).unwrap().cwd.clone();
        assert_eq!(cwd("a"), "/repo");
        assert_eq!(cwd("b"), "/only/cwd");
        assert_eq!(cwd("c"), "");
        assert_eq!(cwd("orphan"), "");
    }

    #[test]
    fn hermes_watermark_filters_and_missing_db_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let p = make_db(dir.path());
        let c = Connection::open(&p).unwrap();
        session(&c, "a", Some("/w"), None);
        usage(&c, "a", None, 0.0, None, 1791698778.5);
        drop(c);
        let ReadBatch { events: evs, watermark: wm, .. } = read_since(&p, 1791698778000).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(wm, 1791698778500);
        let ReadBatch { events: evs, watermark: wm, .. } = read_since(&p, wm).unwrap();
        assert!(evs.is_empty());
        assert_eq!(wm, 1791698778500);
        let ReadBatch { events: evs, watermark: wm, .. } = read_since(&dir.path().join("none.db"), 42).unwrap();
        assert!(evs.is_empty());
        assert_eq!(wm, 42);
    }

    #[test]
    fn hermes_null_columns_are_skipped_and_counted() {
        let dir = tempfile::tempdir().unwrap();
        let p = make_db(dir.path());
        let c = Connection::open(&p).unwrap();
        session(&c, "a", Some("/w"), None);
        usage(&c, "a", None, 0.0, None, 1791698778.0);
        c.execute(
            "INSERT INTO session_model_usage (session_id, model, last_seen) VALUES (NULL,'m',1791698790.0)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO session_model_usage (session_id, model, last_seen) VALUES ('a',NULL,1791698791.0)",
            [],
        )
        .unwrap();
        drop(c);
        let b = read_since(&p, 0).unwrap();
        assert_eq!(b.events.len(), 1);
        assert_eq!(b.skipped, 2);
        assert_eq!(b.watermark, 1791698791000);
    }
}

use std::path::Path;

use anyhow::Result;
use chrono::DateTime;
use serde_json::Value;

use super::ReadBatch;
use crate::model::{Agent, Tokens, UsageEvent};

/// Reads OpenCode assistant messages whose `time_updated` (ms) is above `watermark`.
/// Assistant rows are updated while streaming, so a row can be returned again with the same
/// `dedup_key`; the caller must persist with `Store::upsert_events`, not `insert_events`.
/// Malformed rows (NULL/wrong-typed columns or invalid JSON) are skipped and counted; the
/// watermark still advances past them.
pub fn read_since(db: &Path, watermark: i64) -> Result<ReadBatch> {
    let mut batch = ReadBatch { watermark, ..Default::default() };
    let Some(conn) = super::open_read_only(db)? else { return Ok(batch) };
    // No JSON functions in SQL: an invalid `data` value would abort the whole query.
    let mut stmt = conn.prepare(
        "SELECT id, session_id, time_updated, data FROM message WHERE time_updated > ?1 ORDER BY time_updated",
    )?;
    let mut rows = stmt.query([watermark])?;
    while let Some(r) = rows.next()? {
        let ms: i64 = r.get(2)?;
        batch.watermark = batch.watermark.max(ms);
        let id = r.get::<_, Option<String>>(0).ok().flatten();
        let session_id = r.get::<_, Option<String>>(1).ok().flatten();
        let data = r.get::<_, Option<String>>(3).ok().flatten();
        let (Some(id), Some(session_id), Some(data)) = (id, session_id, data) else {
            batch.skipped += 1;
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&data) else {
            batch.skipped += 1;
            continue;
        };
        if v.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let n = |path: &[&str]| -> u64 {
            path.iter().try_fold(&v, |cur, k| cur.get(k)).and_then(Value::as_u64).unwrap_or(0)
        };
        batch.events.push(UsageEvent {
            ts: DateTime::from_timestamp_millis(ms).unwrap_or_default(),
            agent: Agent::OpenCode,
            model: v.get("modelID").and_then(Value::as_str).unwrap_or_default().to_owned(),
            cwd: v.pointer("/path/cwd").and_then(Value::as_str).unwrap_or_default().to_owned(),
            session_id,
            tokens: Tokens {
                input: n(&["tokens", "input"]),
                output: n(&["tokens", "output"]),
                cache_read: n(&["tokens", "cache", "read"]),
                cache_write_5m: n(&["tokens", "cache", "write"]),
                cache_write_1h: 0,
                reasoning: n(&["tokens", "reasoning"]),
            },
            reported_cost_usd: v.get("cost").and_then(Value::as_f64).filter(|c| *c > 0.0),
            dedup_key: format!("oc:{id}"),
        });
    }
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

    fn make_db(dir: &Path) -> std::path::PathBuf {
        let p = dir.join("opencode.db");
        Connection::open(&p)
            .unwrap()
            .execute_batch(
                "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER,
                   time_updated INTEGER, data TEXT);",
            )
            .unwrap();
        p
    }

    fn msg(c: &Connection, id: &str, t: i64, data: &str) {
        c.execute("INSERT INTO message VALUES (?1,'ses_1',?2,?2,?3)", params![id, t, data]).unwrap();
    }

    const ASSISTANT: &str = r#"{"role":"assistant","modelID":"big-pickle","providerID":"opencode","cost":0,
        "tokens":{"total":8781,"input":8742,"output":11,"reasoning":28,"cache":{"write":5,"read":3}},
        "path":{"cwd":"/Users/dev/Documents/Acme/pos-backoffice"}}"#;

    #[test]
    fn opencode_zero_cost_becomes_none() {
        let dir = tempfile::tempdir().unwrap();
        let p = make_db(dir.path());
        let c = Connection::open(&p).unwrap();
        msg(&c, "msg_a", 1782120256885, ASSISTANT);
        msg(&c, "msg_b", 1782120256999, &ASSISTANT.replace("\"cost\":0", "\"cost\":0.0123"));
        drop(c);
        let ReadBatch { events: evs, watermark: wm, .. } = read_since(&p, 0).unwrap();
        assert_eq!(wm, 1782120256999);
        assert_eq!(evs.len(), 2);
        let a = evs.iter().find(|e| e.dedup_key == "oc:msg_a").unwrap();
        assert_eq!(a.reported_cost_usd, None);
        assert_eq!(a.model, "big-pickle");
        assert_eq!(a.agent, Agent::OpenCode);
        assert_eq!(a.session_id, "ses_1");
        assert_eq!(a.cwd, "/Users/dev/Documents/Acme/pos-backoffice");
        assert_eq!(a.ts.timestamp_millis(), 1782120256885);
        assert_eq!(
            a.tokens,
            Tokens { input: 8742, output: 11, cache_read: 3, cache_write_5m: 5, cache_write_1h: 0, reasoning: 28 }
        );
        let b = evs.iter().find(|e| e.dedup_key == "oc:msg_b").unwrap();
        assert_eq!(b.reported_cost_usd, Some(0.0123));
    }

    #[test]
    fn opencode_reads_only_assistant_rows() {
        let dir = tempfile::tempdir().unwrap();
        let p = make_db(dir.path());
        let c = Connection::open(&p).unwrap();
        msg(&c, "msg_user", 1782120256000, r#"{"role":"user"}"#);
        msg(&c, "msg_a", 1782120256885, ASSISTANT);
        drop(c);
        let evs = read_since(&p, 0).unwrap().events;
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].dedup_key, "oc:msg_a");
        let ReadBatch { events: evs, watermark: wm, .. } = read_since(&p, 1782120256885).unwrap();
        assert!(evs.is_empty());
        assert_eq!(wm, 1782120256885);
    }

    #[test]
    fn opencode_missing_db_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let ReadBatch { events: evs, watermark: wm, .. } = read_since(&dir.path().join("none.db"), 7).unwrap();
        assert!(evs.is_empty());
        assert_eq!(wm, 7);
    }

    #[test]
    fn opencode_malformed_json_is_skipped_and_counted() {
        let dir = tempfile::tempdir().unwrap();
        let p = make_db(dir.path());
        let c = Connection::open(&p).unwrap();
        msg(&c, "msg_a", 1782120256885, ASSISTANT);
        msg(&c, "msg_bad", 1782120256900, "{not json");
        msg(&c, "msg_b", 1782120256950, ASSISTANT);
        drop(c);
        let b = read_since(&p, 0).unwrap();
        assert_eq!(b.events.len(), 2);
        assert_eq!(b.skipped, 1);
        assert_eq!(b.watermark, 1782120256950);
    }

    #[test]
    fn opencode_null_columns_are_skipped_and_counted() {
        let dir = tempfile::tempdir().unwrap();
        let p = make_db(dir.path());
        let c = Connection::open(&p).unwrap();
        msg(&c, "msg_a", 1782120256885, ASSISTANT);
        c.execute("INSERT INTO message VALUES ('msg_nodata','ses_1',1782120256900,1782120256900,NULL)", []).unwrap();
        c.execute("INSERT INTO message VALUES ('msg_nosess',NULL,1782120256910,1782120256910,?1)", [ASSISTANT]).unwrap();
        drop(c);
        let b = read_since(&p, 0).unwrap();
        assert_eq!(b.events.len(), 1);
        assert_eq!(b.skipped, 2);
        assert_eq!(b.watermark, 1782120256910);
    }
}

use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{jsonl, ScanStats};
use crate::model::{Agent, Tokens, UsageEvent};
use crate::store::{CostFn, Store};

/// Parses one Claude Code log line into a usage event.
/// Returns None for malformed JSON, non-assistant lines, synthetic messages and empty usage.
pub fn parse_line(line: &str, path: &Path) -> Option<UsageEvent> {
    let v: Value = serde_json::from_str(line).ok()?;
    parse_value(&v, path)
}

fn parse_value(v: &Value, path: &Path) -> Option<UsageEvent> {
    if v.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let msg = v.get("message")?;
    let usage = msg.get("usage")?;
    let model = msg.get("model")?.as_str()?;
    if model == "<synthetic>" {
        return None;
    }
    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);

    let cw_total = n("cache_creation_input_tokens");
    let (cw_5m, cw_1h) = match usage.get("cache_creation") {
        Some(cc) => (
            cc.get("ephemeral_5m_input_tokens").and_then(Value::as_u64).unwrap_or(0),
            cc.get("ephemeral_1h_input_tokens").and_then(Value::as_u64).unwrap_or(0),
        ),
        None => (cw_total, 0),
    };
    let tokens = Tokens {
        input: n("input_tokens"),
        output: n("output_tokens"),
        cache_read: n("cache_read_input_tokens"),
        cache_write_5m: cw_5m,
        cache_write_1h: cw_1h,
        reasoning: 0,
    };
    if tokens.input + tokens.output + tokens.cache_read + tokens.cache_write_5m + tokens.cache_write_1h == 0 {
        return None;
    }

    let ts = DateTime::parse_from_rfc3339(v.get("timestamp")?.as_str()?).ok()?.with_timezone(&Utc);
    let msg_id = msg.get("id")?.as_str()?;
    let request_id = v.get("requestId").and_then(Value::as_str).unwrap_or("");
    let session_id = subagent_parent_session(path)
        .or_else(|| v.get("sessionId").and_then(Value::as_str).map(str::to_owned))?;

    Some(UsageEvent {
        ts,
        agent: Agent::ClaudeCode,
        model: model.to_owned(),
        cwd: v.get("cwd").and_then(Value::as_str).unwrap_or("").to_owned(),
        session_id,
        tokens,
        reported_cost_usd: None,
        dedup_key: format!("cc:{msg_id}:{request_id}"),
    })
}

/// For `.../<sessionId>/subagents/agent-*.jsonl` returns `<sessionId>`.
fn subagent_parent_session(path: &Path) -> Option<String> {
    let dir = path.parent()?;
    if dir.file_name()? != "subagents" {
        return None;
    }
    Some(dir.parent()?.file_name()?.to_string_lossy().into_owned())
}

/// All `.jsonl` logs under `root` (the `projects/` dir), including `subagents/`. Sorted.
pub fn log_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "jsonl") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Reads new lines of one file since the stored cursor and parses them.
/// Does not persist the cursor: the caller does so after inserting the events.
pub fn scan_file(store: &Store, path: &Path) -> Result<(Vec<UsageEvent>, crate::store::FileCursor, ScanStats)> {
    let cursor = store.get_cursor(&path.to_string_lossy())?;
    let (lines, new_cursor) = jsonl::read_new_lines(path, cursor.as_ref())?;
    let mut stats = ScanStats::default();
    let mut events = Vec::new();
    for line in &lines {
        stats.lines += 1;
        match serde_json::from_str::<Value>(line) {
            Ok(v) => {
                if let Some(ev) = parse_value(&v, path) {
                    events.push(ev);
                }
            }
            Err(_) => stats.skipped += 1,
        }
    }
    stats.events = events.len() as u64;
    Ok((events, new_cursor, stats))
}

/// Incrementally ingests every log under `root` into the store, advancing cursors.
/// An unreadable file is counted as skipped and does not abort the pass.
pub fn scan_all(store: &Store, root: &Path, cost: CostFn) -> Result<ScanStats> {
    let mut total = ScanStats::default();
    for path in log_files(root) {
        let Ok((events, cursor, s)) = scan_file(store, &path) else {
            total.skipped += 1;
            continue;
        };
        store.insert_events(&events, cost)?;
        store.set_cursor(&cursor)?;
        total.lines += s.lines;
        total.skipped += s.skipped;
        total.events += s.events;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/claude");

    fn fixture_lines() -> Vec<String> {
        std::fs::read_to_string(format!("{FIX}/session.jsonl")).unwrap().lines().map(str::to_owned).collect()
    }

    fn session_path() -> PathBuf {
        PathBuf::from(format!("{FIX}/session.jsonl"))
    }

    #[test]
    fn parses_assistant_usage() {
        let ev = parse_line(&fixture_lines()[1], &session_path()).unwrap();
        assert_eq!(ev.agent, Agent::ClaudeCode);
        assert_eq!(ev.model, "claude-fable-5");
        assert_eq!(ev.cwd, "/work/app");
        assert_eq!(ev.session_id, "sess-1");
        assert_eq!(ev.ts.to_rfc3339(), "2026-07-06T06:03:18.278+00:00");
        assert_eq!(
            ev.tokens,
            Tokens { input: 21741, output: 571, cache_read: 16056, cache_write_5m: 0, cache_write_1h: 4484, reasoning: 0 }
        );
        assert_eq!(ev.dedup_key, "cc:msg_A:req_A");
        assert_eq!(ev.reported_cost_usd, None);
    }

    #[test]
    fn cache_creation_without_breakdown_counts_as_5m() {
        let line = r#"{"type":"assistant","sessionId":"s","requestId":"r","timestamp":"2026-07-06T06:03:18Z","message":{"id":"m","model":"x","usage":{"input_tokens":1,"cache_creation_input_tokens":50,"output_tokens":2}}}"#;
        let ev = parse_line(line, Path::new("/p/s.jsonl")).unwrap();
        assert_eq!((ev.tokens.cache_write_5m, ev.tokens.cache_write_1h), (50, 0));
    }

    #[test]
    fn ignores_non_assistant_and_lines_without_usage() {
        let lines = fixture_lines();
        let p = session_path();
        assert!(parse_line(&lines[0], &p).is_none(), "user line");
        assert!(parse_line(&lines[4], &p).is_none(), "synthetic / zero tokens");
        assert!(parse_line(&lines[5], &p).is_none(), "no usage");
        assert!(parse_line(&lines[6], &p).is_none(), "malformed");
    }

    #[test]
    fn streaming_duplicates_share_dedup_key() {
        let lines = fixture_lines();
        let p = session_path();
        let keys: Vec<_> = lines[1..4].iter().map(|l| parse_line(l, &p).unwrap().dedup_key).collect();
        assert_eq!(keys, ["cc:msg_A:req_A"; 3]);
    }

    #[test]
    fn subagent_file_maps_to_parent_session() {
        let p = PathBuf::from(format!("{FIX}/abc-123/subagents/agent-x.jsonl"));
        let line = std::fs::read_to_string(&p).unwrap();
        let ev = parse_line(line.trim(), &p).unwrap();
        assert_eq!(ev.session_id, "abc-123");
        assert_eq!(ev.tokens.cache_write_5m, 100);
    }

    #[test]
    fn log_files_finds_nested_subagent_logs() {
        let files = log_files(Path::new(FIX));
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.ends_with("abc-123/subagents/agent-x.jsonl")));
        assert!(files.iter().any(|f| f.ends_with("session.jsonl")));
    }

    #[test]
    fn scan_all_is_incremental_dedups_and_counts_skipped() {
        let store = Store::open_in_memory().unwrap();
        let cost = |_: &UsageEvent| None;
        let stats = scan_all(&store, Path::new(FIX), &cost).unwrap();
        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.events, 4); // 3 streaming copies + 1 subagent

        // Second pass reads nothing new.
        let again = scan_all(&store, Path::new(FIX), &cost).unwrap();
        assert_eq!((again.lines, again.events, again.skipped), (0, 0, 0));
    }
}

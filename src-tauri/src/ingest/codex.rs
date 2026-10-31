use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{jsonl, ScanStats};
use crate::model::{Agent, Tokens, UsageEvent};
use crate::store::{CostFn, FileCursor, Store};

#[derive(Clone, Debug, PartialEq)]
pub struct RawWindow {
    pub used_pct: f64,
    pub window_minutes: u32,
    pub resets_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CodexRateLimits {
    pub observed_at: DateTime<Utc>,
    pub plan: Option<String>,
    pub primary: Option<RawWindow>,
    pub secondary: Option<RawWindow>,
}

/// Per-file parsing state, kept in memory between scans.
#[derive(Clone, Debug, Default)]
pub struct CodexCtx {
    pub session_id: String,
    pub cwd: String,
    pub model: String,
    pub seq: u64,
    /// Previous cumulative `total_token_usage` [input, cached, output, reasoning].
    prev_total: [u64; 4],
    /// True once the file has been replayed from its start in this process.
    primed: bool,
}

pub fn parse_file_lines(lines: &[String], ctx: &mut CodexCtx, path: &Path) -> (Vec<UsageEvent>, Option<CodexRateLimits>) {
    let (events, rl, _) = parse_lines(lines, 0, ctx, path);
    (events, rl)
}

fn field_array(v: &Value) -> [u64; 4] {
    let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
    [n("input_tokens"), n("cached_input_tokens"), n("output_tokens"), n("reasoning_output_tokens")]
}

fn parse_window(v: Option<&Value>) -> Option<RawWindow> {
    let v = v?;
    Some(RawWindow {
        used_pct: v.get("used_percent")?.as_f64()?,
        window_minutes: u32::try_from(v.get("window_minutes")?.as_u64()?).ok()?,
        resets_at: DateTime::from_timestamp(v.get("resets_at")?.as_i64()?, 0)?,
    })
}

fn parse_rate_limits(rl: &Value, observed_at: DateTime<Utc>) -> Option<CodexRateLimits> {
    if !rl.is_object() {
        return None;
    }
    Some(CodexRateLimits {
        observed_at,
        plan: rl.get("plan_type").and_then(Value::as_str).map(str::to_owned),
        primary: parse_window(rl.get("primary")),
        secondary: parse_window(rl.get("secondary")),
    })
}

/// Parses lines in order, updating `ctx`. Lines before `emit_from` only update the context
/// (used to rebuild it after a restart). Returns events, the latest rate limits and the
/// number of malformed lines.
fn parse_lines(
    lines: &[String],
    emit_from: usize,
    ctx: &mut CodexCtx,
    path: &Path,
) -> (Vec<UsageEvent>, Option<CodexRateLimits>, u64) {
    let mut events = Vec::new();
    let mut latest = None;
    let mut skipped = 0;
    for (i, line) in lines.iter().enumerate() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            if i >= emit_from {
                skipped += 1;
            }
            continue;
        };
        let Some(payload) = v.get("payload") else { continue };
        match v.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                if let Some(id) = payload.get("id").and_then(Value::as_str) {
                    ctx.session_id = id.to_owned();
                }
                if let Some(cwd) = payload.get("cwd").and_then(Value::as_str) {
                    ctx.cwd = cwd.to_owned();
                }
            }
            Some("turn_context") => {
                if let Some(m) = payload.get("model").and_then(Value::as_str) {
                    ctx.model = m.to_owned();
                }
                if let Some(cwd) = payload.get("cwd").and_then(Value::as_str) {
                    ctx.cwd = cwd.to_owned();
                }
            }
            Some("event_msg") if payload.get("type").and_then(Value::as_str) == Some("token_count") => {
                let ts_str = v.get("timestamp").and_then(Value::as_str);
                let ts = ts_str.and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|t| t.with_timezone(&Utc));
                if let (Some(ts), Some(rl)) = (ts, payload.get("rate_limits"))
                    && let Some(r) = parse_rate_limits(rl, ts)
                {
                    latest = Some(r);
                }
                let Some(info) = payload.get("info").filter(|i| i.is_object()) else { continue };
                let Some(total_v) = info.get("total_token_usage") else { continue };
                let total = field_array(total_v);
                let prev = std::mem::replace(&mut ctx.prev_total, total);
                let usage = if total == prev {
                    continue;
                } else if total.iter().zip(&prev).any(|(n, p)| n < p) {
                    // Counter reset: the delta is meaningless, trust the per-turn usage.
                    info.get("last_token_usage").map(field_array).unwrap_or(total)
                } else {
                    [total[0] - prev[0], total[1] - prev[1], total[2] - prev[2], total[3] - prev[3]]
                };
                if i < emit_from {
                    continue;
                }
                let (Some(ts), Some(ts_str)) = (ts, ts_str) else { continue };
                if usage.iter().all(|&x| x == 0) {
                    continue;
                }
                let session_id = if ctx.session_id.is_empty() {
                    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
                } else {
                    ctx.session_id.clone()
                };
                ctx.seq += 1;
                events.push(UsageEvent {
                    ts,
                    agent: Agent::Codex,
                    model: ctx.model.clone(),
                    cwd: ctx.cwd.clone(),
                    dedup_key: format!("cx:{session_id}:{ts_str}"),
                    session_id,
                    // Codex `input_tokens` already includes the cached part; pricing accounts for that.
                    tokens: Tokens {
                        input: usage[0],
                        cache_read: usage[1],
                        output: usage[2],
                        reasoning: usage[3],
                        ..Tokens::default()
                    },
                    reported_cost_usd: None,
                });
            }
            _ => {}
        }
    }
    (events, latest, skipped)
}

/// All `.jsonl` logs under `root` (`sessions/` or `archived_sessions/`). Sorted.
/// Symlinks are not followed.
pub fn log_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() && p.extension().is_some_and(|x| x == "jsonl") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Reads new lines of one file since the stored cursor and parses them with `ctx`.
/// If `ctx` has not seen this file in this process (app restart), the file is replayed from
/// the start to rebuild it, and only lines past the stored cursor produce events.
/// Does not persist the cursor: the caller does so after inserting the events.
pub fn scan_file(
    store: &Store,
    path: &Path,
    ctx: &mut CodexCtx,
) -> Result<(Vec<UsageEvent>, FileCursor, ScanStats, Option<CodexRateLimits>)> {
    let cursor = store.get_cursor(&path.to_string_lossy())?;
    let mut emit_from = 0;
    let (lines, new_cursor) = match cursor.as_ref().filter(|c| c.offset > 0 && !ctx.primed) {
        Some(c) => {
            let (lines, nc) = jsonl::read_new_lines(path, None)?;
            let mut end = 0u64;
            emit_from = lines
                .iter()
                .take_while(|l| {
                    end += l.len() as u64 + 1;
                    end <= c.offset
                })
                .count();
            (lines, nc)
        }
        None => {
            let (lines, nc) = jsonl::read_new_lines(path, cursor.as_ref())?;
            if cursor.as_ref().is_some_and(|c| nc.size < c.offset) {
                *ctx = CodexCtx::default(); // truncated/rotated: re-read from start
            }
            (lines, nc)
        }
    };
    ctx.primed = true;
    let (events, rl, skipped) = parse_lines(&lines, emit_from, ctx, path);
    let stats = ScanStats { lines: (lines.len() - emit_from) as u64, skipped, events: events.len() as u64 };
    Ok((events, new_cursor, stats, rl))
}

/// Incrementally ingests every log under `root`, advancing cursors, and returns the most
/// recently observed rate limits. An unreadable file counts as skipped and does not abort the pass.
/// `ctxs` holds per-file parsing state; pass the same map on every pass.
pub fn scan_all(
    store: &Store,
    root: &Path,
    cost: CostFn,
    ctxs: &mut HashMap<PathBuf, CodexCtx>,
) -> Result<(ScanStats, Option<CodexRateLimits>)> {
    let mut total = ScanStats::default();
    let mut latest: Option<CodexRateLimits> = None;
    for path in log_files(root) {
        let ctx = ctxs.entry(path.clone()).or_default();
        let Ok((events, cursor, s, rl)) = scan_file(store, &path, ctx) else {
            total.skipped += 1;
            continue;
        };
        store.insert_events(&events, cost)?;
        store.set_cursor(&cursor)?;
        total.lines += s.lines;
        total.skipped += s.skipped;
        total.events += s.events;
        if let Some(r) = rl
            && latest.as_ref().is_none_or(|l| r.observed_at > l.observed_at)
        {
            latest = Some(r);
        }
    }
    Ok((total, latest))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/codex");

    fn session_path() -> PathBuf {
        PathBuf::from(format!("{FIX}/2026/07/09/rollout-2026-07-09T04-27-25-aaaa.jsonl"))
    }

    fn fixture_lines() -> Vec<String> {
        std::fs::read_to_string(session_path()).unwrap().lines().map(str::to_owned).collect()
    }

    fn tc(ts: &str, total: [u64; 4], last: [u64; 4]) -> String {
        let u = |a: [u64; 4]| {
            format!(
                r#"{{"input_tokens":{},"cached_input_tokens":{},"output_tokens":{},"reasoning_output_tokens":{}}}"#,
                a[0], a[1], a[2], a[3]
            )
        };
        format!(
            r#"{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{},"total_token_usage":{}}}}}}}"#,
            u(last),
            u(total)
        )
    }

    fn meta() -> String {
        r#"{"type":"session_meta","timestamp":"2026-07-09T00:00:00Z","payload":{"id":"s1","cwd":"/c"}}"#.to_owned()
    }

    fn turn(model: &str) -> String {
        format!(r#"{{"type":"turn_context","timestamp":"2026-07-09T00:00:01Z","payload":{{"model":"{model}","cwd":"/c"}}}}"#)
    }

    #[test]
    fn token_count_becomes_event_with_turn_model() {
        let mut ctx = CodexCtx::default();
        let (evs, _) = parse_file_lines(&fixture_lines(), &mut ctx, &session_path());
        assert_eq!(evs.len(), 2);
        let ev = &evs[0];
        assert_eq!(ev.agent, Agent::Codex);
        assert_eq!(ev.model, "gpt-5.5");
        assert_eq!(ev.cwd, "/work/app");
        assert_eq!(ev.session_id, "sess-cx-1");
        assert_eq!(ev.ts.to_rfc3339(), "2026-07-09T04:27:37.484+00:00");
        assert_eq!(
            ev.tokens,
            Tokens { input: 1000, output: 50, cache_read: 800, cache_write_5m: 0, cache_write_1h: 0, reasoning: 20 }
        );
        assert_eq!(ev.dedup_key, "cx:sess-cx-1:2026-07-09T04:27:37.484Z");
        assert_eq!(ev.reported_cost_usd, None);
    }

    #[test]
    fn rate_limits_primary_is_5h_secondary_weekly() {
        let mut ctx = CodexCtx::default();
        let (_, rl) = parse_file_lines(&fixture_lines(), &mut ctx, &session_path());
        let rl = rl.unwrap();
        assert_eq!(rl.plan.as_deref(), Some("plus"));
        assert_eq!(rl.observed_at.to_rfc3339(), "2026-07-09T04:27:37.484+00:00");
        let p = rl.primary.unwrap();
        assert_eq!((p.used_pct, p.window_minutes, p.resets_at.timestamp()), (1.0, 300, 1783588087));
        let s = rl.secondary.unwrap();
        assert_eq!((s.used_pct, s.window_minutes, s.resets_at.timestamp()), (39.0, 10080, 1783997684));
    }

    #[test]
    fn token_count_without_info_is_skipped() {
        let mut ctx = CodexCtx::default();
        let line = r#"{"type":"event_msg","timestamp":"2026-07-09T00:00:02Z","payload":{"type":"token_count","info":null,"rate_limits":{"plan_type":"pro","primary":{"used_percent":7.0,"window_minutes":300,"resets_at":1783588087}}}}"#;
        let (evs, rl) = parse_file_lines(&[meta(), turn("m"), line.to_owned()], &mut ctx, Path::new("/p/x.jsonl"));
        assert!(evs.is_empty());
        let rl = rl.expect("rate limits still observed when info is null");
        assert_eq!(rl.plan.as_deref(), Some("pro"));
        assert!(rl.secondary.is_none());
    }

    #[test]
    fn ctx_survives_across_scans() {
        let mut ctx = CodexCtx::default();
        let p = Path::new("/p/x.jsonl");
        let (e1, _) = parse_file_lines(&[meta(), turn("gpt-5.5"), tc("2026-07-09T00:00:02Z", [10, 0, 1, 0], [10, 0, 1, 0])], &mut ctx, p);
        let (e2, _) = parse_file_lines(&[tc("2026-07-09T00:00:03Z", [30, 5, 3, 0], [20, 5, 2, 0])], &mut ctx, p);
        assert_eq!((e1.len(), e2.len()), (1, 1));
        assert_eq!(e2[0].model, "gpt-5.5");
        assert_eq!(e2[0].session_id, "s1");
        assert_eq!(e2[0].cwd, "/c");
    }

    #[test]
    fn repeated_total_is_skipped() {
        let mut ctx = CodexCtx::default();
        let l = tc("2026-07-09T00:00:02Z", [10, 4, 2, 1], [10, 4, 2, 1]);
        let l2 = tc("2026-07-09T00:00:03Z", [10, 4, 2, 1], [10, 4, 2, 1]);
        let (evs, _) = parse_file_lines(&[meta(), turn("m"), l, l2], &mut ctx, Path::new("/p/x.jsonl"));
        assert_eq!(evs.len(), 1);
    }

    #[test]
    fn usage_is_delta_of_totals() {
        let mut ctx = CodexCtx::default();
        let a = tc("2026-07-09T00:00:02Z", [100, 40, 10, 4], [100, 40, 10, 4]);
        let b = tc("2026-07-09T00:00:03Z", [250, 100, 30, 9], [999, 999, 999, 999]);
        let (evs, _) = parse_file_lines(&[meta(), turn("m"), a, b], &mut ctx, Path::new("/p/x.jsonl"));
        assert_eq!(evs.len(), 2);
        assert_eq!(
            evs[1].tokens,
            Tokens { input: 150, output: 20, cache_read: 60, cache_write_5m: 0, cache_write_1h: 0, reasoning: 5 }
        );
    }

    #[test]
    fn total_decrease_falls_back_to_last_usage() {
        let mut ctx = CodexCtx::default();
        let a = tc("2026-07-09T00:00:02Z", [500, 100, 50, 5], [500, 100, 50, 5]);
        let b = tc("2026-07-09T00:00:03Z", [30, 10, 3, 1], [30, 10, 3, 1]);
        let c = tc("2026-07-09T00:00:04Z", [40, 12, 5, 1], [10, 2, 2, 0]);
        let (evs, _) = parse_file_lines(&[meta(), turn("m"), a, b, c], &mut ctx, Path::new("/p/x.jsonl"));
        assert_eq!(evs.len(), 3);
        assert_eq!(evs[1].tokens.input, 30, "reset uses last_token_usage");
        assert_eq!(evs[2].tokens.input, 10, "baseline is the post-reset total");
    }

    #[test]
    fn log_files_finds_sessions_and_archived() {
        let files = log_files(Path::new(FIX));
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.ends_with("2026/07/09/rollout-2026-07-09T04-27-25-aaaa.jsonl")));
        assert!(files.iter().any(|f| f.ends_with("archived_sessions/rollout-2026-06-01T00-00-00-bbbb.jsonl")));
    }

    #[test]
    fn scan_all_is_incremental_and_returns_latest_rate_limits() {
        let store = Store::open_in_memory().unwrap();
        let cost = |_: &UsageEvent| None;
        let mut ctxs = HashMap::new();
        let (stats, rl) = scan_all(&store, Path::new(FIX), &cost, &mut ctxs).unwrap();
        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.events, 3);
        assert_eq!(rl.unwrap().primary.unwrap().used_pct, 1.0);

        let (again, _) = scan_all(&store, Path::new(FIX), &cost, &mut ctxs).unwrap();
        assert_eq!((again.lines, again.events, again.skipped), (0, 0, 0));
    }

    #[test]
    fn restart_rebuilds_ctx_without_reemitting_old_events() {
        let dir = std::env::temp_dir().join(format!("bt-codex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("rollout-x.jsonl");
        let first = format!("{}\n{}\n{}\n", meta(), turn("gpt-5.5"), tc("2026-07-09T00:00:02Z", [10, 0, 1, 0], [10, 0, 1, 0]));
        std::fs::write(&f, &first).unwrap();
        let store = Store::open_in_memory().unwrap();
        let cost = |_: &UsageEvent| None;
        let (s1, _) = scan_all(&store, &dir, &cost, &mut HashMap::new()).unwrap();
        assert_eq!(s1.events, 1);

        let more = format!("{first}{}\n", tc("2026-07-09T00:00:03Z", [30, 5, 3, 0], [20, 5, 2, 0]));
        std::fs::write(&f, more).unwrap();
        // Fresh ctx = app restarted; the stored cursor is kept.
        let (evs, _, s2, _) = scan_file(&store, &f, &mut CodexCtx::default()).unwrap();
        assert_eq!((s2.lines, s2.events), (1, 1));
        let ev = &evs[0];
        assert_eq!(ev.tokens, Tokens { input: 20, output: 2, cache_read: 5, cache_write_5m: 0, cache_write_1h: 0, reasoning: 0 }, "delta, not full total");
        assert_eq!(ev.model, "gpt-5.5");
        assert_eq!(ev.session_id, "s1");
        assert_eq!(ev.cwd, "/c");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

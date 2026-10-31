//! Cursor usage history from the dashboard's filtered-usage-events endpoint.
//!
//! Conversation contents are never read: `~/.cursor/projects` is only listed (directory entry
//! names) to map a conversation id to its project folder.

use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Result};
use chrono::DateTime;

use crate::model::{Agent, Tokens, UsageEvent};
use crate::quota::claude::QuotaError;
use crate::quota::cursor::{status_result, CursorAuth};

/// 2023-01-01 00:00:00 UTC.
pub const FIRST_SYNC_START_MS: i64 = 1_672_531_200_000;
/// Re-requested on every sync so events that landed late are not missed; `dedup_key` absorbs repeats.
pub const OVERLAP_MS: i64 = 86_400_000;

const PAGE_SIZE: usize = 100;
const MAX_PAGES: u32 = 200;
const MAX_DEPTH: usize = 12;

#[derive(Debug, Default)]
pub struct EventPage {
    pub events: Vec<UsageEvent>,
    pub skipped: u64,
    pub total: u64,
    pub raw_count: usize,
}

#[derive(Debug, Default, PartialEq)]
pub struct SyncStats {
    pub events: u64,
    pub skipped: u64,
    pub max_ts_ms: Option<i64>,
    /// True when the 200-page cap stopped the sync with more pages possibly left. The caller still
    /// advances the watermark (see [`next_watermark`]); older pages beyond the cap are left behind.
    pub truncated: bool,
}

pub fn parse_events(json: &str, projects: &HashMap<String, String>) -> Result<EventPage> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    let total = v.get("totalUsageEventsCount").and_then(|t| t.as_u64()).unwrap_or(0);
    // A range without events comes back without the list; a list missing despite a count is a changed API.
    let none = Vec::new();
    let rows = match v.get("usageEventsDisplay") {
        Some(r) => r.as_array().ok_or_else(|| anyhow!("usageEventsDisplay is not a list"))?,
        None if v.is_object() && total == 0 => &none,
        None => return Err(anyhow!("usageEventsDisplay missing")),
    };
    let mut page = EventPage {
        total,
        raw_count: rows.len(),
        ..Default::default()
    };
    for row in rows {
        match parse_event(row, projects) {
            Some(e) => page.events.push(e),
            None => page.skipped += 1,
        }
    }
    Ok(page)
}

/// Cursor lists one model per effort level (`claude-opus-5-5-medium`, `-high`); they are the
/// same model for usage and price, so the effort suffix is dropped. `-max` is a different model.
fn strip_effort(model: &str) -> &str {
    for suffix in ["-low", "-medium", "-high"] {
        if let Some(base) = model.strip_suffix(suffix).filter(|b| !b.is_empty()) {
            return base;
        }
    }
    model
}

/// A full page whose rows all failed to parse means the response format changed. A short page
/// with no valid rows is just the end of the history.
fn page_unreadable(page: &EventPage) -> bool {
    page.raw_count >= PAGE_SIZE && page.events.is_empty()
}

/// How long history sync rests after a failed one. The quota poll is not affected.
pub const HISTORY_BACKOFF: Duration = Duration::from_secs(3600);
/// The shorter rest after a transient failure (5xx, 429, offline) that retries did not clear.
pub const TRANSIENT_BACKOFF: Duration = Duration::from_secs(600);

/// 5xx, 429 and offline are worth trying again soon; everything else is not.
fn is_transient(err: &QuotaError) -> bool {
    matches!(err, QuotaError::Offline | QuotaError::Http(429)) || matches!(err, QuotaError::Http(c) if *c >= 500)
}

/// How long history rests after `err` stopped a sync.
pub fn history_backoff(err: &QuotaError) -> Duration {
    if is_transient(err) { TRANSIENT_BACKOFF } else { HISTORY_BACKOFF }
}

/// `since_failure` is the time since the last failed history sync, if there is one; `backoff`
/// is the rest that failure earned.
pub fn history_due(since_failure: Option<Duration>, backoff: Duration) -> bool {
    since_failure.is_none_or(|d| d >= backoff)
}

/// Each history slice covers this much time, so progress is saved every 30 days of history.
pub const SLICE_MS: i64 = 30 * 86_400_000;

/// The next slice to sync as `(start, end, is_final)`; the final one ends at `now_ms`.
pub fn next_slice(watermark: i64, now_ms: i64) -> (i64, i64, bool) {
    let start = if watermark == 0 { FIRST_SYNC_START_MS } else { watermark - OVERLAP_MS };
    let end = (start + SLICE_MS).min(now_ms);
    (start, end, end == now_ms)
}

/// Where the watermark goes after a slice. A finished non-final slice moves to its end (even
/// when empty); the final one uses [`next_watermark`]. The result never goes below `current` (the
/// overlap can make the newest event older). A store write failure never gets here: the caller stops first.
pub fn slice_watermark(current: i64, end_ms: i64, is_final: bool, stats: &SyncStats) -> Option<i64> {
    let target = if is_final { next_watermark(stats) } else { Some(end_ms) };
    target.map(|t| t.max(current))
}

/// How long to wait before retry number `attempt + 1` of a request that failed with `err`.
pub fn retry_delay(attempt: u32, err: &QuotaError) -> Option<Duration> {
    if !is_transient(err) {
        return None;
    }
    match attempt {
        0 => Some(Duration::from_secs(5)),
        1 => Some(Duration::from_secs(20)),
        _ => None,
    }
}

/// Runs `attempt`, retrying transient errors per [`retry_delay`]. `stop` is checked before each
/// wait and again after it, so a stop never costs another request. `sleep` holds no lock.
pub async fn retrying<T, A, AF, S, SF, P>(mut attempt: A, mut sleep: S, stop: P) -> Result<T, QuotaError>
where
    A: FnMut() -> AF,
    AF: Future<Output = Result<T, QuotaError>>,
    S: FnMut(Duration) -> SF,
    SF: Future<Output = ()>,
    P: Fn() -> bool,
{
    let mut n = 0;
    loop {
        let err = match attempt().await {
            Ok(v) => return Ok(v),
            Err(e) => e,
        };
        if stop() {
            return Err(err);
        }
        let Some(delay) = retry_delay(n, &err) else { return Err(err) };
        sleep(delay).await;
        if stop() {
            return Err(err);
        }
        n += 1;
    }
}

fn parse_event(row: &serde_json::Value, projects: &HashMap<String, String>) -> Option<UsageEvent> {
    let id = row.get("usageEventId")?.as_str().filter(|s| !s.is_empty())?;
    let ts_ms: i64 = row.get("timestamp")?.as_str()?.parse().ok()?;
    let ts = DateTime::from_timestamp_millis(ts_ms)?;
    let model = row.get("model")?.as_str().filter(|s| !s.is_empty())?;
    let usage = row.get("tokenUsage")?.as_object()?;
    // Absent, negative or wrong-typed counts read as 0.
    let tok = |k: &str| usage.get(k).and_then(|n| n.as_f64()).filter(|n| *n > 0.0).unwrap_or(0.0) as u64;
    let cents = usage.get("totalCents").and_then(|c| c.as_f64()).unwrap_or(0.0);
    // Without a conversation each event is its own session, not one giant merged one.
    let conversation = row.get("conversationId").and_then(|c| c.as_str()).filter(|c| !c.is_empty());
    let session_id = conversation.map_or_else(|| format!("cr-event:{id}"), str::to_string);
    Some(UsageEvent {
        ts,
        agent: Agent::Cursor,
        model: strip_effort(model).to_string(),
        cwd: conversation.and_then(|c| projects.get(c)).cloned().unwrap_or_default(),
        session_id,
        tokens: Tokens {
            input: tok("inputTokens"),
            output: tok("outputTokens"),
            cache_read: tok("cacheReadTokens"),
            cache_write_5m: tok("cacheWriteTokens"),
            cache_write_1h: 0,
            reasoning: 0,
        },
        reported_cost_usd: (cents > 0.0).then(|| cents / 100.0),
        dedup_key: format!("cr:{id}"),
    })
}

pub async fn fetch_page(
    http: &reqwest::Client,
    auth: &CursorAuth,
    base_url: &str,
    start_ms: i64,
    end_ms: i64,
    page: u32,
) -> Result<String, QuotaError> {
    // Dates are strings of epoch ms, as the dashboard sends them.
    let body = serde_json::json!({
        "teamId": 0,
        "startDate": start_ms.to_string(),
        "endDate": end_ms.to_string(),
        "page": page,
        "pageSize": PAGE_SIZE,
    });
    let resp = http
        .post(format!("{base_url}/api/dashboard/get-filtered-usage-events"))
        .header("Cookie", auth.cookie_header())
        .header("Origin", "https://cursor.com")
        .header("User-Agent", "buku-token")
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|_| QuotaError::Offline)?;
    status_result(resp.status().as_u16())?;
    resp.text().await.map_err(|_| QuotaError::Offline)
}

/// Pulls pages until a short one or [`MAX_PAGES`], handing each page's events to `sink` as it
/// arrives. On error, what was already sunk stays; the caller decides the watermark.
pub async fn sync<F, Fut, S>(
    mut fetch: F,
    projects: &HashMap<String, String>,
    mut sink: S,
    pause: Duration,
) -> Result<SyncStats, QuotaError>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<String, QuotaError>>,
    S: FnMut(&[UsageEvent]),
{
    let mut stats = SyncStats::default();
    for page_no in 1..=MAX_PAGES {
        if page_no > 1 && !pause.is_zero() {
            tokio::time::sleep(pause).await;
        }
        let json = fetch(page_no).await?;
        let page = parse_events(&json, projects).map_err(|_| QuotaError::Parse)?;
        if page_unreadable(&page) {
            return Err(QuotaError::Parse);
        }
        sink(&page.events);
        stats.events += page.events.len() as u64;
        stats.skipped += page.skipped;
        let newest = page.events.iter().map(|e| e.ts.timestamp_millis()).max();
        stats.max_ts_ms = stats.max_ts_ms.max(newest);
        if page.raw_count < PAGE_SIZE {
            return Ok(stats);
        }
    }
    stats.truncated = true;
    Ok(stats)
}

/// Where the watermark goes after a finished sync: the newest event seen, truncated or not (a
/// blocked watermark would re-fetch 200 pages on every poll). Nothing when the sync saw no event.
pub fn next_watermark(stats: &SyncStats) -> Option<i64> {
    stats.max_ts_ms
}

/// Conversation id (transcript entry name without extension) to the project's real path.
/// Only directory entry names are read, never file contents.
/// `decoded` remembers slugs across rounds: listing entries is cheap and must happen every round so
/// a new conversation gets its project, while walking folders to decode a slug is not.
pub fn project_map(projects_dir: &Path, decoded: &mut HashMap<String, String>) -> HashMap<String, String> {
    project_map_with_root(projects_dir, Path::new("/"), decoded)
}

fn project_map_with_root(projects_dir: &Path, root: &Path, decoded: &mut HashMap<String, String>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(slugs) = fs::read_dir(projects_dir) else { return map };
    for slug in slugs.flatten() {
        let Some(slug_name) = slug.file_name().to_str().map(str::to_owned) else { continue };
        let Ok(entries) = fs::read_dir(slug.path().join("agent-transcripts")) else { continue };
        let path = match decoded.get(&slug_name) {
            Some(path) => path.clone(),
            None => {
                let Some(path) = decode_slug(&slug_name, root) else { continue };
                decoded.insert(slug_name, path.clone());
                path
            }
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = Path::new(&name).file_stem().and_then(|s| s.to_str()).map(str::to_owned);
            if let Some(name) = name {
                map.insert(name, path.clone());
            }
        }
    }
    map
}

/// Cursor names a project folder after its path with every non-alphanumeric character replaced
/// by `-`, which is ambiguous, so the real folders under `root` are walked to find the path.
pub fn decode_slug(slug: &str, root: &Path) -> Option<String> {
    walk(root, slug, 0).map(|p| p.to_string_lossy().into_owned())
}

fn normalize(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

fn walk(dir: &Path, rest: &str, depth: usize) -> Option<PathBuf> {
    if depth >= MAX_DEPTH || rest.is_empty() {
        return None;
    }
    // `file_type()` does not follow symlinks, so links are never descended into.
    let mut subs: Vec<(String, PathBuf)> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| Some((normalize(e.file_name().to_str()?), e.path())))
        .filter(|(n, _)| !n.is_empty())
        .collect();
    subs.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
    for (name, path) in subs {
        if rest == name {
            return Some(path);
        }
        if let Some(tail) = rest.strip_prefix(name.as_str()).and_then(|t| t.strip_prefix('-'))
            && let Some(found) = walk(&path, tail, depth + 1)
        {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    fn ev(id: &str, ts: i64) -> String {
        format!(
            r#"{{"timestamp":"{ts}","model":"default","usageEventId":"{id}","conversationId":"c1",
            "tokenUsage":{{"inputTokens":1,"outputTokens":2,"cacheReadTokens":3,"cacheWriteTokens":4,"totalCents":140.47}},
            "chargedCents":999.0}}"#
        )
    }

    fn page_json(events: &[String]) -> String {
        format!(r#"{{"totalUsageEventsCount":{},"usageEventsDisplay":[{}]}}"#, events.len(), events.join(","))
    }

    fn full_page(prefix: &str, n: usize, ts: i64) -> String {
        page_json(&(0..n).map(|i| ev(&format!("{prefix}{i}"), ts + i as i64)).collect::<Vec<_>>())
    }

    #[test]
    fn parse_full_event() {
        let projects = HashMap::from([("c1".to_string(), "/Users/dev/proj".to_string())]);
        let page = parse_events(&page_json(&[ev("e1", 1792568013507)]), &projects).unwrap();
        assert_eq!((page.skipped, page.raw_count, page.total), (0, 1, 1));
        let e = &page.events[0];
        assert_eq!(e.dedup_key, "cr:e1");
        assert_eq!(e.session_id, "c1");
        assert_eq!(e.cwd, "/Users/dev/proj");
        assert_eq!(e.model, "default");
        assert_eq!(e.ts.timestamp_millis(), 1792568013507);
        assert_eq!(e.agent, crate::model::Agent::Cursor);
        assert_eq!(
            (e.tokens.input, e.tokens.output, e.tokens.cache_read, e.tokens.cache_write_5m),
            (1, 2, 3, 4)
        );
        assert_eq!((e.tokens.cache_write_1h, e.tokens.reasoning), (0, 0));
        assert_eq!(e.reported_cost_usd, Some(1.4047));
    }

    #[test]
    fn range_without_events_has_no_list() {
        for j in [r#"{"totalUsageEventsCount":0}"#, "{}"] {
            let page = parse_events(j, &HashMap::new()).unwrap();
            assert_eq!((page.raw_count, page.events.len()), (0, 0));
        }
        assert!(parse_events(r#"{"totalUsageEventsCount":3}"#, &HashMap::new()).is_err(), "a count without a list is a changed API");
        assert!(parse_events(r#"{"usageEventsDisplay":1}"#, &HashMap::new()).is_err());
    }

    #[test]
    fn parse_defaults_and_unknown_project() {
        let j = r#"{"totalUsageEventsCount":1,"usageEventsDisplay":[
            {"timestamp":"1000","model":"m","usageEventId":"x","tokenUsage":{"inputTokens":7,"totalCents":0}}]}"#;
        let page = parse_events(j, &HashMap::new()).unwrap();
        let e = &page.events[0];
        assert_eq!((e.session_id.as_str(), e.cwd.as_str()), ("cr-event:x", ""));
        assert_eq!((e.tokens.input, e.tokens.output, e.tokens.cache_read, e.tokens.cache_write_5m), (7, 0, 0, 0));
        assert_eq!(e.reported_cost_usd, None);
    }

    #[test]
    fn events_without_conversation_get_their_own_session() {
        let j = r#"{"totalUsageEventsCount":2,"usageEventsDisplay":[
            {"timestamp":"1000","model":"m","usageEventId":"a","tokenUsage":{}},
            {"timestamp":"1000","model":"m","usageEventId":"b","conversationId":"","tokenUsage":{}}]}"#;
        let page = parse_events(j, &HashMap::new()).unwrap();
        assert_eq!(page.events[0].session_id, "cr-event:a");
        assert_eq!(page.events[1].session_id, "cr-event:b");
    }

    #[test]
    fn effort_suffix_is_stripped_from_the_model_but_max_is_kept() {
        assert_eq!(strip_effort("claude-opus-5-5-medium"), "claude-opus-5-5");
        assert_eq!(strip_effort("claude-opus-5-5-high"), "claude-opus-5-5");
        assert_eq!(strip_effort("gpt-5-low"), "gpt-5");
        assert_eq!(strip_effort("claude-opus-5-5-max"), "claude-opus-5-5-max");
        assert_eq!(strip_effort("default"), "default");
        assert_eq!(strip_effort("high"), "high", "nothing left if stripped");
        let j = r#"{"totalUsageEventsCount":1,"usageEventsDisplay":[
            {"timestamp":"1000","model":"claude-opus-5-5-high","usageEventId":"a","tokenUsage":{}}]}"#;
        assert_eq!(parse_events(j, &HashMap::new()).unwrap().events[0].model, "claude-opus-5-5");
    }

    #[test]
    fn full_page_without_a_valid_event_is_a_parse_error() {
        let bad = |n: usize| {
            let rows: Vec<String> = (0..n).map(|i| format!(r#"{{"timestamp":"x","model":"m","usageEventId":"e{i}","tokenUsage":{{}}}}"#)).collect();
            page_json(&rows)
        };
        let page = parse_events(&bad(100), &HashMap::new()).unwrap();
        assert!(page_unreadable(&page));
        assert!(!page_unreadable(&parse_events(&bad(99), &HashMap::new()).unwrap()), "a short page is just the end");
        assert!(!page_unreadable(&parse_events(&full_page("a", 100, 1), &HashMap::new()).unwrap()));
    }

    #[tokio::test]
    async fn sync_full_unreadable_page_fails_instead_of_ending_quietly() {
        let rows: Vec<String> = (0..100).map(|i| format!(r#"{{"timestamp":"x","model":"m","usageEventId":"e{i}","tokenUsage":{{}}}}"#)).collect();
        let body = page_json(&rows);
        let r = sync(|_| { let b = body.clone(); async move { Ok(b) } }, &HashMap::new(), |_| {}, Duration::ZERO).await;
        assert_eq!(r.unwrap_err(), QuotaError::Parse);
    }

    #[test]
    fn history_due_follows_the_given_backoff() {
        let (ten, hour) = (TRANSIENT_BACKOFF, HISTORY_BACKOFF);
        assert!(history_due(None, hour));
        assert!(!history_due(Some(Duration::ZERO), ten));
        assert!(!history_due(Some(Duration::from_secs(599)), ten));
        assert!(history_due(Some(Duration::from_secs(600)), ten));
        assert!(!history_due(Some(Duration::from_secs(3599)), hour));
        assert!(history_due(Some(Duration::from_secs(3600)), hour));
    }

    #[test]
    fn parse_skips_incomplete_events() {
        let j = r#"{"totalUsageEventsCount":5,"usageEventsDisplay":[
            {"timestamp":"1000","model":"m","usageEventId":"a","conversationId":"c"},
            {"timestamp":"1000","model":"m","conversationId":"c","tokenUsage":{}},
            {"timestamp":"nope","model":"m","usageEventId":"b","tokenUsage":{}},
            {"timestamp":"1000","usageEventId":"c","tokenUsage":{}},
            {"timestamp":"1000","model":"m","usageEventId":"ok","tokenUsage":{}}]}"#;
        let page = parse_events(j, &HashMap::new()).unwrap();
        assert_eq!((page.events.len(), page.skipped, page.raw_count, page.total), (1, 4, 5, 5));
        assert_eq!(page.events[0].dedup_key, "cr:ok");
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse_events("nope", &HashMap::new()).is_err());
        assert!(parse_events("[]", &HashMap::new()).is_err());
    }

    #[tokio::test]
    async fn sync_stops_at_short_page_and_sinks_per_page() {
        let pages = RefCell::new(vec![]);
        let sunk = RefCell::new(vec![]);
        let stats = sync(
            |p| {
                pages.borrow_mut().push(p);
                let body = if p == 1 { full_page("a", 100, 5000) } else { full_page("b", 3, 9000) };
                async move { Ok(body) }
            },
            &HashMap::new(),
            |evs| sunk.borrow_mut().push(evs.len()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert_eq!(*pages.borrow(), [1, 2]);
        assert_eq!(*sunk.borrow(), [100, 3]);
        assert_eq!(stats, SyncStats { events: 103, skipped: 0, max_ts_ms: Some(9002), truncated: false });
    }

    #[test]
    fn next_watermark_cases() {
        let st = |max, truncated| SyncStats { events: 1, skipped: 0, max_ts_ms: max, truncated };
        assert_eq!(next_watermark(&st(Some(7), false)), Some(7));
        assert_eq!(next_watermark(&st(Some(7), true)), Some(7), "truncated still advances");
        assert_eq!(next_watermark(&st(None, false)), None, "no events keeps the old one");
    }

    #[tokio::test]
    async fn sync_stops_when_the_fetch_refuses_and_asks_for_no_more_pages() {
        let (calls, stop) = (RefCell::new(vec![]), std::cell::Cell::new(false));
        let r = sync(
            |p| {
                calls.borrow_mut().push(p);
                let refuse = stop.get();
                stop.set(true); // e.g. the agent was switched off after page 1
                async move { if refuse { Err(QuotaError::Offline) } else { Ok(full_page("a", 100, 1)) } }
            },
            &HashMap::new(),
            |_| {},
            Duration::ZERO,
        )
        .await;
        assert!(r.is_err());
        assert_eq!(*calls.borrow(), [1, 2], "no page after the refusal");
    }

    #[tokio::test]
    async fn sync_caps_at_200_pages() {
        let calls = RefCell::new(0u32);
        let stats = sync(
            |_| {
                *calls.borrow_mut() += 1;
                let body = full_page("x", 100, 1);
                async move { Ok(body) }
            },
            &HashMap::new(),
            |_| {},
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert_eq!(*calls.borrow(), 200);
        assert_eq!(stats.events, 20_000);
        assert!(stats.truncated, "the cap is reported to the caller");
    }

    #[tokio::test]
    async fn sync_error_on_page_two_keeps_page_one() {
        let sunk = RefCell::new(0usize);
        let r = sync(
            |p| async move { if p == 1 { Ok(full_page("a", 100, 1)) } else { Err(QuotaError::Http(500)) } },
            &HashMap::new(),
            |evs| *sunk.borrow_mut() += evs.len(),
            Duration::ZERO,
        )
        .await;
        assert_eq!(r.unwrap_err(), QuotaError::Http(500));
        assert_eq!(*sunk.borrow(), 100);
    }

    #[tokio::test]
    async fn sync_unparseable_page_is_parse_error() {
        let r = sync(|_| async { Ok("nope".to_string()) }, &HashMap::new(), |_| {}, Duration::ZERO).await;
        assert_eq!(r.unwrap_err(), QuotaError::Parse);
    }

    #[tokio::test]
    async fn fetch_page_unreachable_host_is_offline() {
        let auth = crate::quota::cursor::auth_from_cookie("user_1%3A%3Aopaque".into());
        let http = crate::quota::cursor::http_client();
        let r = fetch_page(&http, &auth, "http://127.0.0.1:1", 0, 1, 1).await;
        assert_eq!(r.unwrap_err(), QuotaError::Offline);
    }

    #[test]
    fn decode_slug_handles_hyphen_and_underscore_dirs() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("Users/mac/Documents/my-app_v2/sub")).unwrap();
        let want = root.path().join("Users/mac/Documents/my-app_v2");
        assert_eq!(
            decode_slug("Users-mac-Documents-my-app-v2", root.path()),
            Some(want.to_string_lossy().into_owned())
        );
    }

    #[test]
    fn decode_slug_backtracks_on_ambiguous_names() {
        let root = tempdir().unwrap();
        // `a-b` (longest) is tried first but only `a/b` leads to `c`.
        fs::create_dir_all(root.path().join("a-b")).unwrap();
        fs::create_dir_all(root.path().join("a/b/c")).unwrap();
        let want = root.path().join("a/b/c");
        assert_eq!(decode_slug("a-b-c", root.path()), Some(want.to_string_lossy().into_owned()));
        // The longest match wins when it completes the slug.
        assert_eq!(decode_slug("a-b", root.path()), Some(root.path().join("a-b").to_string_lossy().into_owned()));
    }

    #[test]
    fn decode_slug_fails_on_unknown_numeric_and_partial() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("Users/mac")).unwrap();
        assert_eq!(decode_slug("1775721208693", root.path()), None);
        assert_eq!(decode_slug("empty-window", root.path()), None);
        assert_eq!(decode_slug("Users-mac-gone", root.path()), None);
        assert_eq!(decode_slug("Users-macx", root.path()), None);
    }

    #[test]
    fn decode_slug_ignores_files_and_symlinks() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::create_dir_all(outside.path().join("real")).unwrap();
        fs::write(root.path().join("afile"), "x").unwrap();
        std::os::unix::fs::symlink(outside.path().join("real"), root.path().join("link")).unwrap();
        assert_eq!(decode_slug("afile", root.path()), None);
        assert_eq!(decode_slug("link", root.path()), None);
    }

    #[test]
    fn project_map_lists_entries_without_opening_them() {
        let root = tempdir().unwrap();
        let projects = root.path().join("projects");
        let real = root.path().join("Users/mac/proj-x");
        fs::create_dir_all(&real).unwrap();
        let slug = "Users-mac-proj-x";
        let transcripts = projects.join(slug).join("agent-transcripts");
        fs::create_dir_all(transcripts.join("uuid-folder")).unwrap();
        fs::write(transcripts.join("uuid-file.jsonl"), "secret").unwrap();
        fs::set_permissions(transcripts.join("uuid-file.jsonl"), fs::Permissions::from_mode(0o000)).unwrap();
        fs::create_dir_all(projects.join("1775721208693/agent-transcripts/orphan")).unwrap();
        fs::create_dir_all(projects.join("empty-window")).unwrap();

        let map = project_map_with_root(&projects, root.path(), &mut HashMap::new());
        assert_eq!(map.get("uuid-folder").map(String::as_str), Some(real.to_string_lossy().as_ref()));
        assert_eq!(map.get("uuid-file").map(String::as_str), Some(real.to_string_lossy().as_ref()));
        assert!(!map.contains_key("orphan"));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn a_new_conversation_is_mapped_on_the_next_round_without_walking_again() {
        let root = tempdir().unwrap();
        let projects = root.path().join("projects");
        let real = root.path().join("Users/mac/proj-x");
        fs::create_dir_all(&real).unwrap();
        let transcripts = projects.join("Users-mac-proj-x/agent-transcripts");
        fs::create_dir_all(transcripts.join("first")).unwrap();
        let mut decoded = HashMap::new();
        assert!(project_map_with_root(&projects, root.path(), &mut decoded).contains_key("first"));

        fs::create_dir_all(transcripts.join("second")).unwrap();
        // Gone from disk: only the remembered slug can still name the project.
        fs::remove_dir_all(&real).unwrap();
        let map = project_map_with_root(&projects, root.path(), &mut decoded);
        assert_eq!(map.get("second").map(String::as_str), Some(real.to_string_lossy().as_ref()));
    }

    #[test]
    fn next_slice_first_run_later_run_and_final() {
        let now = FIRST_SYNC_START_MS + 100 * 86_400_000;
        assert_eq!(next_slice(0, now), (FIRST_SYNC_START_MS, FIRST_SYNC_START_MS + SLICE_MS, false));
        let wm = FIRST_SYNC_START_MS + 40 * 86_400_000;
        assert_eq!(next_slice(wm, now), (wm - OVERLAP_MS, wm - OVERLAP_MS + SLICE_MS, false));
        let recent = now - 3_600_000;
        assert_eq!(next_slice(recent, now), (recent - OVERLAP_MS, now, true));
        // Exactly one slice left counts as final.
        let start = now - SLICE_MS;
        assert_eq!(next_slice(start + OVERLAP_MS, now), (start, now, true));
    }

    #[test]
    fn slice_watermark_saves_progress_and_never_goes_backward() {
        let st = |max| SyncStats { events: 1, skipped: 0, max_ts_ms: max, truncated: false };
        // Non-final: the slice end, even when the slice was empty.
        assert_eq!(slice_watermark(0, 500, false, &st(None)), Some(500));
        assert_eq!(slice_watermark(0, 500, false, &st(Some(300))), Some(500));
        // Final: the newest event, as before.
        assert_eq!(slice_watermark(0, 900, true, &st(Some(700))), Some(700));
        assert_eq!(slice_watermark(0, 900, true, &st(None)), None);
        // Never backward (the overlap can make max_ts_ms older than the saved value).
        assert_eq!(slice_watermark(800, 900, true, &st(Some(700))), Some(800));
        assert_eq!(slice_watermark(800, 500, false, &st(None)), Some(800));
    }

    #[test]
    fn retry_delay_per_error_class_and_attempt() {
        let s = Duration::from_secs;
        for e in [QuotaError::Http(503), QuotaError::Http(500), QuotaError::Http(429), QuotaError::Offline] {
            assert_eq!(retry_delay(0, &e), Some(s(5)), "{e:?}");
            assert_eq!(retry_delay(1, &e), Some(s(20)), "{e:?}");
            assert_eq!(retry_delay(2, &e), None, "{e:?}");
        }
        for e in [QuotaError::Expired, QuotaError::Parse, QuotaError::Http(404), QuotaError::Http(400)] {
            assert_eq!(retry_delay(0, &e), None, "{e:?}");
        }
    }

    #[test]
    fn history_backoff_by_error_class() {
        assert_eq!(history_backoff(&QuotaError::Http(503)), TRANSIENT_BACKOFF);
        assert_eq!(history_backoff(&QuotaError::Http(429)), TRANSIENT_BACKOFF);
        assert_eq!(history_backoff(&QuotaError::Offline), TRANSIENT_BACKOFF);
        assert_eq!(TRANSIENT_BACKOFF, Duration::from_secs(600));
        assert_eq!(history_backoff(&QuotaError::Parse), HISTORY_BACKOFF);
        assert_eq!(history_backoff(&QuotaError::Expired), HISTORY_BACKOFF);
        assert_eq!(history_backoff(&QuotaError::Http(404)), HISTORY_BACKOFF);
    }

    #[tokio::test]
    async fn retrying_recovers_after_transient_errors_and_waits_5_then_20() {
        let waits = RefCell::new(vec![]);
        let calls = RefCell::new(0u32);
        let r = retrying(
            || {
                *calls.borrow_mut() += 1;
                let n = *calls.borrow();
                async move { if n < 3 { Err(QuotaError::Http(503)) } else { Ok("ok") } }
            },
            |d| { waits.borrow_mut().push(d); async {} },
            || false,
        )
        .await;
        assert_eq!(r, Ok("ok"));
        assert_eq!(*waits.borrow(), [Duration::from_secs(5), Duration::from_secs(20)]);
    }

    #[tokio::test]
    async fn retrying_gives_up_after_two_waits() {
        let calls = RefCell::new(0u32);
        let r: Result<(), _> = retrying(
            || { *calls.borrow_mut() += 1; async { Err(QuotaError::Http(503)) } },
            |_| async {},
            || false,
        )
        .await;
        assert_eq!(r, Err(QuotaError::Http(503)));
        assert_eq!(*calls.borrow(), 3);
    }

    #[tokio::test]
    async fn retrying_does_not_retry_other_errors() {
        let calls = RefCell::new(0u32);
        let r: Result<(), _> = retrying(
            || { *calls.borrow_mut() += 1; async { Err(QuotaError::Expired) } },
            |_| async {},
            || false,
        )
        .await;
        assert_eq!(r, Err(QuotaError::Expired));
        assert_eq!(*calls.borrow(), 1);
    }

    #[tokio::test]
    async fn retrying_rechecks_stop_before_waiting_and_after_the_wait() {
        // Stopped after the wait: no second request.
        let (calls, stop) = (RefCell::new(0u32), std::cell::Cell::new(false));
        let r: Result<(), _> = retrying(
            || { *calls.borrow_mut() += 1; async { Err(QuotaError::Http(503)) } },
            |_| { stop.set(true); async {} },
            || stop.get(),
        )
        .await;
        assert_eq!(r, Err(QuotaError::Http(503)));
        assert_eq!(*calls.borrow(), 1);
        // Already stopped when the error arrives: no wait at all.
        let waited = RefCell::new(false);
        let r: Result<(), _> = retrying(
            || async { Err(QuotaError::Http(503)) },
            |_| { *waited.borrow_mut() = true; async {} },
            || true,
        )
        .await;
        assert!(r.is_err() && !*waited.borrow());
    }
}

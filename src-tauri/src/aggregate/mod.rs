//! Read side: turns stored events plus quota/settings/RTK inputs into one JSON DTO per tab.
//! Every function takes `tz` and `now` so day and hour buckets are deterministic.
//! Costs only sum known prices; tokens without a price are reported as `unpriced_tokens`.
//! Agents switched off in `Settings::enabled_agents` are left out of every view
//! (a missing key counts as enabled).

mod history;
mod models;
mod overview;
mod projects;
mod sessions;

pub use history::{history, DayCost, HistoryView, PlanValue};
pub use models::{models, CacheInsight, ModelRow, ModelsView, TokenMix};
pub use overview::{overview, HourBucket, Overview, TodaySummary};
pub use projects::{projects, ProjectInsight, ProjectRow, ProjectsView};
pub use sessions::{sessions, DayGroup, SessionFilter, SessionPage, SessionRow};

use anyhow::Result;
use chrono::{DateTime, Days, FixedOffset, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::{Agent, Tokens, STACK_ORDER};
use crate::settings::Settings;
use crate::store::{Store, StoredEvent};

/// Last N local days including today ("today" = since local midnight).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Period {
    #[serde(rename = "today")]
    Today,
    #[serde(rename = "7d")]
    Last7,
    #[serde(rename = "14d")]
    Last14,
    #[serde(rename = "30d")]
    Last30,
}

impl Period {
    pub fn days(self) -> u32 {
        match self {
            Period::Today => 1,
            Period::Last7 => 7,
            Period::Last14 => 14,
            Period::Last30 => 30,
        }
    }
}

/// Shares in percent (sum 100, or all 0 when there is nothing to split).
#[derive(Clone, Debug, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mix {
    pub cache_read: f64,
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
}

pub(crate) fn is_enabled(settings: &Settings, agent: Agent) -> bool {
    settings.enabled_agents.get(&agent).copied().unwrap_or(true)
}

/// Position in `STACK_ORDER` (index into `by_agent` arrays).
fn agent_index(agent: Agent) -> usize {
    STACK_ORDER.iter().position(|a| *a == agent).expect("every agent is in STACK_ORDER")
}

/// Token kinds as shown in the UI: [cache read, input (non-cached), output (incl. reasoning), cache write].
/// Codex reports input including cache reads and output including reasoning.
fn token_kinds(agent: Agent, t: &Tokens) -> [u64; 4] {
    let cache_write = t.cache_write_5m + t.cache_write_1h;
    if agent == Agent::Codex {
        [t.cache_read, t.input.saturating_sub(t.cache_read), t.output, cache_write]
    } else {
        [t.cache_read, t.input, t.output + t.reasoning, cache_write]
    }
}

/// All tokens of an event, counted once (Ruling R12: Codex = input + output).
pub(crate) fn total_tokens(agent: Agent, t: &Tokens) -> u64 {
    if agent == Agent::Codex {
        t.input + t.output
    } else {
        t.input + t.output + t.cache_read + t.cache_write_5m + t.cache_write_1h + t.reasoning
    }
}

fn local_date(ts: DateTime<Utc>, tz: FixedOffset) -> NaiveDate {
    ts.with_timezone(&tz).date_naive()
}

fn local_midnight(date: NaiveDate, tz: FixedOffset) -> DateTime<Utc> {
    date.and_time(NaiveTime::MIN).and_local_timezone(tz).single().expect("fixed offsets are unambiguous").to_utc()
}

/// Start (local midnight) of the last `days` local days including today's.
pub(crate) fn days_start(days: u32, tz: FixedOffset, now: DateTime<Utc>) -> DateTime<Utc> {
    let first = local_date(now, tz) - Days::new(u64::from(days.saturating_sub(1)));
    local_midnight(first, tz)
}

/// `part` as percent of `total` (0 when there is no total).
fn percent(part: f64, total: f64) -> f64 {
    if total > 0.0 { part / total * 100.0 } else { 0.0 }
}

fn percent_mix(parts: [f64; 4]) -> Mix {
    let total: f64 = parts.iter().sum();
    let [cache_read, input, output, cache_write] = parts.map(|p| percent(p, total));
    Mix { cache_read, input, output, cache_write }
}

/// Events in `[from, to)` from enabled agents only.
pub(crate) fn enabled_events(store: &Store, settings: &Settings, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<StoredEvent>> {
    let mut evs = store.events_between(from, to)?;
    evs.retain(|e| is_enabled(settings, e.agent));
    Ok(evs)
}

#[cfg(test)]
pub(crate) mod testkit {
    use std::path::PathBuf;

    use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};

    use crate::model::{Agent, Tokens, UsageEvent};
    use crate::pricing::Pricing;
    use crate::project::ProjectResolver;
    use crate::store::Store;

    /// A home that does not exist, so no git root is ever found.
    pub const HOME: &str = "/zz-buku-token-test-home";

    pub fn tz() -> FixedOffset {
        FixedOffset::east_opt(7 * 3600).unwrap()
    }

    /// Local (+07:00) wall-clock time as UTC.
    pub fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        tz().with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap().to_utc()
    }

    /// Jum, 30 Okt 2026, 12.00 local.
    pub fn now() -> DateTime<Utc> {
        at(2026, 10, 30, 12, 0)
    }

    pub fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    pub fn tok(input: u64, output: u64) -> Tokens {
        Tokens { input, output, ..Default::default() }
    }

    /// Project folder under the fake `~/Documents`.
    pub fn doc(name: &str) -> String {
        format!("{HOME}/Documents/{name}")
    }

    pub fn ev(ts: DateTime<Utc>, agent: Agent, model: &str, cwd: &str, session: &str, tokens: Tokens, cost: Option<f64>) -> UsageEvent {
        UsageEvent {
            ts,
            agent,
            model: model.into(),
            cwd: cwd.into(),
            session_id: session.into(),
            tokens,
            reported_cost_usd: cost,
            dedup_key: String::new(),
        }
    }

    /// Short form: Claude Code, model "m", given cost.
    pub fn cost_at(ts: DateTime<Utc>, agent: Agent, cwd: &str, session: &str, cost: f64) -> UsageEvent {
        ev(ts, agent, "m", cwd, session, tok(100, 10), Some(cost))
    }

    fn keyed(evs: Vec<UsageEvent>) -> Vec<UsageEvent> {
        evs.into_iter()
            .enumerate()
            .map(|(i, mut e)| {
                e.dedup_key = i.to_string();
                e
            })
            .collect()
    }

    /// Stores each event with exactly its given cost (`None` = unpriced).
    pub fn store(evs: Vec<UsageEvent>) -> Store {
        let s = Store::open_in_memory().unwrap();
        s.insert_events(&keyed(evs), &|_| None).unwrap();
        s
    }

    /// Stores events priced by `p`, the way ingest does.
    pub fn store_priced(evs: Vec<UsageEvent>, p: &Pricing) -> Store {
        let s = Store::open_in_memory().unwrap();
        s.insert_events(&keyed(evs), &|e| p.cost(e.agent, &e.model, &e.tokens)).unwrap();
        s
    }

    pub fn resolver() -> ProjectResolver {
        ProjectResolver::new(PathBuf::from(HOME))
    }

    pub fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;

    #[test]
    fn r12_codex_total_is_input_plus_output() {
        let t = Tokens { input: 1000, output: 50, cache_read: 400, reasoning: 20, ..Default::default() };
        assert_eq!(total_tokens(Agent::Codex, &t), 1050);
        assert_eq!(token_kinds(Agent::Codex, &t), [400, 600, 50, 0]);
    }

    #[test]
    fn r12_other_agents_count_every_kind() {
        let t = Tokens { input: 1, output: 2, cache_read: 4, cache_write_5m: 8, cache_write_1h: 16, reasoning: 32 };
        for a in [Agent::ClaudeCode, Agent::OpenCode, Agent::Hermes] {
            assert_eq!(total_tokens(a, &t), 63);
            assert_eq!(token_kinds(a, &t), [4, 1, 34, 24]);
        }
    }

    #[test]
    fn missing_enabled_key_counts_as_enabled() {
        let mut s = Settings::default();
        s.enabled_agents.clear();
        assert!(STACK_ORDER.iter().all(|a| is_enabled(&s, *a)));
        s.enabled_agents.insert(Agent::Codex, false);
        assert!(!is_enabled(&s, Agent::Codex));
        assert!(is_enabled(&s, Agent::Hermes));
    }

    #[test]
    fn agent_index_follows_stack_order() {
        for (i, a) in STACK_ORDER.iter().enumerate() {
            assert_eq!(agent_index(*a), i);
        }
    }

    #[test]
    fn periods_cover_last_n_local_days() {
        assert_eq!([Period::Today, Period::Last7, Period::Last14, Period::Last30].map(Period::days), [1, 7, 14, 30]);
        assert_eq!(days_start(1, tz(), now()), at(2026, 10, 30, 0, 0));
        assert_eq!(days_start(7, tz(), now()), at(2026, 10, 24, 0, 0));
        // 00.30 local on 30 Okt is still 29 Okt in UTC.
        assert_eq!(local_date(at(2026, 10, 30, 0, 30), tz()), date(2026, 10, 30));
        assert_eq!(local_midnight(date(2026, 10, 30), tz()).to_rfc3339(), "2026-10-29T17:00:00+00:00");
    }

    #[test]
    fn period_parses_from_ui_strings() {
        let p: Vec<Period> = serde_json::from_str(r#"["today","7d","14d","30d"]"#).unwrap();
        assert_eq!(p, vec![Period::Today, Period::Last7, Period::Last14, Period::Last30]);
        assert_eq!(serde_json::to_string(&Period::Last7).unwrap(), r#""7d""#);
    }

    #[test]
    fn percent_mix_sums_to_100_or_all_zero() {
        let m = percent_mix([3.0, 1.0, 0.0, 4.0]);
        close(m.cache_read, 37.5);
        close(m.input, 12.5);
        close(m.output, 0.0);
        close(m.cache_write, 50.0);
        assert_eq!(percent_mix([0.0; 4]), Mix::default());
    }

    #[test]
    fn enabled_events_drop_disabled_agents() {
        let s = store(vec![
            cost_at(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, "/p", "a", 1.0),
            cost_at(at(2026, 10, 30, 9, 0), Agent::Codex, "/p", "b", 2.0),
        ]);
        let mut st = Settings::default();
        st.enabled_agents.insert(Agent::Codex, false);
        let got = enabled_events(&s, &st, at(2026, 10, 30, 0, 0), now()).unwrap();
        assert_eq!(got.iter().map(|e| e.agent).collect::<Vec<_>>(), vec![Agent::ClaudeCode]);
    }
}

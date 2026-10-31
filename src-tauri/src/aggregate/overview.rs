//! Ringkasan: quotas per agent, today per agent and per hour, RTK savings.

use anyhow::Result;
use chrono::{DateTime, FixedOffset, Timelike, Utc};
use serde::Serialize;

use super::{agent_index, enabled_events, is_enabled, local_date, local_midnight, total_tokens};
use crate::ingest::antigravity::Activity;
use crate::model::Agent;
use crate::quota::openrouter::OpenRouterView;
use crate::quota::{AgentQuota, Burn};
use crate::rtk::RtkSummary;
use crate::settings::Settings;
use crate::store::Store;

/// First and last local hour drawn in "Hari Ini per Jam"; other hours only count in the totals.
const FIRST_HOUR: u32 = 6;
const LAST_HOUR: u32 = 23;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HourBucket {
    pub hour: u32,
    /// USD per agent in `STACK_ORDER`.
    pub by_agent: [f64; 5],
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodaySummary {
    pub cost_usd: f64,
    pub unpriced_tokens: u64,
    pub tokens: u64,
    /// USD per agent in `STACK_ORDER`, over the whole day (hours outside the chart included).
    pub by_agent: [f64; 5],
    /// Tokens without a known price per agent in `STACK_ORDER`; the UI shows "Harga Belum Ada" for them.
    pub unpriced_by_agent: [u64; 5],
    /// Hours 6..=23, always 18 buckets.
    pub hours: Vec<HourBucket>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub burn: Option<Burn>,
    pub quotas: Vec<AgentQuota>,
    pub today: TodaySummary,
    pub rtk: Option<RtkSummary>,
    /// Filled by the engine. Not an agent: never part of `today`, `quotas` or any total.
    pub openrouter: Option<OpenRouterView>,
    pub antigravity: Option<Activity>,
    pub updated_at: DateTime<Utc>,
}

pub fn overview(
    store: &Store,
    quotas: &[AgentQuota],
    burn: Option<Burn>,
    rtk: Option<RtkSummary>,
    settings: &Settings,
    tz: FixedOffset,
    now: DateTime<Utc>,
) -> Result<Overview> {
    let today_start = local_midnight(local_date(now, tz), tz);
    let mut hours: Vec<HourBucket> = (FIRST_HOUR..=LAST_HOUR).map(|hour| HourBucket { hour, by_agent: [0.0; 5] }).collect();
    let (mut cost_usd, mut tokens, mut unpriced_tokens) = (0.0, 0, 0);
    let (mut by_agent, mut unpriced_by_agent) = ([0.0; 5], [0; 5]);
    for e in enabled_events(store, settings, today_start, now)? {
        let n = total_tokens(e.agent, &e.tokens);
        let i = agent_index(e.agent);
        tokens += n;
        let Some(c) = e.cost_usd else {
            unpriced_tokens += n;
            unpriced_by_agent[i] += n;
            continue;
        };
        cost_usd += c;
        by_agent[i] += c;
        let hour = e.ts.with_timezone(&tz).hour();
        if (FIRST_HOUR..=LAST_HOUR).contains(&hour) {
            hours[(hour - FIRST_HOUR) as usize].by_agent[i] += c;
        }
    }
    let quotas: Vec<AgentQuota> = quotas.iter().filter(|q| is_enabled(settings, q.agent)).cloned().collect();
    Ok(Overview {
        burn: burn.filter(|_| is_enabled(settings, Agent::ClaudeCode)),
        quotas,
        today: TodaySummary { cost_usd, unpriced_tokens, tokens, by_agent, unpriced_by_agent, hours },
        rtk,
        openrouter: None,
        antigravity: None,
        updated_at: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::testkit::*;
    use crate::model::Tokens;
    use crate::quota::{binding_window, is_near_limit, QuotaWindow};
    use chrono::{Duration, TimeZone};

    fn win(id: &str, used: f64) -> QuotaWindow {
        QuotaWindow {
            id: id.into(),
            label: id.into(),
            used_pct: used,
            resets_at: now() + Duration::hours(2),
            observed_at: now(),
            expired: false,
        }
    }

    fn quota(agent: Agent, windows: Vec<QuotaWindow>) -> AgentQuota {
        AgentQuota {
            agent,
            plan: None,
            binding: binding_window(&windows),
            near_limit: is_near_limit(&windows, None),
            windows,
            unavailable_reason: None,
            failure: None,
        }
    }

    fn run(s: &Store, quotas: &[AgentQuota], settings: &Settings) -> Overview {
        overview(s, quotas, None, None, settings, tz(), now()).unwrap()
    }

    fn by_hour(o: &Overview, hour: u32) -> [f64; 5] {
        o.today.hours.iter().find(|b| b.hour == hour).unwrap().by_agent
    }

    #[test]
    fn day_boundary_uses_local_time() {
        let s = store(vec![
            // 29 Okt 18.30 UTC = 30 Okt 01.30 local: today, but outside the 06–23 chart.
            cost_at(Utc.with_ymd_and_hms(2026, 10, 29, 18, 30, 0).unwrap(), Agent::ClaudeCode, "/p", "a", 2.0),
            // 29 Okt 23.59 local: yesterday.
            cost_at(Utc.with_ymd_and_hms(2026, 10, 29, 16, 59, 0).unwrap(), Agent::ClaudeCode, "/p", "a", 5.0),
            cost_at(at(2026, 10, 30, 6, 15), Agent::Codex, "/p", "b", 1.5),
            cost_at(at(2026, 10, 30, 11, 45), Agent::ClaudeCode, "/p", "c", 0.7),
            cost_at(at(2026, 10, 30, 11, 50), Agent::Hermes, "/p", "d", 0.25),
        ]);
        let o = run(&s, &[], &Settings::default());
        close(o.today.cost_usd, 2.0 + 1.5 + 0.7 + 0.25);
        let hours: Vec<u32> = o.today.hours.iter().map(|b| b.hour).collect();
        assert_eq!(hours, (6..=23).collect::<Vec<_>>());
        assert_eq!(by_hour(&o, 6), [0.0, 1.5, 0.0, 0.0, 0.0]);
        assert_eq!(by_hour(&o, 11), [0.7, 0.0, 0.0, 0.25, 0.0]);
        let charted: f64 = o.today.hours.iter().flat_map(|b| b.by_agent).sum();
        close(charted, 1.5 + 0.7 + 0.25);
    }

    #[test]
    fn unpriced_tokens_not_summed_as_zero() {
        let unknown = Tokens { input: 300, output: 40, cache_read: 1000, ..Default::default() };
        let s = store(vec![
            ev(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, "m", "/p", "a", tok(100, 20), Some(1.0)),
            ev(at(2026, 10, 30, 10, 0), Agent::OpenCode, "big-pickle", "/p", "b", unknown, None),
        ]);
        let o = run(&s, &[], &Settings::default());
        assert_eq!(o.today.cost_usd, 1.0);
        assert_eq!(o.today.unpriced_tokens, 1340);
        assert_eq!(o.today.tokens, 120 + 1340);
        assert_eq!(by_hour(&o, 10), [0.0; 5]);
    }

    #[test]
    fn today_tokens_follow_r12() {
        let codex = Tokens { input: 1000, output: 50, cache_read: 400, reasoning: 20, ..Default::default() };
        let claude = Tokens { input: 1, output: 2, cache_read: 4, cache_write_5m: 8, cache_write_1h: 16, reasoning: 32 };
        let s = store(vec![
            ev(at(2026, 10, 30, 9, 0), Agent::Codex, "gpt", "/p", "a", codex, Some(0.1)),
            ev(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, "m", "/p", "b", claude, Some(0.1)),
        ]);
        assert_eq!(run(&s, &[], &Settings::default()).today.tokens, 1050 + 63);
    }

    #[test]
    fn today_by_agent_counts_every_hour_and_follows_the_filter() {
        let s = store(vec![
            // 01.30 local: outside the 06-23 chart, still part of today's per-agent total.
            cost_at(Utc.with_ymd_and_hms(2026, 10, 29, 18, 30, 0).unwrap(), Agent::ClaudeCode, "/p", "a", 2.0),
            cost_at(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, "/p", "a", 0.5),
            cost_at(at(2026, 10, 30, 9, 5), Agent::Codex, "/p", "b", 1.5),
            cost_at(at(2026, 10, 30, 9, 10), Agent::Hermes, "/p", "c", 0.25),
            cost_at(at(2026, 10, 30, 9, 15), Agent::Cursor, "/p", "d", 4.0),
        ]);
        let o = run(&s, &[], &Settings::default());
        assert_eq!(o.today.by_agent, [2.5, 1.5, 0.0, 0.25, 4.0]);
        close(o.today.by_agent.iter().sum::<f64>(), o.today.cost_usd);

        let mut st = Settings::default();
        st.enabled_agents.insert(Agent::Cursor, false);
        st.enabled_agents.insert(Agent::ClaudeCode, false);
        let o = run(&s, &[], &st);
        assert_eq!(o.today.by_agent, [0.0, 1.5, 0.0, 0.25, 0.0]);
    }

    #[test]
    fn today_unpriced_tokens_are_kept_per_agent() {
        let unknown = Tokens { input: 300, output: 40, cache_read: 1000, ..Default::default() };
        let s = store(vec![
            ev(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, "m", "/p", "a", tok(100, 20), Some(1.0)),
            ev(at(2026, 10, 30, 10, 0), Agent::OpenCode, "big-pickle", "/p", "b", unknown, None),
            // Outside the chart hours: still counted.
            ev(Utc.with_ymd_and_hms(2026, 10, 29, 18, 30, 0).unwrap(), Agent::OpenCode, "big-pickle", "/p", "b", tok(5, 5), None),
        ]);
        let o = run(&s, &[], &Settings::default());
        assert_eq!(o.today.unpriced_by_agent, [0, 0, 1350, 0, 0]);
        assert_eq!(o.today.by_agent, [1.0, 0.0, 0.0, 0.0, 0.0], "unknown cost is not summed as 0 for OpenCode");
        assert_eq!(o.today.unpriced_tokens, 1350);
    }

    #[test]
    fn disabled_agents_excluded_from_overview() {
        let s = store(vec![
            cost_at(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, "/p", "a", 3.0),
            cost_at(at(2026, 10, 30, 9, 0), Agent::Hermes, "/p", "b", 1.0),
        ]);
        let mut st = Settings::default();
        st.enabled_agents.insert(Agent::Hermes, false);
        st.enabled_agents.insert(Agent::ClaudeCode, false);
        let cc = quota(Agent::ClaudeCode, vec![win("five_hour", 10.0)]);
        let cx = quota(Agent::Codex, vec![win("seven_day", 50.0)]);
        let burn = Burn { window_start: now(), resets_at: now(), points: vec![], projected_at_reset: 0.0, exhausts_at: None };
        let o = overview(&s, &[cc, cx], Some(burn), None, &st, tz(), now()).unwrap();
        assert_eq!(o.today.cost_usd, 0.0);
        assert_eq!(o.today.tokens, 0);
        assert_eq!(o.quotas.iter().map(|q| q.agent).collect::<Vec<_>>(), vec![Agent::Codex]);
        assert_eq!(o.burn, None, "the burn chart is Claude's 5-hour window");
    }

    #[test]
    fn overview_passes_through_and_serializes_camel_case() {
        let s = store(vec![]);
        let rtk = RtkSummary { saved_tokens: 5, approx_usd: Some(0.5), model: Some("m".into()) };
        let cc = quota(Agent::ClaudeCode, vec![win("five_hour", 38.0)]);
        let o = overview(&s, &[cc], None, Some(rtk.clone()), &Settings::default(), tz(), now()).unwrap();
        assert_eq!(o.rtk, Some(rtk));
        assert_eq!(o.updated_at, now());
        let v = serde_json::to_value(&o).unwrap();
        assert!(v.get("hero").is_none());
        assert_eq!(v["quotas"][0]["binding"]["usedPct"], 38.0);
        assert_eq!(v["updatedAt"], "2026-10-30T05:00:00Z");
        assert_eq!(v["today"]["unpricedTokens"], 0);
        assert_eq!(v["today"]["byAgent"], serde_json::json!([0.0, 0.0, 0.0, 0.0, 0.0]));
        assert_eq!(v["today"]["unpricedByAgent"], serde_json::json!([0, 0, 0, 0, 0]));
        assert_eq!(v["today"]["hours"][0]["byAgent"], serde_json::json!([0.0, 0.0, 0.0, 0.0, 0.0]));
        assert_eq!(v["rtk"]["savedTokens"], 5);
    }
}

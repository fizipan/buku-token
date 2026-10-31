//! Dumps sample DTOs as JSON for the TypeScript contract test (`src/test/fixtures/*.json`).
//! Run: `TS_FIXTURE_DIR=../src/test/fixtures cargo test ts_fixtures -- --ignored`.
use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;

use crate::aggregate::*;
use crate::engine::{ActivitySource, ScanStatus, SourceStatus};
use crate::model::Agent;
use crate::project::ProjectRef;
use crate::ingest::antigravity::Activity;
use crate::quota::openrouter::{KeyLimit, LimitReset, OpenRouterData, OpenRouterView};
use crate::quota::{AgentQuota, Burn, QuotaWindow};
use crate::rtk::RtkSummary;
use crate::fmt_id;
use crate::i18n;
use crate::settings::{Cycle, Lang, Plan, Settings};

fn ts(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}
fn day(s: &str) -> NaiveDate {
    s.parse().unwrap()
}
fn proj(name: &str) -> ProjectRef {
    ProjectRef { path: format!("/Users/dev/Documents/{name}"), name: name.into(), display_path: name.into() }
}
fn window(id: &str, lang: Lang, used: f64) -> QuotaWindow {
    QuotaWindow {
        id: id.into(),
        label: i18n::window_label(id, lang),
        used_pct: used,
        resets_at: ts("2026-10-30T15:00:00Z"),
        observed_at: ts("2026-10-30T10:00:00Z"),
        expired: false,
    }
}
fn dump<T: Serialize>(dir: &Path, name: &str, v: &T) {
    std::fs::write(dir.join(format!("{name}.json")), serde_json::to_string_pretty(v).unwrap() + "\n").unwrap();
}

fn overview(lang: Lang) -> Overview {
    let w = window("five_hour", lang, 62.0);
    Overview {
        burn: Some(Burn {
            window_start: ts("2026-10-30T10:00:00Z"),
            resets_at: ts("2026-10-30T15:00:00Z"),
            points: vec![(0, 0.0), (60, 24.5)],
            projected_at_reset: 88.0,
            exhausts_at: None,
        }),
        quotas: vec![
            AgentQuota {
                agent: Agent::ClaudeCode,
                plan: Some("Max 5x".into()),
                windows: vec![w.clone(), window("seven_day", lang, 30.0)],
                binding: Some(w),
                near_limit: false,
                unavailable_reason: None,
                failure: None,
            },
            AgentQuota {
                agent: Agent::Codex,
                plan: None,
                windows: vec![],
                binding: None,
                near_limit: false,
                unavailable_reason: Some(if lang == Lang::Id { "Belum ada data" } else { "No data yet" }.into()),
                failure: None,
            },
        ],
        today: TodaySummary {
            cost_usd: 15.25,
            unpriced_tokens: 1200,
            tokens: 1_240_000,
            by_agent: [12.5, 2.75, 0.0, 0.0, 0.0],
            unpriced_by_agent: [0, 0, 1200, 0, 0],
            hours: (6..=23)
                .map(|h| HourBucket { hour: h, by_agent: [if h == 12 { 3.5 } else { 0.0 }, 0.0, 0.0, 0.0, 0.0] })
                .collect(),
        },
        rtk: Some(RtkSummary { saved_tokens: 98_000, approx_usd: Some(1.4), model: Some("claude-opus-5-5".into()) }),
        openrouter: Some(OpenRouterView {
            data: Some(OpenRouterData {
                free: false,
                balance_usd: Some(62.75),
                today_usd: 1.25,
                week_usd: 6.5,
                month_usd: 21.5,
                limit: Some(KeyLimit { limit_usd: 50.0, remaining_usd: 28.5, reset: Some(LimitReset::Monthly) }),
            }),
            unavailable_reason: None,
            failure: None,
        }),
        antigravity: Some(Activity { today: 3, last_7_days: 12, last_active: Some(ts("2026-10-30T09:00:00Z")), total_sessions: 121, total_steps: 23021 }),
        updated_at: ts("2026-10-30T12:40:00Z"),
    }
}

#[test]
#[ignore = "manual: writes JSON fixtures for the TypeScript contract test"]
fn write_ts_fixtures() {
    let dir = std::path::PathBuf::from(std::env::var("TS_FIXTURE_DIR").expect("set TS_FIXTURE_DIR"));
    std::fs::create_dir_all(&dir).unwrap();

    dump(&dir, "overview-quota", &overview(Lang::Id));
    dump(&dir, "overview-quota-en", &overview(Lang::En));

    let row = |id: &str, agent, cost: Option<f64>| SessionRow {
        session_id: id.into(),
        agent,
        project: proj("buku-token"),
        started_at: ts("2026-10-30T05:40:00Z"),
        duration_min: 72,
        main_model: "claude-opus-5-5".into(),
        tokens: 612_000,
        cost_usd: cost,
        unpriced_tokens: if cost.is_none() { 612_000 } else { 0 },
    };
    for (name, lang) in [("session-page", Lang::Id), ("session-page-en", Lang::En)] {
        dump(
            &dir,
            name,
            &SessionPage {
                days: vec![DayGroup {
                    date: day("2026-10-30"),
                    label: fmt_id::day_label(day("2026-10-30"), day("2026-10-30"), lang),
                    cost_usd: 15.25,
                    unpriced_tokens: 612_000,
                    sessions: vec![row("s1", Agent::ClaudeCode, Some(15.25)), row("s2", Agent::OpenCode, None)],
                }],
                has_more: true,
                projects: vec![proj("buku-token")],
            },
        );
    }

    let mrow = |m: &str, agent, cost: Option<f64>| ModelRow {
        model: m.into(),
        agent,
        cost_usd: cost,
        unpriced_tokens: 0,
        tokens: 1000,
        input: 100,
        output: 200,
        cache_read: 600,
        cache_write: 100,
        share: cost.map(|_| 80.0),
    };
    dump(
        &dir,
        "models-view",
        &ModelsView {
            rows: vec![mrow("claude-opus-5-5", Agent::ClaudeCode, Some(12.0)), mrow("big-pickle", Agent::OpenCode, None)],
            total_cost_usd: 12.0,
            total_tokens: 2000,
            unpriced_tokens: 1000,
            token_mix: TokenMix {
                tokens: Mix { cache_read: 60.0, input: 10.0, output: 20.0, cache_write: 10.0 },
                cost: Mix { cache_read: 20.0, input: 20.0, output: 50.0, cache_write: 10.0 },
            },
            cache_insight: Some(CacheInsight { token_share: 60.0, cost_share: 20.0, cost_without_cache: 40.0 }),
        },
    );

    let prow = |name: &str, folded| ProjectRow {
        project: proj(name),
        by_agent: [8.0, 2.0, 0.0, 0.0, 0.0],
        cost_usd: 10.0,
        unpriced_tokens: 0,
        sessions: 4,
        share: 50.0,
        folded,
    };
    dump(
        &dir,
        "projects-view",
        &ProjectsView {
            rows: vec![prow("buku-token", 0), prow("Project Lainnya", 3)],
            total_cost_usd: 20.0,
            insight: Some(ProjectInsight { project: proj("buku-token"), share: 50.0, prev_share: 35.0 }),
        },
    );

    dump(
        &dir,
        "history-view",
        &HistoryView {
            days: vec![DayCost { date: day("2026-10-29"), by_agent: [5.0, 1.0, 0.0, 0.5, 0.0], unpriced_tokens: 700 }],
            total_usd: 6.5,
            avg_per_day: 6.5,
            unpriced_tokens: 0,
            plans: vec![PlanValue {
                plan_id: "claude-max".into(),
                name: "Claude Max 5×".into(),
                agents: vec![Agent::ClaudeCode],
                price_usd: 100.0,
                cycle: Cycle { start: day("2026-10-01"), end_exclusive: day("2026-11-01"), day_of_cycle: 30, length: 31 },
                api_equivalent_usd: 420.0,
                multiple: 4.2,
                projected_usd: 434.0,
                projected_multiple: 4.34,
            }],
        },
    );

    let s = Settings {
        plans: vec![Plan { id: "p".into(), name: "Plan".into(), price_usd: 20.0, renewal_day: 5, agents: vec![Agent::Codex] }],
        enabled_agents: HashMap::from([(Agent::ClaudeCode, true), (Agent::OpenCode, false)]),
        ..Settings::default()
    };
    dump(&dir, "settings", &s);

    dump(
        &dir,
        "scan-status",
        &ScanStatus {
            running: false,
            done: 648,
            total: 648,
            skipped: 2,
            errors: 0,
            last_scan_at: Some(ts("2026-10-30T12:40:00Z")),
            pricing_version: "2026-10-30".into(),
            sources: vec![SourceStatus { agent: Agent::ClaudeCode, path: "/Users/dev/.claude/projects".into(), found: true }],
            activity_sources: vec![ActivitySource {
                name: "Antigravity".into(),
                path: "/Users/dev/.gemini/antigravity/conversations".into(),
                found: true,
            }],
        },
    );
}

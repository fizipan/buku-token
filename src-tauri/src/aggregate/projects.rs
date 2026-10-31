//! Project: cost per project stacked by agent, top 6 plus "N Project Lainnya".

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use chrono::{DateTime, Days, FixedOffset, Utc};
use serde::Serialize;

use super::{agent_index, days_start, enabled_events, local_date, local_midnight, percent, total_tokens, Period};
use crate::model::Agent;
use crate::project::{ProjectRef, ProjectResolver};
use crate::settings::{Lang, Settings};
use crate::store::{Store, StoredEvent};

const TOP_PROJECTS: usize = 6;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    /// For the folded row: path "", name "N Project Lainnya", display_path = the folded display paths joined by ", ".
    pub project: ProjectRef,
    /// USD per agent in `STACK_ORDER`.
    pub by_agent: [f64; 5],
    pub cost_usd: f64,
    pub unpriced_tokens: u64,
    /// Distinct sessions with events in the period.
    pub sessions: u32,
    /// Percent of the period's priced cost.
    pub share: f64,
    /// 0 for a real project; N for the "N Project Lainnya" row.
    pub folded: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInsight {
    /// The costliest project of the period.
    pub project: ProjectRef,
    pub share: f64,
    /// Its share in the preceding period of the same number of days.
    pub prev_share: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectsView {
    pub rows: Vec<ProjectRow>,
    pub total_cost_usd: f64,
    /// `None` when the period or the preceding one has no priced cost.
    pub insight: Option<ProjectInsight>,
}

pub fn projects(
    store: &Store,
    projects: &mut ProjectResolver,
    settings: &Settings,
    period: Period,
    tz: FixedOffset,
    now: DateTime<Utc>,
) -> Result<ProjectsView> {
    let start = days_start(period.days(), tz, now);
    let mut rows = project_rows(&enabled_events(store, settings, start, now)?, projects, settings.language);
    let total_cost_usd: f64 = rows.iter().map(|r| r.cost_usd).sum();
    for r in &mut rows {
        r.share = percent(r.cost_usd, total_cost_usd);
    }
    rows.sort_by(|a, b| b.cost_usd.total_cmp(&a.cost_usd).then_with(|| a.project.name.cmp(&b.project.name)));

    let mut insight = None;
    if let Some(top) = rows.first().filter(|r| r.cost_usd > 0.0) {
        let prev_start = local_midnight(local_date(start, tz) - Days::new(u64::from(period.days())), tz);
        let (mut prev_total, mut prev_top) = (0.0, 0.0);
        for e in enabled_events(store, settings, prev_start, start)? {
            let c = e.cost_usd.unwrap_or(0.0);
            prev_total += c;
            if projects.resolve(&e.cwd, settings.language).path == top.project.path {
                prev_top += c;
            }
        }
        if prev_total > 0.0 {
            insight = Some(ProjectInsight { project: top.project.clone(), share: top.share, prev_share: percent(prev_top, prev_total) });
        }
    }

    if rows.len() > TOP_PROJECTS {
        let tail = rows.split_off(TOP_PROJECTS);
        rows.push(fold(&tail, settings.language));
    }
    Ok(ProjectsView { rows, total_cost_usd, insight })
}

type SessionSet<'a> = HashSet<(Agent, &'a str)>;

/// One row per project, unsorted, `share` not yet set.
fn project_rows(evs: &[StoredEvent], projects: &mut ProjectResolver, lang: Lang) -> Vec<ProjectRow> {
    // project path → (row, distinct (agent, session_id))
    let mut acc: HashMap<String, (ProjectRow, SessionSet)> = HashMap::new();
    for e in evs {
        let project = projects.resolve(&e.cwd, lang);
        let (row, sessions) = acc.entry(project.path.clone()).or_insert_with(|| {
            let row = ProjectRow { project, by_agent: [0.0; 5], cost_usd: 0.0, unpriced_tokens: 0, sessions: 0, share: 0.0, folded: 0 };
            (row, HashSet::new())
        });
        sessions.insert((e.agent, &e.session_id));
        match e.cost_usd {
            Some(c) => {
                row.cost_usd += c;
                row.by_agent[agent_index(e.agent)] += c;
            }
            None => row.unpriced_tokens += total_tokens(e.agent, &e.tokens),
        }
    }
    acc.into_values()
        .map(|(mut row, sessions)| {
            row.sessions = sessions.len() as u32;
            row
        })
        .collect()
}

/// "N Project Lainnya": sums of the rows beyond the top 6.
fn fold(tail: &[ProjectRow], lang: Lang) -> ProjectRow {
    let display_path = tail.iter().map(|r| r.project.display_path.as_str()).collect::<Vec<_>>().join(", ");
    let mut other = ProjectRow {
        project: ProjectRef { path: String::new(), name: crate::i18n::other_projects(tail.len(), lang), display_path },
        by_agent: [0.0; 5],
        cost_usd: 0.0,
        unpriced_tokens: 0,
        sessions: 0,
        share: 0.0,
        folded: tail.len() as u32,
    };
    for r in tail {
        for (sum, c) in other.by_agent.iter_mut().zip(r.by_agent) {
            *sum += c;
        }
        other.cost_usd += r.cost_usd;
        other.unpriced_tokens += r.unpriced_tokens;
        other.sessions += r.sessions;
        other.share += r.share;
    }
    other
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::testkit::*;
    use crate::model::Agent;

    fn run(s: &Store, settings: &Settings, period: Period) -> ProjectsView {
        projects(s, &mut resolver(), settings, period, tz(), now()).unwrap()
    }

    /// p1..p8 cost $8..$1 this week; p7 has two sessions, p8 has Claude and Codex.
    fn eight() -> Store {
        let day = at(2026, 10, 28, 10, 0);
        let mut evs: Vec<_> = (1..=6).map(|i| cost_at(day, Agent::ClaudeCode, &doc(&format!("p{i}")), &format!("s{i}"), 9.0 - i as f64)).collect();
        evs.push(cost_at(day, Agent::ClaudeCode, &doc("p7"), "s7a", 1.5));
        evs.push(cost_at(day, Agent::ClaudeCode, &doc("p7"), "s7b", 0.5));
        evs.push(cost_at(day, Agent::ClaudeCode, &doc("p8"), "s8", 0.6));
        evs.push(cost_at(day, Agent::Codex, &doc("p8"), "s8x", 0.4));
        store(evs)
    }

    #[test]
    fn projects_fold_tail_into_lainnya() {
        let v = run(&eight(), &Settings::default(), Period::Last7);
        assert_eq!(v.rows.len(), 7);
        let names: Vec<&str> = v.rows.iter().map(|r| r.project.name.as_str()).collect();
        assert_eq!(names, ["p1", "p2", "p3", "p4", "p5", "p6", "2 Project Lainnya"]);
        close(v.total_cost_usd, 36.0);
        let top = &v.rows[0];
        assert_eq!((top.sessions, top.folded, top.project.display_path.as_str()), (1, 0, "p1"));
        close(top.share, 8.0 / 36.0 * 100.0);
        let rest = &v.rows[6];
        assert_eq!(rest.folded, 2);
        assert_eq!(rest.sessions, 4);
        assert_eq!(rest.project.path, "");
        assert_eq!(rest.project.display_path, "p7, p8");
        close(rest.cost_usd, 3.0);
        close(rest.by_agent[0], 2.6);
        close(rest.by_agent[1], 0.4);
        close(rest.share, 3.0 / 36.0 * 100.0);
        close(v.rows.iter().map(|r| r.share).sum(), 100.0);
    }

    #[test]
    fn the_folded_row_is_named_in_english_when_asked() {
        let en = Settings { language: Lang::En, ..Settings::default() };
        let v = run(&eight(), &en, Period::Last7);
        assert_eq!(v.rows[6].project.name, "2 Other Projects");
        assert_eq!(v.rows[6].folded, 2);
    }

    #[test]
    fn six_or_fewer_projects_are_not_folded() {
        let day = at(2026, 10, 30, 9, 0);
        let s = store(vec![
            cost_at(day, Agent::ClaudeCode, &doc("a"), "s1", 1.0),
            cost_at(day, Agent::Codex, &doc("a"), "s2", 2.0),
            cost_at(day, Agent::Hermes, &doc("b"), "s3", 0.5),
        ]);
        let v = run(&s, &Settings::default(), Period::Today);
        assert_eq!(v.rows.len(), 2);
        assert_eq!(v.rows[0].by_agent, [1.0, 2.0, 0.0, 0.0, 0.0]);
        assert_eq!(v.rows[0].sessions, 2);
        assert_eq!(v.rows[1].by_agent, [0.0, 0.0, 0.0, 0.5, 0.0]);
        assert!(v.rows.iter().all(|r| r.folded == 0));
    }

    #[test]
    fn unpriced_tokens_reported_per_project() {
        let s = store(vec![
            ev(at(2026, 10, 30, 9, 0), Agent::OpenCode, "big-pickle", &doc("free"), "s", tok(300, 30), None),
            cost_at(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, &doc("paid"), "t", 1.0),
        ]);
        let v = run(&s, &Settings::default(), Period::Today);
        let free = v.rows.iter().find(|r| r.project.name == "free").unwrap();
        assert_eq!((free.cost_usd, free.unpriced_tokens, free.sessions), (0.0, 330, 1));
        assert_eq!(v.rows[0].project.name, "paid");
    }

    #[test]
    fn project_insight_none_without_previous_period() {
        assert_eq!(run(&eight(), &Settings::default(), Period::Last7).insight, None);
        assert_eq!(run(&store(vec![]), &Settings::default(), Period::Last7).insight, None);
    }

    #[test]
    fn project_insight_compares_previous_equal_length_period() {
        let (a, b, c) = (doc("A"), doc("B"), doc("C"));
        let s = store(vec![
            cost_at(at(2026, 10, 25, 9, 0), Agent::ClaudeCode, &a, "1", 39.0),
            cost_at(at(2026, 10, 26, 9, 0), Agent::ClaudeCode, &b, "2", 30.0),
            cost_at(at(2026, 10, 30, 9, 0), Agent::Codex, &c, "3", 31.0),
            // Previous 7 days: 17–23 Okt.
            cost_at(at(2026, 10, 17, 0, 30), Agent::ClaudeCode, &a, "4", 22.0),
            cost_at(at(2026, 10, 23, 23, 30), Agent::ClaudeCode, &b, "5", 78.0),
            // Before that: ignored.
            cost_at(at(2026, 10, 16, 23, 0), Agent::ClaudeCode, &a, "6", 100.0),
        ]);
        let i = run(&s, &Settings::default(), Period::Last7).insight.unwrap();
        assert_eq!(i.project.name, "A");
        close(i.share, 39.0);
        close(i.prev_share, 22.0);
    }

    #[test]
    fn today_insight_compares_with_yesterday() {
        let (a, b) = (doc("A"), doc("B"));
        let s = store(vec![
            cost_at(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, &a, "1", 3.0),
            cost_at(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, &b, "2", 1.0),
            cost_at(at(2026, 10, 29, 9, 0), Agent::ClaudeCode, &b, "3", 3.0),
            cost_at(at(2026, 10, 28, 9, 0), Agent::ClaudeCode, &a, "4", 50.0),
        ]);
        let i = run(&s, &Settings::default(), Period::Today).insight.unwrap();
        assert_eq!(i.project.name, "A");
        close(i.share, 75.0);
        close(i.prev_share, 0.0);
    }

    #[test]
    fn disabled_agents_excluded_from_projects() {
        let mut st = Settings::default();
        st.enabled_agents.insert(Agent::Codex, false);
        let v = run(&eight(), &st, Period::Last7);
        close(v.total_cost_usd, 35.6);
        let rest = v.rows.last().unwrap();
        assert_eq!((rest.sessions, rest.by_agent[1]), (3, 0.0));
    }

    #[test]
    fn serializes_camel_case() {
        let v = serde_json::to_value(run(&eight(), &Settings::default(), Period::Last7)).unwrap();
        assert_eq!(v["rows"][6]["folded"], 2);
        assert_eq!(v["rows"][0]["byAgent"][0], 8.0);
        assert_eq!(v["rows"][0]["project"]["displayPath"], "p1");
        assert!(v["totalCostUsd"].is_number());
        assert_eq!(v["insight"], serde_json::Value::Null);
    }
}

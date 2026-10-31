//! Mutasi: sessions grouped by the local day they started, newest first.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use super::{is_enabled, local_date, total_tokens};
use crate::fmt_id::day_label;
use crate::model::Agent;
use crate::project::{ProjectRef, ProjectResolver};
use crate::settings::Settings;
use crate::store::{SessionStart, Store, StoredEvent};

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFilter {
    pub agent: Option<Agent>,
    /// `ProjectRef::path` ("" = Tanpa Project).
    pub project_path: Option<String>,
    /// Only sessions that started before this local date (next page).
    pub before: Option<NaiveDate>,
    /// Number of days that have sessions to return.
    pub days: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub session_id: String,
    pub agent: Agent,
    /// Project of the session's first event.
    pub project: ProjectRef,
    pub started_at: DateTime<Utc>,
    /// First to last event, whole minutes.
    pub duration_min: u32,
    /// Model with the highest cost in the session (most tokens when nothing is priced).
    pub main_model: String,
    pub tokens: u64,
    /// Sum of known prices; `None` when no event of the session has a price.
    pub cost_usd: Option<f64>,
    pub unpriced_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayGroup {
    pub date: NaiveDate,
    /// "Hari Ini · Jum, 30 Okt" / "Kemarin · Kam, 29 Okt" / "Rab, 28 Okt".
    pub label: String,
    /// Sum of known prices.
    pub cost_usd: f64,
    /// Tokens of this day's sessions that have no price ("–" when `cost_usd` is 0).
    pub unpriced_tokens: u64,
    pub sessions: Vec<SessionRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPage {
    pub days: Vec<DayGroup>,
    /// True when an older day with sessions exists ("Muat 3 Hari Sebelumnya").
    pub has_more: bool,
    /// Options for the project Select: every project with a session (ignores the filter), by name.
    pub projects: Vec<ProjectRef>,
}

pub fn sessions(
    store: &Store,
    projects: &mut ProjectResolver,
    settings: &Settings,
    filter: &SessionFilter,
    tz: FixedOffset,
    now: DateTime<Utc>,
) -> Result<SessionPage> {
    let mut by_path: BTreeMap<String, ProjectRef> = BTreeMap::new();
    let mut picked: Vec<(NaiveDate, SessionStart, ProjectRef)> = Vec::new();
    for s in store.session_starts()? {
        if !is_enabled(settings, s.agent) {
            continue;
        }
        let project = projects.resolve(&s.cwd, settings.language);
        by_path.entry(project.path.clone()).or_insert_with(|| project.clone());
        let date = local_date(s.start, tz);
        let wanted = filter.agent.is_none_or(|a| a == s.agent)
            && filter.project_path.as_ref().is_none_or(|p| *p == project.path)
            && filter.before.is_none_or(|b| date < b);
        if wanted {
            picked.push((date, s, project));
        }
    }
    picked.sort_by_key(|p| Reverse(p.1.start));

    let today = local_date(now, tz);
    let mut days: Vec<DayGroup> = Vec::new();
    let mut has_more = false;
    for (date, start, project) in picked {
        if days.last().map(|d| d.date) != Some(date) {
            if days.len() == filter.days as usize {
                has_more = true;
                break;
            }
            days.push(DayGroup { date, label: day_label(date, today, settings.language), cost_usd: 0.0, unpriced_tokens: 0, sessions: Vec::new() });
        }
        let row = session_row(&start, project, &store.session_events(start.agent, &start.session_id)?);
        let day = days.last_mut().expect("pushed above");
        day.cost_usd += row.cost_usd.unwrap_or(0.0);
        day.unpriced_tokens += row.unpriced_tokens;
        day.sessions.push(row);
    }

    let mut options: Vec<ProjectRef> = by_path.into_values().collect();
    options.sort_by_cached_key(|p| p.name.to_lowercase());
    Ok(SessionPage { days, has_more, projects: options })
}

/// `evs` = every event of the session, oldest first.
fn session_row(start: &SessionStart, project: ProjectRef, evs: &[StoredEvent]) -> SessionRow {
    let (mut tokens, mut unpriced_tokens, mut cost) = (0, 0, None::<f64>);
    // model → (priced cost, tokens)
    let mut by_model: BTreeMap<&str, (f64, u64)> = BTreeMap::new();
    for e in evs {
        let n = total_tokens(e.agent, &e.tokens);
        tokens += n;
        match e.cost_usd {
            Some(c) => cost = Some(cost.unwrap_or(0.0) + c),
            None => unpriced_tokens += n,
        }
        let m = by_model.entry(&e.model).or_default();
        m.0 += e.cost_usd.unwrap_or(0.0);
        m.1 += n;
    }
    let main_model = by_model
        .into_iter()
        .max_by(|a, b| a.1.0.total_cmp(&b.1.0).then(a.1.1.cmp(&b.1.1)))
        .map(|(m, _)| m.to_string())
        .unwrap_or_default();
    let last = evs.last().map_or(start.start, |e| e.ts);
    SessionRow {
        session_id: start.session_id.clone(),
        agent: start.agent,
        project,
        started_at: start.start,
        duration_min: u32::try_from((last - start.start).num_minutes()).unwrap_or(0),
        main_model,
        tokens,
        cost_usd: cost,
        unpriced_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::testkit::*;
    use crate::model::Tokens;

    fn all(days: u32) -> SessionFilter {
        SessionFilter { agent: None, project_path: None, before: None, days }
    }

    /// Mirrors the mockup: two sessions today, one yesterday that runs past midnight,
    /// one on Rab 28 Okt (partly unpriced) and one on Sel 20 Okt.
    fn sample() -> Store {
        let lab = doc("TOOLS-LAB");
        let scaffold = doc("TIL/scaffold-ai");
        let cms = doc("Acme/cms-klien-a");
        let promo = doc("Acme/landing-promo");
        store(vec![
            ev(at(2026, 10, 30, 10, 2), Agent::ClaudeCode, "claude-opus-5-5", &lab, "s-a", tok(1000, 100), Some(2.9)),
            ev(at(2026, 10, 30, 10, 40), Agent::ClaudeCode, "claude-opus-5-5", &lab, "s-a", tok(1000, 100), Some(1.0)),
            ev(at(2026, 10, 30, 8, 47), Agent::Codex, "gpt-5.5", &scaffold, "s-b", tok(600, 12), Some(1.85)),
            ev(at(2026, 10, 30, 9, 11), Agent::Codex, "gpt-5.5", &scaffold, "s-b", tok(0, 0), None),
            ev(at(2026, 10, 29, 21, 15), Agent::ClaudeCode, "claude-opus-5-5", &cms, "s-c", tok(10, 1), Some(12.3)),
            ev(at(2026, 10, 30, 0, 30), Agent::ClaudeCode, "claude-opus-5-5", &cms, "s-c", tok(10, 1), Some(1.0)),
            ev(at(2026, 10, 28, 14, 30), Agent::OpenCode, "kimi-k3", &promo, "s-d", tok(800, 80), Some(1.4)),
            ev(at(2026, 10, 28, 14, 52), Agent::OpenCode, "big-pickle", &promo, "s-d", tok(50, 5), None),
            ev(at(2026, 10, 20, 9, 0), Agent::ClaudeCode, "claude-sonnet-5-5", "", "s-e", tok(5, 5), Some(2.0)),
        ])
    }

    fn run(s: &Store, settings: &Settings, f: &SessionFilter) -> SessionPage {
        sessions(s, &mut resolver(), settings, f, tz(), now()).unwrap()
    }

    fn ids(day: &DayGroup) -> Vec<&str> {
        day.sessions.iter().map(|r| r.session_id.as_str()).collect()
    }

    #[test]
    fn sessions_grouped_by_local_day_with_labels() {
        let p = run(&sample(), &Settings::default(), &all(3));
        let labels: Vec<&str> = p.days.iter().map(|d| d.label.as_str()).collect();
        assert_eq!(labels, ["Hari Ini · Jum, 30 Okt", "Kemarin · Kam, 29 Okt", "Rab, 28 Okt"]);
        assert_eq!(p.days.iter().map(|d| d.date).collect::<Vec<_>>(), [date(2026, 10, 30), date(2026, 10, 29), date(2026, 10, 28)]);
        assert_eq!(ids(&p.days[0]), ["s-a", "s-b"], "newest first");
        // s-c started yesterday; its event after midnight still belongs to it.
        assert_eq!(ids(&p.days[1]), ["s-c"]);
        close(p.days[1].cost_usd, 13.3);
        close(p.days[0].cost_usd, 3.9 + 1.85);
        assert!(p.has_more, "Sel 20 Okt is older");

        let a = &p.days[0].sessions[0];
        assert_eq!(a.agent, Agent::ClaudeCode);
        assert_eq!(a.started_at, at(2026, 10, 30, 10, 2));
        assert_eq!(a.duration_min, 38);
        assert_eq!(a.tokens, 2200);
        assert_eq!(a.project.name, "TOOLS-LAB");
        assert_eq!(a.project.display_path, "TOOLS-LAB");
        assert_eq!(a.main_model, "claude-opus-5-5");
        close(a.cost_usd.unwrap(), 3.9);
    }

    #[test]
    fn english_labels_and_no_project_name_follow_the_language_setting() {
        let en = Settings { language: crate::settings::Lang::En, ..Settings::default() };
        let p = run(&sample(), &en, &all(3));
        let labels: Vec<&str> = p.days.iter().map(|d| d.label.as_str()).collect();
        assert_eq!(labels, ["Today · Fri, Oct 30", "Yesterday · Thu, Oct 29", "Wed, Oct 28"]);
        let old = run(&sample(), &en, &SessionFilter { before: Some(date(2026, 10, 28)), ..all(3) });
        assert_eq!(old.days[0].sessions[0].project.name, "No Project");
        assert_eq!(old.days[0].sessions[0].project.path, "", "the path, not the name, identifies it");
    }

    #[test]
    fn next_page_starts_before_the_given_date() {
        let s = sample();
        let p = run(&s, &Settings::default(), &SessionFilter { before: Some(date(2026, 10, 28)), ..all(3) });
        assert_eq!(p.days.len(), 1);
        assert_eq!(p.days[0].label, "Sel, 20 Okt");
        assert_eq!(p.days[0].sessions[0].project.name, "Tanpa Project");
        assert!(!p.has_more);
        let p = run(&s, &Settings::default(), &all(4));
        assert_eq!(p.days.len(), 4);
        assert!(!p.has_more);
        let p = run(&s, &Settings::default(), &all(1));
        assert_eq!(p.days.len(), 1);
        assert!(p.has_more);
    }

    #[test]
    fn main_model_is_highest_cost() {
        let p = doc("x");
        let s = store(vec![
            ev(at(2026, 10, 30, 9, 0), Agent::OpenCode, "cheap", &p, "s", tok(90_000, 1000), Some(0.2)),
            ev(at(2026, 10, 30, 9, 5), Agent::OpenCode, "cheap", &p, "s", tok(90_000, 1000), Some(0.2)),
            ev(at(2026, 10, 30, 9, 10), Agent::OpenCode, "pricey", &p, "s", tok(100, 10), Some(0.5)),
            ev(at(2026, 10, 30, 9, 15), Agent::OpenCode, "unknown", &p, "s", tok(500_000, 0), None),
        ]);
        let row = &run(&s, &Settings::default(), &all(1)).days[0].sessions[0];
        assert_eq!(row.main_model, "pricey");
        close(row.cost_usd.unwrap(), 0.9);
        assert_eq!(row.unpriced_tokens, 500_000);
        assert_eq!(row.duration_min, 15);
    }

    #[test]
    fn unpriced_session_has_no_cost_and_main_model_by_tokens() {
        let p = doc("x");
        let s = store(vec![
            ev(at(2026, 10, 30, 9, 0), Agent::OpenCode, "a", &p, "s", tok(10, 0), None),
            ev(at(2026, 10, 30, 9, 1), Agent::OpenCode, "b", &p, "s", tok(20, 0), None),
        ]);
        let page = run(&s, &Settings::default(), &all(1));
        let row = &page.days[0].sessions[0];
        assert_eq!(row.cost_usd, None);
        assert_eq!(row.main_model, "b");
        assert_eq!(row.unpriced_tokens, 30);
        assert_eq!(page.days[0].cost_usd, 0.0);
        assert_eq!(page.days[0].unpriced_tokens, 30);
    }

    #[test]
    fn day_sums_the_unpriced_tokens_of_its_sessions() {
        let p = run(&sample(), &Settings::default(), &all(3));
        assert_eq!(p.days[2].date, date(2026, 10, 28));
        assert_eq!(p.days[2].unpriced_tokens, 55, "big-pickle only; the priced model is not counted");
        assert_eq!(p.days[1].unpriced_tokens, 0);
    }

    #[test]
    fn session_tokens_follow_r12() {
        let codex = Tokens { input: 1000, output: 50, cache_read: 400, reasoning: 20, ..Default::default() };
        let s = store(vec![ev(at(2026, 10, 30, 9, 0), Agent::Codex, "gpt", &doc("x"), "s", codex, Some(0.1))]);
        assert_eq!(run(&s, &Settings::default(), &all(1)).days[0].sessions[0].tokens, 1050);
    }

    #[test]
    fn filters_by_agent_and_project() {
        let s = sample();
        let p = run(&s, &Settings::default(), &SessionFilter { agent: Some(Agent::ClaudeCode), ..all(2) });
        assert_eq!(p.days.iter().map(ids).collect::<Vec<_>>(), vec![vec!["s-a"], vec!["s-c"]]);
        assert!(p.has_more);
        let cms = doc("Acme/cms-klien-a");
        let p = run(&s, &Settings::default(), &SessionFilter { project_path: Some(cms), ..all(3) });
        assert_eq!(p.days.iter().map(ids).collect::<Vec<_>>(), vec![vec!["s-c"]]);
        assert!(!p.has_more);
        let p = run(&s, &Settings::default(), &SessionFilter { project_path: Some(String::new()), ..all(3) });
        assert_eq!(p.days.iter().map(ids).collect::<Vec<_>>(), vec![vec!["s-e"]]);
    }

    #[test]
    fn disabled_agents_excluded_from_sessions() {
        let mut st = Settings::default();
        st.enabled_agents.insert(Agent::Codex, false);
        st.enabled_agents.insert(Agent::OpenCode, false);
        let p = run(&sample(), &st, &all(3));
        let all_ids: Vec<&str> = p.days.iter().flat_map(ids).collect();
        assert_eq!(all_ids, ["s-a", "s-c", "s-e"]);
        assert!(!p.projects.iter().any(|r| r.name == "landing-promo"));
    }

    #[test]
    fn project_options_list_every_project_by_name() {
        let p = run(&sample(), &Settings::default(), &SessionFilter { agent: Some(Agent::Codex), ..all(1) });
        let names: Vec<&str> = p.projects.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["cms-klien-a", "landing-promo", "scaffold-ai", "Tanpa Project", "TOOLS-LAB"]);
    }

    #[test]
    fn filter_and_page_serialize_camel_case() {
        let f: SessionFilter = serde_json::from_str(r#"{"agent":"cx","projectPath":"/p","before":"2026-10-28","days":3}"#).unwrap();
        assert_eq!(f, SessionFilter { agent: Some(Agent::Codex), project_path: Some("/p".into()), before: Some(date(2026, 10, 28)), days: 3 });
        let f: SessionFilter = serde_json::from_str(r#"{"days":2}"#).unwrap();
        assert_eq!(f, all(2));
        let v = serde_json::to_value(run(&sample(), &Settings::default(), &all(1))).unwrap();
        assert_eq!(v["hasMore"], true);
        assert_eq!(v["days"][0]["date"], "2026-10-30");
        assert_eq!(v["days"][0]["sessions"][0]["startedAt"], "2026-10-30T03:02:00Z");
        assert_eq!(v["days"][0]["sessions"][0]["durationMin"], 38);
        assert_eq!(v["days"][0]["sessions"][0]["mainModel"], "claude-opus-5-5");
        assert_eq!(v["days"][0]["sessions"][0]["project"]["displayPath"], "TOOLS-LAB");
    }
}

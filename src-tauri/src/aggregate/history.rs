//! Riwayat: daily cost stacked by agent, plus subscription value per plan.

use anyhow::Result;
use chrono::{DateTime, Days, FixedOffset, NaiveDate, Utc};
use serde::Serialize;

use super::{agent_index, enabled_events, is_enabled, local_date, local_midnight, total_tokens};
use crate::model::Agent;
use crate::settings::{current_cycle, Cycle, Settings};
use crate::store::Store;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayCost {
    pub date: NaiveDate,
    /// USD per agent in `STACK_ORDER`.
    pub by_agent: [f64; 5],
    /// Tokens of this day with no price ("–" when the day has no priced cost).
    pub unpriced_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanValue {
    pub plan_id: String,
    pub name: String,
    /// The plan's agents that are enabled (plans with none are left out).
    pub agents: Vec<Agent>,
    pub price_usd: f64,
    /// Current billing cycle; independent of the history period.
    pub cycle: Cycle,
    /// Priced cost of `agents` from the cycle start (local midnight) until now.
    pub api_equivalent_usd: f64,
    /// api_equivalent / price (0 when the price is 0).
    pub multiple: f64,
    /// api_equivalent / day_of_cycle × cycle length (an estimate).
    pub projected_usd: f64,
    pub projected_multiple: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryView {
    /// Last `days` local days, oldest first (today last).
    pub days: Vec<DayCost>,
    pub total_usd: f64,
    pub avg_per_day: f64,
    pub unpriced_tokens: u64,
    pub plans: Vec<PlanValue>,
}

pub fn history(store: &Store, settings: &Settings, days: u32, tz: FixedOffset, now: DateTime<Utc>) -> Result<HistoryView> {
    let today = local_date(now, tz);
    let mut day_costs: Vec<DayCost> = (0..days)
        .rev()
        .map(|back| DayCost { date: today - Days::new(u64::from(back)), by_agent: [0.0; 5], unpriced_tokens: 0 })
        .collect();
    let (mut total_usd, mut unpriced_tokens) = (0.0, 0);
    if let Some(first) = day_costs.first().map(|d| d.date) {
        for e in enabled_events(store, settings, local_midnight(first, tz), now)? {
            let i = (local_date(e.ts, tz) - first).num_days() as usize;
            let Some(c) = e.cost_usd else {
                let n = total_tokens(e.agent, &e.tokens);
                unpriced_tokens += n;
                day_costs[i].unpriced_tokens += n;
                continue;
            };
            total_usd += c;
            day_costs[i].by_agent[agent_index(e.agent)] += c;
        }
    }
    let avg_per_day = if days > 0 { total_usd / f64::from(days) } else { 0.0 };

    let mut plans = Vec::new();
    for plan in &settings.plans {
        let agents: Vec<Agent> = plan.agents.iter().copied().filter(|a| is_enabled(settings, *a)).collect();
        if agents.is_empty() {
            continue;
        }
        let cycle = current_cycle(plan.renewal_day, today);
        let api_equivalent_usd: f64 = enabled_events(store, settings, local_midnight(cycle.start, tz), now)?
            .iter()
            .filter(|e| agents.contains(&e.agent))
            .filter_map(|e| e.cost_usd)
            .sum();
        let projected_usd = api_equivalent_usd / f64::from(cycle.day_of_cycle) * f64::from(cycle.length);
        let per_price = |usd: f64| if plan.price_usd > 0.0 { usd / plan.price_usd } else { 0.0 };
        plans.push(PlanValue {
            plan_id: plan.id.clone(),
            name: plan.name.clone(),
            agents,
            price_usd: plan.price_usd,
            multiple: per_price(api_equivalent_usd),
            projected_multiple: per_price(projected_usd),
            cycle,
            api_equivalent_usd,
            projected_usd,
        });
    }
    Ok(HistoryView { days: day_costs, total_usd, avg_per_day, unpriced_tokens, plans })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::testkit::*;
    use crate::settings::Plan;

    fn plan(id: &str, price: f64, renewal_day: u8, agents: Vec<Agent>) -> Plan {
        Plan { id: id.into(), name: format!("Plan {id}"), price_usd: price, renewal_day, agents }
    }

    fn with_plans(plans: Vec<Plan>) -> Settings {
        Settings { plans, ..Settings::default() }
    }

    #[test]
    fn days_stack_by_agent_oldest_first() {
        let s = store(vec![
            cost_at(at(2026, 10, 23, 23, 0), Agent::ClaudeCode, "/p", "a", 100.0),
            cost_at(at(2026, 10, 24, 0, 30), Agent::ClaudeCode, "/p", "a", 2.0),
            cost_at(at(2026, 10, 27, 15, 0), Agent::Codex, "/p", "b", 3.0),
            cost_at(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, "/p", "c", 4.0),
            cost_at(at(2026, 10, 30, 10, 0), Agent::Hermes, "/p", "d", 1.0),
            ev(at(2026, 10, 30, 11, 0), Agent::OpenCode, "big-pickle", "/p", "e", tok(10, 0), None),
        ]);
        let h = history(&s, &with_plans(vec![]), 7, tz(), now()).unwrap();
        let dates: Vec<NaiveDate> = h.days.iter().map(|d| d.date).collect();
        assert_eq!(dates, (24..=30).map(|d| date(2026, 10, d)).collect::<Vec<_>>());
        assert_eq!(h.days[0].by_agent, [2.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(h.days[3].by_agent, [0.0, 3.0, 0.0, 0.0, 0.0]);
        assert_eq!(h.days[6].by_agent, [4.0, 0.0, 0.0, 1.0, 0.0]);
        close(h.total_usd, 10.0);
        close(h.avg_per_day, 10.0 / 7.0);
        assert_eq!(h.unpriced_tokens, 10);
        assert_eq!(h.days[6].unpriced_tokens, 10);
        assert_eq!(h.days[0].unpriced_tokens, 0);
    }

    #[test]
    fn zero_days_is_empty() {
        let h = history(&store(vec![]), &with_plans(vec![]), 0, tz(), now()).unwrap();
        assert!(h.days.is_empty());
        assert_eq!((h.total_usd, h.avg_per_day), (0.0, 0.0));
    }

    #[test]
    fn plan_value_uses_current_cycle_and_linked_agents() {
        let s = store(vec![
            cost_at(at(2026, 9, 30, 23, 0), Agent::ClaudeCode, "/p", "a", 50.0),
            cost_at(at(2026, 10, 1, 0, 30), Agent::ClaudeCode, "/p", "a", 700.0),
            cost_at(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, "/p", "b", 84.0),
            cost_at(at(2026, 10, 15, 9, 0), Agent::Codex, "/p", "c", 40.0),
        ]);
        let h = history(&s, &with_plans(vec![plan("claude-max", 100.0, 1, vec![Agent::ClaudeCode])]), 7, tz(), now()).unwrap();
        let p = &h.plans[0];
        assert_eq!((p.plan_id.as_str(), p.name.as_str(), p.agents.as_slice(), p.price_usd), ("claude-max", "Plan claude-max", &[Agent::ClaudeCode][..], 100.0));
        assert_eq!((p.cycle.start, p.cycle.end_exclusive, p.cycle.day_of_cycle, p.cycle.length), (date(2026, 10, 1), date(2026, 11, 1), 30, 31));
        close(p.api_equivalent_usd, 784.0);
        close(p.multiple, 7.84);
        close(p.projected_usd, 784.0 / 30.0 * 31.0);
        close(p.projected_multiple, 784.0 / 30.0 * 31.0 / 100.0);
    }

    #[test]
    fn plan_projection_scales_to_cycle_length() {
        let s = store(vec![
            cost_at(at(2026, 11, 1, 0, 10), Agent::ClaudeCode, "/p", "a", 100.0),
            cost_at(at(2026, 11, 12, 9, 0), Agent::ClaudeCode, "/p", "b", 200.0),
        ]);
        let now = at(2026, 11, 12, 12, 0);
        let h = history(&s, &with_plans(vec![plan("max", 100.0, 1, vec![Agent::ClaudeCode])]), 7, tz(), now).unwrap();
        let p = &h.plans[0];
        assert_eq!((p.cycle.day_of_cycle, p.cycle.length), (12, 30));
        close(p.api_equivalent_usd, 300.0);
        close(p.projected_usd, 750.0);
        close(p.projected_multiple, 7.5);
    }

    #[test]
    fn plans_count_only_enabled_agents() {
        let s = store(vec![
            cost_at(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, "/p", "a", 10.0),
            cost_at(at(2026, 10, 30, 9, 0), Agent::Codex, "/p", "b", 5.0),
        ]);
        let mut st = with_plans(vec![
            plan("plus", 20.0, 1, vec![Agent::Codex]),
            plan("both", 50.0, 1, vec![Agent::ClaudeCode, Agent::Codex]),
            plan("free", 0.0, 1, vec![Agent::ClaudeCode]),
        ]);
        st.enabled_agents.insert(Agent::Codex, false);
        let h = history(&s, &st, 7, tz(), now()).unwrap();
        let ids: Vec<&str> = h.plans.iter().map(|p| p.plan_id.as_str()).collect();
        assert_eq!(ids, ["both", "free"], "a plan with no enabled agent is left out");
        assert_eq!(h.plans[0].agents, [Agent::ClaudeCode]);
        close(h.plans[0].api_equivalent_usd, 10.0);
        assert_eq!((h.plans[1].multiple, h.plans[1].projected_multiple), (0.0, 0.0));
        close(h.total_usd, 10.0);
    }

    #[test]
    fn serializes_camel_case() {
        let s = store(vec![cost_at(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, "/p", "a", 10.0)]);
        let v = serde_json::to_value(history(&s, &Settings::default(), 7, tz(), now()).unwrap()).unwrap();
        assert_eq!(v["days"][0]["date"], "2026-10-24");
        assert_eq!(v["days"][6]["byAgent"][0], 10.0);
        assert_eq!(v["totalUsd"], 10.0);
        assert!(v["avgPerDay"].is_number());
        assert_eq!(v["plans"][0]["planId"], "claude-max");
        assert_eq!(v["plans"][0]["agents"][0], "cc");
        assert_eq!(v["plans"][0]["apiEquivalentUsd"], 10.0);
        assert_eq!(v["plans"][0]["cycle"]["dayOfCycle"], 30);
        assert!(v["plans"][0]["projectedMultiple"].is_number());
        assert_eq!(v["plans"][1]["priceUsd"], 20.0);
    }
}

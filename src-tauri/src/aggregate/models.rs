//! Model: cost and tokens per (agent, model), token mix and the cache callout.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{DateTime, FixedOffset, Utc};
use serde::Serialize;

use super::{agent_index, days_start, enabled_events, percent, percent_mix, token_kinds, total_tokens, Mix, Period};
use crate::model::Agent;
use crate::pricing::Pricing;
use crate::settings::Settings;
use crate::store::Store;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRow {
    pub model: String,
    pub agent: Agent,
    /// Sum of known prices; `None` when no event of this model has a price.
    pub cost_usd: Option<f64>,
    pub unpriced_tokens: u64,
    /// R12 total.
    pub tokens: u64,
    /// Non-cached input (Codex: input − cache_read).
    pub input: u64,
    /// Output including reasoning.
    pub output: u64,
    pub cache_read: u64,
    /// 5m + 1h cache writes.
    pub cache_write: u64,
    /// Percent of the period's priced cost; `None` when the row has no price.
    pub share: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenMix {
    /// Share of all tokens per kind.
    pub tokens: Mix,
    /// Share of API-equivalent cost per kind (events whose model is in the price table).
    pub cost: Mix,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheInsight {
    /// Percent of tokens that are cache reads.
    pub token_share: f64,
    /// Percent of cost that is cache reads.
    pub cost_share: f64,
    /// Period cost if cache reads were billed at the model's input rate.
    pub cost_without_cache: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsView {
    /// By cost, highest first; unpriced rows last.
    pub rows: Vec<ModelRow>,
    pub total_cost_usd: f64,
    pub total_tokens: u64,
    pub unpriced_tokens: u64,
    pub token_mix: TokenMix,
    /// `None` without cache reads or priced cost.
    pub cache_insight: Option<CacheInsight>,
}

pub fn models(
    store: &Store,
    pricing: &Pricing,
    settings: &Settings,
    period: Period,
    tz: FixedOffset,
    now: DateTime<Utc>,
) -> Result<ModelsView> {
    let mut by_model: HashMap<(Agent, String), ModelRow> = HashMap::new();
    let (mut total_cost_usd, mut total_tokens_all, mut unpriced_tokens) = (0.0, 0, 0);
    // [cache read, input, output, cache write]
    let mut token_sums = [0.0; 4];
    let mut cost_sums = [0.0; 4];
    // cost_without_cache − total cost: cache reads re-billed at the input rate.
    let mut uncached_extra = 0.0;
    for e in enabled_events(store, settings, days_start(period.days(), tz, now), now)? {
        let kinds = token_kinds(e.agent, &e.tokens);
        let n = total_tokens(e.agent, &e.tokens);
        total_tokens_all += n;
        for (sum, k) in token_sums.iter_mut().zip(kinds) {
            *sum += k as f64;
        }
        let row = by_model.entry((e.agent, e.model.clone())).or_insert_with(|| ModelRow {
            model: e.model.clone(),
            agent: e.agent,
            cost_usd: None,
            unpriced_tokens: 0,
            tokens: 0,
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            share: None,
        });
        row.tokens += n;
        row.cache_read += kinds[0];
        row.input += kinds[1];
        row.output += kinds[2];
        row.cache_write += kinds[3];
        let Some(c) = e.cost_usd else {
            row.unpriced_tokens += n;
            unpriced_tokens += n;
            continue;
        };
        row.cost_usd = Some(row.cost_usd.unwrap_or(0.0) + c);
        total_cost_usd += c;
        // Reported costs for models missing from the price table cannot be split.
        if let Some(parts) = pricing.cost_parts(e.agent, &e.model, &e.tokens) {
            for (sum, p) in cost_sums.iter_mut().zip([parts.cache_read, parts.input, parts.output, parts.cache_write]) {
                *sum += p;
            }
            if let Some(input_rate) = pricing.input_price(&e.model) {
                uncached_extra += kinds[0] as f64 * input_rate - parts.cache_read;
            }
        }
    }

    let mut rows: Vec<ModelRow> = by_model.into_values().collect();
    for r in &mut rows {
        r.share = r.cost_usd.map(|c| percent(c, total_cost_usd));
    }
    rows.sort_by(|a, b| {
        let cost = |r: &ModelRow| r.cost_usd.unwrap_or(-1.0);
        cost(b)
            .total_cmp(&cost(a))
            .then(b.tokens.cmp(&a.tokens))
            .then_with(|| a.model.cmp(&b.model))
            .then(agent_index(a.agent).cmp(&agent_index(b.agent)))
    });

    let token_mix = TokenMix { tokens: percent_mix(token_sums), cost: percent_mix(cost_sums) };
    let cache_insight = (token_sums[0] > 0.0 && cost_sums.iter().sum::<f64>() > 0.0).then_some(CacheInsight {
        token_share: token_mix.tokens.cache_read,
        cost_share: token_mix.cost.cache_read,
        cost_without_cache: total_cost_usd + uncached_extra,
    });
    Ok(ModelsView { rows, total_cost_usd, total_tokens: total_tokens_all, unpriced_tokens, token_mix, cache_insight })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::testkit::*;
    use crate::model::Tokens;

    const PRICES: &str = r#"{
        "m-claude": {"input_cost_per_token": 3e-6, "output_cost_per_token": 15e-6,
                     "cache_read_input_token_cost": 0.3e-6, "cache_creation_input_token_cost": 3.75e-6},
        "m-gpt": {"input_cost_per_token": 1e-6, "output_cost_per_token": 8e-6,
                  "cache_read_input_token_cost": 0.1e-6}
    }"#;

    fn pricing() -> Pricing {
        Pricing::from_json_str(PRICES).unwrap()
    }

    fn claude_tokens() -> Tokens {
        Tokens { input: 1000, output: 100, cache_read: 10_000, cache_write_5m: 1000, ..Default::default() }
    }

    fn codex_tokens() -> Tokens {
        Tokens { input: 1000, output: 10, cache_read: 400, reasoning: 5, ..Default::default() }
    }

    /// Claude $0.01125, Codex $0.00072, OpenCode big-pickle unpriced (550 tokens).
    fn sample() -> Store {
        let p = doc("x");
        store_priced(
            vec![
                ev(at(2026, 10, 30, 9, 0), Agent::Codex, "m-gpt", &p, "b", codex_tokens(), None),
                ev(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, "m-claude", &p, "a", claude_tokens(), None),
                ev(at(2026, 10, 30, 10, 0), Agent::OpenCode, "big-pickle", &p, "c", tok(500, 50), None),
            ],
            &pricing(),
        )
    }

    fn run(s: &Store, settings: &Settings, period: Period) -> ModelsView {
        models(s, &pricing(), settings, period, tz(), now()).unwrap()
    }

    fn mix_sum(m: &Mix) -> f64 {
        m.cache_read + m.input + m.output + m.cache_write
    }

    #[test]
    fn rows_sorted_by_cost_with_token_columns() {
        let v = run(&sample(), &Settings::default(), Period::Today);
        let names: Vec<&str> = v.rows.iter().map(|r| r.model.as_str()).collect();
        assert_eq!(names, ["m-claude", "m-gpt", "big-pickle"]);
        let cc = &v.rows[0];
        assert_eq!((cc.agent, cc.input, cc.output, cc.cache_read, cc.cache_write, cc.tokens), (Agent::ClaudeCode, 1000, 100, 10_000, 1000, 12_100));
        close(cc.cost_usd.unwrap(), 0.01125);
        close(cc.share.unwrap(), 0.01125 / 0.01197 * 100.0);
        let cx = &v.rows[1];
        assert_eq!((cx.agent, cx.input, cx.output, cx.cache_read, cx.cache_write, cx.tokens), (Agent::Codex, 600, 10, 400, 0, 1010));
        close(cx.cost_usd.unwrap(), 0.00072);
        let oc = &v.rows[2];
        assert_eq!((oc.cost_usd, oc.share, oc.unpriced_tokens, oc.tokens), (None, None, 550, 550));
        close(v.total_cost_usd, 0.01197);
        assert_eq!(v.total_tokens, 13_660);
        assert_eq!(v.unpriced_tokens, 550);
    }

    #[test]
    fn token_mix_splits_tokens_and_cost_by_kind() {
        let v = run(&sample(), &Settings::default(), Period::Today);
        let t = &v.token_mix.tokens;
        close(t.cache_read, 10_400.0 / 13_660.0 * 100.0);
        close(t.input, 2100.0 / 13_660.0 * 100.0);
        close(t.output, 160.0 / 13_660.0 * 100.0);
        close(t.cache_write, 1000.0 / 13_660.0 * 100.0);
        close(mix_sum(t), 100.0);
        let c = &v.token_mix.cost;
        close(c.cache_read, 0.00304 / 0.01197 * 100.0);
        close(c.input, 0.0036 / 0.01197 * 100.0);
        close(c.output, 0.00158 / 0.01197 * 100.0);
        close(c.cache_write, 0.00375 / 0.01197 * 100.0);
        close(mix_sum(c), 100.0);
    }

    #[test]
    fn cache_insight_prices_cache_reads_at_input_rate() {
        let v = run(&sample(), &Settings::default(), Period::Today);
        let ci = v.cache_insight.expect("cache reads present");
        close(ci.token_share, v.token_mix.tokens.cache_read);
        close(ci.cost_share, v.token_mix.cost.cache_read);
        // 0.01197 − cache reads ($0.003 + $0.00004) + the same tokens at input rate ($0.03 + $0.0004).
        close(ci.cost_without_cache, 0.03933);
    }

    #[test]
    fn cache_insight_none_without_cache_reads() {
        let s = store_priced(vec![ev(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, "m-claude", &doc("x"), "a", tok(10, 10), None)], &pricing());
        assert_eq!(run(&s, &Settings::default(), Period::Today).cache_insight, None);
        let empty = run(&store(vec![]), &Settings::default(), Period::Today);
        assert!(empty.rows.is_empty() && empty.cache_insight.is_none());
        assert_eq!(empty.token_mix.tokens, Mix::default());
    }

    #[test]
    fn reported_cost_without_rates_counts_in_total_not_in_cost_mix() {
        let s = store_priced(
            vec![
                ev(at(2026, 10, 30, 9, 0), Agent::ClaudeCode, "m-claude", &doc("x"), "a", claude_tokens(), None),
                ev(at(2026, 10, 30, 9, 0), Agent::Hermes, "minimax-m3", &doc("x"), "h", tok(1000, 100), Some(0.5)),
            ],
            &pricing(),
        );
        let v = run(&s, &Settings::default(), Period::Today);
        assert_eq!(v.rows[0].model, "minimax-m3");
        close(v.total_cost_usd, 0.51125);
        close(v.token_mix.cost.cache_read, 0.003 / 0.01125 * 100.0);
        close(v.cache_insight.unwrap().cost_without_cache, 0.51125 - 0.003 + 0.03);
    }

    #[test]
    fn same_model_under_two_agents_is_two_rows() {
        let s = store_priced(
            vec![
                ev(at(2026, 10, 30, 9, 0), Agent::Codex, "m-gpt", &doc("x"), "a", tok(1000, 0), None),
                ev(at(2026, 10, 30, 9, 0), Agent::OpenCode, "m-gpt", &doc("x"), "b", tok(2000, 0), None),
            ],
            &pricing(),
        );
        let v = run(&s, &Settings::default(), Period::Today);
        let rows: Vec<(Agent, u64)> = v.rows.iter().map(|r| (r.agent, r.input)).collect();
        assert_eq!(rows, [(Agent::OpenCode, 2000), (Agent::Codex, 1000)]);
    }

    #[test]
    fn period_covers_last_n_local_days() {
        let s = store(vec![
            cost_at(at(2026, 10, 23, 23, 0), Agent::ClaudeCode, "/p", "a", 100.0),
            cost_at(at(2026, 10, 24, 0, 30), Agent::ClaudeCode, "/p", "a", 2.0),
            cost_at(at(2026, 10, 29, 23, 59), Agent::ClaudeCode, "/p", "a", 3.0),
            cost_at(at(2026, 10, 30, 0, 1), Agent::ClaudeCode, "/p", "a", 4.0),
        ]);
        close(run(&s, &Settings::default(), Period::Last7).total_cost_usd, 9.0);
        close(run(&s, &Settings::default(), Period::Today).total_cost_usd, 4.0);
        close(run(&s, &Settings::default(), Period::Last30).total_cost_usd, 109.0);
    }

    #[test]
    fn disabled_agents_excluded_from_models() {
        let mut st = Settings::default();
        st.enabled_agents.insert(Agent::Codex, false);
        let v = run(&sample(), &st, Period::Today);
        assert!(v.rows.iter().all(|r| r.agent != Agent::Codex));
        close(v.total_cost_usd, 0.01125);
        assert_eq!(v.total_tokens, 12_100 + 550);
    }

    #[test]
    fn serializes_camel_case() {
        let v = serde_json::to_value(run(&sample(), &Settings::default(), Period::Today)).unwrap();
        assert_eq!(v["rows"][0]["cacheRead"], 10_000);
        assert_eq!(v["rows"][0]["cacheWrite"], 1000);
        assert_eq!(v["rows"][0]["agent"], "cc");
        assert_eq!(v["rows"][2]["costUsd"], serde_json::Value::Null);
        assert!(v["tokenMix"]["cost"]["cacheRead"].is_number());
        assert!(v["cacheInsight"]["costWithoutCache"].is_number());
        assert_eq!(v["unpricedTokens"], 550);
    }
}

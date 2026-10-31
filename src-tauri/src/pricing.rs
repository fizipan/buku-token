use crate::model::{Agent, Tokens};
use anyhow::Result;
use std::collections::HashMap;

const BUNDLED_JSON: &str = include_str!("../resources/model_prices.json");
const BUNDLED_VERSION: &str = include_str!("../resources/model_prices.VERSION");
const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

#[derive(Clone, Debug)]
struct Entry {
    input: f64,
    output: f64,
    cache_read: Option<f64>,
    cache_write_5m: Option<f64>,
    cache_write_1h: Option<f64>,
}

pub struct Pricing {
    entries: HashMap<String, Entry>,
    version: String,
}

/// USD of one event split by token kind. `input` is non-cached input only,
/// `output` includes reasoning, `cache_write` covers both 5m and 1h writes.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CostParts {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl CostParts {
    pub fn total(&self) -> f64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
}

impl Pricing {
    pub fn bundled() -> Pricing {
        let mut p = Pricing::from_json_str(BUNDLED_JSON).expect("bundled price table is valid");
        p.version = BUNDLED_VERSION.trim().to_string();
        p
    }

    pub fn from_json_str(s: &str) -> Result<Pricing> {
        let raw: HashMap<String, serde_json::Value> = serde_json::from_str(s)?;
        let entries = raw
            .into_iter()
            .filter(|(k, _)| k != "sample_spec")
            .filter_map(|(k, v)| {
                let f = |name: &str| v.get(name).and_then(|x| x.as_f64());
                Some((
                    k,
                    Entry {
                        input: f("input_cost_per_token")?,
                        output: f("output_cost_per_token")?,
                        cache_read: f("cache_read_input_token_cost"),
                        cache_write_5m: f("cache_creation_input_token_cost"),
                        cache_write_1h: f("cache_creation_input_token_cost_above_1hr"),
                    },
                ))
            })
            .collect::<HashMap<_, _>>();
        anyhow::ensure!(!entries.is_empty(), "price table has no priced entries");
        Ok(Pricing { entries, version: "custom".to_string() })
    }

    pub fn version(&self) -> String {
        self.version.clone()
    }

    /// Labels a table loaded with `from_json_str` (e.g. with its download date).
    pub fn with_version(mut self, version: &str) -> Pricing {
        self.version = version.to_string();
        self
    }

    /// USD for one event. `None` when the model has no known price.
    /// Reasoning tokens are billed at the output rate (Codex output already includes them). Codex reports `input`
    /// including cached tokens; every other agent reports it excluding them.
    pub fn cost(&self, agent: Agent, model: &str, t: &Tokens) -> Option<f64> {
        self.cost_parts(agent, model, t).map(|p| p.total())
    }

    /// `cost()` split by token kind (same rules); `None` when `cost()` is `None`.
    pub fn cost_parts(&self, agent: Agent, model: &str, t: &Tokens) -> Option<CostParts> {
        let e = self.lookup(model)?;
        let fresh_input = if agent == Agent::Codex { t.input.saturating_sub(t.cache_read) } else { t.input };
        // Codex's output_tokens already include reasoning; other agents report them separately.
        let billed_output = if agent == Agent::Codex { t.output } else { t.output + t.reasoning };
        // No unlabeled estimates: a needed cache rate that is missing means no price.
        let read = match (t.cache_read, e.cache_read) {
            (0, r) => r.unwrap_or(0.0),
            (_, r) => r?,
        };
        let write_5m = match (t.cache_write_5m, e.cache_write_5m) {
            (0, w) => w.unwrap_or(0.0),
            (_, w) => w?,
        };
        let write_1h = e.cache_write_1h.unwrap_or(e.input * 2.0);
        Some(CostParts {
            input: fresh_input as f64 * e.input,
            output: billed_output as f64 * e.output,
            cache_read: t.cache_read as f64 * read,
            cache_write: t.cache_write_5m as f64 * write_5m + t.cache_write_1h as f64 * write_1h,
        })
    }

    pub fn input_price(&self, model: &str) -> Option<f64> {
        self.lookup(model).map(|e| e.input)
    }

    /// Exact name, then `anthropic/`, then `openai/`, then the same without a `-YYYYMMDD` suffix.
    fn lookup(&self, model: &str) -> Option<&Entry> {
        let direct = |name: &str| {
            self.entries
                .get(name)
                .or_else(|| self.entries.get(&format!("anthropic/{name}")))
                .or_else(|| self.entries.get(&format!("openai/{name}")))
        };
        direct(model).or_else(|| strip_date_suffix(model).and_then(direct))
    }
}

fn strip_date_suffix(model: &str) -> Option<&str> {
    let (head, tail) = model.rsplit_once('-')?;
    (tail.len() == 8 && tail.bytes().all(|b| b.is_ascii_digit())).then_some(head)
}

/// Fetches the latest LiteLLM price table. Only called from the Settings button.
pub async fn download_latest() -> Result<String> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()?;
    let body = client.get(LITELLM_URL).send().await?.error_for_status()?.text().await?;
    Pricing::from_json_str(&body)?; // validate before handing it back
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
        "sample_spec": {"input_cost_per_token": 0.0, "output_cost_per_token": 0.0},
        "no-prices": {"mode": "embedding"},
        "m-a": {"input_cost_per_token": 3e-6, "output_cost_per_token": 15e-6,
                "cache_read_input_token_cost": 0.3e-6, "cache_creation_input_token_cost": 3.75e-6},
        "m-1h": {"input_cost_per_token": 3e-6, "output_cost_per_token": 15e-6,
                 "cache_creation_input_token_cost": 3.75e-6,
                 "cache_creation_input_token_cost_above_1hr": 7e-6},
        "gpt-x": {"input_cost_per_token": 1e-6, "output_cost_per_token": 8e-6,
                  "cache_read_input_token_cost": 0.1e-6},
        "anthropic/m-b": {"input_cost_per_token": 2e-6, "output_cost_per_token": 4e-6},
        "openai/m-c": {"input_cost_per_token": 5e-6, "output_cost_per_token": 6e-6},
        "m-d": {"input_cost_per_token": 7e-6, "output_cost_per_token": 8e-6}
    }"#;

    fn p() -> Pricing {
        Pricing::from_json_str(FIXTURE).unwrap()
    }

    fn close(a: Option<f64>, b: f64) {
        let a = a.expect("expected a cost");
        assert!((a - b).abs() < 1e-12, "{a} != {b}");
    }

    #[test]
    fn claude_cost_uses_all_token_kinds() {
        let t = Tokens { input: 1000, output: 100, cache_read: 10_000, cache_write_5m: 1000, ..Default::default() };
        close(p().cost(Agent::ClaudeCode, "m-a", &t), 0.01125);
    }

    #[test]
    fn cache_write_1h_falls_back_to_twice_input() {
        let t = Tokens { cache_write_1h: 1000, ..Default::default() };
        close(p().cost(Agent::ClaudeCode, "m-a", &t), 1000.0 * 6e-6);
    }

    #[test]
    fn cache_write_1h_uses_explicit_rate_when_present() {
        let t = Tokens { cache_write_1h: 1000, ..Default::default() };
        close(p().cost(Agent::ClaudeCode, "m-1h", &t), 1000.0 * 7e-6);
    }

    #[test]
    fn reasoning_is_priced_at_output_rate() {
        let t = Tokens { reasoning: 100, ..Default::default() };
        close(p().cost(Agent::OpenCode, "gpt-x", &t), 100.0 * 8e-6);
    }

    #[test]
    fn codex_input_includes_cached() {
        let t = Tokens { input: 1000, output: 10, cache_read: 400, ..Default::default() };
        close(p().cost(Agent::Codex, "gpt-x", &t), 600.0 * 1e-6 + 400.0 * 0.1e-6 + 10.0 * 8e-6);
    }

    #[test]
    fn non_codex_input_excludes_cached() {
        let t = Tokens { input: 1000, cache_read: 400, ..Default::default() };
        close(p().cost(Agent::OpenCode, "gpt-x", &t), 1000.0 * 1e-6 + 400.0 * 0.1e-6);
    }

    #[test]
    fn lookup_tries_provider_prefixes() {
        let t = Tokens { input: 1000, ..Default::default() };
        close(p().cost(Agent::ClaudeCode, "m-b", &t), 1000.0 * 2e-6);
        close(p().cost(Agent::Codex, "m-c", &t), 1000.0 * 5e-6);
    }

    #[test]
    fn lookup_strips_date_suffix() {
        assert_eq!(p().input_price("m-d-20250929"), Some(7e-6));
        assert_eq!(p().input_price("m-b-20250929"), Some(2e-6));
    }

    #[test]
    fn exact_match_wins_over_prefixed() {
        let p = Pricing::from_json_str(
            r#"{"x": {"input_cost_per_token": 1e-6, "output_cost_per_token": 1e-6},
                "anthropic/x": {"input_cost_per_token": 9e-6, "output_cost_per_token": 9e-6}}"#,
        )
        .unwrap();
        assert_eq!(p.input_price("x"), Some(1e-6));
    }

    #[test]
    fn unknown_model_is_none() {
        let t = Tokens { input: 5, ..Default::default() };
        assert!(p().cost(Agent::OpenCode, "big-pickle", &t).is_none());
        assert!(p().input_price("big-pickle").is_none());
    }

    #[test]
    fn sample_spec_and_unpriced_entries_are_ignored() {
        assert!(p().input_price("sample_spec").is_none());
        assert!(p().input_price("no-prices").is_none());
    }

    #[test]
    fn empty_table_is_an_error() {
        assert!(Pricing::from_json_str("{}").is_err());
        assert!(Pricing::from_json_str(r#"{"sample_spec": {"input_cost_per_token": 1e-6, "output_cost_per_token": 1e-6}, "x": {"mode": "embedding"}}"#).is_err());
    }

    #[test]
    fn missing_cache_rate_with_cache_tokens_is_none() {
        let t = Tokens { cache_read: 10, ..Default::default() };
        assert!(p().cost(Agent::ClaudeCode, "m-b", &t).is_none());
        let t = Tokens { cache_write_5m: 10, ..Default::default() };
        assert!(p().cost(Agent::ClaudeCode, "gpt-x", &t).is_none());
    }

    #[test]
    fn missing_cache_rates_with_zero_cache_tokens_still_price() {
        let t = Tokens { input: 1000, output: 10, ..Default::default() };
        close(p().cost(Agent::ClaudeCode, "m-b", &t), 1000.0 * 2e-6 + 10.0 * 4e-6);
    }

    #[test]
    fn codex_reasoning_not_double_counted() {
        let t = Tokens { input: 29838, output: 314, reasoning: 66, ..Default::default() };
        close(p().cost(Agent::Codex, "gpt-x", &t), 29838.0 * 1e-6 + 314.0 * 8e-6);
    }

    #[test]
    fn cost_parts_split_by_token_kind_and_sum_to_cost() {
        let t = Tokens { input: 1000, output: 100, cache_read: 10_000, cache_write_5m: 1000, cache_write_1h: 10, reasoning: 20 };
        let parts = p().cost_parts(Agent::ClaudeCode, "m-a", &t).unwrap();
        close(Some(parts.input), 1000.0 * 3e-6);
        close(Some(parts.output), 120.0 * 15e-6);
        close(Some(parts.cache_read), 10_000.0 * 0.3e-6);
        close(Some(parts.cache_write), 1000.0 * 3.75e-6 + 10.0 * 6e-6);
        close(p().cost(Agent::ClaudeCode, "m-a", &t), parts.total());
    }

    #[test]
    fn cost_parts_codex_moves_cached_input_out_of_input() {
        let t = Tokens { input: 1000, output: 10, cache_read: 400, reasoning: 5, ..Default::default() };
        let parts = p().cost_parts(Agent::Codex, "gpt-x", &t).unwrap();
        close(Some(parts.input), 600.0 * 1e-6);
        close(Some(parts.output), 10.0 * 8e-6);
        close(Some(parts.cache_read), 400.0 * 0.1e-6);
        assert_eq!(parts.cache_write, 0.0);
        close(p().cost(Agent::Codex, "gpt-x", &t), parts.total());
    }

    #[test]
    fn cost_parts_none_when_cost_is_none() {
        let t = Tokens { input: 5, ..Default::default() };
        assert!(p().cost_parts(Agent::OpenCode, "big-pickle", &t).is_none());
        let t = Tokens { cache_read: 10, ..Default::default() };
        assert!(p().cost_parts(Agent::ClaudeCode, "m-b", &t).is_none());
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(Pricing::from_json_str("not json").is_err());
        assert!(Pricing::from_json_str("[1,2]").is_err());
    }

    #[test]
    fn bundled_snapshot_loads_with_real_models() {
        let b = Pricing::bundled();
        assert!(!b.version().is_empty());
        assert!(b.input_price("claude-opus-5-5").is_some_and(|v| v > 0.0), "no real Claude price");
        assert!(b.input_price("gpt-5.5").is_some_and(|v| v > 0.0), "no real OpenAI price");
    }
}

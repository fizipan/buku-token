use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    #[serde(rename = "cc")]
    ClaudeCode,
    #[serde(rename = "cx")]
    Codex,
    #[serde(rename = "oc")]
    OpenCode,
    #[serde(rename = "hm")]
    Hermes,
    #[serde(rename = "cr")]
    Cursor,
}

pub const STACK_ORDER: [Agent; 5] = [Agent::ClaudeCode, Agent::Codex, Agent::OpenCode, Agent::Hermes, Agent::Cursor];

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub reasoning: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CostSource {
    Computed,
    Reported,
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UsageEvent {
    pub ts: chrono::DateTime<chrono::Utc>,
    pub agent: Agent,
    pub model: String,
    pub cwd: String,
    pub session_id: String,
    pub tokens: Tokens,
    pub reported_cost_usd: Option<f64>,
    pub dedup_key: String,
}

/// "plus" -> "Plus". Only the first letter changes.
pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().into_iter().flat_map(char::to_uppercase).chain(c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capitalize_changes_only_the_first_letter() {
        assert_eq!(capitalize("plus"), "Plus");
        assert_eq!(capitalize("ünder"), "Ünder");
        assert_eq!(capitalize("pRo"), "PRo");
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn stack_order_ends_with_cursor_and_serializes_as_cr() {
        assert_eq!(STACK_ORDER, [Agent::ClaudeCode, Agent::Codex, Agent::OpenCode, Agent::Hermes, Agent::Cursor]);
        assert_eq!(serde_json::to_string(&Agent::Cursor).unwrap(), "\"cr\"");
        assert_eq!(serde_json::from_str::<Agent>("\"cr\"").unwrap(), Agent::Cursor);
    }
}

//! Claude subscription quota via the OAuth usage endpoint.

use std::fmt;

use chrono::{DateTime, Utc};

use super::{binding_window, is_near_limit, AgentQuota, QuotaWindow};
use crate::i18n::{self, Service};
use crate::model::{capitalize, Agent};
use crate::settings::Lang;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaError {
    Expired,
    KeychainDenied,
    /// The Keychain failed for a reason that is not a refusal; polling carries on and tries again later.
    KeychainFailed,
    NotFound,
    Offline,
    Http(u16),
    Parse,
}

impl QuotaError {
    pub fn reason(&self, lang: Lang) -> String {
        i18n::quota_reason(Service::Claude, self, lang)
    }
}

impl fmt::Display for QuotaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason(Lang::default()))
    }
}

impl std::error::Error for QuotaError {}

pub struct OAuthToken {
    pub access_token: String,
    pub expires_at: DateTime<Utc>,
    pub plan: Option<String>,
}

impl fmt::Debug for OAuthToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthToken")
            .field("access_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("plan", &self.plan)
            .finish()
    }
}

pub trait CredentialSource {
    fn access_token(&self) -> Result<OAuthToken, QuotaError>;
}

/// Reads Claude Code's own login from the macOS Keychain. Read-only: the token is
/// never logged, stored or refreshed here.
pub struct KeychainSource;

impl CredentialSource for KeychainSource {
    fn access_token(&self) -> Result<OAuthToken, QuotaError> {
        const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;
        let user = std::env::var("USER").map_err(|_| QuotaError::NotFound)?;
        let bytes = security_framework::passwords::get_generic_password("Claude Code-credentials", &user)
            .map_err(|e| match e.code() {
                ERR_SEC_ITEM_NOT_FOUND => QuotaError::NotFound,
                // errSecAuthFailed, errSecUserCanceled, errSecInteractionNotAllowed and the rest
                _ => QuotaError::KeychainDenied,
            })?;
        parse_credentials(std::str::from_utf8(&bytes).map_err(|_| QuotaError::Parse)?)
    }
}

/// "max" + tier "default_claude_max_5x" -> "Max 5×"; otherwise the capitalized plan.
fn display_plan(sub: &str, tier: Option<&str>) -> String {
    let mut name = capitalize(sub);
    if sub == "max"
        && let Some(mult) = tier.and_then(|t| t.rsplit('_').next()).and_then(|m| m.strip_suffix('x'))
        && !mult.is_empty()
        && mult.chars().all(|c| c.is_ascii_digit())
    {
        name = format!("{name} {mult}\u{d7}");
    }
    name
}

pub fn parse_credentials(json: &str) -> Result<OAuthToken, QuotaError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| QuotaError::Parse)?;
    let o = v.get("claudeAiOauth").ok_or(QuotaError::NotFound)?;
    let access_token = o.get("accessToken").and_then(|t| t.as_str()).ok_or(QuotaError::Parse)?.to_string();
    let ms = o.get("expiresAt").and_then(|t| t.as_i64()).ok_or(QuotaError::Parse)?;
    let expires_at = DateTime::from_timestamp_millis(ms).ok_or(QuotaError::Parse)?;
    let plan = o
        .get("subscriptionType")
        .and_then(|t| t.as_str())
        .map(|sub| display_plan(sub, o.get("rateLimitTier").and_then(|t| t.as_str())));
    Ok(OAuthToken { access_token, expires_at, plan })
}

/// The windows we show, in display order. Allow-listed on purpose: an unknown key such as
/// `seven_day_oauth_apps` must never become the binding window.
const WINDOWS: [&str; 4] = ["five_hour", "seven_day", "seven_day_opus", "seven_day_sonnet"];

// TODO(verify): confirm field names against a live /api/oauth/usage response (Task 19)
pub fn parse_usage(json: &str, observed_at: DateTime<Utc>) -> Result<Vec<QuotaWindow>, QuotaError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| QuotaError::Parse)?;
    let obj = v.as_object().ok_or(QuotaError::Parse)?;
    Ok(WINDOWS
        .iter()
        .filter_map(|key| {
            let w = obj.get(*key)?;
            let used_pct = w.get("utilization")?.as_f64()?;
            let resets_at = DateTime::parse_from_rfc3339(w.get("resets_at")?.as_str()?).ok()?.to_utc();
            Some(QuotaWindow {
                id: (*key).to_string(),
                label: i18n::window_label(key, Lang::default()),
                used_pct,
                resets_at,
                observed_at,
                expired: resets_at < observed_at,
            })
        })
        .collect())
}

pub async fn fetch(src: &dyn CredentialSource, http: &reqwest::Client) -> Result<AgentQuota, QuotaError> {
    fetch_at(src, http, USAGE_URL, Utc::now()).await
}

async fn fetch_at(
    src: &dyn CredentialSource,
    http: &reqwest::Client,
    url: &str,
    now: DateTime<Utc>,
) -> Result<AgentQuota, QuotaError> {
    let token = src.access_token()?;
    // Never refresh: an expired token is reported before any network traffic.
    if token.expires_at <= now {
        return Err(QuotaError::Expired);
    }
    let resp = http
        .get(url)
        .bearer_auth(&token.access_token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "buku-token")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| QuotaError::Offline)?;
    match resp.status().as_u16() {
        401 => return Err(QuotaError::Expired),
        c if !(200..300).contains(&c) => return Err(QuotaError::Http(c)),
        _ => {}
    }
    let body = resp.text().await.map_err(|_| QuotaError::Offline)?;
    let windows = parse_usage(&body, now)?;
    Ok(AgentQuota {
        agent: Agent::ClaudeCode,
        plan: token.plan,
        binding: binding_window(&windows),
        near_limit: is_near_limit(&windows, None),
        windows,
        unavailable_reason: None,
        failure: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    struct Stub(Result<(String, DateTime<Utc>), QuotaError>);
    impl CredentialSource for Stub {
        fn access_token(&self) -> Result<OAuthToken, QuotaError> {
            self.0.clone().map(|(t, e)| OAuthToken { access_token: t, expires_at: e, plan: Some("Max".into()) })
        }
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 30, 5, 0, 0).unwrap()
    }

    #[test]
    fn parse_usage_fixture() {
        let ws = parse_usage(include_str!("../../tests/fixtures/claude_usage.json"), now()).unwrap();
        assert_eq!(ws.len(), 2, "null seven_day_opus is skipped");
        assert_eq!((ws[0].id.as_str(), ws[0].label.as_str(), ws[0].used_pct), ("five_hour", "Sesi 5 jam", 38.0));
        assert_eq!((ws[1].id.as_str(), ws[1].label.as_str(), ws[1].used_pct), ("seven_day", "Mingguan", 61.0));
        assert_eq!(ws[0].resets_at, Utc.with_ymd_and_hms(2026, 10, 30, 5, 40, 0).unwrap());
        assert!(!ws[0].expired);
    }

    #[test]
    fn parse_usage_extra_windows_and_expiry() {
        let j = r#"{"five_hour":{"utilization":10,"resets_at":"2026-10-30T04:00:00Z"},
            "seven_day_opus":{"utilization":5.5,"resets_at":"2026-11-01T00:00:00Z"},
            "seven_day_sonnet":{"utilization":1,"resets_at":null},"extra_usage":{"is_enabled":false}}"#;
        let ws = parse_usage(j, now()).unwrap();
        assert_eq!(ws.len(), 2);
        assert!(ws[0].expired);
        assert_eq!((ws[1].id.as_str(), ws[1].label.as_str()), ("seven_day_opus", "Mingguan Opus"));
    }

    #[test]
    fn parse_usage_allow_lists_known_windows_only() {
        let w = |pct: u32| format!(r#"{{"utilization":{pct},"resets_at":"2026-11-01T00:00:00Z"}}"#);
        let j = format!(
            r#"{{"seven_day_sonnet":{},"seven_day_opus":{},"seven_day":{},"five_hour":{},
                "seven_day_oauth_apps":{},"seven_day_cowork":{},"one_hour":{},"extra_usage":{}}}"#,
            w(4),
            w(3),
            w(2),
            w(1),
            w(99),
            w(98),
            w(97),
            w(96)
        );
        let ws = parse_usage(&j, now()).unwrap();
        let got: Vec<(&str, &str, f64)> = ws.iter().map(|x| (x.id.as_str(), x.label.as_str(), x.used_pct)).collect();
        assert_eq!(
            got,
            [
                ("five_hour", "Sesi 5 jam", 1.0),
                ("seven_day", "Mingguan", 2.0),
                ("seven_day_opus", "Mingguan Opus", 3.0),
                ("seven_day_sonnet", "Mingguan Sonnet", 4.0),
            ]
        );
    }

    #[test]
    fn parse_usage_rejects_garbage() {
        assert_eq!(parse_usage("not json", now()), Err(QuotaError::Parse));
        assert_eq!(parse_usage("[]", now()), Err(QuotaError::Parse));
    }

    #[test]
    fn parse_credentials_maps_plan_and_expiry() {
        let j = r#"{"claudeAiOauth":{"accessToken":"sk-secret","expiresAt":1793000000000,"subscriptionType":"max","rateLimitTier":"default_claude_max_5x"}}"#;
        let t = parse_credentials(j).unwrap();
        assert_eq!(t.access_token, "sk-secret");
        assert_eq!(t.expires_at.timestamp_millis(), 1793000000000);
        assert_eq!(t.plan.as_deref(), Some("Max 5×"));
        let j2 = r#"{"claudeAiOauth":{"accessToken":"x","expiresAt":1,"subscriptionType":"pro"}}"#;
        assert_eq!(parse_credentials(j2).unwrap().plan.as_deref(), Some("Pro"));
        assert_eq!(parse_credentials("{}").unwrap_err(), QuotaError::NotFound);
        assert_eq!(parse_credentials("nope").unwrap_err(), QuotaError::Parse);
    }

    #[test]
    fn token_debug_is_redacted() {
        let t = OAuthToken { access_token: "sk-secret".into(), expires_at: now(), plan: None };
        assert!(!format!("{t:?}").contains("sk-secret"));
    }

    #[tokio::test]
    async fn expired_token_reports_reason_without_refresh() {
        let src = Stub(Ok(("tok".into(), now() - Duration::minutes(1))));
        // Unresolvable URL: if a request were attempted we'd get Offline, not Expired.
        let r = fetch_at(&src, &reqwest::Client::new(), "http://127.0.0.1:1/usage", now()).await;
        assert_eq!(r.unwrap_err(), QuotaError::Expired);
    }

    #[tokio::test]
    async fn credential_errors_pass_through() {
        let src = Stub(Err(QuotaError::KeychainDenied));
        let r = fetch_at(&src, &reqwest::Client::new(), "http://127.0.0.1:1/usage", now()).await;
        assert_eq!(r.unwrap_err(), QuotaError::KeychainDenied);
    }

    #[tokio::test]
    async fn valid_token_unreachable_host_is_offline() {
        let src = Stub(Ok(("tok".into(), now() + Duration::hours(1))));
        let r = fetch_at(&src, &reqwest::Client::new(), "http://127.0.0.1:1/usage", now()).await;
        assert_eq!(r.unwrap_err(), QuotaError::Offline);
    }

    #[test]
    fn reasons_match_spec() {
        assert_eq!(QuotaError::Expired.reason(Lang::Id), "Buka Claude Code untuk memperbarui login");
        assert_eq!(QuotaError::KeychainDenied.reason(Lang::Id), "Izin Keychain ditolak");
        assert_eq!(QuotaError::Offline.reason(Lang::Id), "Sedang offline");
        assert_eq!(QuotaError::NotFound.reason(Lang::Id), "Login Claude Code tidak ditemukan");
        assert_eq!(QuotaError::NotFound.reason(Lang::En), "Claude Code login not found");
        assert_eq!(QuotaError::Http(500).reason(Lang::En), "Quota server replied HTTP 500");
    }
}

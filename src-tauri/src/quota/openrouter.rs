//! OpenRouter balance and spend through the account's own API key.
//!
//! The key is pasted in Settings and kept only in Buku Token's own Keychain item. It is never
//! logged, written elsewhere, returned to the UI or put in an error. OpenRouter is not an
//! `Agent`: its spend can come from OpenCode or Hermes, which are already counted, so nothing
//! here reaches the donut, the totals, the menu bar, the notifications or the share card.

use std::fmt;

use serde::Serialize;

use super::claude::QuotaError;
use crate::i18n::{self, Service};
use crate::settings::Lang;
use super::cursor::{KeychainItem, SessionStore};

pub(crate) const BASE_URL: &str = "https://openrouter.ai";
const MAX_KEY_BYTES: usize = 512;
/// Buku Token's own OpenRouter item (service `Buku Token OpenRouter Key`).
pub const OPENROUTER_KEY: KeychainItem = KeychainItem("Buku Token OpenRouter Key");

pub struct OpenRouterAuth {
    key: String,
}

impl fmt::Debug for OpenRouterAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenRouterAuth").field("key", &"<redacted>").finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LimitReset {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyLimit {
    pub limit_usd: f64,
    pub remaining_usd: f64,
    pub reset: Option<LimitReset>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterData {
    pub free: bool,
    pub balance_usd: Option<f64>,
    pub today_usd: f64,
    pub week_usd: f64,
    pub month_usd: f64,
    pub limit: Option<KeyLimit>,
}

/// What the UI gets: the numbers, or the fixed reason there are none.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterView {
    pub data: Option<OpenRouterData>,
    pub unavailable_reason: Option<String>,
    /// The error behind the reason, so the text can be made in the language current at read time.
    #[serde(skip)]
    pub failure: Option<QuotaError>,
}

impl OpenRouterView {
    /// The view with its reason worded in `lang`.
    pub fn localized(mut self, lang: Lang) -> Self {
        if let Some(e) = &self.failure {
            self.unavailable_reason = Some(reason(e, lang));
        }
        self
    }
}

impl OpenRouterAuth {
    pub fn bearer(&self) -> String {
        format!("Bearer {}", self.key)
    }
}

/// Cleans a pasted key. Anything that could split or inject a header is refused.
pub fn normalize_key(input: &str) -> Result<String, QuotaError> {
    let key = input.trim();
    let bad = |c: char| !c.is_ascii() || c.is_ascii_control() || c == ' ';
    if key.is_empty() || key.len() > MAX_KEY_BYTES || key.chars().any(bad) {
        return Err(QuotaError::Parse);
    }
    Ok(key.to_string())
}

pub fn auth_from_key(key: String) -> OpenRouterAuth {
    OpenRouterAuth { key }
}

/// The stored key, or `None` when nothing is connected.
pub fn load_auth(store: &dyn SessionStore) -> Result<Option<OpenRouterAuth>, QuotaError> {
    Ok(store.load()?.map(auth_from_key))
}

/// First half of connecting: the pasted text becomes a key, or a fixed error text.
pub fn prepare_connect(input: &str, lang: Lang) -> Result<OpenRouterAuth, String> {
    normalize_key(input).map(auth_from_key).map_err(|_| i18n::invalid_key(lang).to_string())
}

/// Second half, pure: the key is stored only if the server accepted it and `still_current` (no
/// disconnect or newer connect happened while the server was asked). Errors are fixed texts.
pub fn finish_connect(
    store: &dyn SessionStore,
    auth: &OpenRouterAuth,
    fetched: Result<OpenRouterData, QuotaError>,
    still_current: &dyn Fn() -> bool,
    lang: Lang,
) -> Result<(), String> {
    match fetched {
        Ok(_) => {}
        Err(QuotaError::Expired) => return Err(i18n::invalid_key(lang).into()),
        Err(e) => return Err(reason(&e, lang)),
    }
    if !still_current() {
        return Err(i18n::connect_stale(lang).into());
    }
    store.save(&auth.key).map_err(|e| reason(&e, lang))
}

/// Same rules as Cursor's (no redirects, 15 s timeout), but its own client: the key must never
/// follow a redirect to another host.
pub fn http_client() -> reqwest::Client {
    super::cursor::http_client()
}

/// `GET /api/v1/key`: what this key spent, and its limit if it has one. The balance is not here.
pub fn parse_key(json: &str) -> Result<OpenRouterData, QuotaError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| QuotaError::Parse)?;
    let d = v.get("data").and_then(|d| d.as_object()).ok_or(QuotaError::Parse)?;
    let num = |k: &str| d.get(k).and_then(serde_json::Value::as_f64);
    let spent = ["usage_daily", "usage_weekly", "usage_monthly"].map(num);
    if spent.iter().all(Option::is_none) {
        return Err(QuotaError::Parse);
    }
    let [today, week, month] = spent.map(|n| n.unwrap_or(0.0).max(0.0));
    let limit = num("limit").zip(num("limit_remaining")).map(|(limit, left)| KeyLimit {
        limit_usd: limit.max(0.0),
        remaining_usd: left.max(0.0),
        reset: match d.get("limit_reset").and_then(|r| r.as_str()) {
            Some("daily") => Some(LimitReset::Daily),
            Some("weekly") => Some(LimitReset::Weekly),
            Some("monthly") => Some(LimitReset::Monthly),
            _ => None,
        },
    });
    Ok(OpenRouterData {
        free: d.get("is_free_tier").and_then(serde_json::Value::as_bool).unwrap_or(false),
        balance_usd: None,
        today_usd: today,
        week_usd: week,
        month_usd: month,
        limit,
    })
}

/// `GET /api/v1/credits`: credits bought minus credits used. A missing field means no balance.
pub fn parse_credits(json: &str) -> Result<Option<f64>, QuotaError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| QuotaError::Parse)?;
    let num = |k: &str| v.pointer(&format!("/data/{k}")).and_then(serde_json::Value::as_f64);
    Ok(num("total_credits").zip(num("total_usage")).map(|(bought, used)| (bought - used).max(0.0)))
}

/// 401/403 mean the key is not accepted, any other non-2xx is reported as is. Redirects are off,
/// so 3xx is an error.
pub fn status_result(code: u16) -> Result<(), QuotaError> {
    match code {
        401 | 403 => Err(QuotaError::Expired),
        c if !(200..300).contains(&c) => Err(QuotaError::Http(c)),
        _ => Ok(()),
    }
}

/// The balance is a bonus: some keys get 403 from `/credits`, and any failure other than a
/// revoked key just leaves it out, so the spend numbers still show.
pub fn credits_balance(code: u16, body: &str) -> Result<Option<f64>, QuotaError> {
    match code {
        401 => Err(QuotaError::Expired),
        200..300 => Ok(parse_credits(body).unwrap_or(None)),
        _ => Ok(None),
    }
}

pub async fn fetch(http: &reqwest::Client, auth: &OpenRouterAuth) -> Result<OpenRouterData, QuotaError> {
    fetch_at(http, auth, BASE_URL).await
}

async fn fetch_at(http: &reqwest::Client, auth: &OpenRouterAuth, base_url: &str) -> Result<OpenRouterData, QuotaError> {
    let mut bearer = reqwest::header::HeaderValue::from_str(&auth.bearer()).map_err(|_| QuotaError::Parse)?;
    bearer.set_sensitive(true);
    let get = |path: &str| {
        http.get(format!("{base_url}{path}")).header(reqwest::header::AUTHORIZATION, bearer.clone()).header("User-Agent", "buku-token").send()
    };
    let resp = get("/api/v1/key").await.map_err(|_| QuotaError::Offline)?;
    status_result(resp.status().as_u16())?;
    let body = resp.text().await.map_err(|_| QuotaError::Offline)?;
    let mut data = parse_key(&body)?;
    data.balance_usd = match get("/api/v1/credits").await {
        Ok(resp) => {
            let code = resp.status().as_u16();
            credits_balance(code, &resp.text().await.unwrap_or_default())?
        }
        Err(_) => None,
    };
    Ok(data)
}

/// One poll. `None` without a stored key: nothing is sent.
pub async fn round(store: &dyn SessionStore, http: &reqwest::Client, base_url: &str) -> Option<Result<OpenRouterData, QuotaError>> {
    match load_auth(store) {
        Ok(Some(auth)) => Some(fetch_at(http, &auth, base_url).await),
        Ok(None) => None,
        Err(e) => Some(Err(e)),
    }
}

pub fn reason(e: &QuotaError, lang: Lang) -> String {
    i18n::quota_reason(Service::OpenRouter, e, lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "sk-or-v1-test";

    fn auth() -> OpenRouterAuth {
        auth_from_key(KEY.into())
    }

    #[derive(Default)]
    struct MemStore(std::cell::RefCell<Option<String>>);

    impl SessionStore for MemStore {
        fn load(&self) -> Result<Option<String>, QuotaError> {
            Ok(self.0.borrow().clone())
        }
        fn save(&self, key: &str) -> Result<(), QuotaError> {
            *self.0.borrow_mut() = Some(key.to_string());
            Ok(())
        }
        fn clear(&self) -> Result<(), QuotaError> {
            *self.0.borrow_mut() = None;
            Ok(())
        }
    }

    fn data() -> OpenRouterData {
        parse_key(include_str!("../../tests/fixtures/openrouter_key.json")).unwrap()
    }

    #[test]
    fn debug_redacts_the_key() {
        let d = format!("{:?}", auth());
        assert!(!d.contains(KEY), "{d}");
        assert!(d.contains("<redacted>"));
    }

    #[test]
    fn bearer_header_exact() {
        assert_eq!(auth().bearer(), format!("Bearer {KEY}"));
    }

    #[test]
    fn normalize_key_trims_and_keeps_the_value() {
        assert_eq!(normalize_key(&format!("  {KEY}\n")).unwrap(), KEY);
        assert_eq!(normalize_key(&"a".repeat(MAX_KEY_BYTES)).unwrap().len(), MAX_KEY_BYTES);
    }

    #[test]
    fn normalize_key_rejects_unsafe_input() {
        let long = "a".repeat(MAX_KEY_BYTES + 1);
        for bad in ["", "   ", &long, "a b", "a\nb", "a\r\nX-Evil: 1", "a\tb", "a\0b", "kunci\u{e9}", "a\u{200b}b"] {
            assert_eq!(normalize_key(bad).unwrap_err(), QuotaError::Parse, "{bad:?}");
        }
    }

    #[test]
    fn parse_key_fixture() {
        let d = data();
        assert!(!d.free);
        assert_eq!((d.today_usd, d.week_usd, d.month_usd), (1.25, 6.5, 21.5));
        assert_eq!(d.balance_usd, None, "the balance comes from /credits");
        let l = d.limit.unwrap();
        assert_eq!((l.limit_usd, l.remaining_usd, l.reset), (50.0, 28.5, Some(LimitReset::Monthly)));
    }

    #[test]
    fn parse_key_with_null_limit_and_missing_fields() {
        let j = r#"{"data":{"usage_daily":0.5,"usage_weekly":1,"usage_monthly":2,"is_free_tier":true,"limit":null,"limit_remaining":null,"limit_reset":null}}"#;
        let d = parse_key(j).unwrap();
        assert!(d.free && d.limit.is_none());
        assert_eq!((d.today_usd, d.week_usd, d.month_usd), (0.5, 1.0, 2.0));
        let sparse = parse_key(r#"{"data":{"usage_monthly":3}}"#).unwrap();
        assert_eq!((sparse.today_usd, sparse.week_usd, sparse.month_usd), (0.0, 0.0, 3.0));
        assert!(!sparse.free && sparse.limit.is_none());
        let half = parse_key(r#"{"data":{"usage_monthly":3,"limit":10}}"#).unwrap();
        assert!(half.limit.is_none(), "a bar needs both the limit and what is left");
    }

    #[test]
    fn parse_key_allow_lists_the_reset_and_clamps_negatives() {
        let j = |r: &str| format!(r#"{{"data":{{"usage_monthly":1,"limit":10,"limit_remaining":-2,"limit_reset":"{r}"}}}}"#);
        let l = parse_key(&j("daily")).unwrap().limit.unwrap();
        assert_eq!((l.reset, l.remaining_usd), (Some(LimitReset::Daily), 0.0));
        assert_eq!(parse_key(&j("weekly")).unwrap().limit.unwrap().reset, Some(LimitReset::Weekly));
        assert_eq!(parse_key(&j("sometimes")).unwrap().limit.unwrap().reset, None);
    }

    #[test]
    fn parse_key_rejects_garbage() {
        for bad in ["nope", "{}", r#"{"data":null}"#, r#"{"data":{}}"#, r#"{"data":{"label":"x"}}"#, r#"{"data":{"usage_monthly":"1"}}"#] {
            assert_eq!(parse_key(bad).unwrap_err(), QuotaError::Parse, "{bad}");
        }
    }

    #[test]
    fn parse_credits_fixture_and_missing_fields() {
        assert_eq!(parse_credits(include_str!("../../tests/fixtures/openrouter_credits.json")).unwrap(), Some(62.75));
        assert_eq!(parse_credits(r#"{"data":{"total_credits":5,"total_usage":9}}"#).unwrap(), Some(0.0), "never negative");
        for none in [r#"{"data":{}}"#, r#"{"data":{"total_credits":5}}"#, r#"{"data":{"total_usage":5}}"#, "{}"] {
            assert_eq!(parse_credits(none).unwrap(), None, "{none}");
        }
        assert_eq!(parse_credits("nope").unwrap_err(), QuotaError::Parse);
    }

    #[test]
    fn credits_403_means_no_balance_without_an_error() {
        let ok = include_str!("../../tests/fixtures/openrouter_credits.json");
        assert_eq!(credits_balance(200, ok), Ok(Some(62.75)));
        assert_eq!(credits_balance(403, "{}"), Ok(None));
        assert_eq!(credits_balance(401, "{}"), Err(QuotaError::Expired), "a revoked key is not hidden");
        for code in [404, 429, 500, 302] {
            assert_eq!(credits_balance(code, ok), Ok(None), "{code}");
        }
        assert_eq!(credits_balance(200, "garbage"), Ok(None));
    }

    #[test]
    fn status_result_maps_codes() {
        assert_eq!(status_result(401), Err(QuotaError::Expired));
        assert_eq!(status_result(403), Err(QuotaError::Expired));
        assert_eq!(status_result(302), Err(QuotaError::Http(302)));
        assert_eq!(status_result(429), Err(QuotaError::Http(429)));
        assert_eq!(status_result(500), Err(QuotaError::Http(500)));
        assert_eq!(status_result(200), Ok(()));
    }

    #[test]
    fn connect_saves_only_after_the_server_accepts() {
        let store = MemStore::default();
        let a = prepare_connect(&format!(" {KEY} "), Lang::Id).unwrap();
        assert_eq!(a.bearer(), format!("Bearer {KEY}"));
        assert_eq!(finish_connect(&store, &a, Ok(data()), &|| true, Lang::Id), Ok(()));
        assert_eq!(store.load().unwrap().as_deref(), Some(KEY));
    }

    #[test]
    fn connect_does_not_save_a_rejected_key() {
        for (err, msg) in [
            (QuotaError::Expired, "Key Tidak Valid"),
            (QuotaError::Offline, "Sedang Offline"),
            (QuotaError::Http(500), "Server OpenRouter Menjawab HTTP 500"),
            (QuotaError::Parse, "Respons OpenRouter Tidak Dikenali"),
        ] {
            let store = MemStore::default();
            let a = prepare_connect(KEY, Lang::Id).unwrap();
            let e = finish_connect(&store, &a, Err(err), &|| true, Lang::Id).unwrap_err();
            assert_eq!(e, msg);
            assert!(!e.contains(KEY));
            assert_eq!(store.load().unwrap(), None, "{msg}");
        }
    }

    #[test]
    fn connect_does_not_save_when_the_session_changed_meanwhile() {
        let store = MemStore::default();
        let a = prepare_connect(KEY, Lang::Id).unwrap();
        let e = finish_connect(&store, &a, Ok(data()), &|| false, Lang::Id).unwrap_err();
        assert_eq!(e, "Koneksi Dibatalkan, Coba Lagi");
        assert_eq!(store.load().unwrap(), None, "a Putuskan during the wait stays final");
    }

    #[test]
    fn connect_with_unusable_input_is_invalid_and_does_not_echo_it() {
        for bad in ["a b", "", "kunci\u{e9}"] {
            assert_eq!(prepare_connect(bad, Lang::Id).unwrap_err(), "Key Tidak Valid");
        }
    }

    #[test]
    fn load_auth_is_none_without_a_key() {
        let store = MemStore::default();
        assert!(load_auth(&store).unwrap().is_none());
        store.save(KEY).unwrap();
        assert_eq!(load_auth(&store).unwrap().unwrap().bearer(), format!("Bearer {KEY}"));
        store.clear().unwrap();
        assert!(load_auth(&store).unwrap().is_none());
    }

    #[tokio::test]
    async fn without_a_key_nothing_is_sent() {
        // An unreachable host would answer Some(Err(Offline)) if a request were tried.
        let r = round(&MemStore::default(), &http_client(), "http://127.0.0.1:1").await;
        assert_eq!(r, None);
    }

    #[tokio::test]
    async fn with_a_key_an_unreachable_host_is_offline() {
        let store = MemStore::default();
        store.save(KEY).unwrap();
        let r = round(&store, &http_client(), "http://127.0.0.1:1").await;
        assert_eq!(r, Some(Err(QuotaError::Offline)));
        let r = fetch_at(&http_client(), &auth(), "http://127.0.0.1:1").await;
        assert_eq!(r.unwrap_err(), QuotaError::Offline);
    }

    struct Denied;
    impl SessionStore for Denied {
        fn load(&self) -> Result<Option<String>, QuotaError> {
            Err(QuotaError::KeychainDenied)
        }
        fn save(&self, _: &str) -> Result<(), QuotaError> {
            Err(QuotaError::KeychainDenied)
        }
        fn clear(&self) -> Result<(), QuotaError> {
            Err(QuotaError::KeychainDenied)
        }
    }

    #[tokio::test]
    async fn a_refused_keychain_is_reported_without_a_request() {
        let r = round(&Denied, &http_client(), "http://127.0.0.1:1").await;
        assert_eq!(r, Some(Err(QuotaError::KeychainDenied)));
    }

    #[test]
    fn connect_errors_follow_the_language() {
        assert_eq!(prepare_connect("a b", Lang::En).unwrap_err(), "Invalid Key");
        let store = MemStore::default();
        let a = prepare_connect(KEY, Lang::En).unwrap();
        assert_eq!(finish_connect(&store, &a, Err(QuotaError::Expired), &|| true, Lang::En).unwrap_err(), "Invalid Key");
        assert_eq!(finish_connect(&store, &a, Ok(data()), &|| false, Lang::En).unwrap_err(), "Connection Cancelled, Try Again");
        let view = OpenRouterView { data: None, unavailable_reason: None, failure: Some(QuotaError::Offline) };
        assert_eq!(view.clone().localized(Lang::En).unavailable_reason.as_deref(), Some("Offline"));
        assert_eq!(view.localized(Lang::Id).unavailable_reason.as_deref(), Some("Sedang Offline"));
    }

    #[test]
    fn reasons_are_fixed_texts() {
        assert_eq!(reason(&QuotaError::Expired, Lang::Id), "Key OpenRouter Ditolak, Tempel Ulang Di Pengaturan");
        assert_eq!(reason(&QuotaError::Offline, Lang::Id), "Sedang Offline");
        assert_eq!(reason(&QuotaError::Http(503), Lang::Id), "Server OpenRouter Menjawab HTTP 503");
        assert_eq!(reason(&QuotaError::Parse, Lang::Id), "Respons OpenRouter Tidak Dikenali");
        assert_eq!(reason(&QuotaError::NotFound, Lang::Id), "Respons OpenRouter Tidak Dikenali");
        assert_eq!(reason(&QuotaError::KeychainDenied, Lang::Id), "Akses Keychain Ditolak");
    }

    #[test]
    fn view_serializes_camel_case_and_holds_no_key() {
        let v = OpenRouterView { data: Some(data()), unavailable_reason: None, failure: None };
        let j = serde_json::to_string(&v).unwrap();
        for f in ["unavailableReason", "balanceUsd", "todayUsd", "weekUsd", "monthUsd", "limitUsd", "remainingUsd", "\"reset\":\"monthly\""] {
            assert!(j.contains(f), "{f} in {j}");
        }
        assert!(!j.contains("sk-or") && !j.contains("invented-label"), "{j}");
    }
}

//! Cursor plan quota via the dashboard usage-summary endpoint.
//!
//! The login is the user's own cursor.com session cookie, pasted in Settings and kept only in
//! Buku Token's own Keychain item. Cursor's database and Keychain are never opened. The cookie
//! is never logged, refreshed, written elsewhere, returned to the UI or put in an error.

use std::fmt;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};

use super::claude::QuotaError;
use super::{AgentQuota, QuotaWindow};
use crate::i18n::{self, Service};
use crate::model::{capitalize, Agent};
use crate::settings::Lang;

pub(crate) const BASE_URL: &str = "https://cursor.com";

const COOKIE_NAME: &str = "WorkosCursorSessionToken";
const MAX_COOKIE_BYTES: usize = 8192;
const KEYCHAIN_SERVICE: &str = "Buku Token Cursor Session";

pub struct CursorAuth {
    cookie: String,
    pub expires_at: Option<DateTime<Utc>>,
}

impl fmt::Debug for CursorAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CursorAuth")
            .field("cookie", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl CursorAuth {
    pub fn cookie_header(&self) -> String {
        format!("{COOKIE_NAME}={}", self.cookie)
    }
}

/// Cleans a pasted cookie value. Anything that could split or inject a header is refused.
pub fn normalize_cookie(input: &str) -> Result<String, QuotaError> {
    let trimmed = input.trim();
    let value = trimmed.strip_prefix(&format!("{COOKIE_NAME}=")).unwrap_or(trimmed).replace("::", "%3A%3A");
    let bad = |c: char| !c.is_ascii() || c.is_ascii_control() || c == ' ' || c == ';';
    if value.is_empty() || value.len() > MAX_COOKIE_BYTES || value.chars().any(bad) {
        return Err(QuotaError::Parse);
    }
    Ok(value)
}

/// `exp` of a JWT payload. The signature is not checked: the value is only forwarded to Cursor.
fn jwt_exp(jwt: &str) -> Option<DateTime<Utc>> {
    let payload = jwt.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    DateTime::from_timestamp(v.get("exp")?.as_i64()?, 0)
}

/// Wraps a normalized cookie. The value is opaque; a readable JWT only adds an expiry.
pub fn auth_from_cookie(cookie: String) -> CursorAuth {
    let token = cookie.rsplit("%3A%3A").next().unwrap_or(&cookie);
    let expires_at = jwt_exp(token);
    CursorAuth { cookie, expires_at }
}

/// Where the cookie lives between runs.
pub trait SessionStore {
    fn load(&self) -> Result<Option<String>, QuotaError>;
    fn save(&self, cookie: &str) -> Result<(), QuotaError>;
    fn clear(&self) -> Result<(), QuotaError>;
}

/// One of Buku Token's own Keychain items, named by its service. Shared by Cursor's cookie and
/// OpenRouter's key; it never touches another app's item.
pub struct KeychainItem(pub &'static str);

/// Buku Token's own Cursor item (service `Buku Token Cursor Session`).
pub const CURSOR_SESSION: KeychainItem = KeychainItem(KEYCHAIN_SERVICE);

const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

/// Security framework codes for a refusal: errSecAuthFailed, errSecUserCanceled and
/// errSecInteractionNotAllowed. Only these block polling until the user acts.
const ERR_SEC_AUTH_FAILED: i32 = -25293;
const ERR_SEC_USER_CANCELED: i32 = -128;
const ERR_SEC_INTERACTION_NOT_ALLOWED: i32 = -25308;

/// A Keychain code as a [`QuotaError`]: a real refusal is `KeychainDenied`, anything else is a
/// transient failure that is reported but never stops polling.
pub fn keychain_error(code: i32) -> QuotaError {
    match code {
        ERR_SEC_AUTH_FAILED | ERR_SEC_USER_CANCELED | ERR_SEC_INTERACTION_NOT_ALLOWED => QuotaError::KeychainDenied,
        _ => QuotaError::KeychainFailed,
    }
}

fn keychain_user() -> Result<String, QuotaError> {
    std::env::var("USER").map_err(|_| QuotaError::KeychainFailed)
}

impl SessionStore for KeychainItem {
    fn load(&self) -> Result<Option<String>, QuotaError> {
        match security_framework::passwords::get_generic_password(self.0, &keychain_user()?) {
            Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| QuotaError::Parse),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(keychain_error(e.code())),
        }
    }

    fn save(&self, cookie: &str) -> Result<(), QuotaError> {
        security_framework::passwords::set_generic_password(self.0, &keychain_user()?, cookie.as_bytes())
            .map_err(|e| keychain_error(e.code()))
    }

    fn clear(&self) -> Result<(), QuotaError> {
        match security_framework::passwords::delete_generic_password(self.0, &keychain_user()?) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(e) => Err(keychain_error(e.code())),
        }
    }
}

/// Hubungkan's check-then-save runs under the same lock as [`disconnect_locked`], so one of them
/// finishes before the other starts.
pub fn connect_locked<T>(lock: &std::sync::Mutex<()>, save: impl FnOnce() -> T) -> T {
    let _held = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    save()
}

/// Putuskan: under the lock, `end_session` (which moves the epoch) runs first and the item is
/// cleared after. A Hubungkan waiting for the lock then sees the new epoch and does not save.
pub fn disconnect_locked(lock: &std::sync::Mutex<()>, store: &dyn SessionStore, end_session: impl FnOnce()) -> Result<(), QuotaError> {
    let _held = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    end_session();
    store.clear()
}

/// The stored login, or `None` when nothing is connected.
pub fn load_auth(store: &dyn SessionStore) -> Result<Option<CursorAuth>, QuotaError> {
    Ok(store.load()?.map(auth_from_cookie))
}


/// First half of connecting: the pasted text becomes a login, or a fixed error text.
pub fn prepare_connect(input: &str, lang: Lang) -> Result<CursorAuth, String> {
    normalize_cookie(input).map(auth_from_cookie).map_err(|_| i18n::invalid_cookie(lang).to_string())
}

/// Second half, pure: given what the server answered for `auth`, stores the cookie only if it
/// was accepted and `still_current` (the cookie did not change while the server was asked: a
/// Putuskan in between must not be undone). Errors are fixed texts and never carry the cookie.
pub fn finish_connect(
    store: &dyn SessionStore,
    auth: &CursorAuth,
    fetched: Result<AgentQuota, QuotaError>,
    still_current: &dyn Fn() -> bool,
    lang: Lang,
) -> Result<Option<DateTime<Utc>>, String> {
    match fetched {
        Ok(_) => {}
        Err(QuotaError::Expired) => return Err(i18n::invalid_cookie(lang).into()),
        Err(e) => return Err(reason(&e, lang)),
    }
    if !still_current() {
        return Err(i18n::connect_stale(lang).into());
    }
    store.save(&auth.cookie).map_err(|e| reason(&e, lang))?;
    Ok(auth.expires_at)
}

/// Redirects are off so the session cookie can never follow a redirect to another host.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_default()
}

/// The windows we show, in display order, allow-listed like Claude's.
const WINDOWS: [(&str, &str); 3] = [("total", "totalPercentUsed"), ("auto", "autoPercentUsed"), ("api", "apiPercentUsed")];

pub fn parse_summary(json: &str, now: DateTime<Utc>) -> Result<(Option<String>, Vec<QuotaWindow>), QuotaError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| QuotaError::Parse)?;
    let resets_at = DateTime::parse_from_rfc3339(v.get("billingCycleEnd").and_then(|e| e.as_str()).ok_or(QuotaError::Parse)?)
        .map_err(|_| QuotaError::Parse)?
        .to_utc();
    let plan = v.get("membershipType").and_then(|m| m.as_str()).filter(|m| !m.is_empty()).map(capitalize);
    let usage = v.pointer("/individualUsage/plan");
    let windows = WINDOWS
        .iter()
        .filter_map(|(id, key)| {
            let used_pct = usage?.get(*key)?.as_f64()?;
            Some(QuotaWindow {
                id: (*id).to_string(),
                label: i18n::window_label(id, Lang::default()),
                used_pct,
                resets_at,
                observed_at: now,
                expired: resets_at < now,
            })
        })
        .collect();
    Ok((plan, windows))
}

/// Re-derives `binding`/`near_limit` from a stored quota whose windows' `expired` flags were
/// refreshed at read time.
pub(crate) fn build_quota_from(q: AgentQuota) -> AgentQuota {
    if q.failure.is_some() {
        return q;
    }
    build_quota(q.plan, q.windows)
}

/// Deliberately not `binding_window`/`is_near_limit`: Cursor's API pool can be nearly empty
/// while the user works in Auto, so only the `total` window drives the headline and alert.
pub(crate) fn build_quota(plan: Option<String>, windows: Vec<QuotaWindow>) -> AgentQuota {
    let total = windows.iter().find(|w| w.id == "total" && !w.expired).cloned();
    AgentQuota {
        agent: Agent::Cursor,
        plan,
        near_limit: total.as_ref().is_some_and(|w| w.remaining() <= super::NEAR_LIMIT_REMAINING),
        binding: total,
        windows,
        unavailable_reason: None,
        failure: None,
    }
}

/// Maps a Cursor HTTP status to the error the rest of the app understands: 401/403 mean the
/// session is gone, any other non-2xx is reported as is. Redirects are off, so 3xx is an error.
pub fn status_result(code: u16) -> Result<(), QuotaError> {
    match code {
        401 | 403 => Err(QuotaError::Expired),
        c if !(200..300).contains(&c) => Err(QuotaError::Http(c)),
        _ => Ok(()),
    }
}

pub async fn fetch_summary(
    http: &reqwest::Client,
    auth: &CursorAuth,
    now: DateTime<Utc>,
) -> Result<AgentQuota, QuotaError> {
    fetch_summary_at(http, auth, BASE_URL, now).await
}

async fn fetch_summary_at(
    http: &reqwest::Client,
    auth: &CursorAuth,
    base_url: &str,
    now: DateTime<Utc>,
) -> Result<AgentQuota, QuotaError> {
    // Never refresh: a cookie known to be expired is reported before any network traffic.
    if auth.expires_at.is_some_and(|exp| exp <= now) {
        return Err(QuotaError::Expired);
    }
    let resp = http
        .get(format!("{base_url}/api/usage-summary"))
        .header("Cookie", auth.cookie_header())
        .header("Origin", "https://cursor.com")
        .header("User-Agent", "buku-token")
        .send()
        .await
        .map_err(|_| QuotaError::Offline)?;
    status_result(resp.status().as_u16())?;
    let body = resp.text().await.map_err(|_| QuotaError::Offline)?;
    let (plan, windows) = parse_summary(&body, now)?;
    Ok(build_quota(plan, windows))
}

pub fn reason(e: &QuotaError, lang: Lang) -> String {
    i18n::quota_reason(Service::Cursor, e, lang)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use chrono::{Duration, TimeZone};

    const SECRET: &str = "SECRET-TOKEN-VALUE";

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 30, 5, 0, 0).unwrap()
    }

    fn jwt(sub: &str, exp: i64) -> String {
        let h = URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#);
        let p = URL_SAFE_NO_PAD.encode(format!(r#"{{"sub":"{sub}","exp":{exp}}}"#));
        format!("{h}.{p}.sig")
    }

    fn auth(expires_at: Option<DateTime<Utc>>) -> CursorAuth {
        CursorAuth { cookie: SECRET.into(), expires_at }
    }

    /// In-memory stand-in: tests never touch the real Keychain.
    #[derive(Default)]
    struct MemStore(std::cell::RefCell<Option<String>>);

    impl SessionStore for MemStore {
        fn load(&self) -> Result<Option<String>, QuotaError> {
            Ok(self.0.borrow().clone())
        }
        fn save(&self, cookie: &str) -> Result<(), QuotaError> {
            *self.0.borrow_mut() = Some(cookie.to_string());
            Ok(())
        }
        fn clear(&self) -> Result<(), QuotaError> {
            *self.0.borrow_mut() = None;
            Ok(())
        }
    }

    fn ok_quota() -> AgentQuota {
        build_quota(Some("Pro".into()), vec![])
    }

    fn summary() -> (Option<String>, Vec<QuotaWindow>) {
        parse_summary(include_str!("../../tests/fixtures/cursor_usage_summary.json"), now()).unwrap()
    }

    #[test]
    fn parse_summary_fixture() {
        let (plan, ws) = summary();
        assert_eq!(plan.as_deref(), Some("Pro"));
        let got: Vec<(&str, &str, f64)> = ws.iter().map(|w| (w.id.as_str(), w.label.as_str(), w.used_pct)).collect();
        assert_eq!(got, [("total", "Total", 75.0), ("auto", "Auto", 70.0), ("api", "API", 90.0)]);
        assert_eq!(ws[0].resets_at, Utc.with_ymd_and_hms(2026, 11, 5, 2, 0, 0).unwrap());
        assert!(!ws[0].expired);
        assert_eq!(ws[0].observed_at, now());
    }

    #[test]
    fn parse_summary_skips_missing_and_unknown_and_flags_expiry() {
        let j = r#"{"billingCycleEnd":"2026-10-01T00:00:00.000Z","membershipType":"ultra",
            "individualUsage":{"plan":{"autoPercentUsed":10,"bonusPercentUsed":99,"somethingNew":5}}}"#;
        let (plan, ws) = parse_summary(j, now()).unwrap();
        assert_eq!(plan.as_deref(), Some("Ultra"));
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].id, "auto");
        assert!(ws[0].expired);
    }

    #[test]
    fn parse_summary_rejects_garbage() {
        assert_eq!(parse_summary("nope", now()).unwrap_err(), QuotaError::Parse);
        assert_eq!(parse_summary("{}", now()).unwrap_err(), QuotaError::Parse);
        let no_end = r#"{"membershipType":"pro","individualUsage":{"plan":{"totalPercentUsed":1}}}"#;
        assert_eq!(parse_summary(no_end, now()).unwrap_err(), QuotaError::Parse);
    }

    #[tokio::test]
    async fn binding_and_near_limit_follow_total_not_api() {
        // Fixture: api 90% used (10% left) but total 75% used.
        let (_, ws) = summary();
        assert!(ws.iter().any(|w| w.id == "api" && w.remaining() <= 15.0));
        let q = build_quota(Some("Pro".into()), ws);
        assert_eq!(q.agent, Agent::Cursor);
        assert_eq!(q.binding.as_ref().unwrap().id, "total");
        assert!(!q.near_limit);
    }

    #[test]
    fn near_limit_when_total_is_low() {
        let j = r#"{"billingCycleEnd":"2026-11-07T00:00:00Z","individualUsage":{"plan":{"totalPercentUsed":90,"autoPercentUsed":10}}}"#;
        let (plan, ws) = parse_summary(j, now()).unwrap();
        assert!(build_quota(plan, ws).near_limit);
        let j = r#"{"billingCycleEnd":"2026-10-01T00:00:00Z","individualUsage":{"plan":{"totalPercentUsed":99}}}"#;
        let (plan, ws) = parse_summary(j, now()).unwrap();
        let q = build_quota(plan, ws);
        assert!(!q.near_limit && q.binding.is_none(), "expired total window is not live");
    }

    #[test]
    fn debug_redacts_cookie() {
        let d = format!("{:?}", auth(Some(now())));
        assert!(!d.contains(SECRET), "{d}");
        assert!(d.contains("<redacted>"));
    }

    #[test]
    fn cookie_header_exact() {
        assert_eq!(auth(None).cookie_header(), format!("WorkosCursorSessionToken={SECRET}"));
    }

    #[test]
    fn normalize_cookie_cleans_prefix_and_separator() {
        assert_eq!(normalize_cookie("  user_01::abc  ").unwrap(), "user_01%3A%3Aabc");
        assert_eq!(normalize_cookie("WorkosCursorSessionToken=user_01::abc").unwrap(), "user_01%3A%3Aabc");
        assert_eq!(normalize_cookie("user_01%3A%3Aabc").unwrap(), "user_01%3A%3Aabc");
        assert_eq!(normalize_cookie("opaque-value").unwrap(), "opaque-value");
    }

    #[test]
    fn normalize_cookie_rejects_unsafe_input() {
        let long = "a".repeat(MAX_COOKIE_BYTES + 1);
        assert!(normalize_cookie(&"a".repeat(MAX_COOKIE_BYTES)).is_ok());
        for bad in ["", "   ", "WorkosCursorSessionToken=", &long, "a b", "a;b", "a\nb", "a\r\nSet-Cookie: x", "a\tb", "a\0b", "kuki\u{e9}", "a\u{200b}b"] {
            assert_eq!(normalize_cookie(bad).unwrap_err(), QuotaError::Parse, "{bad:?}");
        }
    }

    #[test]
    fn auth_from_cookie_reads_jwt_expiry_and_tolerates_opaque() {
        let a = auth_from_cookie(format!("user_01%3A%3A{}", jwt("auth0|user_01", 1793000000)));
        assert_eq!(a.expires_at.unwrap().timestamp(), 1793000000);
        assert!(a.cookie_header().starts_with("WorkosCursorSessionToken=user_01%3A%3A"));
        let bare = auth_from_cookie(jwt("auth0|user_01", 1793000001));
        assert_eq!(bare.expires_at.unwrap().timestamp(), 1793000001);
        for opaque in ["opaque", "user%3A%3Aopaque", "a.b.c", "a.!!!.c", &format!("a.{}.c", URL_SAFE_NO_PAD.encode("{}"))] {
            assert!(auth_from_cookie(opaque.to_string()).expires_at.is_none(), "{opaque}");
        }
    }

    #[tokio::test]
    async fn expired_cookie_makes_no_request() {
        let a = auth(Some(now() - Duration::minutes(1)));
        let r = fetch_summary_at(&http_client(), &a, "http://127.0.0.1:1", now()).await;
        assert_eq!(r.unwrap_err(), QuotaError::Expired);
        let a = auth(Some(now()));
        let r = fetch_summary_at(&http_client(), &a, "http://127.0.0.1:1", now()).await;
        assert_eq!(r.unwrap_err(), QuotaError::Expired, "expires_at <= now");
    }

    #[tokio::test]
    async fn cookie_without_expiry_still_tries_the_network() {
        let r = fetch_summary_at(&http_client(), &auth(None), "http://127.0.0.1:1", now()).await;
        assert_eq!(r.unwrap_err(), QuotaError::Offline);
    }

    #[test]
    fn connect_saves_only_after_the_server_accepts() {
        let store = MemStore::default();
        let auth = prepare_connect("WorkosCursorSessionToken=u::t", Lang::Id).unwrap();
        assert_eq!(auth.cookie_header(), "WorkosCursorSessionToken=u%3A%3At");
        assert_eq!(finish_connect(&store, &auth, Ok(ok_quota()), &|| true, Lang::Id), Ok(None));
        assert_eq!(store.load().unwrap().as_deref(), Some("u%3A%3At"));
    }

    #[test]
    fn connect_does_not_save_a_rejected_cookie() {
        for (err, msg) in [
            (QuotaError::Expired, "Cookie Tidak Valid"),
            (QuotaError::Offline, "Sedang Offline"),
            (QuotaError::Http(500), "Server Cursor Menjawab HTTP 500"),
            (QuotaError::Parse, "Respons Cursor Tidak Dikenali"),
        ] {
            let store = MemStore::default();
            let auth = prepare_connect("u::secret", Lang::Id).unwrap();
            let e = finish_connect(&store, &auth, Err(err), &|| true, Lang::Id).unwrap_err();
            assert_eq!(e, msg);
            assert!(!e.contains("secret"));
            assert_eq!(store.load().unwrap(), None, "{msg}");
        }
    }

    #[test]
    fn connect_does_not_save_when_the_session_changed_meanwhile() {
        let store = MemStore::default();
        let auth = prepare_connect("u::t", Lang::Id).unwrap();
        let e = finish_connect(&store, &auth, Ok(ok_quota()), &|| false, Lang::Id).unwrap_err();
        assert_eq!(e, "Koneksi Dibatalkan, Coba Lagi");
        assert_eq!(store.load().unwrap(), None, "a Putuskan during the wait stays final");
    }

    #[test]
    fn a_connect_that_takes_the_lock_after_a_disconnect_does_not_save() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Mutex;
        let (lock, store, epoch) = (Mutex::new(()), MemStore::default(), AtomicU64::new(0));
        let auth = prepare_connect("u::t", Lang::Id).unwrap();
        let started = epoch.load(Ordering::SeqCst); // Hubungkan captured this before asking the server
        store.save("old").unwrap();
        let order = std::cell::RefCell::new(vec![]);
        // Putuskan: the epoch moves while the lock is held, before the item is cleared.
        disconnect_locked(&lock, &store, || {
            epoch.fetch_add(1, Ordering::SeqCst);
            order.borrow_mut().push(("bump", store.load().unwrap().is_some()));
        })
        .unwrap();
        assert_eq!(order.into_inner(), [("bump", true)], "the bump comes before the clear");
        assert_eq!(store.load().unwrap(), None);
        let current = || epoch.load(Ordering::SeqCst) == started;
        let r = connect_locked(&lock, || finish_connect(&store, &auth, Ok(ok_quota()), &current, Lang::Id));
        assert_eq!(r.unwrap_err(), "Koneksi Dibatalkan, Coba Lagi");
        assert_eq!(store.load().unwrap(), None, "the credential does not come back");
    }

    #[test]
    fn a_connect_that_finished_first_is_cleared_by_the_disconnect() {
        let (lock, store) = (std::sync::Mutex::new(()), MemStore::default());
        let auth = prepare_connect("u::t", Lang::Id).unwrap();
        connect_locked(&lock, || finish_connect(&store, &auth, Ok(ok_quota()), &|| true, Lang::Id)).unwrap();
        disconnect_locked(&lock, &store, || {}).unwrap();
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn connect_with_unusable_input_is_invalid() {
        assert_eq!(prepare_connect("a b;c", Lang::Id).unwrap_err(), "Cookie Tidak Valid");
        assert_eq!(prepare_connect("", Lang::Id).unwrap_err(), "Cookie Tidak Valid");
    }

    #[test]
    fn load_auth_is_none_without_a_cookie() {
        let store = MemStore::default();
        assert!(load_auth(&store).unwrap().is_none());
        store.save("u%3A%3At").unwrap();
        assert_eq!(load_auth(&store).unwrap().unwrap().cookie_header(), "WorkosCursorSessionToken=u%3A%3At");
        store.clear().unwrap();
        store.clear().unwrap();
        assert!(load_auth(&store).unwrap().is_none());
    }

    #[tokio::test]
    async fn unreachable_host_is_offline() {
        let a = auth(Some(now() + Duration::hours(1)));
        let r = fetch_summary_at(&http_client(), &a, "http://127.0.0.1:1", now()).await;
        assert_eq!(r.unwrap_err(), QuotaError::Offline);
    }

    #[test]
    fn only_real_refusals_count_as_denied() {
        for code in [-25293, -128, -25308] {
            assert_eq!(keychain_error(code), QuotaError::KeychainDenied, "{code}");
        }
        for code in [-25291, -25299, -34018, -50, 0] {
            assert_eq!(keychain_error(code), QuotaError::KeychainFailed, "{code} is transient");
        }
        assert_eq!(reason(&QuotaError::KeychainFailed, Lang::Id), "Keychain Tidak Bisa Dibaca");
        assert_eq!(reason(&QuotaError::KeychainFailed, Lang::En), "Keychain Could Not Be Read");
    }

    #[test]
    fn status_result_maps_codes() {
        assert_eq!(status_result(401), Err(QuotaError::Expired));
        assert_eq!(status_result(403), Err(QuotaError::Expired));
        assert_eq!(status_result(302), Err(QuotaError::Http(302)));
        assert_eq!(status_result(500), Err(QuotaError::Http(500)));
        assert_eq!(status_result(200), Ok(()));
        assert_eq!(status_result(204), Ok(()));
    }

    #[test]
    fn reasons_match_spec() {
        assert_eq!(reason(&QuotaError::Expired, Lang::Id), "Sesi Cursor Habis, Tempel Ulang Di Pengaturan");
        assert_eq!(reason(&QuotaError::Offline, Lang::Id), "Sedang Offline");
        assert_eq!(reason(&QuotaError::Http(503), Lang::Id), "Server Cursor Menjawab HTTP 503");
        assert_eq!(reason(&QuotaError::Parse, Lang::Id), "Respons Cursor Tidak Dikenali");
        assert_eq!(reason(&QuotaError::NotFound, Lang::Id), "Respons Cursor Tidak Dikenali");
        assert_eq!(reason(&QuotaError::KeychainDenied, Lang::Id), "Akses Keychain Ditolak");
    }
}

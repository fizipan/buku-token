//! Quota notifications. `decide` is a pure function over the quotas, the burn projection, the settings
//! and the keys already sent; `combine` folds a burst into one notification; the Tauri wiring at the bottom is
//! the only part that touches the notification API.
//!
//! A notification carries only the agent name, the window label and a percentage or a clock time.

use std::collections::HashSet;

use chrono::{DateTime, FixedOffset, Utc};

use crate::aggregate::is_enabled;
use crate::model::Agent;
use crate::quota::{AgentQuota, Burn, NEAR_LIMIT_REMAINING};
use crate::i18n;
use crate::settings::{Lang, Settings};

/// Title of a notification that lists several; the app's name is the same in every language.
const BURST_TITLE: &str = "Buku Token";

/// A projection needs at least this much data in the window, or it is too noisy to announce.
const MIN_DATA_MINUTES: i64 = 30;

/// A window's reset time drifts by seconds between polls, so keys of one cycle differ slightly.
/// A stored key of the same agent, window and kind within this many seconds counts as the same cycle.
const SAME_CYCLE_SECS: i64 = 3600;

/// Whether `sent` holds a key for this agent and window (and kind) whose reset is within an hour.
fn already_sent(sent: &HashSet<String>, agent: Agent, window: &str, resets_at: DateTime<Utc>, estimate: bool) -> bool {
    let mine = key(agent, window, resets_at, estimate);
    let (prefix, _) = split(&mine);
    sent.iter().any(|k| {
        let (p, ts) = split(k);
        p == prefix && (ts - resets_at.timestamp()).abs() <= SAME_CYCLE_SECS
    })
}

/// `agent:window:ts[:est]` into (`agent:window[:est]`, ts). An unreadable key never matches.
fn split(k: &str) -> (String, i64) {
    let parts: Vec<&str> = k.split(':').collect();
    match parts.as_slice() {
        [a, w, ts] => (format!("{a}:{w}"), ts.parse().unwrap_or(i64::MIN / 2)),
        [a, w, ts, "est"] => (format!("{a}:{w}:est"), ts.parse().unwrap_or(i64::MIN / 2)),
        _ => (k.to_string(), i64::MIN / 2),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    /// `agent:window:resets_at`, plus `:est` for the projection. Sent once per key.
    pub key: String,
    pub title: String,
    pub body: String,
    /// When the key stops mattering (the window's reset): older keys are pruned.
    pub expires: DateTime<Utc>,
}

fn key(agent: Agent, window: &str, resets_at: DateTime<Utc>, estimate: bool) -> String {
    let agent = serde_json::to_value(agent).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
    format!("{agent}:{window}:{}{}", resets_at.timestamp(), if estimate { ":est" } else { "" })
}

pub fn decide(
    quotas: &[AgentQuota],
    burn: Option<&Burn>,
    settings: &Settings,
    sent: &HashSet<String>,
    now: DateTime<Utc>,
    tz: FixedOffset,
) -> Vec<Notice> {
    let mut out = Vec::new();
    let lang = settings.language;
    for q in quotas.iter().filter(|q| is_enabled(settings, q.agent)) {
        let name = crate::share::agent_name(q.agent);
        if settings.notify_low_quota {
            // Cursor answers for its `total` window alone, like the menu bar and `near_limit`: the API
            // pool can run low while the user lives on Auto.
            let watched = |id: &str| q.agent != Agent::Cursor || id == "total";
            for w in q.windows.iter().filter(|w| watched(&w.id) && !w.expired && w.resets_at > now && w.remaining() <= NEAR_LIMIT_REMAINING) {
                let k = key(q.agent, &w.id, w.resets_at, false);
                if !already_sent(sent, q.agent, &w.id, w.resets_at, false) {
                    let left = w.remaining().clamp(0.0, 100.0).round();
                    out.push(Notice { key: k, title: name.into(), body: i18n::low_quota_body(&i18n::window_label(&w.id, lang), left, lang), expires: w.resets_at });
                }
            }
        }
        if settings.notify_projected_exhaustion && q.agent == Agent::ClaudeCode {
            let five_hour = q.windows.iter().find(|w| w.id == "five_hour" && !w.expired);
            if let (Some(w), Some(b)) = (five_hour, burn)
                && let Some(at) = b.exhausts_at
                && at > now
                && b.resets_at == w.resets_at
                && b.points.iter().any(|p| p.0 >= MIN_DATA_MINUTES)
            {
                let k = key(q.agent, &w.id, w.resets_at, true);
                if !already_sent(sent, q.agent, &w.id, w.resets_at, true) {
                    let clock = at.with_timezone(&tz).format(if lang == Lang::Id { "%H.%M" } else { "%H:%M" }).to_string();
                    out.push(Notice {
                        key: k,
                        title: name.into(),
                        body: i18n::estimate_body(&i18n::window_label(&w.id, lang), &clock, lang),
                        expires: w.resets_at,
                    });
                }
            }
        }
    }
    out
}

/// One notification for everything due in a pass, so switching on with many low windows is not a burst.
/// A single notice keeps its own title; several share the app name and list one line each.
pub fn combine(notices: &[Notice]) -> Option<(String, String)> {
    match notices {
        [] => None,
        [one] => Some((one.title.clone(), one.body.clone())),
        many => {
            let lines: Vec<String> = many.iter().map(|n| format!("{} · {}", n.title, n.body)).collect();
            Some((BURST_TITLE.into(), lines.join("\n")))
        }
    }
}

// --- Tauri wiring ---------------------------------------------------------------------------

use std::sync::Mutex;

use chrono::Local;
use tauri::{AppHandle, Listener, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::engine::Engine;

pub fn setup(app: &tauri::App) {
    let handle = app.handle().clone();
    app.listen(crate::commands::DATA_UPDATED, move |_| {
        let app = handle.clone();
        tauri::async_runtime::spawn_blocking(move || check(&app));
    });
}

/// The thin sender: shows one notification. The plugin cannot tell whether macOS displayed it.
fn show(app: &AppHandle, title: &str, body: &str) -> bool {
    app.notification().builder().title(title).body(body).show().is_ok()
}

/// The "Kirim Tes" button: one sample notification through the real sender.
pub fn send_test(app: &AppHandle) {
    show(app, BURST_TITLE, i18n::test_body(app.state::<Engine>().settings().language));
}

/// Sends what is due as one notification and remembers it. One check at a time, so two overlapping
/// updates never send twice.
fn check(app: &AppHandle) {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let engine = app.state::<Engine>();
    let notices = match engine.pending_notices(*Local::now().offset(), Utc::now()) {
        Ok(n) => n,
        Err(e) => return eprintln!("[buku-token] notifications: {e:#}"),
    };
    let Some((title, body)) = combine(&notices) else { return };
    if !show(app, &title, &body) {
        return;
    }
    for n in &notices {
        if let Err(e) = engine.mark_notified(n) {
            eprintln!("[buku-token] cannot remember a sent notification: {e:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::QuotaWindow;
    use chrono::{Duration, TimeZone};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 30, 10, 0, 0).unwrap()
    }
    fn tz() -> FixedOffset {
        FixedOffset::east_opt(7 * 3600).unwrap()
    }
    fn win(id: &str, used: f64, resets_in_min: i64) -> QuotaWindow {
        QuotaWindow {
            id: id.into(),
            label: if id == "five_hour" { "5 jam".into() } else { "7 hari".into() },
            used_pct: used,
            resets_at: now() + Duration::minutes(resets_in_min),
            observed_at: now(),
            expired: false,
        }
    }
    fn quota(agent: Agent, windows: Vec<QuotaWindow>) -> AgentQuota {
        AgentQuota { agent, plan: None, binding: windows.first().cloned(), windows, near_limit: false, unavailable_reason: None, failure: None }
    }
    fn on() -> Settings {
        Settings { notify_low_quota: true, notify_projected_exhaustion: true, ..Settings::default() }
    }
    fn burn(w: &QuotaWindow, last_minute: i64, exhausts_in_min: i64) -> Burn {
        Burn {
            window_start: w.resets_at - Duration::hours(5),
            resets_at: w.resets_at,
            points: vec![(0, 0.0), (last_minute, 50.0)],
            projected_at_reset: 100.0,
            exhausts_at: Some(now() + Duration::minutes(exhausts_in_min)),
        }
    }
    fn none() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn low_quota_fires_at_the_threshold_and_not_above() {
        let at = quota(Agent::Codex, vec![win("five_hour", 85.0, 120)]);
        let above = quota(Agent::Codex, vec![win("five_hour", 84.9, 120)]);
        let got = decide(&[at], None, &on(), &none(), now(), tz());
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].title.as_str(), got[0].body.as_str()), ("Codex", "Sesi 5 jam · sisa 15%"));
        assert!(decide(&[above], None, &on(), &none(), now(), tz()).is_empty());
    }

    #[test]
    fn english_notices_say_so_and_keep_the_same_language_free_keys() {
        let w = win("five_hour", 50.0, 200);
        let q = quota(Agent::ClaudeCode, vec![w.clone(), win("seven_day", 95.0, 4000)]);
        let (id, en) = (on(), Settings { language: Lang::En, ..on() });
        let b = burn(&w, 60, 60);
        let from_id = decide(std::slice::from_ref(&q), Some(&b), &id, &none(), now(), tz());
        let from_en = decide(std::slice::from_ref(&q), Some(&b), &en, &none(), now(), tz());
        let bodies: Vec<&str> = from_en.iter().map(|n| n.body.as_str()).collect();
        assert_eq!(bodies, ["Weekly · 5% left", "5h Session · estimated to run out at 18:00"]);
        // A language switch must not make a sent notification fire again: keys carry ids, not words.
        assert_eq!(from_id.iter().map(|n| &n.key).collect::<Vec<_>>(), from_en.iter().map(|n| &n.key).collect::<Vec<_>>());
        let sent: HashSet<String> = from_id.iter().map(|n| n.key.clone()).collect();
        assert!(decide(std::slice::from_ref(&q), Some(&b), &en, &sent, now(), tz()).is_empty());
    }

    #[test]
    fn a_notice_is_sent_once_per_cycle_and_again_in_the_next() {
        let q = quota(Agent::Codex, vec![win("five_hour", 90.0, 120)]);
        let first = decide(std::slice::from_ref(&q), None, &on(), &none(), now(), tz());
        let sent: HashSet<String> = first.iter().map(|n| n.key.clone()).collect();
        assert!(decide(&[q], None, &on(), &sent, now(), tz()).is_empty());
        let next_cycle = quota(Agent::Codex, vec![win("five_hour", 90.0, 120 + 300)]);
        assert_eq!(decide(&[next_cycle], None, &on(), &sent, now(), tz()).len(), 1);
    }

    #[test]
    fn cursor_notifies_on_its_total_window_only() {
        let calm = quota(Agent::Cursor, vec![win("total", 40.0, 4000), win("auto", 90.0, 4000), win("api", 97.0, 4000)]);
        assert!(decide(&[calm], None, &on(), &none(), now(), tz()).is_empty(), "a low API pool alone stays quiet");
        let low = quota(Agent::Cursor, vec![win("total", 92.0, 4000), win("auto", 90.0, 4000), win("api", 97.0, 4000)]);
        let got = decide(&[low], None, &on(), &none(), now(), tz());
        assert_eq!(got.len(), 1);
        assert!(got[0].key.starts_with("cr:total:"));
    }

    #[test]
    fn each_window_has_its_own_key() {
        let q = quota(Agent::Codex, vec![win("five_hour", 90.0, 120), win("seven_day", 95.0, 4000)]);
        let got = decide(&[q], None, &on(), &none(), now(), tz());
        assert_eq!(got.len(), 2);
        assert_ne!(got[0].key, got[1].key);
        assert!(got[0].key.starts_with("cx:five_hour:"));
    }

    #[test]
    fn disabled_agents_expired_windows_and_switched_off_settings_stay_quiet() {
        let q = quota(Agent::Codex, vec![win("five_hour", 90.0, 120)]);
        let mut off = on();
        off.enabled_agents.insert(Agent::Codex, false);
        assert!(decide(std::slice::from_ref(&q), None, &off, &none(), now(), tz()).is_empty());
        assert!(decide(std::slice::from_ref(&q), None, &Settings::default(), &none(), now(), tz()).is_empty());
        let mut expired = win("five_hour", 90.0, 120);
        expired.expired = true;
        assert!(decide(&[quota(Agent::Codex, vec![expired])], None, &on(), &none(), now(), tz()).is_empty());
        let past = win("five_hour", 90.0, -5);
        assert!(decide(&[quota(Agent::Codex, vec![past])], None, &on(), &none(), now(), tz()).is_empty());
    }

    #[test]
    fn the_estimate_needs_thirty_minutes_of_data_and_says_it_is_an_estimate() {
        let w = win("five_hour", 50.0, 200);
        let q = quota(Agent::ClaudeCode, vec![w.clone()]);
        let only_estimate = Settings { notify_projected_exhaustion: true, ..Settings::default() };
        assert!(decide(std::slice::from_ref(&q), Some(&burn(&w, 29, 60)), &only_estimate, &none(), now(), tz()).is_empty());
        let got = decide(&[q], Some(&burn(&w, 30, 60)), &only_estimate, &none(), now(), tz());
        assert_eq!(got.len(), 1);
        // 11.00 UTC is 18.00 at +07:00
        assert_eq!(got[0].body, "Sesi 5 jam · perkiraan habis pukul 18.00");
        assert!(got[0].key.ends_with(":est"));
    }

    #[test]
    fn the_estimate_is_once_per_cycle_and_skips_a_projection_without_exhaustion() {
        let w = win("five_hour", 50.0, 200);
        let q = quota(Agent::ClaudeCode, vec![w.clone()]);
        let b = burn(&w, 60, 60);
        let first = decide(std::slice::from_ref(&q), Some(&b), &on(), &none(), now(), tz());
        let sent: HashSet<String> = first.iter().map(|n| n.key.clone()).collect();
        assert!(decide(std::slice::from_ref(&q), Some(&b), &on(), &sent, now(), tz()).is_empty());
        let calm = Burn { exhausts_at: None, ..b };
        assert!(decide(&[q], Some(&calm), &on(), &none(), now(), tz()).is_empty());
    }

    #[test]
    fn a_notice_carries_only_agent_window_and_numbers() {
        let w = win("five_hour", 90.0, 200);
        let q = quota(Agent::ClaudeCode, vec![w.clone()]);
        for n in decide(&[q], Some(&burn(&w, 60, 60)), &on(), &none(), now(), tz()) {
            assert!(!n.title.contains('/') && !n.body.contains('/'), "{n:?}");
            assert_eq!(n.title, "Claude Code");
        }
    }

    #[test]
    fn a_reset_time_that_drifts_within_a_cycle_does_not_send_again() {
        let q = quota(Agent::Codex, vec![win("five_hour", 90.0, 120)]);
        let first = decide(std::slice::from_ref(&q), None, &on(), &none(), now(), tz());
        let sent: HashSet<String> = first.iter().map(|n| n.key.clone()).collect();
        for drift in [1, 20, -21, 59 * 60] {
            let mut w = win("five_hour", 90.0, 120);
            w.resets_at += Duration::seconds(drift);
            let drifted = quota(Agent::Codex, vec![w]);
            assert!(decide(&[drifted], None, &on(), &sent, now(), tz()).is_empty(), "{drift}s");
        }
        let next = quota(Agent::Codex, vec![win("five_hour", 90.0, 120 + 300)]);
        assert_eq!(decide(&[next], None, &on(), &sent, now(), tz()).len(), 1, "the next cycle sends again");
        // another window of the same agent, and the estimate of the same window, are separate
        let other = quota(Agent::Codex, vec![win("seven_day", 90.0, 120)]);
        assert_eq!(decide(&[other], None, &on(), &sent, now(), tz()).len(), 1);
    }

    #[test]
    fn the_estimate_tolerates_drift_too_but_stays_apart_from_the_low_quota_key() {
        let w = win("five_hour", 90.0, 200);
        let q = quota(Agent::ClaudeCode, vec![w.clone()]);
        let all = decide(std::slice::from_ref(&q), Some(&burn(&w, 60, 60)), &on(), &none(), now(), tz());
        assert_eq!(all.len(), 2);
        let low_only: HashSet<String> = all.iter().filter(|n| !n.key.ends_with(":est")).map(|n| n.key.clone()).collect();
        assert_eq!(decide(std::slice::from_ref(&q), Some(&burn(&w, 60, 60)), &on(), &low_only, now(), tz()).len(), 1);
        let mut drifted = w.clone();
        drifted.resets_at += Duration::seconds(20);
        let all_sent: HashSet<String> = all.iter().map(|n| n.key.clone()).collect();
        let q2 = quota(Agent::ClaudeCode, vec![drifted.clone()]);
        assert!(decide(&[q2], Some(&burn(&drifted, 60, 60)), &on(), &all_sent, now(), tz()).is_empty());
    }

    #[test]
    fn one_notice_goes_out_as_is_and_several_become_one_combined_notification() {
        let n = |agent: &str, body: &str| Notice { key: body.into(), title: agent.into(), body: body.into(), expires: now() };
        assert_eq!(combine(&[]), None);
        assert_eq!(combine(&[n("Codex", "5 jam · sisa 12%")]), Some(("Codex".into(), "5 jam · sisa 12%".into())));
        let (title, body) = combine(&[n("Codex", "5 jam · sisa 12%"), n("Codex", "7 hari · sisa 9%"), n("Claude Code", "5 jam · sisa 3%")]).unwrap();
        assert_eq!(title, "Buku Token");
        assert_eq!(body, "Codex · 5 jam · sisa 12%\nCodex · 7 hari · sisa 9%\nClaude Code · 5 jam · sisa 3%");
    }

}

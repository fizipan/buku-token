//! Codex quota windows from the latest rate-limit snapshot.

use chrono::{DateTime, Utc};

use super::{binding_window, is_near_limit, AgentQuota, QuotaWindow};
use crate::ingest::codex::{CodexRateLimits, RawWindow};
use crate::i18n;
use crate::model::{capitalize, Agent};
use crate::settings::Lang;

fn to_window(raw: &RawWindow, observed_at: DateTime<Utc>, now: DateTime<Utc>, lang: Lang) -> QuotaWindow {
    let id = match raw.window_minutes {
        300 => "five_hour".to_string(),
        10080 => "seven_day".to_string(),
        m => format!("window_{m}"),
    };
    QuotaWindow {
        label: i18n::window_label(&id, lang),
        id,
        used_pct: raw.used_pct,
        resets_at: raw.resets_at,
        observed_at,
        expired: raw.resets_at < now,
    }
}

pub fn from_rate_limits(rl: &CodexRateLimits, now: DateTime<Utc>, lang: Lang) -> AgentQuota {
    let windows: Vec<QuotaWindow> = [&rl.primary, &rl.secondary]
        .into_iter()
        .flatten()
        .map(|w| to_window(w, rl.observed_at, now, lang))
        .collect();
    AgentQuota {
        agent: Agent::Codex,
        plan: rl.plan.as_deref().map(capitalize),
        binding: binding_window(&windows),
        near_limit: is_near_limit(&windows, None),
        windows,
        unavailable_reason: None,
        failure: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    #[test]
    fn codex_expired_window_excluded_from_binding() {
        let now = Utc.with_ymd_and_hms(2026, 10, 30, 8, 0, 0).unwrap();
        let rl = CodexRateLimits {
            observed_at: now - Duration::hours(1),
            plan: Some("plus".into()),
            primary: Some(RawWindow { used_pct: 97.0, window_minutes: 300, resets_at: now - Duration::minutes(5) }),
            secondary: Some(RawWindow { used_pct: 40.0, window_minutes: 10080, resets_at: now + Duration::days(3) }),
        };
        let q = from_rate_limits(&rl, now, Lang::Id);
        assert_eq!(q.windows.len(), 2);
        assert!(q.windows[0].expired && !q.windows[1].expired);
        assert_eq!(q.windows[0].label, "Sesi 5 jam");
        assert_eq!(q.windows[1].label, "Mingguan");
        assert_eq!(q.binding.unwrap().id, "seven_day");
        assert!(!q.near_limit, "expired 97% window must not trigger near_limit");
        assert_eq!(q.plan.as_deref(), Some("Plus"));
        let en = from_rate_limits(&rl, now, Lang::En);
        assert_eq!((en.windows[0].label.as_str(), en.windows[1].label.as_str()), ("5h Session", "Weekly"));
        assert_eq!(en.windows[0].id, "five_hour", "the id does not change with the language");
    }

    #[test]
    fn codex_no_windows_has_no_binding() {
        let now = Utc.with_ymd_and_hms(2026, 10, 30, 8, 0, 0).unwrap();
        let rl = CodexRateLimits { observed_at: now, plan: None, primary: None, secondary: None };
        let q = from_rate_limits(&rl, now, Lang::Id);
        assert!(q.windows.is_empty() && q.binding.is_none());
    }
}

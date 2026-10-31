//! Quota windows shared by all agents: binding window, projection and pace.

pub mod claude;
pub mod codex;
pub mod cursor;
pub mod openrouter;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::model::Agent;

/// Remaining percentage at or below which a window counts as "near limit".
pub(crate) const NEAR_LIMIT_REMAINING: f64 = 15.0;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    /// "five_hour" | "seven_day" | "seven_day_opus" ...
    pub id: String,
    pub label: String,
    pub used_pct: f64,
    pub resets_at: DateTime<Utc>,
    pub observed_at: DateTime<Utc>,
    pub expired: bool,
}

impl QuotaWindow {
    pub fn remaining(&self) -> f64 {
        100.0 - self.used_pct
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentQuota {
    pub agent: Agent,
    pub plan: Option<String>,
    pub windows: Vec<QuotaWindow>,
    pub binding: Option<QuotaWindow>,
    pub near_limit: bool,
    pub unavailable_reason: Option<String>,
    /// Why the quota is unavailable, kept as the error and not as a text, so the text is made in the
    /// language that is current when it is read (`unavailable_reason` is filled in then).
    #[serde(skip)]
    pub failure: Option<claude::QuotaError>,
}

impl AgentQuota {
    /// A failed poll: no windows, only the reason.
    pub fn failed(agent: Agent, failure: claude::QuotaError) -> Self {
        AgentQuota { agent, plan: None, windows: vec![], binding: None, near_limit: false, unavailable_reason: None, failure: Some(failure) }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Burn {
    pub window_start: DateTime<Utc>,
    pub resets_at: DateTime<Utc>,
    /// (minutes since window_start, used_pct)
    pub points: Vec<(i64, f64)>,
    pub projected_at_reset: f64,
    pub exhausts_at: Option<DateTime<Utc>>,
}

/// Non-expired window with the least remaining quota.
pub fn binding_window(ws: &[QuotaWindow]) -> Option<QuotaWindow> {
    ws.iter()
        .filter(|w| !w.expired)
        .min_by(|a, b| a.remaining().total_cmp(&b.remaining()))
        .cloned()
}

/// True when any live window has <= 15% left, or the burn projects exhaustion.
pub fn is_near_limit(ws: &[QuotaWindow], burn: Option<&Burn>) -> bool {
    ws.iter().any(|w| !w.expired && w.remaining() <= NEAR_LIMIT_REMAINING)
        || burn.is_some_and(|b| b.exhausts_at.is_some())
}

/// Linear projection: average rate since `window_start`, extended to `resets_at`.
pub fn project(points: &[(i64, f64)], window_start: DateTime<Utc>, resets_at: DateTime<Utc>) -> Burn {
    let total_min = (resets_at - window_start).num_seconds() as f64 / 60.0;
    let last = points.iter().max_by_key(|p| p.0).copied();
    let (mut projected, mut exhausts_at) = (last.map_or(0.0, |p| p.1), None);
    if let Some((min, used)) = last
        && min > 0
        && used > 0.0
    {
        let rate = used / min as f64; // pct per minute
        projected = rate * total_min;
        let exhaust_min = 100.0 / rate;
        if exhaust_min < total_min {
            exhausts_at = Some(window_start + Duration::seconds((exhaust_min * 60.0).round() as i64));
        }
    }
    Burn {
        window_start,
        resets_at,
        points: points.to_vec(),
        projected_at_reset: projected.clamp(0.0, 100.0),
        exhausts_at,
    }
}

/// Percent (0..100) of a 7-day window already elapsed.
pub fn weekly_pace(resets_at: DateTime<Utc>, now: DateTime<Utc>) -> f64 {
    let week = Duration::days(7).num_seconds() as f64;
    let remaining = (resets_at - now).num_seconds() as f64;
    ((week - remaining) / week * 100.0).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone};

    fn win(id: &str, used: f64, expired: bool) -> QuotaWindow {
        let t = Utc.with_ymd_and_hms(2026, 10, 30, 5, 0, 0).unwrap();
        QuotaWindow { id: id.into(), label: id.into(), used_pct: used, resets_at: t, observed_at: t, expired }
    }

    #[test]
    fn binding_is_lowest_remaining() {
        let ws = [win("five_hour", 38.0, false), win("seven_day", 95.0, false)];
        assert_eq!(binding_window(&ws).unwrap().id, "seven_day");
    }

    #[test]
    fn binding_ignores_expired_and_empty() {
        assert!(binding_window(&[]).is_none());
        let ws = [win("five_hour", 10.0, false), win("seven_day", 99.0, true)];
        assert_eq!(binding_window(&ws).unwrap().id, "five_hour");
    }

    #[test]
    fn near_limit_on_any_window_at_15_remaining() {
        assert!(is_near_limit(&[win("five_hour", 10.0, false), win("seven_day", 85.0, false)], None));
        assert!(!is_near_limit(&[win("five_hour", 10.0, false), win("seven_day", 84.9, false)], None));
        assert!(!is_near_limit(&[win("seven_day", 99.0, true)], None));
    }

    fn start() -> DateTime<Utc> {
        // 12.40 local at +07:00
        FixedOffset::east_opt(7 * 3600).unwrap().with_ymd_and_hms(2026, 10, 30, 12, 40, 0).unwrap().to_utc()
    }

    #[test]
    fn projection_flags_exhaustion() {
        let s = start();
        let b = project(&[(100, 45.0), (205, 92.0)], s, s + Duration::minutes(300));
        let want = FixedOffset::east_opt(7 * 3600).unwrap().with_ymd_and_hms(2026, 10, 30, 16, 23, 0).unwrap().to_utc();
        let got = b.exhausts_at.expect("exhaustion expected");
        assert!((got - want).num_seconds().abs() <= 60, "{got} vs {want}");
        assert_eq!(b.projected_at_reset, 100.0);
        assert!(is_near_limit(&[], Some(&b)));
    }

    #[test]
    fn projection_without_exhaustion() {
        let s = start();
        let b = project(&[(159, 38.0)], s, s + Duration::minutes(300));
        assert!((b.projected_at_reset - 71.7).abs() < 0.1, "{}", b.projected_at_reset);
        assert!(b.exhausts_at.is_none());
    }

    #[test]
    fn projection_handles_no_points() {
        let s = start();
        let b = project(&[], s, s + Duration::minutes(300));
        assert_eq!(b.projected_at_reset, 0.0);
        assert!(b.exhausts_at.is_none());
    }

    #[test]
    fn weekly_pace_midweek() {
        let now = Utc.with_ymd_and_hms(2026, 10, 30, 0, 0, 0).unwrap();
        assert!((weekly_pace(now + Duration::hours(84), now) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn serializes_camel_case() {
        let v = serde_json::to_value(win("five_hour", 1.0, false)).unwrap();
        assert!(v.get("usedPct").is_some() && v.get("resetsAt").is_some());
    }
}

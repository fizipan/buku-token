use crate::model::{Agent, STACK_ORDER};
use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

const DEFAULT_USD_TO_IDR: f64 = 16300.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Plan {
    pub id: String,
    pub name: String,
    pub price_usd: f64,
    pub renewal_day: u8,
    pub agents: Vec<Agent>,
}

impl Default for Plan {
    fn default() -> Self {
        Plan { id: String::new(), name: String::new(), price_usd: 0.0, renewal_day: 1, agents: vec![] }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// The language of every text the app shows: the UI, the menu bar menu, notifications and the share image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    Id,
    En,
}

/// Whether quota rows and the menu bar show what is left or what is used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuotaDisplay {
    #[default]
    Left,
    Used,
}

/// Whether a quota row shows the time left until the reset or the clock time of the reset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResetDisplay {
    Countdown,
    #[default]
    Clock,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub usd_to_idr: f64,
    pub plans: Vec<Plan>,
    pub enabled_agents: HashMap<Agent, bool>,
    pub launch_at_login: bool,
    pub last_tab: String,
    pub onboarded: bool,
    /// Set once the first successful Cursor connect has handled the Cursor Pro plan, so deleting
    /// it later sticks. (`cursorPlanSeeded` from earlier builds is an unknown key and is ignored.)
    pub cursor_plan_added: bool,
    pub theme: Theme,
    pub quota_display: QuotaDisplay,
    pub reset_display: ResetDisplay,
    pub notify_low_quota: bool,
    pub notify_projected_exhaustion: bool,
    pub hide_on_screen_share: bool,
    /// OpenRouter balance and spend. Not an `Agent`: it is outside `enabled_agents` and every total.
    pub openrouter_enabled: bool,
    /// Antigravity activity from file dates. Not an `Agent` either.
    pub antigravity_enabled: bool,
    pub language: Lang,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            usd_to_idr: DEFAULT_USD_TO_IDR,
            plans: vec![
                Plan {
                    id: "claude-max".into(),
                    name: "Claude Max 5×".into(),
                    price_usd: 100.0,
                    renewal_day: 1,
                    agents: vec![Agent::ClaudeCode],
                },
                Plan {
                    id: "chatgpt-plus".into(),
                    name: "ChatGPT Plus".into(),
                    price_usd: 20.0,
                    renewal_day: 1,
                    agents: vec![Agent::Codex],
                },
            ],
            enabled_agents: STACK_ORDER.iter().map(|a| (*a, true)).collect(),
            launch_at_login: false,
            last_tab: "ringkasan".into(),
            onboarded: false,
            cursor_plan_added: false,
            theme: Theme::default(),
            quota_display: QuotaDisplay::default(),
            reset_display: ResetDisplay::default(),
            notify_low_quota: false,
            notify_projected_exhaustion: false,
            hide_on_screen_share: false,
            openrouter_enabled: true,
            antigravity_enabled: true,
            language: Lang::default(),
        }
    }
}

const CURSOR_PLAN_ID: &str = "cursor-pro";

/// What the first successful Cursor connect does to the plans: adds Cursor Pro, or, when a plan
/// with that id already exists, only moves its renewal day. Returns `None` once done before, so
/// later connects never touch the plans and a deleted plan stays deleted.
pub fn with_cursor_plan(current: &Settings, renewal_day: u8) -> Option<Settings> {
    if current.cursor_plan_added {
        return None;
    }
    let renewal_day = renewal_day.clamp(1, 31);
    let mut s = Settings { cursor_plan_added: true, ..current.clone() };
    match s.plans.iter_mut().find(|p| p.id == CURSOR_PLAN_ID) {
        Some(p) => p.renewal_day = renewal_day,
        None => s.plans.push(Plan {
            id: CURSOR_PLAN_ID.into(),
            name: "Cursor Pro".into(),
            price_usd: 20.0,
            renewal_day,
            agents: vec![Agent::Cursor],
        }),
    }
    Some(s)
}

/// Parses field by field: a value that does not fit its field (an unknown enum name, a string for
/// a number) leaves that one field at its default and keeps all the others.
fn parse_lenient(raw: &str) -> Option<Settings> {
    let serde_json::Value::Object(fields) = serde_json::from_str(raw).ok()? else { return None };
    let mut acc = serde_json::to_value(Settings::default()).ok()?;
    for (key, mut value) in fields {
        // One broken plan must not cost the others: keep every entry that still reads as a plan.
        if key == "plans"
            && let serde_json::Value::Array(plans) = &mut value
        {
            plans.retain(|p| serde_json::from_value::<Plan>(p.clone()).is_ok());
        }
        let mut next = acc.clone();
        next.as_object_mut()?.insert(key, value);
        if serde_json::from_value::<Settings>(next.clone()).is_ok() {
            acc = next;
        }
    }
    serde_json::from_value(acc).ok()
}

/// Missing file yields defaults. A file that is not a JSON object is kept as `settings.json.bad`
/// before defaults take over, so the next save does not destroy it. Values are sanitized.
pub fn load(path: &Path) -> Settings {
    let mut s = match std::fs::read_to_string(path) {
        Ok(raw) => parse_lenient(&raw).unwrap_or_else(|| {
            let _ = std::fs::copy(path, path.with_extension("json.bad"));
            Settings::default()
        }),
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                let _ = std::fs::copy(path, path.with_extension("json.bad"));
            }
            Settings::default()
        }
    };
    if !(s.usd_to_idr.is_finite() && s.usd_to_idr > 0.0) {
        s.usd_to_idr = DEFAULT_USD_TO_IDR;
    }
    for p in &mut s.plans {
        p.renewal_day = p.renewal_day.clamp(1, 31);
    }
    s.enabled_agents.entry(Agent::Cursor).or_insert(true);
    s
}

/// Applies the top-level fields present in `patch` (camelCase, as the UI sends them) to
/// `current`, so saving `{ lastTab }` alone leaves every other setting as it was.
pub fn merge(current: &Settings, patch: serde_json::Map<String, serde_json::Value>) -> Result<Settings> {
    let mut v = serde_json::to_value(current)?;
    if let Some(fields) = v.as_object_mut() {
        fields.extend(patch);
    }
    Ok(serde_json::from_value(v)?)
}

/// Atomic write (temp file in the same dir, then rename); creates parent dir.
pub fn save(path: &Path, s: &Settings) -> Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(s)?)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cycle {
    pub start: NaiveDate,
    pub end_exclusive: NaiveDate,
    pub day_of_cycle: u32,
    pub length: u32,
}

/// The renewal date in (year, month), clamped to the month's last day.
fn renewal_in(year: i32, month: u32, renewal_day: u8) -> NaiveDate {
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("valid month");
    let (ny, nm) = next_month(year, month);
    let days = NaiveDate::from_ymd_opt(ny, nm, 1).expect("valid month").signed_duration_since(first).num_days() as u32;
    NaiveDate::from_ymd_opt(year, month, (renewal_day as u32).clamp(1, days)).expect("clamped day")
}

fn next_month(year: i32, month: u32) -> (i32, u32) {
    if month == 12 { (year + 1, 1) } else { (year, month + 1) }
}

pub fn current_cycle(renewal_day: u8, today: NaiveDate) -> Cycle {
    let this = renewal_in(today.year(), today.month(), renewal_day);
    let start = if today >= this {
        this
    } else if today.month() == 1 {
        renewal_in(today.year() - 1, 12, renewal_day)
    } else {
        renewal_in(today.year(), today.month() - 1, renewal_day)
    };
    let (ey, em) = next_month(start.year(), start.month());
    let end_exclusive = renewal_in(ey, em, renewal_day);
    Cycle {
        start,
        end_exclusive,
        day_of_cycle: (today - start).num_days() as u32 + 1,
        length: (end_exclusive - start).num_days() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn default_matches_spec() {
        let s = Settings::default();
        assert_eq!(s.usd_to_idr, 16300.0);
        assert_eq!(s.plans.len(), 2, "Cursor Pro is added on connect, not before");
        assert_eq!(s.plans[0].id, "claude-max");
        assert_eq!(s.plans[0].name, "Claude Max 5×");
        assert_eq!(s.plans[0].price_usd, 100.0);
        assert_eq!(s.plans[0].renewal_day, 1);
        assert_eq!(s.plans[0].agents, vec![Agent::ClaudeCode]);
        assert_eq!(s.plans[1].id, "chatgpt-plus");
        assert_eq!(s.plans[1].name, "ChatGPT Plus");
        assert_eq!(s.plans[1].price_usd, 20.0);
        assert_eq!(s.plans[1].agents, vec![Agent::Codex]);
        assert!(!s.cursor_plan_added);
        assert_eq!((s.theme, s.quota_display, s.reset_display), (Theme::System, QuotaDisplay::Left, ResetDisplay::Clock));
        assert_eq!(s.enabled_agents.len(), 5);
        assert!(s.enabled_agents.values().all(|v| *v));
        assert!(!s.launch_at_login);
        assert_eq!(s.last_tab, "ringkasan");
        assert!(!s.onboarded);
    }

    #[test]
    fn language_defaults_to_indonesian_and_round_trips() {
        assert_eq!(Settings::default().language, Lang::Id);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, r#"{"plans": []}"#).unwrap();
        assert_eq!(load(&p).language, Lang::Id, "an old file has no language");
        let en = merge(&Settings::default(), serde_json::from_str(r#"{"language":"en"}"#).unwrap()).unwrap();
        assert_eq!(en.language, Lang::En);
        assert_eq!(serde_json::to_value(&en).unwrap()["language"], "en");
    }

    #[test]
    fn old_file_without_cr_key_enables_cursor_and_adds_no_plan() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, r#"{"enabledAgents": {"cc": true, "cx": false}, "plans": [], "cursorPlanSeeded": true}"#).unwrap();
        let s = load(&p);
        assert_eq!(s.enabled_agents.get(&Agent::Cursor), Some(&true));
        assert_eq!(s.enabled_agents.get(&Agent::Codex), Some(&false));
        assert!(s.plans.is_empty(), "loading never adds the Cursor plan");
        assert!(!s.cursor_plan_added);
    }

    #[test]
    fn first_connect_adds_cursor_plan_with_the_billing_day_once() {
        let base = Settings::default();
        let s = with_cursor_plan(&base, 7).unwrap();
        assert!(s.cursor_plan_added);
        let plan = s.plans.iter().find(|p| p.id == "cursor-pro").unwrap();
        assert_eq!(
            *plan,
            Plan { id: "cursor-pro".into(), name: "Cursor Pro".into(), price_usd: 20.0, renewal_day: 7, agents: vec![Agent::Cursor] }
        );
        assert_eq!(s.plans.len(), 3);
        assert_eq!(s.enabled_agents.get(&Agent::Cursor), Some(&true), "cr stays on by default");

        // The user deletes it; a later connect (even with another day) leaves the plans alone.
        let deleted = Settings { plans: base.plans.clone(), ..s };
        assert_eq!(with_cursor_plan(&deleted, 12), None);
    }

    #[test]
    fn first_connect_updates_an_existing_cursor_plan_instead_of_adding_a_second() {
        let mut cur = Settings::default();
        cur.plans.push(Plan { id: "cursor-pro".into(), name: "Mine".into(), price_usd: 40.0, renewal_day: 1, agents: vec![Agent::Cursor] });
        let s = with_cursor_plan(&cur, 7).unwrap();
        let ours: Vec<&Plan> = s.plans.iter().filter(|p| p.id == "cursor-pro").collect();
        assert_eq!(ours.len(), 1);
        assert_eq!((ours[0].renewal_day, ours[0].price_usd, ours[0].name.as_str()), (7, 40.0, "Mine"));
        assert!(s.cursor_plan_added);
    }

    #[test]
    fn connect_day_is_clamped_like_other_plans() {
        let s = with_cursor_plan(&Settings::default(), 0).unwrap();
        assert_eq!(s.plans.last().unwrap().renewal_day, 1);
        let s = with_cursor_plan(&Settings::default(), 40).unwrap();
        assert_eq!(s.plans.last().unwrap().renewal_day, 31);
    }

    #[test]
    fn plan_flag_survives_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        let s = with_cursor_plan(&Settings::default(), 7).unwrap();
        save(&p, &s).unwrap();
        assert_eq!(load(&p), s);
    }

    #[test]
    fn explicit_cursor_disabled_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, r#"{"enabledAgents": {"cr": false}}"#).unwrap();
        let s = load(&p);
        assert_eq!(s.enabled_agents.get(&Agent::Cursor), Some(&false));
        assert_eq!(s.plans, Settings::default().plans);
    }

    #[test]
    fn corrupt_file_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        assert_eq!(load(&p), Settings::default()); // missing
        std::fs::write(&p, "{ not json").unwrap();
        assert_eq!(load(&p), Settings::default());
    }

    #[test]
    fn one_invalid_field_falls_back_alone_and_keeps_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(
            &p,
            r#"{"theme": "sepia", "usdToIdr": "lots", "onboarded": true, "language": "en", "quotaDisplay": "used", "lastTab": "model", "plans": [{"agents": ["zz"]}]}"#,
        )
        .unwrap();
        let s = load(&p);
        assert_eq!(s.theme, Theme::System, "the bad enum falls back");
        assert_eq!(s.usd_to_idr, DEFAULT_USD_TO_IDR);
        assert!(s.plans.is_empty(), "the only plan here is broken, and it is dropped alone");
        assert!(s.onboarded, "Welcome must not come back");
        assert_eq!((s.language, s.quota_display), (Lang::En, QuotaDisplay::Used));
        assert_eq!(s.last_tab, "model");
        assert!(!dir.path().join("settings.json.bad").exists(), "a readable object is not set aside");
    }

    #[test]
    fn one_broken_plan_does_not_drop_the_other_plans() {
        let raw = r#"{"plans":[{"id":"a","name":"A","priceUsd":10},{"id":"b","priceUsd":"not a number"},{"id":"c","name":"C","priceUsd":20}]}"#;
        let ids: Vec<String> = parse_lenient(raw).unwrap().plans.into_iter().map(|p| p.id).collect();
        assert_eq!(ids, ["a", "c"]);
    }

    #[test]
    fn unreadable_file_is_copied_to_bad_before_defaults_take_over() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        assert_eq!(load(&p), Settings::default());
        assert!(!dir.path().join("settings.json.bad").exists(), "a missing file leaves nothing behind");
        std::fs::write(&p, "{ not json").unwrap();
        assert_eq!(load(&p), Settings::default());
        assert_eq!(std::fs::read_to_string(dir.path().join("settings.json.bad")).unwrap(), "{ not json");
        std::fs::write(&p, "[1, 2]").unwrap();
        assert_eq!(load(&p), Settings::default(), "not an object either");
    }

    #[test]
    fn save_load_roundtrip_creates_parent_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nested/cfg/settings.json");
        let mut s = Settings { usd_to_idr: 17000.0, onboarded: true, ..Settings::default() };
        s.enabled_agents.insert(Agent::Hermes, false);
        save(&p, &s).unwrap();
        assert_eq!(load(&p), s);
        let n = std::fs::read_dir(p.parent().unwrap()).unwrap().count();
        assert_eq!(n, 1);
    }

    #[test]
    fn partial_update_keeps_other_fields() {
        let mut cur = Settings { usd_to_idr: 17000.0, ..Settings::default() };
        cur.enabled_agents.insert(Agent::Hermes, false);
        let patch = |v: serde_json::Value| v.as_object().unwrap().clone();

        let s = merge(&cur, patch(serde_json::json!({ "lastTab": "model", "notASetting": 1 }))).unwrap();
        assert_eq!(s.last_tab, "model");
        assert_eq!(Settings { last_tab: cur.last_tab.clone(), ..s }, cur);

        let s = merge(&cur, patch(serde_json::json!({ "enabledAgents": { "cx": false } }))).unwrap();
        assert_eq!(s.enabled_agents.len(), 1, "a field is replaced whole, not merged key by key");

        assert!(merge(&cur, patch(serde_json::json!({ "usdToIdr": "lots" }))).is_err());
    }

    #[test]
    fn json_shape_is_camel_case_with_agent_keys() {
        let v = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(v["usdToIdr"], 16300.0);
        assert_eq!(v["enabledAgents"]["cc"], true);
        assert_eq!(v["enabledAgents"]["hm"], true);
        assert_eq!(v["plans"][0]["priceUsd"], 100.0);
        assert_eq!(v["plans"][0]["renewalDay"], 1);
        assert_eq!(v["plans"][0]["agents"][0], "cc");
        assert_eq!(v["lastTab"], "ringkasan");
        let back: Settings = serde_json::from_value(v).unwrap();
        assert_eq!(back, Settings::default());
    }

    #[test]
    fn old_file_with_missing_and_unknown_fields_loads() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, r#"{"usdToIdr": 16000, "futureField": [1,2]}"#).unwrap();
        let s = load(&p);
        assert_eq!(s.usd_to_idr, 16000.0);
        assert_eq!(s.plans, Settings::default().plans);
        assert_eq!(s.last_tab, "ringkasan");
    }

    #[test]
    fn display_preferences_use_lowercase_names_and_old_files_get_defaults() {
        let v = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!((v["theme"].as_str(), v["quotaDisplay"].as_str(), v["resetDisplay"].as_str()), (Some("system"), Some("left"), Some("clock")));

        let patch = serde_json::json!({ "theme": "dark", "quotaDisplay": "used", "resetDisplay": "countdown" });
        let s = merge(&Settings::default(), patch.as_object().unwrap().clone()).unwrap();
        assert_eq!((s.theme, s.quota_display, s.reset_display), (Theme::Dark, QuotaDisplay::Used, ResetDisplay::Countdown));

        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, r#"{"usdToIdr": 16000, "onboarded": true, "lastTab": "model"}"#).unwrap();
        let s = load(&p);
        assert!(s.onboarded);
        assert_eq!((s.theme, s.quota_display, s.reset_display), (Theme::System, QuotaDisplay::Left, ResetDisplay::Clock));
    }

    #[test]
    fn notification_and_privacy_switches_default_off_and_old_files_load() {
        let s = Settings::default();
        assert!(!s.notify_low_quota && !s.notify_projected_exhaustion && !s.hide_on_screen_share);
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!((&v["notifyLowQuota"], &v["notifyProjectedExhaustion"], &v["hideOnScreenShare"]), (&false.into(), &false.into(), &false.into()));

        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, r#"{"onboarded": true}"#).unwrap();
        let old = load(&p);
        assert!(old.onboarded && !old.notify_low_quota && !old.hide_on_screen_share);

        let patch = serde_json::json!({ "notifyLowQuota": true, "hideOnScreenShare": true });
        let s = merge(&Settings::default(), patch.as_object().unwrap().clone()).unwrap();
        assert!(s.notify_low_quota && s.hide_on_screen_share && !s.notify_projected_exhaustion);
    }

    #[test]
    fn openrouter_and_antigravity_default_on_and_old_files_load() {
        let s = Settings::default();
        assert!(s.openrouter_enabled && s.antigravity_enabled);
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!((&v["openrouterEnabled"], &v["antigravityEnabled"]), (&true.into(), &true.into()));
        assert!(!s.enabled_agents.keys().any(|a| format!("{a:?}").contains("Router")), "neither is an Agent");

        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, r#"{"onboarded": true}"#).unwrap();
        let old = load(&p);
        assert!(old.openrouter_enabled && old.antigravity_enabled);

        let patch = serde_json::json!({ "openrouterEnabled": false });
        let s = merge(&Settings::default(), patch.as_object().unwrap().clone()).unwrap();
        assert!(!s.openrouter_enabled && s.antigravity_enabled);
    }

    #[test]
    fn load_sanitizes_values() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(
            &p,
            r#"{"usdToIdr": -5, "plans":[{"id":"a","name":"A","priceUsd":10,"renewalDay":0,"agents":["cc"]},{"id":"b","name":"B","priceUsd":10,"renewalDay":99,"agents":[]}]}"#,
        )
        .unwrap();
        let s = load(&p);
        assert_eq!(s.usd_to_idr, 16300.0);
        assert_eq!(s.plans[0].renewal_day, 1);
        assert_eq!(s.plans[1].renewal_day, 31);
    }

    #[test]
    fn cycle_on_renewal_day_starts_today() {
        let c = current_cycle(12, d(2026, 11, 12));
        assert_eq!(c.start, d(2026, 11, 12));
        assert_eq!(c.end_exclusive, d(2026, 12, 12));
        assert_eq!(c.day_of_cycle, 1);
        assert_eq!(c.length, 30);
    }

    #[test]
    fn cycle_before_renewal_day_uses_previous_month() {
        let c = current_cycle(12, d(2026, 11, 5));
        assert_eq!(c.start, d(2026, 10, 12));
        assert_eq!(c.end_exclusive, d(2026, 11, 12));
        assert_eq!(c.length, 31);
        assert_eq!(c.day_of_cycle, 25);
    }

    #[test]
    fn renewal_31_in_february() {
        let c = current_cycle(31, d(2027, 2, 28));
        assert_eq!(c.start, d(2027, 2, 28));
        assert_eq!(c.end_exclusive, d(2027, 3, 31));
        assert_eq!(c.day_of_cycle, 1);
    }

    #[test]
    fn renewal_31_before_short_month_end_uses_previous_long_month() {
        let c = current_cycle(31, d(2027, 2, 27));
        assert_eq!(c.start, d(2027, 1, 31));
        assert_eq!(c.end_exclusive, d(2027, 2, 28));
    }

    #[test]
    fn cycle_wraps_year_boundary() {
        let c = current_cycle(15, d(2027, 1, 3));
        assert_eq!(c.start, d(2026, 12, 15));
        assert_eq!(c.end_exclusive, d(2027, 1, 15));
        let c = current_cycle(20, d(2026, 12, 25));
        assert_eq!(c.end_exclusive, d(2027, 1, 20));
    }

    #[test]
    fn cycle_serializes_dates_camel_case() {
        let v = serde_json::to_value(current_cycle(12, d(2026, 11, 12))).unwrap();
        assert_eq!(v["start"], "2026-11-12");
        assert_eq!(v["endExclusive"], "2026-12-12");
        assert_eq!(v["dayOfCycle"], 1);
        assert_eq!(v["length"], 30);
    }
}

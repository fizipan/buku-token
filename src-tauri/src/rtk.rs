use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtkDay {
    pub date: NaiveDate,
    pub saved_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RtkSummary {
    pub saved_tokens: u64,
    pub approx_usd: Option<f64>,
    pub model: Option<String>,
}

#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    daily: Vec<RawDay>,
}

#[derive(Deserialize)]
struct RawDay {
    date: NaiveDate,
    #[serde(default)]
    saved_tokens: u64,
}

pub fn parse_daily(json: &str) -> Result<Vec<RtkDay>> {
    let raw: Raw = serde_json::from_str(json)?;
    Ok(raw
        .daily
        .into_iter()
        .map(|d| RtkDay { date: d.date, saved_tokens: d.saved_tokens })
        .collect())
}

/// The row for `today`, or a zero row when rtk has nothing for that date.
pub fn today_row(days: &[RtkDay], today: NaiveDate) -> RtkDay {
    days.iter()
        .find(|d| d.date == today)
        .cloned()
        .unwrap_or(RtkDay { date: today, saved_tokens: 0 })
}

pub fn approx_usd(saved: u64, input_price_per_token: Option<f64>) -> Option<f64> {
    input_price_per_token.map(|p| saved as f64 * p)
}

const TIMEOUT: Duration = Duration::from_secs(5);

fn candidates() -> Vec<PathBuf> {
    let mut v = vec![
        PathBuf::from("rtk"),
        PathBuf::from("/opt/homebrew/bin/rtk"),
        PathBuf::from("/usr/local/bin/rtk"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        v.push(PathBuf::from(home).join(".cargo/bin/rtk"));
    }
    v
}

/// Runs `bin gain --daily --format json`. `Err(NotFound)` means try the next candidate.
fn run(bin: &Path) -> std::io::Result<Option<String>> {
    let mut child = Command::new(bin)
        .args(["gain", "--daily", "--format", "json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut out = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break Some(st);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let text = reader.join().unwrap_or_default();
    Ok(status.filter(|s| s.success()).map(|_| text))
}

/// Thin wrapper around the real `rtk` binary. `None` if missing, failing, slow or unparsable.
pub fn read_today(today: NaiveDate) -> Option<RtkDay> {
    for bin in candidates() {
        match run(&bin) {
            Ok(Some(text)) => {
                return parse_daily(&text).ok().map(|d| today_row(&d, today));
            }
            Ok(None) => return None,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{"summary":{"total_commands":45887,"total_input":444103104,"total_output":430720039,"total_saved":13840862,"avg_savings_pct":3.1165875390954256,"total_time_ms":96285703,"avg_time_ms":2098},
 "daily":[{"date":"2026-08-06","commands":242,"input_tokens":112398,"output_tokens":65932,"saved_tokens":47800,"savings_pct":42.527447107599784,"total_time_ms":123105,"avg_time_ms":508},
          {"date":"2026-08-07","commands":286,"input_tokens":738761,"output_tokens":54789,"saved_tokens":684144,"savings_pct":92.60694595410423,"total_time_ms":1040639,"avg_time_ms":3638}]}"#;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn rtk_parses_daily_rows() {
        let days = parse_daily(FIXTURE).unwrap();
        assert_eq!(
            days,
            vec![
                RtkDay { date: d("2026-08-06"), saved_tokens: 47800 },
                RtkDay { date: d("2026-08-07"), saved_tokens: 684144 },
            ]
        );
    }

    #[test]
    fn missing_daily_is_empty_and_garbage_is_error() {
        assert!(parse_daily(r#"{"summary":{}}"#).unwrap().is_empty());
        assert!(parse_daily("not json").is_err());
    }

    #[test]
    fn rtk_picks_today_row() {
        let days = parse_daily(FIXTURE).unwrap();
        assert_eq!(today_row(&days, d("2026-08-07")).saved_tokens, 684144);
    }

    #[test]
    fn rtk_missing_today_is_zero() {
        let days = parse_daily(FIXTURE).unwrap();
        let r = today_row(&days, d("2026-08-08"));
        assert_eq!(r, RtkDay { date: d("2026-08-08"), saved_tokens: 0 });
    }

    #[test]
    fn approx_usd_uses_input_price() {
        let v = approx_usd(2_100_000, Some(3e-6)).unwrap();
        assert!((v - 6.3).abs() < 1e-9);
        assert_eq!(approx_usd(100, None), None);
    }

    #[test]
    fn summary_serializes_camel_case() {
        let s = RtkSummary { saved_tokens: 5, approx_usd: None, model: Some("m".into()) };
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(j, r#"{"savedTokens":5,"approxUsd":null,"model":"m"}"#);
    }
}

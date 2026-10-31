//! Date and number labels shown verbatim by the UI and the share image (numbers mirror
//! `src/lib/format.ts`). Indonesian: `1.234,56`, `31 Okt`, `1,24 jt`. English: `1,234.56`, `Oct 31`, `1.24M`.

use chrono::{Datelike, NaiveDate};

use crate::settings::Lang;

const DAYS_ID: [&str; 7] = ["Min", "Sen", "Sel", "Rab", "Kam", "Jum", "Sab"];
const MONTHS_ID: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];
const DAYS_EN: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS_EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// "Jum, 30 Okt" or "Fri, Oct 30".
pub fn short_date(d: NaiveDate, lang: Lang) -> String {
    let wd = d.weekday().num_days_from_sunday() as usize;
    let m = d.month0() as usize;
    match lang {
        Lang::Id => format!("{}, {} {}", DAYS_ID[wd], d.day(), MONTHS_ID[m]),
        Lang::En => format!("{}, {} {}", DAYS_EN[wd], MONTHS_EN[m], d.day()),
    }
}

/// "Hari Ini · Jum, 30 Okt", "Kemarin · Kam, 29 Okt", older days "Rab, 28 Okt"; "Today · Fri, Oct 30" and "Yesterday · ..." in English.
pub fn day_label(d: NaiveDate, today: NaiveDate, lang: Lang) -> String {
    let (now, before) = match lang {
        Lang::Id => ("Hari Ini", "Kemarin"),
        Lang::En => ("Today", "Yesterday"),
    };
    if d == today {
        format!("{now} · {}", short_date(d, lang))
    } else if today.pred_opt() == Some(d) {
        format!("{before} · {}", short_date(d, lang))
    } else {
        short_date(d, lang)
    }
}

/// `v` rounded half away from zero to `digits` decimals, like `Intl.NumberFormat`.
fn round_to(v: f64, digits: i32) -> f64 {
    let k = 10f64.powi(digits);
    (v * k).round() / k
}

/// "1.234,5" (Indonesian) or "1,234.5" (English): at most `max_frac` decimals, trailing zeros dropped.
fn decimal(v: f64, max_frac: usize, lang: Lang) -> String {
    let (group, point) = match lang {
        Lang::Id => ('.', ','),
        Lang::En => (',', '.'),
    };
    let text = format!("{:.max_frac$}", round_to(v, max_frac as i32));
    let (int, frac) = text.split_once('.').unwrap_or((&text, ""));
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(group);
        }
        grouped.push(c);
    }
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() { grouped } else { format!("{grouped}{point}{frac}") }
}

/// "1.234".
pub fn int(n: u64, lang: Lang) -> String {
    decimal(n as f64, 0, lang)
}

/// "$1.842,30" or "$1,842.30".
pub fn usd(v: f64, lang: Lang) -> String {
    let text = format!("{:.2}", round_to(v, 2));
    let (int, frac) = text.split_once('.').expect("two decimals");
    let point = if lang == Lang::Id { ',' } else { '.' };
    format!("${}{point}{frac}", decimal(int.parse().unwrap_or(0.0), 0, lang))
}

/// "$2.741" or "$2,741".
pub fn usd0(v: f64, lang: Lang) -> String {
    format!("${}", decimal(v, 0, lang))
}

/// "5,2 M", "1,24 jt", "612 rb", "980" in Indonesian; "5.2B", "1.24M", "612K", "980" in English.
/// The unit is chosen after rounding.
pub fn tokens(n: u64, lang: Lang) -> String {
    let n = n as f64;
    let (billion, million, thousand) = match lang {
        Lang::Id => (" M", " jt", " rb"),
        Lang::En => ("B", "M", "K"),
    };
    if round_to(n / 1e6, 2) >= 1000.0 {
        format!("{}{billion}", decimal(n / 1e9, 2, lang))
    } else if round_to(n / 1e3, 0) >= 1000.0 {
        format!("{}{million}", decimal(n / 1e6, 2, lang))
    } else if round_to(n, 0) >= 1000.0 {
        format!("{}{thousand}", decimal(n / 1e3, 0, lang))
    } else {
        decimal(n, 0, lang)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: Lang = Lang::Id;
    const EN: Lang = Lang::En;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn labels_today_yesterday_and_older() {
        let today = d(2026, 10, 30);
        assert_eq!(day_label(today, today, ID), "Hari Ini · Jum, 30 Okt");
        assert_eq!(day_label(d(2026, 10, 29), today, ID), "Kemarin · Kam, 29 Okt");
        assert_eq!(day_label(d(2026, 10, 28), today, ID), "Rab, 28 Okt");
        assert_eq!(day_label(today, today, EN), "Today · Fri, Oct 30");
        assert_eq!(day_label(d(2026, 10, 29), today, EN), "Yesterday · Thu, Oct 29");
        assert_eq!(day_label(d(2026, 10, 28), today, EN), "Wed, Oct 28");
    }

    #[test]
    fn yesterday_across_month_and_year() {
        assert_eq!(day_label(d(2026, 10, 31), d(2026, 11, 1), ID), "Kemarin · Sab, 31 Okt");
        assert_eq!(day_label(d(2026, 12, 31), d(2027, 1, 1), ID), "Kemarin · Kam, 31 Des");
        assert_eq!(day_label(d(2026, 12, 31), d(2027, 1, 1), EN), "Yesterday · Thu, Dec 31");
    }

    #[test]
    fn every_day_abbreviation() {
        // 25 Okt 2026 is a Sunday.
        let got: Vec<String> = (25..=31).map(|day| short_date(d(2026, 10, day), ID)).collect();
        let want = ["Min, 25 Okt", "Sen, 26 Okt", "Sel, 27 Okt", "Rab, 28 Okt", "Kam, 29 Okt", "Jum, 30 Okt", "Sab, 31 Okt"];
        assert_eq!(got, want);
        let got: Vec<String> = (25..=31).map(|day| short_date(d(2026, 10, day), EN)).collect();
        let want = ["Sun, Oct 25", "Mon, Oct 26", "Tue, Oct 27", "Wed, Oct 28", "Thu, Oct 29", "Fri, Oct 30", "Sat, Oct 31"];
        assert_eq!(got, want);
    }

    #[test]
    fn every_month_abbreviation() {
        let got: Vec<String> = (1..=12).map(|m| short_date(d(2026, m, 1), ID).split(' ').nth(2).unwrap().to_string()).collect();
        let want = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];
        assert_eq!(got, want);
        let got: Vec<String> = (1..=12).map(|m| short_date(d(2026, m, 1), EN).split(' ').nth(1).unwrap().to_string()).collect();
        let want = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        assert_eq!(got, want);
    }

    #[test]
    fn usd_follows_the_language_separators() {
        assert_eq!(usd(1842.3, ID), "$1.842,30");
        assert_eq!(usd(0.5, ID), "$0,50");
        assert_eq!(usd(1234567.891, ID), "$1.234.567,89");
        assert_eq!(usd0(2741.4, ID), "$2.741");
        assert_eq!(usd0(0.4, ID), "$0");
        assert_eq!(usd(1234.56, EN), "$1,234.56");
        assert_eq!(usd(0.5, EN), "$0.50");
        assert_eq!(usd(1234567.891, EN), "$1,234,567.89");
        assert_eq!(usd0(2741.4, EN), "$2,741");
        assert_eq!(int(1234, ID), "1.234");
        assert_eq!(int(1234, EN), "1,234");
    }

    #[test]
    fn tokens_pick_the_unit_after_rounding() {
        let nums = [980, 999, 1000, 612_000, 999_499, 999_500, 1_240_000, 212_000_000, 999_996_000, 5_200_000_000];
        let got: Vec<String> = nums.iter().map(|n| tokens(*n, ID)).collect();
        assert_eq!(got, ["980", "999", "1 rb", "612 rb", "999 rb", "1 jt", "1,24 jt", "212 jt", "1 M", "5,2 M"]);
        let got: Vec<String> = nums.iter().map(|n| tokens(*n, EN)).collect();
        assert_eq!(got, ["980", "999", "1K", "612K", "999K", "1M", "1.24M", "212M", "1B", "5.2B"]);
        assert_eq!(tokens(0, ID), "0");
        assert_eq!(tokens(1_840_000, EN), "1.84M");
    }
}

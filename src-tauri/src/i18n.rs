//! Texts that are born in Rust, in Indonesian and English: quota window labels and the reasons a quota is
//! unavailable, the menu bar menu, the share image, notifications and the like. The UI has its own
//! dictionary (`src/lib/i18n`). Nothing here is stored: SQLite keeps ids, never a label in a language, and
//! every text is made at the moment it is shown from the current `Settings::language`.

use crate::quota::claude::QuotaError;
use crate::settings::Lang;

fn pick(lang: Lang, id: &'static str, en: &'static str) -> &'static str {
    match lang {
        Lang::Id => id,
        Lang::En => en,
    }
}

/// Who an unavailable reason is about: each service words its own errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    Claude,
    Cursor,
    OpenRouter,
}

/// Why a quota or balance is unavailable, as a short fixed text.
pub fn quota_reason(service: Service, e: &QuotaError, lang: Lang) -> String {
    use QuotaError::{Expired, Http, KeychainDenied, KeychainFailed, NotFound, Offline, Parse};
    match (service, e) {
        (Service::Claude, Expired) => pick(lang, "Buka Claude Code untuk memperbarui login", "Open Claude Code to refresh your login").into(),
        (Service::Claude, KeychainDenied) => pick(lang, "Izin Keychain ditolak", "Keychain permission denied").into(),
        (Service::Claude, KeychainFailed) => pick(lang, "Keychain tidak bisa dibaca", "Keychain could not be read").into(),
        (Service::Claude, Offline) => pick(lang, "Sedang offline", "Offline").into(),
        (Service::Claude, NotFound) => pick(lang, "Login Claude Code tidak ditemukan", "Claude Code login not found").into(),
        (Service::Claude, Http(c)) => match lang {
            Lang::Id => format!("Server kuota menjawab HTTP {c}"),
            Lang::En => format!("Quota server replied HTTP {c}"),
        },
        (Service::Claude, Parse) => pick(lang, "Respons kuota tidak dikenali", "Quota response not recognized").into(),
        (Service::Cursor, Expired) => pick(lang, "Sesi Cursor Habis, Tempel Ulang Di Pengaturan", "Cursor Session Expired, Paste Again In Settings").into(),
        (Service::OpenRouter, Expired) => {
            pick(lang, "Key OpenRouter Ditolak, Tempel Ulang Di Pengaturan", "OpenRouter Key Rejected, Paste Again In Settings").into()
        }
        (_, KeychainDenied) => pick(lang, "Akses Keychain Ditolak", "Keychain Access Denied").into(),
        (_, KeychainFailed) => pick(lang, "Keychain Tidak Bisa Dibaca", "Keychain Could Not Be Read").into(),
        (_, Offline) => pick(lang, "Sedang Offline", "Offline").into(),
        // NotFound is never produced for Cursor or OpenRouter: no cookie or key means no row at all.
        (Service::Cursor, Http(c)) => match lang {
            Lang::Id => format!("Server Cursor Menjawab HTTP {c}"),
            Lang::En => format!("Cursor Server Replied HTTP {c}"),
        },
        (Service::OpenRouter, Http(c)) => match lang {
            Lang::Id => format!("Server OpenRouter Menjawab HTTP {c}"),
            Lang::En => format!("OpenRouter Server Replied HTTP {c}"),
        },
        (Service::Cursor, Parse | NotFound) => pick(lang, "Respons Cursor Tidak Dikenali", "Cursor Response Not Recognized").into(),
        (Service::OpenRouter, Parse | NotFound) => pick(lang, "Respons OpenRouter Tidak Dikenali", "OpenRouter Response Not Recognized").into(),
    }
}

/// Putuskan could not clear the Keychain item, so the credential may still be stored.
pub fn disconnect_failed(service: Service, lang: Lang) -> String {
    let (id, en) = match service {
        Service::OpenRouter => ("Key OpenRouter mungkin masih tersimpan di Keychain. Coba Putuskan lagi.", "The OpenRouter key may still be stored in the Keychain. Try Disconnect again."),
        _ => ("Cookie Cursor mungkin masih tersimpan di Keychain. Coba Putuskan lagi.", "The Cursor cookie may still be stored in the Keychain. Try Disconnect again."),
    };
    pick(lang, id, en).into()
}

/// The pasted Cursor cookie did not pass the checks, or the server refused it.
pub fn invalid_cookie(lang: Lang) -> &'static str {
    pick(lang, "Cookie Tidak Valid", "Invalid Cookie")
}

/// The pasted OpenRouter key did not pass the checks, or the server refused it.
pub fn invalid_key(lang: Lang) -> &'static str {
    pick(lang, "Key Tidak Valid", "Invalid Key")
}

/// Another connect or a disconnect finished while this one waited for the server.
pub fn connect_stale(lang: Lang) -> &'static str {
    pick(lang, "Koneksi Dibatalkan, Coba Lagi", "Connection Cancelled, Try Again")
}

/// The label of a quota window, made from its id. Never stored: the id is what is kept.
pub fn window_label(id: &str, lang: Lang) -> String {
    match id {
        "five_hour" => pick(lang, "Sesi 5 jam", "5h Session").into(),
        "seven_day" => pick(lang, "Mingguan", "Weekly").into(),
        "seven_day_opus" => pick(lang, "Mingguan Opus", "Weekly Opus").into(),
        "seven_day_sonnet" => pick(lang, "Mingguan Sonnet", "Weekly Sonnet").into(),
        "total" => "Total".into(),
        "auto" => "Auto".into(),
        "api" => "API".into(),
        other => match other.strip_prefix("window_").filter(|m| !m.is_empty() && m.bytes().all(|b| b.is_ascii_digit())) {
            Some(m) => match lang {
                Lang::Id => format!("{m} menit"),
                Lang::En => format!("{m} min"),
            },
            None => other.to_string(),
        },
    }
}

/// A session or usage with no project.
pub fn no_project(lang: Lang) -> &'static str {
    pick(lang, "Tanpa Project", "No Project")
}

/// The row that folds the projects beyond the top six.
pub fn other_projects(n: usize, lang: Lang) -> String {
    match (lang, n) {
        (Lang::Id, n) => format!("{n} Project Lainnya"),
        (Lang::En, 1) => "1 Other Project".into(),
        (Lang::En, n) => format!("{n} Other Projects"),
    }
}

/// The menu of the menu bar item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrayMenuText {
    pub refresh: &'static str,
    pub settings: &'static str,
    pub quit: &'static str,
}

pub fn tray_menu(lang: Lang) -> TrayMenuText {
    TrayMenuText {
        refresh: pick(lang, "Perbarui", "Refresh"),
        settings: pick(lang, "Pengaturan…", "Settings…"),
        quit: pick(lang, "Keluar Buku Token", "Quit Buku Token"),
    }
}

/// Title of the settings window.
pub fn settings_title(lang: Lang) -> &'static str {
    pick(lang, "Pengaturan Buku Token", "Buku Token Settings")
}

/// Texts of the share card. The card never carries a project name, path or conversation text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShareText {
    pub title: &'static str,
    pub no_usage: &'static str,
    pub tokens: &'static str,
    pub api_equivalent: &'static str,
    pub total_tokens: &'static str,
    pub sessions: &'static str,
    pub top_model: &'static str,
    pub made_with: &'static str,
    pub other_model: &'static str,
}

pub fn share(lang: Lang) -> ShareText {
    ShareText {
        title: pick(lang, "Pemakaian AI-ku", "My AI Usage"),
        no_usage: pick(lang, "Belum Ada Pemakaian", "No Usage Yet"),
        tokens: pick(lang, "Token", "Tokens"),
        api_equivalent: pick(lang, "Setara API", "API Equivalent"),
        total_tokens: pick(lang, "Total Token", "Total Tokens"),
        sessions: pick(lang, "Sesi", "Sessions"),
        top_model: pick(lang, "Model Teratas", "Top Model"),
        made_with: pick(lang, "Dibuat dengan Buku Token", "Made With Buku Token"),
        other_model: pick(lang, "Model Lain", "Other Model"),
    }
}

/// The pill on the share card.
pub fn share_period(period: crate::aggregate::Period, lang: Lang) -> &'static str {
    use crate::aggregate::Period;
    match period {
        Period::Today => pick(lang, "Hari Ini", "Today"),
        Period::Last7 => pick(lang, "7 Hari Terakhir", "Last 7 Days"),
        Period::Last14 => pick(lang, "14 Hari Terakhir", "Last 14 Days"),
        Period::Last30 => pick(lang, "30 Hari Terakhir", "Last 30 Days"),
    }
}

/// Fixed texts of the share commands: an error never carries a file path.
pub fn share_render_error(lang: Lang) -> &'static str {
    pick(lang, "Gagal membuat gambar", "Could not create the image")
}

pub fn share_copy_error(lang: Lang) -> &'static str {
    pick(lang, "Gagal menyalin gambar", "Could not copy the image")
}

pub fn share_save_error(lang: Lang) -> &'static str {
    pick(lang, "Gagal menyimpan gambar ke Downloads", "Could not save the image to Downloads")
}

/// Notification texts. A notification carries only the agent name, the window label and a figure.
pub fn low_quota_body(label: &str, left: f64, lang: Lang) -> String {
    match lang {
        Lang::Id => format!("{label} · sisa {left}%"),
        Lang::En => format!("{label} · {left}% left"),
    }
}

pub fn estimate_body(label: &str, clock: &str, lang: Lang) -> String {
    match lang {
        Lang::Id => format!("{label} · perkiraan habis pukul {clock}"),
        Lang::En => format!("{label} · estimated to run out at {clock}"),
    }
}

pub fn test_body(lang: Lang) -> &'static str {
    pick(lang, "Ini notifikasi percobaan.", "This is a test notification.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::Period;

    const SERVICES: [Service; 3] = [Service::Claude, Service::Cursor, Service::OpenRouter];
    const ERRORS: [QuotaError; 8] = [
        QuotaError::Expired,
        QuotaError::KeychainDenied,
        QuotaError::KeychainFailed,
        QuotaError::NotFound,
        QuotaError::Offline,
        QuotaError::Http(503),
        QuotaError::Http(429),
        QuotaError::Parse,
    ];

    #[test]
    fn a_failed_disconnect_says_the_secret_may_still_be_stored() {
        for s in [Service::Cursor, Service::OpenRouter] {
            let (id, en) = (disconnect_failed(s, Lang::Id), disconnect_failed(s, Lang::En));
            assert!(id.contains("masih tersimpan") && id.contains("lagi"), "{id}");
            assert!(en.contains("may still be stored") && en.contains("again"), "{en}");
        }
    }

    #[test]
    fn every_reason_exists_in_both_languages_and_differs() {
        for s in SERVICES {
            for e in &ERRORS {
                let (id, en) = (quota_reason(s, e, Lang::Id), quota_reason(s, e, Lang::En));
                assert!(!id.is_empty() && !en.is_empty());
                assert!(id != en, "{s:?} {e:?}: {id}");
            }
        }
    }

    #[test]
    fn english_reasons_carry_no_indonesian() {
        let banned = ["Sedang", "Ditolak", "ditolak", "Tempel", "Tidak", "tidak", "Menjawab", "menjawab", "Respons ", "respons ", "Buka", "Sesi"];
        for s in SERVICES {
            for e in &ERRORS {
                let en = quota_reason(s, e, Lang::En);
                assert!(banned.iter().all(|w| !en.contains(w)), "{s:?} {e:?}: {en}");
            }
        }
        assert_eq!(quota_reason(Service::Cursor, &QuotaError::Http(503), Lang::En), "Cursor Server Replied HTTP 503");
        assert_eq!(quota_reason(Service::OpenRouter, &QuotaError::Expired, Lang::En), "OpenRouter Key Rejected, Paste Again In Settings");
        assert_eq!(quota_reason(Service::Claude, &QuotaError::KeychainDenied, Lang::En), "Keychain permission denied");
    }

    #[test]
    fn window_labels_come_from_ids_in_both_languages() {
        let ids = ["five_hour", "seven_day", "seven_day_opus", "seven_day_sonnet", "total", "auto", "api", "window_60"];
        let id: Vec<String> = ids.iter().map(|i| window_label(i, Lang::Id)).collect();
        let en: Vec<String> = ids.iter().map(|i| window_label(i, Lang::En)).collect();
        assert_eq!(id, ["Sesi 5 jam", "Mingguan", "Mingguan Opus", "Mingguan Sonnet", "Total", "Auto", "API", "60 menit"]);
        assert_eq!(en, ["5h Session", "Weekly", "Weekly Opus", "Weekly Sonnet", "Total", "Auto", "API", "60 min"]);
        assert_eq!(window_label("something_new", Lang::En), "something_new");
        assert_eq!(window_label("window_", Lang::En), "window_");
    }

    #[test]
    fn project_texts_follow_the_language() {
        assert_eq!((no_project(Lang::Id), no_project(Lang::En)), ("Tanpa Project", "No Project"));
        assert_eq!(other_projects(2, Lang::Id), "2 Project Lainnya");
        assert_eq!(other_projects(1, Lang::En), "1 Other Project");
        assert_eq!(other_projects(3, Lang::En), "3 Other Projects");
    }

    #[test]
    fn tray_menu_texts_differ_per_language() {
        let id = tray_menu(Lang::Id);
        assert_eq!((id.refresh, id.settings, id.quit), ("Perbarui", "Pengaturan…", "Keluar Buku Token"));
        let en = tray_menu(Lang::En);
        assert_eq!((en.refresh, en.settings, en.quit), ("Refresh", "Settings…", "Quit Buku Token"));
        assert_eq!(settings_title(Lang::En), "Buku Token Settings");
    }

    #[test]
    fn share_and_notification_texts_exist_in_both_languages() {
        for lang in [Lang::Id, Lang::En] {
            let s = share(lang);
            for t in [s.title, s.no_usage, s.tokens, s.api_equivalent, s.total_tokens, s.sessions, s.top_model, s.made_with, s.other_model] {
                assert!(!t.is_empty());
            }
            for p in [Period::Today, Period::Last7, Period::Last14, Period::Last30] {
                assert!(!share_period(p, lang).is_empty());
            }
        }
        assert_eq!(share(Lang::En).other_model, "Other Model");
        assert_eq!(share_period(Period::Last7, Lang::En), "Last 7 Days");
        assert_eq!(low_quota_body("Weekly", 12.0, Lang::En), "Weekly · 12% left");
        assert_eq!(low_quota_body("Mingguan", 12.0, Lang::Id), "Mingguan · sisa 12%");
        assert_eq!(estimate_body("5h Session", "16:23", Lang::En), "5h Session · estimated to run out at 16:23");
    }
}

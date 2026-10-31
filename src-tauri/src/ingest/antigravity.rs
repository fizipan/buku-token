//! Antigravity activity from file names, modification times and one summaries table.
//!
//! The conversation folders hold the tool's own data, and `~/.gemini` also holds its token file.
//! No conversation file is opened or read: the counting takes `(name, modified_at)` pairs and
//! `Summary` values (id, step count, date), so it cannot reach content, the lister only calls
//! `read_dir` and `metadata`, and the one database read selects three named columns.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Duration, FixedOffset, Utc};
use serde::Serialize;

/// Under the home folder. The same conversation can show up in both.
pub const FOLDERS: [&str; 2] = [".gemini/antigravity/conversations", ".gemini/antigravity-ide/conversations"];

pub fn folders(home: &Path) -> Vec<PathBuf> {
    FOLDERS.iter().map(|f| home.join(f)).collect()
}

/// The CLI keeps one summary row per conversation here.
pub const SUMMARIES: &str = ".gemini/antigravity-cli/conversation_summaries.db";

/// A file's name and when it was last changed: all the counting ever sees of a file.
pub type Stamped = (String, DateTime<Utc>);

/// What is kept of a summary row: the id, the step count and the date. Nothing else is selected.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Summary {
    pub id: String,
    pub steps: u64,
    /// `None` for the placeholder date the CLI writes for sessions it has no time for.
    pub modified: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    /// Conversations changed today (local calendar day).
    pub today: u32,
    /// Conversations changed today or in the 6 days before (7 local calendar days).
    pub last_7_days: u32,
    /// The latest real change time of any conversation.
    pub last_active: Option<DateTime<Utc>>,
    /// Every conversation known, from the `.pb` files and the summaries together.
    pub total_sessions: u32,
    pub total_steps: u64,
}

/// A date before this is the CLI's placeholder, not a time.
fn real_date(t: DateTime<Utc>) -> Option<DateTime<Utc>> {
    (t.year() >= 2000).then_some(t)
}

fn parse_date(text: &str) -> Option<DateTime<Utc>> {
    ["%Y-%m-%d %H:%M:%S%.f%:z", "%Y-%m-%d %H:%M:%S%:z"]
        .iter()
        .find_map(|f| DateTime::parse_from_str(text.trim(), f).ok())
        .and_then(|t| real_date(t.to_utc()))
}

/// `%`-escapes a path for a `file:` URI (a space, `?` or `#` would otherwise end or split it).
fn uri_path(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str()
        .as_bytes()
        .iter()
        .map(|b| if b.is_ascii_alphanumeric() || b"/-_.~".contains(b) { (*b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// Id, step count and date of every summary row; empty when the file is missing, is not a
/// database or has another schema. The database is opened read-only and immutable (no journal or
/// lock file is created next to it), and the query names its three columns: any other column
/// stays unread.
pub fn read_summaries(db: &Path) -> Vec<Summary> {
    summaries_from(db).unwrap_or_default()
}

fn summaries_from(db: &Path) -> rusqlite::Result<Vec<Summary>> {
    use rusqlite::{Connection, OpenFlags};
    if !db.is_file() {
        return Ok(Vec::new());
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(format!("file:{}?immutable=1", uri_path(db)), flags)?;
    let mut stmt = conn.prepare("SELECT conversation_id, step_count, last_modified_time FROM conversation_summaries")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, Option<String>>(2)?)))?;
    Ok(rows
        .flatten()
        .map(|(id, steps, when)| Summary { id, steps: steps.unwrap_or(0).max(0) as u64, modified: when.as_deref().and_then(parse_date) })
        .collect())
}

/// File names and modification times of the `.pb` conversations directly inside the given
/// folders. A missing folder or an entry that cannot be statted is skipped. Only regular `*.pb`
/// files count: not sidecar databases, dotfiles or symlinks. `entry.metadata()` is an `lstat`,
/// never an open.
pub fn list(folders: &[PathBuf]) -> Vec<Stamped> {
    folders
        .iter()
        .filter_map(|folder| std::fs::read_dir(folder).ok())
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with('.') || !name.ends_with(".pb") {
                return None;
            }
            let meta = entry.metadata().ok().filter(|m| m.is_file())?;
            Some((name, DateTime::from(meta.modified().ok()?)))
        })
        .collect()
}

/// Every conversation, counted once by id: a `.pb` file name without its extension is the same id
/// as a summary row's. The later of the two dates wins; the placeholder date is no date. A date
/// after `now` (clock skew) counts for nothing.
pub fn activity(files: &[Stamped], summaries: &[Summary], now: DateTime<Utc>, tz: FixedOffset) -> Activity {
    let mut sessions: HashMap<&str, (Option<DateTime<Utc>>, u64)> = HashMap::new();
    let later = |a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>| a.max(b);
    for (name, modified) in files {
        let entry = sessions.entry(name.strip_suffix(".pb").unwrap_or(name)).or_default();
        entry.0 = later(entry.0, real_date(*modified));
    }
    for sum in summaries {
        let entry = sessions.entry(&sum.id).or_default();
        entry.0 = later(entry.0, sum.modified.and_then(real_date));
        entry.1 = entry.1.max(sum.steps);
    }
    let today = now.with_timezone(&tz).date_naive();
    let week_start = today - Duration::days(6);
    let mut out = Activity::default();
    for (modified, steps) in sessions.into_values() {
        out.total_sessions += 1;
        out.total_steps += steps;
        let Some(modified) = modified.filter(|m| *m <= now) else { continue };
        out.last_active = out.last_active.max(Some(modified));
        let day = modified.with_timezone(&tz).date_naive();
        if day < week_start {
            continue;
        }
        out.last_7_days += 1;
        out.today += u32::from(day == today);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn act(files: &[Stamped], now: DateTime<Utc>, tz: FixedOffset) -> Activity {
        activity(files, &[], now, tz)
    }

    fn counts(a: Activity) -> (u32, u32) {
        (a.today, a.last_7_days)
    }

    fn jkt() -> FixedOffset {
        FixedOffset::east_opt(7 * 3600).unwrap()
    }

    /// 2026-10-31 15:00 in Jakarta.
    fn now() -> DateTime<Utc> {
        jkt().with_ymd_and_hms(2026, 10, 31, 15, 0, 0).unwrap().to_utc()
    }

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        jkt().with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap().to_utc()
    }

    fn f(name: &str, t: DateTime<Utc>) -> (String, DateTime<Utc>) {
        (name.into(), t)
    }

    #[test]
    fn counts_today_and_seven_local_days() {
        let files = [f("a", at(31, 0)), f("b", at(31, 14)), f("c", at(30, 23)), f("d", at(25, 0)), f("e", at(24, 23))];
        // 25 Oct is the 7th day counting 31 Oct; 24 Oct is out.
        assert_eq!(counts(act(&files, now(), jkt())), (2, 4));
    }

    #[test]
    fn days_follow_the_local_zone() {
        // 17:30 UTC on the 30th is 00:30 on the 31st in Jakarta, but still the 30th in UTC.
        let late = f("a", Utc.with_ymd_and_hms(2026, 10, 30, 17, 30, 0).unwrap());
        assert_eq!(act(std::slice::from_ref(&late), now(), jkt()).today, 1);
        assert_eq!(act(&[late], now(), FixedOffset::east_opt(0).unwrap()).today, 0);
    }

    #[test]
    fn the_same_name_in_two_folders_counts_once_at_its_latest_time() {
        let files = [f("x", at(25, 9)), f("x", at(31, 9)), f("y", at(31, 10)), f("y", at(31, 10))];
        assert_eq!(counts(act(&files, now(), jkt())), (2, 2));
        let old_then_new = [f("z", at(31, 9)), f("z", at(1, 9))];
        assert_eq!(counts(act(&old_then_new, now(), jkt())), (1, 1));
    }

    #[test]
    fn nothing_and_the_future_count_for_nothing() {
        assert_eq!(act(&[], now(), jkt()), Activity::default());
        assert_eq!(counts(act(&[f("a", at(31, 23))], now() - Duration::hours(24), jkt())), (0, 0), "clock skew");
    }

    #[test]
    fn the_counting_function_only_takes_names_and_times() {
        // If this compiles, `activity` has no path or handle to read from.
        let _: fn(&[Stamped], &[Summary], DateTime<Utc>, FixedOffset) -> Activity = activity;
    }

    #[test]
    fn production_code_never_opens_or_reads_a_file() {
        let src = include_str!("antigravity.rs");
        let prod = &src[..src.find("#[cfg(test)]").unwrap()];
        for forbidden in ["File::open", "fs::read(", "read_to_string", "OpenOptions", "fs::File", "BufReader", "oauth", "title", "preview", "workspace", "SELECT *"] {
            assert!(!prod.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn missing_folders_give_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list(&folders(dir.path())).is_empty());
    }

    #[test]
    fn listing_gives_names_and_times_without_reading_content() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let [a, b] = [0, 1].map(|i| folders(dir.path())[i].clone());
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::create_dir(a.join("subfolder")).unwrap();
        let stamp = std::time::SystemTime::from(at(30, 8));
        for (folder, name) in [(&a, "one.pb"), (&b, "one.pb"), (&b, "two.pb")] {
            let file = std::fs::File::create(folder.join(name)).unwrap();
            file.set_modified(stamp).unwrap();
            // Nobody can read it: it still lists, so the lister never opens a file.
            std::fs::set_permissions(folder.join(name), std::fs::Permissions::from_mode(0o000)).unwrap();
        }
        let mut got = list(&folders(dir.path()));
        got.sort();
        let want = at(30, 8);
        assert_eq!(got, [f("one.pb", want), f("one.pb", want), f("two.pb", want)]);
        assert_eq!(counts(act(&got, now(), jkt())), (0, 2));
    }

    #[test]
    fn only_pb_conversations_count_not_sidecars_dotfiles_or_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let a = folders(dir.path())[0].clone();
        std::fs::create_dir_all(&a).unwrap();
        for name in ["real.pb", "other.db", "real.pb-wal", "real.pb.tmp", ".DS_Store", ".hidden.pb", "noext", "pb"] {
            std::fs::write(a.join(name), "").unwrap();
        }
        std::fs::create_dir(a.join("dir.pb")).unwrap();
        std::os::unix::fs::symlink(a.join("real.pb"), a.join("link.pb")).unwrap();
        let names: Vec<String> = list(&folders(dir.path())).into_iter().map(|f| f.0).collect();
        assert_eq!(names, ["real.pb"]);
    }

    #[test]
    fn a_time_later_today_but_after_now_does_not_count() {
        let later = f("a", at(31, 16));
        assert_eq!(counts(act(&[later], now(), jkt())), (0, 0));
    }

    // ---- the CLI summaries database: id, step count and date only ----

    const SECRET: &str = "RAHASIA-JUDUL-PROYEK";

    fn summaries_db(dir: &Path, rows: &[(&str, i64, &str)]) -> PathBuf {
        let db = dir.join("conversation_summaries.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE conversation_summaries (conversation_id TEXT PRIMARY KEY, title TEXT, preview TEXT,
               workspace_uris TEXT, step_count INTEGER, last_modified_time TEXT);",
        )
        .unwrap();
        for (id, steps, when) in rows {
            conn.execute(
                "INSERT INTO conversation_summaries VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![id, format!("{SECRET}-judul"), format!("{SECRET}-preview"), format!("file:///{SECRET}/kode"), steps, when],
            )
            .unwrap();
        }
        db
    }

    fn s(id: &str, steps: u64, when: Option<DateTime<Utc>>) -> Summary {
        Summary { id: id.into(), steps, modified: when }
    }

    #[test]
    fn summaries_give_id_steps_and_date_and_never_the_other_columns() {
        let dir = tempfile::tempdir().unwrap();
        let db = summaries_db(
            dir.path(),
            &[("a", 10, "2026-07-09 09:29:13.478317+00:00"), ("b", 4, "2026-07-01 00:00:00+00:00"), ("c", 0, "0001-01-01 00:00:00+00:00")],
        );
        let mut got = read_summaries(&db);
        got.sort_by(|x, y| x.id.cmp(&y.id));
        let at_utc = |m, d, h, mi, sec| Utc.with_ymd_and_hms(2026, m, d, h, mi, sec).unwrap();
        assert_eq!(got[0].modified.map(|t| t.timestamp()), Some(at_utc(7, 9, 9, 29, 13).timestamp()));
        assert_eq!((got[1].id.as_str(), got[1].steps, got[1].modified), ("b", 4, Some(at_utc(7, 1, 0, 0, 0))));
        assert_eq!((got[2].id.as_str(), got[2].modified), ("c", None), "the placeholder date is no date");
        let shown = format!("{got:?}{}", serde_json::to_string(&activity(&[], &got, now(), jkt())).unwrap());
        assert!(!shown.contains(SECRET) && !shown.contains("judul") && !shown.contains("preview"), "{shown}");
    }

    #[test]
    fn reading_the_summaries_leaves_no_files_behind_and_works_in_odd_paths() {
        let dir = tempfile::tempdir().unwrap();
        let odd = dir.path().join("a b?c#d%e");
        std::fs::create_dir(&odd).unwrap();
        let db = summaries_db(&odd, &[("a", 1, "2026-07-09 09:29:13+00:00")]);
        let before: Vec<_> = std::fs::read_dir(&odd).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(read_summaries(&db).len(), 1);
        let after: Vec<_> = std::fs::read_dir(&odd).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(before, after, "immutable read-only: no -wal, -shm or journal");
    }

    #[test]
    fn a_missing_file_or_another_schema_falls_back_without_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_summaries(&dir.path().join("none.db")).is_empty());
        let other = dir.path().join("other.db");
        rusqlite::Connection::open(&other).unwrap().execute_batch("CREATE TABLE x (a TEXT);").unwrap();
        assert!(read_summaries(&other).is_empty());
        let narrow = dir.path().join("narrow.db");
        rusqlite::Connection::open(&narrow).unwrap().execute_batch("CREATE TABLE conversation_summaries (conversation_id TEXT);").unwrap();
        assert!(read_summaries(&narrow).is_empty());
        let junk = dir.path().join("junk.db");
        std::fs::write(&junk, "not a database at all").unwrap();
        assert!(read_summaries(&junk).is_empty());
    }

    #[test]
    fn files_and_summaries_merge_by_id_counting_each_session_once_at_the_later_date() {
        let files = [f("a.pb", at(10, 9)), f("a.pb", at(12, 9)), f("only-file.pb", at(31, 9))];
        let sums = [s("a", 7, Some(at(31, 10))), s("only-db", 5, Some(at(30, 9))), s("old", 11, Some(at(1, 9))), s("never", 2, None)];
        let a = activity(&files, &sums, now(), jkt());
        assert_eq!((a.total_sessions, a.total_steps), (5, 25));
        assert_eq!((a.today, a.last_7_days), (2, 3), "a and only-file today, only-db yesterday; old and never are older");
        assert_eq!(a.last_active, Some(at(31, 10)));
    }

    #[test]
    fn placeholder_dates_never_count_as_recent_or_last_active() {
        let sums = [s("x", 3, None), s("y", 4, None)];
        let a = activity(&[], &sums, now(), jkt());
        assert_eq!((a.today, a.last_7_days, a.last_active), (0, 0, None));
        assert_eq!((a.total_sessions, a.total_steps), (2, 7));
    }

    #[test]
    fn old_sessions_still_total_and_report_when_the_last_one_was() {
        let files = [f("a.pb", at(9, 9))];
        let a = act(&files, now(), jkt());
        assert_eq!((a.today, a.last_7_days, a.total_sessions, a.total_steps), (0, 0, 1, 0));
        assert_eq!(a.last_active, Some(at(9, 9)));
        assert_eq!(act(&[], now(), jkt()).total_sessions, 0);
    }
}

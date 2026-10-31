pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod cursor;
pub mod hermes;
pub mod jsonl;
pub mod opencode;

use std::ffi::OsString;
use std::fs::{self, DirBuilder, Permissions};
use std::io::ErrorKind;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OpenFlags};

/// Opens a SQLite file read-only. `Ok(None)` when it does not exist. Only ever called on a
/// [`read_snapshot`] copy: opening another tool's WAL database in place, even read-only, creates or
/// rewrites the `-shm` file next to it.
pub(crate) fn open_read_only(path: &Path) -> Result<Option<Connection>> {
    if !path.exists() {
        return Ok(None);
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(path, flags)?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    Ok(Some(conn))
}

/// `<db>-wal` and the like.
fn sibling(db: &Path, suffix: &str) -> PathBuf {
    let mut s = OsString::from(db.as_os_str());
    s.push(suffix);
    PathBuf::from(s)
}

/// Size and modification time; `None` when the file does not exist.
fn file_stamp(p: &Path) -> std::io::Result<Option<(u64, SystemTime)>> {
    match fs::metadata(p) {
        Ok(m) => Ok(Some((m.len(), m.modified()?))),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Size and mtime of an agent database and its `-wal`: the files its owner writes when rows
/// change. Equal stamps mean there is nothing new to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DbStamp {
    db: (u64, SystemTime),
    wal: Option<(u64, SystemTime)>,
}

impl DbStamp {
    /// `Ok(None)` when the database does not exist (agent not installed).
    pub fn of(db: &Path) -> Result<Option<DbStamp>> {
        let Some(main) = file_stamp(db)? else { return Ok(None) };
        Ok(Some(DbStamp { db: main, wal: file_stamp(&sibling(db, "-wal"))? }))
    }
}

/// Copies taken while the owner keeps writing are retried this often within one pass.
const SNAPSHOT_ATTEMPTS: usize = 3;

/// Removes a snapshot folder and everything in it. Fine when it does not exist.
pub fn remove_snapshot(dir: &Path) -> Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e).with_context(|| format!("cannot remove {}", dir.display())),
        _ => Ok(()),
    }
}

/// Copies `src` to `dst`, readable by this user only (a copy keeps the source's mode).
fn copy_private(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::copy(src, dst)?;
    fs::set_permissions(dst, Permissions::from_mode(0o600))
}

/// Takes a fresh copy of `<db>` and `<db>-wal` in `dir` (mode 0700), and returns it with the
/// stamp of the originals it matches. Originals that change during the copy are copied again.
fn snapshot(db: &Path, dir: &Path) -> Result<Option<(PathBuf, DbStamp)>> {
    let copy = dir.join(db.file_name().context("database path has no file name")?);
    for _ in 0..SNAPSHOT_ATTEMPTS {
        let Some(before) = DbStamp::of(db)? else { return Ok(None) };
        // Starts empty: a stale -wal or -shm would be applied to the fresh copy.
        remove_snapshot(dir)?;
        DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        fs::set_permissions(dir, Permissions::from_mode(0o700))?;
        let copied = copy_private(db, &copy).and_then(|()| match before.wal {
            Some(_) => copy_private(&sibling(db, "-wal"), &sibling(&copy, "-wal")),
            None => Ok(()),
        });
        match copied {
            // The owner deleted its -wal (or the database) mid-copy: take it again.
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(e).with_context(|| format!("cannot copy {}", db.display())),
            Ok(()) => {}
        }
        if DbStamp::of(db)? == Some(before) {
            return Ok(Some((copy, before)));
        }
    }
    bail!("{} kept changing while it was being copied", db.display())
}

/// Runs `read` on a private copy of another tool's SQLite database (`<db>`, plus `<db>-wal` when
/// present) in `dir`, our own folder, then deletes the copy, whatever happened: these databases
/// can hold conversations, so no copy stays on disk. Nothing in the agent's folder is created,
/// modified or locked. An error goes to the caller, which counts it and tries again next pass.
/// `Ok(None)` when the database does not exist.
pub fn read_snapshot<T>(db: &Path, dir: &Path, read: impl FnOnce(&Path) -> Result<T>) -> Result<Option<(T, DbStamp)>> {
    let result = snapshot(db, dir).and_then(|taken| taken.map(|(copy, stamp)| Ok((read(&copy)?, stamp))).transpose());
    let removed = remove_snapshot(dir);
    let value = result?;
    removed?;
    Ok(value)
}

/// Result of reading one agent SQLite DB.
#[derive(Debug, Default, PartialEq)]
pub struct ReadBatch {
    pub events: Vec<crate::model::UsageEvent>,
    /// New watermark (max timestamp seen in ms, including skipped rows).
    pub watermark: i64,
    /// Malformed rows (NULL/wrong-typed columns or invalid JSON), skipped but counted.
    pub skipped: u64,
}

/// Counters for one scan pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanStats {
    /// Complete lines read.
    pub lines: u64,
    /// Malformed JSON lines, skipped.
    pub skipped: u64,
    /// Usage events produced.
    pub events: u64,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Name, size, mtime and bytes of every file in `dir`.
    pub(crate) fn folder_state(dir: &Path) -> Vec<(String, u64, SystemTime, Vec<u8>)> {
        let mut v: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                let m = e.metadata().unwrap();
                (e.file_name().to_string_lossy().into_owned(), m.len(), m.modified().unwrap(), std::fs::read(e.path()).unwrap())
            })
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    /// A WAL database whose rows live only in its `-wal`, left behind like an idle tool leaves
    /// it: `<db>`, `<db>-wal` and `<db>-shm` all present, nothing checkpointed.
    pub(crate) fn wal_only_db(path: &Path, sql: &str) {
        let c = Connection::open(path).unwrap();
        c.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true).unwrap();
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;").unwrap();
        c.execute_batch(sql).unwrap();
        drop(c);
        assert!(sibling(path, "-wal").exists() && sibling(path, "-shm").exists());
    }

    fn mode(p: &Path) -> u32 {
        fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn snapshot_reads_wal_rows_without_touching_the_agent_folder() {
        let agent = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let dir = cache.path().join("snapshots/t");
        let db = agent.path().join("t.db");
        wal_only_db(&db, "CREATE TABLE t(a); WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<500) INSERT INTO t SELECT i FROM n;");
        assert!(fs::metadata(&db).unwrap().len() <= 4096, "rows must live in the WAL only");
        // The copy must not inherit a wider mode.
        for f in ["t.db", "t.db-wal"] {
            fs::set_permissions(agent.path().join(f), Permissions::from_mode(0o644)).unwrap();
        }
        let before = folder_state(agent.path());

        for _ in 0..2 {
            let (n, stamp) = read_snapshot(&db, &dir, |copy| {
                assert!(copy.starts_with(&dir));
                assert_eq!(mode(&dir), 0o700);
                assert_eq!((mode(copy), mode(&sibling(copy, "-wal"))), (0o600, 0o600));
                let conn = open_read_only(copy)?.unwrap();
                Ok(conn.query_row("SELECT count(*) FROM t", [], |r| r.get::<_, i64>(0))?)
            })
            .unwrap()
            .unwrap();
            assert_eq!(n, 500);
            assert_eq!(Some(stamp), DbStamp::of(&db).unwrap());
            assert!(!dir.exists(), "the copy is gone right after the read");
        }
        assert_eq!(folder_state(agent.path()), before, "agent folder must stay byte-for-byte untouched");
    }

    #[test]
    fn snapshot_is_removed_when_the_read_fails() {
        let agent = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let dir = cache.path().join("t");
        let db = agent.path().join("t.db");
        fs::write(&db, b"not a database").unwrap();
        let r: Result<Option<((), DbStamp)>> = read_snapshot(&db, &dir, |copy| {
            assert!(copy.exists());
            bail!("torn copy")
        });
        assert!(r.is_err());
        assert!(!dir.exists());
    }

    #[test]
    fn snapshot_drops_a_stale_wal_copy_and_skips_missing_dbs() {
        let agent = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let dir = cache.path().join("t");
        let db = agent.path().join("t.db");
        Connection::open(&db).unwrap().execute_batch("CREATE TABLE t(a); INSERT INTO t VALUES (1);").unwrap();
        // Leftovers from a run that was killed mid-read, in a folder that was too open.
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, Permissions::from_mode(0o755)).unwrap();
        fs::write(dir.join("t.db-wal"), b"junk").unwrap();
        fs::write(dir.join("t.db-shm"), b"junk").unwrap();
        let (n, _) = read_snapshot(&db, &dir, |copy| {
            assert!(!sibling(copy, "-wal").exists());
            assert_eq!(mode(&dir), 0o700);
            let conn = open_read_only(copy)?.unwrap();
            Ok(conn.query_row("SELECT count(*) FROM t", [], |r| r.get::<_, i64>(0))?)
        })
        .unwrap()
        .unwrap();
        assert_eq!(n, 1);
        assert!(!dir.exists());

        let none = read_snapshot(&agent.path().join("none.db"), &dir, |_| -> Result<()> { panic!("no database, no read") }).unwrap();
        assert!(none.is_none());
        assert!(DbStamp::of(&agent.path().join("none.db")).unwrap().is_none());
        assert!(!agent.path().join("none.db").exists());
    }

    #[test]
    fn opened_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.db");
        Connection::open(&p).unwrap().execute_batch("CREATE TABLE t(a)").unwrap();
        let ro = open_read_only(&p).unwrap().unwrap();
        assert!(ro.execute("INSERT INTO t VALUES (1)", []).is_err());
        assert!(open_read_only(&dir.path().join("nope.db")).unwrap().is_none());
        assert!(!dir.path().join("nope.db").exists());
    }
}

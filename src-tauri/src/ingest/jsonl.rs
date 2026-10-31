use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
use std::time::UNIX_EPOCH;

use anyhow::Result;

use crate::store::FileCursor;

/// Reads complete (`\n`-terminated) lines appended since `cursor`.
/// A trailing partial line is left unconsumed. If the file is smaller than the
/// stored offset (truncated/rotated), it is re-read from the start.
pub fn read_new_lines(path: &Path, cursor: Option<&FileCursor>) -> Result<(Vec<String>, FileCursor)> {
    let file = File::open(path)?;
    let meta = file.metadata()?;
    let size = meta.len();
    let mtime = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut offset = cursor.map_or(0, |c| c.offset);
    if size < offset {
        offset = 0;
    }

    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(offset))?;

    let mut lines = Vec::new();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 || buf.last() != Some(&b'\n') {
            break;
        }
        offset += n as u64;
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        lines.push(String::from_utf8_lossy(&buf).into_owned());
    }

    Ok((
        lines,
        FileCursor { path: path.to_string_lossy().into_owned(), size, mtime, offset },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn append(p: &Path, s: &str) {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).unwrap();
        f.write_all(s.as_bytes()).unwrap();
    }

    #[test]
    fn partial_last_line_is_not_consumed() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.jsonl");
        append(&p, "a\nb\npartial");
        let (lines, cur) = read_new_lines(&p, None).unwrap();
        assert_eq!(lines, ["a", "b"]);
        assert_eq!(cur.offset, 4);
        assert_eq!(cur.size, 11);

        append(&p, "ial\n");
        let (lines, cur2) = read_new_lines(&p, Some(&cur)).unwrap();
        assert_eq!(lines, ["partialial"]);
        assert_eq!(cur2.offset, 15);

        let (lines, cur3) = read_new_lines(&p, Some(&cur2)).unwrap();
        assert!(lines.is_empty());
        assert_eq!(cur3.offset, 15);
    }

    #[test]
    fn truncated_file_rescans_from_start() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.jsonl");
        append(&p, "one\ntwo\nthree\n");
        let (_, cur) = read_new_lines(&p, None).unwrap();
        assert_eq!(cur.offset, 14);

        std::fs::write(&p, "x\ny\n").unwrap();
        let (lines, cur2) = read_new_lines(&p, Some(&cur)).unwrap();
        assert_eq!(lines, ["x", "y"]);
        assert_eq!(cur2.offset, 4);
    }

    #[test]
    fn records_mtime_in_unix_seconds() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.jsonl");
        append(&p, "a\n");
        let (_, cur) = read_new_lines(&p, None).unwrap();
        assert!(cur.mtime > 1_600_000_000);
        assert_eq!(cur.path, p.to_string_lossy());
    }
}

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Error, Result};

/// A file or folder name from the frontend, rejected if it could reach outside its folder.
pub(crate) fn checked_name(name: &str) -> Result<&str> {
    let bad = name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']);
    if bad {
        return Err(Error::Invalid(format!("{name:?} isn't a valid name")));
    }
    Ok(name)
}

/// Writes to a temporary file beside `path` and renames it into place, so a crash mid-write
/// leaves the old file intact rather than a truncated one.
pub(crate) fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent folder"))?;
    fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;
    // NamedTempFile is created 0600; keep an existing file's permissions, or use the usual
    // ones for a new file.
    match fs::metadata(path) {
        Ok(existing) => fs::set_permissions(tmp.path(), existing.permissions())?,
        Err(_) => set_default_permissions(tmp.path())?,
    }
    tmp.persist(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_default_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))
}

#[cfg(not(unix))]
fn set_default_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// `dir/stem.ext`, or `dir/stem-2.ext`, `-3`… if taken.
pub(crate) fn unique_file(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    (1..)
        .map(|n| match n {
            1 => dir.join(format!("{stem}.{ext}")),
            n => dir.join(format!("{stem}-{n}.{ext}")),
        })
        .find(|path| !path.exists())
        .expect("some suffix is free")
}

pub(crate) fn unique_dir(parent: &Path, stem: &str) -> PathBuf {
    (1..)
        .map(|n| match n {
            1 => parent.join(stem),
            n => parent.join(format!("{stem}-{n}")),
        })
        .find(|path| !path.exists())
        .expect("some suffix is free")
}

/// UTC time as `2026-10-03T16:15:00Z`.
pub(crate) fn utc_iso(time: SystemTime) -> String {
    let (y, mo, d, h, mi, s) = utc_parts(time);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// UTC time for file names, which can't contain `:` on Windows: `2026-10-03T161500Z`.
pub(crate) fn utc_stamp(time: SystemTime) -> String {
    let (y, mo, d, h, mi, s) = utc_parts(time);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}{mi:02}{s:02}Z")
}

fn utc_parts(time: SystemTime) -> (i64, u32, u32, u64, u64, u64) {
    let secs = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (y, mo, d) = civil_from_days((secs / 86_400) as i64);
    let day = secs % 86_400;
    (y, mo, d, day / 3600, day % 3600 / 60, day % 60)
}

/// Year, month and day for a count of days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn formats_utc_times() {
        assert_eq!(utc_iso(at(0)), "1970-01-01T00:00:00Z");
        assert_eq!(utc_iso(at(951_782_400)), "2000-02-29T00:00:00Z");
        assert_eq!(utc_iso(at(1_700_000_000)), "2023-11-14T22:13:20Z");
        assert_eq!(utc_stamp(at(1_700_000_000)), "2023-11-14T221320Z");
    }

    #[test]
    fn rejects_names_that_escape_their_folder() {
        for bad in ["", ".", "..", "../x", "a/b", "a\\b"] {
            assert!(checked_name(bad).is_err(), "{bad:?}");
        }
        assert!(checked_name("night-market").is_ok());
    }
}

//! Where a vault's files live, behind a trait, so the same vault logic serves the desktop
//! (files on disk) and the phone (a snapshot fetched from the GitHub API). See DESIGN §9.
//!
//! The trait is deliberately object-safe: `Vault` holds an `Arc<dyn Store>` rather than a type
//! parameter, so adding the phone's implementation doesn't put `<S>` through every signature in
//! this crate and in `src-tauri`.
//!
//! It is sync, and the phone's implementation is expected to hold a snapshot it fetched before
//! calling in, buffering writes to send as one commit. That keeps the vault's own logic free of
//! async, which only the phone would need.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::{Error, Result};

/// One entry directly inside a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub is_dir: bool,
}

/// `Debug` is a supertrait so the vault's own types can keep deriving it, and `Send + Sync`
/// because the desktop keeps the open vault in Tauri's shared state.
pub trait Store: Send + Sync + fmt::Debug {
    /// A file's bytes. **A missing file must be `Error::Io` whose `kind()` is `NotFound`**:
    /// callers tell "no file yet" from "couldn't be read" by that, and treat the first as a
    /// default rather than an error.
    fn read(&self, path: &Path) -> Result<Vec<u8>>;

    /// Replaces a file, creating its folder if needed. On disk this is atomic, so a crash
    /// mid-write leaves the old file rather than a truncated one.
    fn write(&self, path: &Path, contents: &[u8]) -> Result<()>;

    /// Writes several files as one change, all or nothing. Moving a scene or renaming a note
    /// touches more than one file, and the vault must never be left half-changed; the phone
    /// sends these as a single commit.
    fn write_all(&self, files: &[(PathBuf, Vec<u8>)]) -> Result<()>;

    fn remove_file(&self, path: &Path) -> Result<()>;

    fn create_dir_all(&self, path: &Path) -> Result<()>;

    fn is_file(&self, path: &Path) -> bool;

    fn is_dir(&self, path: &Path) -> bool;

    /// Entries directly inside `dir`, in no particular order. A folder that isn't there lists
    /// as empty rather than failing, which is how the vault treats absent folders throughout.
    fn list(&self, dir: &Path) -> Result<Vec<Entry>>;

    /// The clock. It belongs here because `SystemTime::now()` panics on `wasm32-unknown-unknown`,
    /// so the phone has to supply the time from JavaScript.
    fn now(&self) -> SystemTime;

    fn exists(&self, path: &Path) -> bool {
        self.is_file(path) || self.is_dir(path)
    }

    /// A file's text, or `None` if it isn't there. Most callers want this: an absent file
    /// usually means a default, not a failure.
    fn read_opt(&self, path: &Path) -> Result<Option<String>> {
        match self.read_to_string(path) {
            Ok(text) => Ok(Some(text)),
            Err(Error::Io(e)) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn read_to_string(&self, path: &Path) -> Result<String> {
        let bytes = self.read(path)?;
        String::from_utf8(bytes).map_err(|_| Error::Invalid(format!("{} isn't text", path.display())))
    }
}

/// `dir/stem.ext`, or `dir/stem-2.ext`, `-3`… if taken.
pub(crate) fn unique_file(store: &dyn Store, dir: &Path, stem: &str, ext: &str) -> PathBuf {
    (1..)
        .map(|n| match n {
            1 => dir.join(format!("{stem}.{ext}")),
            n => dir.join(format!("{stem}-{n}.{ext}")),
        })
        .find(|path| !store.exists(path))
        .expect("some suffix is free")
}

pub(crate) fn unique_dir(store: &dyn Store, parent: &Path, stem: &str) -> PathBuf {
    (1..)
        .map(|n| match n {
            1 => parent.join(stem),
            n => parent.join(format!("{stem}-{n}")),
        })
        .find(|path| !store.exists(path))
        .expect("some suffix is free")
}

#[cfg(feature = "fs")]
pub use disk::Disk;

#[cfg(feature = "fs")]
mod disk {
    use std::fs;
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};
    use std::time::SystemTime;

    use super::{Entry, Store};
    use crate::Result;

    /// The vault as files on this computer, which is what the desktop app uses.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct Disk;

    impl Store for Disk {
        fn read(&self, path: &Path) -> Result<Vec<u8>> {
            Ok(fs::read(path)?)
        }

        fn write(&self, path: &Path, contents: &[u8]) -> Result<()> {
            Ok(write_atomically(path, contents)?)
        }

        /// One file at a time. A half-finished write leaves the vault's git history to sort out,
        /// which is what the desktop has always done; the phone is the one that needs a commit.
        fn write_all(&self, files: &[(PathBuf, Vec<u8>)]) -> Result<()> {
            for (path, contents) in files {
                write_atomically(path, contents)?;
            }
            Ok(())
        }

        fn remove_file(&self, path: &Path) -> Result<()> {
            Ok(fs::remove_file(path)?)
        }

        fn create_dir_all(&self, path: &Path) -> Result<()> {
            Ok(fs::create_dir_all(path)?)
        }

        fn is_file(&self, path: &Path) -> bool {
            path.is_file()
        }

        fn is_dir(&self, path: &Path) -> bool {
            path.is_dir()
        }

        fn list(&self, dir: &Path) -> Result<Vec<Entry>> {
            if !dir.is_dir() {
                return Ok(Vec::new());
            }
            let mut entries = Vec::new();
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                entries.push(Entry { is_dir: path.is_dir(), path });
            }
            Ok(entries)
        }

        fn now(&self) -> SystemTime {
            SystemTime::now()
        }
    }

    /// Writes to a temporary file beside `path` and renames it into place, so a crash mid-write
    /// leaves the old file intact rather than a truncated one.
    fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
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
}

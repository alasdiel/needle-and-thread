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

pub use snapshot::{Change, Snapshot};

#[cfg(feature = "fs")]
pub use disk::Disk;

mod snapshot {
    use std::collections::BTreeMap;
    use std::io;
    use std::path::{Component, Path, PathBuf};
    use std::sync::Mutex;
    use std::time::SystemTime;

    use super::{Entry, Store};
    use crate::{Error, Result};

    /// What a flush has to send: the file's new contents, or that it's gone.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Change {
        Write(Vec<u8>),
        Remove,
    }

    /// A vault held in memory, which is how the phone works: it fetches what it needs over the
    /// GitHub API, hands the files to this, and the ordinary vault logic runs against it
    /// unchanged. Writes are buffered rather than sent one by one, so a change touching several
    /// files leaves as a single commit (DESIGN §9).
    ///
    /// Nothing here talks to the network: the phone crate fetches and flushes around it, which
    /// keeps this testable without wasm or a connection.
    #[derive(Debug)]
    pub struct Snapshot {
        inner: Mutex<Inner>,
        now: SystemTime,
    }

    #[derive(Debug, Default)]
    struct Inner {
        /// Every path known to exist, whether or not its contents have been fetched.
        known: BTreeMap<PathBuf, Option<Vec<u8>>>,
        /// The blob sha each file was read at, so a flush can refuse a stale write.
        shas: BTreeMap<PathBuf, String>,
        /// Writes and deletes made since the last flush, in the order they happened.
        changed: BTreeMap<PathBuf, Change>,
    }

    impl Snapshot {
        /// `now` is the time to stamp changes with, since wasm has no clock of its own.
        pub fn new(now: SystemTime) -> Self {
            Self {
                inner: Mutex::new(Inner::default()),
                now,
            }
        }

        /// Records that a file exists, without its contents — what the repo's tree gives.
        pub fn add_path(&self, path: impl Into<PathBuf>, sha: impl Into<String>) {
            let (path, sha) = (path.into(), sha.into());
            let mut inner = self.lock();
            inner.known.entry(path.clone()).or_default();
            inner.shas.insert(path, sha);
        }

        /// Fills in a file's contents, once they've been fetched.
        pub fn add_contents(&self, path: impl Into<PathBuf>, contents: Vec<u8>) {
            self.lock().known.insert(path.into(), Some(contents));
        }

        /// Whether `path` is known to exist but hasn't been fetched yet. The phone checks this
        /// to know what to go and get before running vault logic that will read it.
        pub fn needs_fetch(&self, path: &Path) -> bool {
            let inner = self.lock();
            !inner.changed.contains_key(path) && matches!(inner.known.get(path), Some(None))
        }

        /// Everything known to exist but not yet fetched.
        pub fn unfetched(&self) -> Vec<PathBuf> {
            let inner = self.lock();
            inner
                .known
                .iter()
                .filter(|(path, contents)| contents.is_none() && !inner.changed.contains_key(*path))
                .map(|(path, _)| path.clone())
                .collect()
        }

        /// The buffered changes, for the phone to send as one commit.
        pub fn changes(&self) -> Vec<(PathBuf, Change)> {
            self.lock().changed.iter().map(|(p, c)| (p.clone(), c.clone())).collect()
        }

        /// The sha a file was read at, which a flush sends back so GitHub refuses a stale write.
        pub fn sha(&self, path: &Path) -> Option<String> {
            self.lock().shas.get(path).cloned()
        }

        /// Settles the buffered changes once they're safely sent, taking the new shas. They move
        /// into the snapshot proper rather than being dropped: forgetting them would leave the
        /// app showing what the file said *before* it was saved.
        pub fn flushed(&self, shas: &[(PathBuf, String)]) {
            let mut inner = self.lock();
            for (path, change) in std::mem::take(&mut inner.changed) {
                match change {
                    Change::Write(bytes) => {
                        inner.known.insert(path, Some(bytes));
                    }
                    Change::Remove => {
                        inner.known.remove(&path);
                        inner.shas.remove(&path);
                    }
                }
            }
            for (path, sha) in shas {
                inner.shas.insert(path.clone(), sha.clone());
            }
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
            self.inner.lock().unwrap_or_else(|e| e.into_inner())
        }
    }

    impl Inner {
        /// A path's contents as they stand, counting buffered changes.
        fn current(&self, path: &Path) -> Option<&Vec<u8>> {
            match self.changed.get(path) {
                Some(Change::Write(bytes)) => Some(bytes),
                Some(Change::Remove) => None,
                None => self.known.get(path).and_then(|c| c.as_ref()),
            }
        }

        fn is_gone(&self, path: &Path) -> bool {
            matches!(self.changed.get(path), Some(Change::Remove))
        }

        /// Every path that exists now, buffered writes included and deletes excluded.
        fn paths(&self) -> impl Iterator<Item = &PathBuf> {
            self.known
                .keys()
                .chain(self.changed.keys())
                .filter(|path| !self.is_gone(path))
        }
    }

    impl Store for Snapshot {
        fn read(&self, path: &Path) -> Result<Vec<u8>> {
            let inner = self.lock();
            match inner.current(path) {
                Some(bytes) => Ok(bytes.clone()),
                // Not fetched and not there look the same to vault logic, which is why the
                // phone fetches what it needs first; `needs_fetch` tells the two apart.
                None => Err(Error::Io(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("{} isn't in this snapshot", path.display()),
                ))),
            }
        }

        fn write(&self, path: &Path, contents: &[u8]) -> Result<()> {
            self.lock().changed.insert(path.to_owned(), Change::Write(contents.to_vec()));
            Ok(())
        }

        fn write_all(&self, files: &[(PathBuf, Vec<u8>)]) -> Result<()> {
            let mut inner = self.lock();
            for (path, contents) in files {
                inner.changed.insert(path.clone(), Change::Write(contents.clone()));
            }
            Ok(())
        }

        fn remove_file(&self, path: &Path) -> Result<()> {
            self.lock().changed.insert(path.to_owned(), Change::Remove);
            Ok(())
        }

        /// Git has no empty folders, so there's nothing to make: a folder exists once a file in
        /// it does.
        fn create_dir_all(&self, _path: &Path) -> Result<()> {
            Ok(())
        }

        fn is_file(&self, path: &Path) -> bool {
            self.lock().current(path).is_some()
        }

        fn is_dir(&self, path: &Path) -> bool {
            let inner = self.lock();
            // A folder is real if anything lives under it. An empty path is the vault's root.
            path.components().next().is_none() || inner.paths().any(|known| known.starts_with(path) && known != path)
        }

        fn list(&self, dir: &Path) -> Result<Vec<Entry>> {
            let inner = self.lock();
            let mut entries: BTreeMap<PathBuf, bool> = BTreeMap::new();
            for path in inner.paths() {
                let Ok(rest) = path.strip_prefix(dir) else { continue };
                let mut parts = rest.components();
                let Some(Component::Normal(first)) = parts.next() else { continue };
                // One level down only: a deeper path means `first` is a folder.
                let is_dir = parts.next().is_some();
                let child = dir.join(first);
                entries.entry(child).and_modify(|d| *d |= is_dir).or_insert(is_dir);
            }
            Ok(entries.into_iter().map(|(path, is_dir)| Entry { path, is_dir }).collect())
        }

        fn now(&self) -> SystemTime {
            self.now
        }
    }
}

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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, UNIX_EPOCH};

    use super::{Change, Snapshot, Store};
    use crate::Vault;

    fn snapshot() -> Snapshot {
        Snapshot::new(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
    }

    /// The phone fills a snapshot the way the GitHub API hands it over: the tree first, then
    /// contents for what it opens.
    fn filled() -> Snapshot {
        let snap = snapshot();
        for (path, body) in [
            (".needle/vault.toml", "version = 1\n"),
            ("projects/tidewater/project.toml", "title = \"Tidewater\"\nkind = \"fiction\"\n"),
            ("projects/tidewater/outline.toml", ""),
            ("projects/tidewater/manuscript/night-market.md", "+++\nid = \"sc_1\"\ntitle = \"The night market\"\n+++\n\nThe market opened at dusk.\n"),
        ] {
            snap.add_path(path, format!("sha-{path}"));
            snap.add_contents(path, body.as_bytes().to_vec());
        }
        snap
    }

    #[test]
    fn reads_what_was_put_in_and_nothing_else() {
        let snap = filled();
        assert_eq!(snap.read_to_string(Path::new(".needle/vault.toml")).unwrap(), "version = 1\n");
        let missing = snap.read(Path::new("projects/tidewater/nope.md")).unwrap_err();
        assert!(matches!(&missing, crate::Error::Io(e) if e.kind() == std::io::ErrorKind::NotFound), "{missing:?}");
        // read_opt turns that into None, which is what most of the vault wants.
        assert_eq!(snap.read_opt(Path::new("projects/tidewater/nope.md")).unwrap(), None);
    }

    /// A path from the tree with no contents yet reads as missing, so the phone has to fetch it
    /// first. The two cases have to be distinguishable, or it can't know what to go and get.
    #[test]
    fn says_what_still_needs_fetching() {
        let snap = snapshot();
        snap.add_path("projects/tidewater/manuscript/harbor.md", "sha-1");
        let path = Path::new("projects/tidewater/manuscript/harbor.md");
        assert!(snap.needs_fetch(path));
        assert_eq!(snap.unfetched(), vec![PathBuf::from("projects/tidewater/manuscript/harbor.md")]);
        assert!(snap.read(path).is_err(), "unfetched reads as missing");

        snap.add_contents(path, b"+++\n+++\n\nHarbor.\n".to_vec());
        assert!(!snap.needs_fetch(path));
        assert!(snap.unfetched().is_empty());
        assert!(snap.read(path).is_ok());
    }

    #[test]
    fn writes_are_buffered_and_read_back_before_they_are_sent() {
        let snap = filled();
        let path = Path::new("projects/tidewater/manuscript/night-market.md");
        snap.write(path, b"+++\n+++\n\nRewritten.\n").unwrap();
        // You read your own writing straight away, though nothing has gone to GitHub yet.
        assert!(snap.read_to_string(path).unwrap().contains("Rewritten."));
        assert_eq!(snap.changes().len(), 1);
        assert_eq!(snap.sha(path).as_deref(), Some("sha-projects/tidewater/manuscript/night-market.md"));
    }

    #[test]
    fn several_files_leave_as_one_change() {
        let snap = filled();
        snap.write_all(&[
            (PathBuf::from("projects/tidewater/outline.toml"), b"moved".to_vec()),
            (PathBuf::from("projects/tidewater/manuscript/new.md"), b"+++\n+++\n".to_vec()),
        ])
        .unwrap();
        let changes = snap.changes();
        assert_eq!(changes.len(), 2, "one commit's worth: {changes:?}");
        assert!(snap.is_file(Path::new("projects/tidewater/manuscript/new.md")));
    }

    #[test]
    fn a_removed_file_goes_from_listings_and_reads() {
        let snap = filled();
        let path = Path::new("projects/tidewater/manuscript/night-market.md");
        snap.remove_file(path).unwrap();
        assert!(!snap.is_file(path));
        assert!(snap.read(path).is_err());
        assert_eq!(snap.changes(), vec![(path.to_owned(), Change::Remove)]);
        assert!(snap.list(Path::new("projects/tidewater/manuscript")).unwrap().is_empty());
    }

    #[test]
    fn flushing_forgets_the_buffer_and_takes_the_new_shas() {
        let snap = filled();
        let path = PathBuf::from("projects/tidewater/outline.toml");
        snap.write(&path, b"changed").unwrap();
        snap.flushed(&[(path.clone(), "sha-after".into())]);
        assert!(snap.changes().is_empty());
        assert_eq!(snap.sha(&path).as_deref(), Some("sha-after"));
        assert_eq!(snap.read_to_string(&path).unwrap(), "changed", "the change stays, it's just sent");
    }

    /// Folders are worked out from the paths, since a flat list is all the tree gives.
    #[test]
    fn lists_one_level_and_tells_folders_from_files() {
        let snap = filled();
        let mut entries = snap
            .list(Path::new("projects/tidewater"))
            .unwrap()
            .into_iter()
            .map(|e| (e.path.to_string_lossy().into_owned(), e.is_dir))
            .collect::<Vec<_>>();
        entries.sort();
        assert_eq!(entries, [
            ("projects/tidewater/manuscript".to_owned(), true),
            ("projects/tidewater/outline.toml".to_owned(), false),
            ("projects/tidewater/project.toml".to_owned(), false),
        ]);
        assert!(snap.is_dir(Path::new("projects/tidewater/manuscript")));
        assert!(!snap.is_dir(Path::new("projects/tidewater/project.toml")));
        assert!(snap.list(Path::new("worlds")).unwrap().is_empty(), "a folder with nothing in it");
    }

    /// The point of the whole exercise: the ordinary vault logic, unchanged, running on a
    /// snapshot instead of a disk.
    #[test]
    fn the_vault_opens_and_reads_a_project_out_of_a_snapshot() {
        let vault = Vault::open_on(Arc::new(filled()), Path::new("")).unwrap();
        let projects = vault.projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].config.title, "Tidewater");
        assert_eq!(projects[0].slug, "tidewater");
        let scene = projects[0].scene("night-market").unwrap();
        assert!(scene.markdown().contains("The market opened at dusk."));
    }

    /// Writing through the vault buffers rather than escaping to GitHub a file at a time.
    #[test]
    fn saving_a_scene_through_the_vault_buffers_the_change() {
        let snap = Arc::new(filled());
        let vault = Vault::open_on(snap.clone(), Path::new("")).unwrap();
        let project = vault.project("tidewater").unwrap();
        assert!(project.save_body("night-market", "The market opened at noon.").unwrap());
        assert_eq!(snap.changes().len(), 1);
        assert!(project.scene("night-market").unwrap().markdown().contains("at noon"));
    }
}

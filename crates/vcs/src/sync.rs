//! Bringing in snapshots made elsewhere (DESIGN §5, Sync): another computer, or the phone, may
//! have pushed to the same repository. Before pushing, the vault fetches; if anything new came,
//! it's taken in, as a fast-forward when this vault has nothing of its own since, otherwise as a
//! merge snapshot with both histories as its parents. Nothing already saved changes, so named
//! versions and History stay as they were.
//!
//! When both sides changed the same lines of a file, this vault's version stays where it is and
//! the other is kept under `.needle/conflicts/`, to be compared and merged by hand. It isn't put
//! beside the original: a second copy of a note would give two notes the same title.

use git2::build::CheckoutBuilder;
use git2::{AutotagOption, ErrorCode, FetchOptions, IndexEntry, Oid, RemoteCallbacks, Tree};

use super::{BRANCH, Error, NAMED_VERSIONS, Vault, credentials};

/// Where the remote's branch is kept after a fetch, and its named versions.
const FROM_REMOTE: &str = "refs/remotes/backup/main";
const REMOTE_VERSIONS: &str = "refs/remotes/backup/versions/";
/// Where the other side's version of a file goes when both sides changed it.
pub const CONFLICTS: &str = ".needle/conflicts";

/// What a fetch found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Incoming {
    /// The repository has nothing this vault lacks.
    Nothing,
    /// It has snapshots to take in before this vault can push.
    New,
    /// Its history isn't this vault's at all, so they can't be joined.
    Unrelated,
}

/// What taking in changed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TakenIn {
    /// Files that changed on disk, relative to the vault, with `/` between folders.
    pub changed: Vec<String>,
    /// Files both sides changed, where this vault's version was kept.
    pub clashes: Vec<Clash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clash {
    /// The file, still holding this vault's version.
    pub path: String,
    /// Where the other side's version was put.
    pub copy: String,
}

impl Vault {
    /// Fetches the repository at `url`: its branch, and its named versions to adopt any this
    /// vault hasn't got. Doesn't change the vault itself; `take_in` does that.
    pub fn fetch(&self, url: &str, token: Option<&str>) -> Result<Incoming, Error> {
        // What a previous fetch left must not stand in for a repository that's now empty.
        if let Ok(mut old) = self.repo.find_reference(FROM_REMOTE) {
            old.delete()?;
        }
        let refspecs = [
            format!("+refs/heads/{BRANCH}:{FROM_REMOTE}"),
            format!("+{NAMED_VERSIONS}*:{REMOTE_VERSIONS}*"),
        ];
        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(credentials(token));
        let mut options = FetchOptions::new();
        options.remote_callbacks(callbacks);
        options.download_tags(AutotagOption::None);
        self.repo.remote_anonymous(url)?.fetch(&refspecs, Some(&mut options), None)?;
        self.incoming()
    }

    /// Whether the last fetch brought anything to take in.
    pub fn incoming(&self) -> Result<Incoming, Error> {
        let Some(theirs) = self.remote_head()? else { return Ok(Incoming::Nothing) };
        let Some(ours) = self.head()? else { return Ok(Incoming::New) };
        if ours.id() == theirs {
            return Ok(Incoming::Nothing);
        }
        match self.repo.merge_base(ours.id(), theirs) {
            Ok(base) if base == theirs => Ok(Incoming::Nothing),
            Ok(_) => Ok(Incoming::New),
            Err(e) if e.code() == ErrorCode::NotFound => Ok(Incoming::Unrelated),
            Err(e) => Err(e),
        }
    }

    /// Takes in what the last fetch brought, writing it into the vault's files. Snapshot first,
    /// so the files on disk are all in history: they're replaced by the result. `from` names the
    /// repository in the merge snapshot's message ("github.com").
    pub fn take_in(&self, from: &str) -> Result<TakenIn, Error> {
        self.adopt_named_versions()?;
        let Some(theirs) = self.remote_head()? else { return Ok(TakenIn::default()) };
        let theirs = self.repo.find_commit(theirs)?;
        let Some(ours) = self.head()? else {
            let changed = self.move_to(None, &theirs.tree()?)?;
            self.repo.reference(&format!("refs/heads/{BRANCH}"), theirs.id(), true, "taken in")?;
            return Ok(TakenIn { changed, clashes: Vec::new() });
        };
        let base = match self.repo.merge_base(ours.id(), theirs.id()) {
            Ok(base) => base,
            Err(e) if e.code() == ErrorCode::NotFound => {
                return Err(Error::from_str("the repository's history isn't this vault's, so nothing was taken in or sent"));
            }
            Err(e) => return Err(e),
        };
        if base == theirs.id() {
            return Ok(TakenIn::default());
        }
        if base == ours.id() {
            let changed = self.move_to(Some(&ours.tree()?), &theirs.tree()?)?;
            self.repo.reference(&format!("refs/heads/{BRANCH}"), theirs.id(), true, "taken in")?;
            return Ok(TakenIn { changed, clashes: Vec::new() });
        }

        let mut index = self.repo.merge_commits(&ours, &theirs, None)?;
        let mut clashes = Vec::new();
        if index.has_conflicts() {
            let conflicts = index.conflicts()?.collect::<Result<Vec<_>, _>>()?;
            let stamp = stamp(theirs.time().seconds());
            for conflict in conflicts {
                let Some(path) = [&conflict.our, &conflict.their, &conflict.ancestor]
                    .into_iter()
                    .flatten()
                    .next()
                    .map(|entry| String::from_utf8_lossy(&entry.path).into_owned())
                else {
                    continue;
                };
                index.conflict_remove(std::path::Path::new(&path))?;
                match (conflict.our, conflict.their) {
                    (Some(our), Some(their)) => {
                        index.add(&settled(our))?;
                        let copy = format!("{CONFLICTS}/{stamp}/{path}");
                        let mut theirs = settled(their);
                        theirs.path = copy.clone().into_bytes();
                        index.add(&theirs)?;
                        clashes.push(Clash { path, copy });
                    }
                    // One side changed a file the other deleted: the changed one is kept, since
                    // a deletion can be done again but lost writing can't be got back.
                    (Some(kept), None) | (None, Some(kept)) => index.add(&settled(kept))?,
                    (None, None) => {}
                }
            }
        }
        let merged = self.repo.find_tree(index.write_tree_to(&self.repo)?)?;
        let ours_tree = ours.tree()?;
        let changed = self.move_to(Some(&ours_tree), &merged)?;

        let mut message = format!("Brought in from {from}: {}", self.describe(Some(&ours_tree), &merged)?);
        match clashes.len() {
            0 => {}
            1 => message.push_str(" · 1 file to merge by hand"),
            n => message.push_str(&format!(" · {n} files to merge by hand")),
        }
        let signature = self.signature()?;
        self.repo.commit(Some("HEAD"), &signature, &signature, &message, &merged, &[&ours, &theirs])?;
        Ok(TakenIn { changed, clashes })
    }

    fn remote_head(&self) -> Result<Option<Oid>, Error> {
        match self.repo.refname_to_id(FROM_REMOTE) {
            Ok(id) => Ok(Some(id)),
            Err(e) if e.code() == ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Named versions made elsewhere become this vault's too. One this vault already has keeps
    /// its own name, since renaming moves a tag and this vault's rename may not be pushed yet.
    fn adopt_named_versions(&self) -> Result<(), Error> {
        for reference in self.repo.references_glob(&format!("{REMOTE_VERSIONS}*"))? {
            let reference = reference?;
            let (Ok(name), Some(target)) = (reference.name(), reference.target()) else { continue };
            let local = format!("{NAMED_VERSIONS}{}", &name[REMOTE_VERSIONS.len()..]);
            if self.repo.find_reference(&local).is_err() {
                self.repo.reference(&local, target, false, "named elsewhere")?;
            }
        }
        Ok(())
    }

    /// Writes `to` into the vault's files, replacing what's there, and returns which files changed.
    fn move_to(&self, from: Option<&Tree>, to: &Tree) -> Result<Vec<String>, Error> {
        let diff = self.repo.diff_tree_to_tree(from, Some(to), None)?;
        let changed = diff
            .deltas()
            .filter_map(|d| d.new_file().path().or(d.old_file().path()).map(|p| p.to_string_lossy().replace('\\', "/")))
            .collect();
        self.repo.checkout_tree(to.as_object(), Some(CheckoutBuilder::new().force()))?;
        Ok(changed)
    }
}

/// An index entry taken out of a conflict, as an ordinary one.
fn settled(mut entry: IndexEntry) -> IndexEntry {
    const STAGE: u16 = 0x3000;
    entry.flags &= !STAGE;
    entry
}

/// A folder name for one taking-in's conflicts, from when the other side's snapshot was made:
/// `2026-10-10-0941` (UTC).
fn stamp(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let minutes = seconds.rem_euclid(86_400) / 60;
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}-{:02}{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests;

//! A vault's history, stored as git commits: automatic snapshots while writing, named
//! versions on request, per-file history, and pushing to GitHub.

mod schedule;

use std::collections::HashMap;
use std::path::Path;

use git2::{
    Commit, Cred, Delta, ErrorCode, IndexAddOption, Oid, PushOptions, RemoteCallbacks, Repository,
    RepositoryInitOptions, Signature, Sort, Tree,
};
use needle_core::{scene::SceneFile, words::count_markdown_words};

pub use git2::Error;
pub use schedule::Scheduler;

const BRANCH: &str = "main";
const NAMED_VERSIONS: &str = "refs/tags/versions/";

pub struct Vault {
    repo: Repository,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub id: String,
    /// Seconds since the Unix epoch.
    pub time: i64,
    pub message: String,
    pub name: Option<String>,
}

impl Vault {
    /// Opens the vault's history, starting one if the folder has none yet.
    pub fn open_or_init(root: &Path) -> Result<Self, Error> {
        let repo = match Repository::open(root) {
            Ok(repo) => repo,
            Err(e) if e.code() == ErrorCode::NotFound => {
                let mut options = RepositoryInitOptions::new();
                options.initial_head(BRANCH);
                Repository::init_opts(root, &options)?
            }
            Err(e) => return Err(e),
        };
        Ok(Self { repo })
    }

    /// Commits every change in the vault, or returns None if nothing changed. Without a
    /// `message`, one is written from the changes, e.g. "Edited night-market (+12 words)".
    pub fn snapshot(&self, message: Option<&str>) -> Result<Option<Oid>, Error> {
        let mut index = self.repo.index()?;
        index.add_all(["*"], IndexAddOption::DEFAULT, None)?;
        index.update_all(["*"], None)?;
        index.write()?;
        let tree = self.repo.find_tree(index.write_tree()?)?;

        let parent = self.head()?;
        let parent_tree = parent.as_ref().map(Commit::tree).transpose()?;
        let unchanged = match &parent_tree {
            Some(parent_tree) => parent_tree.id() == tree.id(),
            None => tree.is_empty(),
        };
        if unchanged {
            return Ok(None);
        }

        let message = match message {
            Some(message) => message.to_owned(),
            None => self.describe(parent_tree.as_ref(), &tree)?,
        };
        let signature = self.signature()?;
        let parents: Vec<&Commit> = parent.iter().collect();
        self.repo
            .commit(Some("HEAD"), &signature, &signature, &message, &tree, &parents)
            .map(Some)
    }

    /// Snapshots that changed `path` (relative to the vault root), newest first.
    pub fn history(&self, path: &Path) -> Result<Vec<Version>, Error> {
        let Some(head) = self.head()? else {
            return Ok(Vec::new());
        };
        let names = self.version_names()?;
        let mut walk = self.repo.revwalk()?;
        // Topological so snapshots taken within the same second still come out newest first.
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
        walk.push(head.id())?;

        let mut versions = Vec::new();
        for id in walk {
            let commit = self.repo.find_commit(id?)?;
            let here = blob_at(&commit.tree()?, path);
            let before = match commit.parent(0) {
                Ok(parent) => blob_at(&parent.tree()?, path),
                Err(_) => None,
            };
            if here.is_some() && here != before {
                versions.push(Version {
                    id: commit.id().to_string(),
                    time: commit.time().seconds(),
                    message: commit.message().unwrap_or_default().trim().to_owned(),
                    name: names.get(&commit.id()).cloned(),
                });
            }
        }
        Ok(versions)
    }

    /// The contents of `path` as of snapshot `id`, if it existed then.
    pub fn read(&self, id: &str, path: &Path) -> Result<Option<String>, Error> {
        let commit = self.repo.find_commit(Oid::from_str(id)?)?;
        let Some(blob) = blob_at(&commit.tree()?, path) else {
            return Ok(None);
        };
        let blob = self.repo.find_blob(blob)?;
        Ok(Some(String::from_utf8_lossy(blob.content()).into_owned()))
    }

    /// Names a snapshot, like Google Docs' "Name this version". Naming it again replaces the name.
    pub fn name_version(&self, id: &str, name: &str) -> Result<(), Error> {
        let commit = self.repo.find_commit(Oid::from_str(id)?)?;
        let tag = format!("versions/{}", commit.id());
        self.repo.tag(&tag, commit.as_object(), &self.signature()?, name, true)?;
        Ok(())
    }

    /// Pushes history and named versions to `url`. Never forces: if the remote has snapshots
    /// this vault doesn't, the push fails and nothing is overwritten. `token` is a GitHub token
    /// for HTTPS URLs.
    pub fn push(&self, url: &str, token: Option<&str>) -> Result<(), Error> {
        let mut refspecs = vec![format!("refs/heads/{BRANCH}:refs/heads/{BRANCH}")];
        for reference in self.repo.references_glob(&format!("{NAMED_VERSIONS}*"))? {
            let reference = reference?;
            let name = reference.name()?;
            // Renaming a version moves its tag, so tags (unlike the branch) may be replaced.
            refspecs.push(format!("+{name}:{name}"));
        }

        let mut rejected = None;
        {
            let mut callbacks = RemoteCallbacks::new();
            if let Some(token) = token {
                let mut tried = false;
                callbacks.credentials(move |_url, _user, _allowed| {
                    // libgit2 asks again after a failed attempt; don't loop on a bad token.
                    if std::mem::replace(&mut tried, true) {
                        return Err(Error::from_str("GitHub didn't accept the token"));
                    }
                    Cred::userpass_plaintext("x-access-token", token)
                });
            }
            callbacks.push_update_reference(|reference, status| {
                if let Some(status) = status {
                    rejected = Some(format!("{reference} was rejected: {status}"));
                }
                Ok(())
            });
            let mut options = PushOptions::new();
            options.remote_callbacks(callbacks);
            self.repo.remote_anonymous(url)?.push(&refspecs, Some(&mut options))?;
        }
        match rejected {
            Some(message) => Err(Error::from_str(&message)),
            None => Ok(()),
        }
    }

    fn head(&self) -> Result<Option<Commit<'_>>, Error> {
        match self.repo.head() {
            Ok(head) => head.peel_to_commit().map(Some),
            Err(e) if matches!(e.code(), ErrorCode::UnbornBranch | ErrorCode::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn signature(&self) -> Result<Signature<'static>, Error> {
        self.repo
            .signature()
            .or_else(|_| Signature::now("Needle and Thread", "needle-and-thread@localhost"))
    }

    fn version_names(&self) -> Result<HashMap<Oid, String>, Error> {
        let mut names = HashMap::new();
        for reference in self.repo.references_glob(&format!("{NAMED_VERSIONS}*"))? {
            if let Ok(tag) = reference?.peel_to_tag() {
                let name = tag.message()?.unwrap_or_default().trim().to_owned();
                names.insert(tag.target_id(), name);
            }
        }
        Ok(names)
    }

    fn describe(&self, old: Option<&Tree>, new: &Tree) -> Result<String, Error> {
        if old.is_none() {
            return Ok("First snapshot".to_owned());
        }
        let diff = self.repo.diff_tree_to_tree(old, Some(new), None)?;
        let mut names = Vec::new();
        let mut words: i64 = 0;
        let mut statuses = Vec::new();
        for delta in diff.deltas() {
            let Some(path) = delta.new_file().path().or(delta.old_file().path()) else {
                continue;
            };
            if path.extension().is_some_and(|ext| ext == "md") {
                words += self.words_in(delta.new_file().id())? - self.words_in(delta.old_file().id())?;
            }
            names.push(path.file_stem().unwrap_or(path.as_os_str()).to_string_lossy().into_owned());
            statuses.push(delta.status());
        }

        let verb = match statuses.as_slice() {
            [Delta::Added] => "Added",
            [Delta::Deleted] => "Deleted",
            _ => "Edited",
        };
        let what = match names.as_slice() {
            [] => "the vault".to_owned(),
            [one] => one.clone(),
            [a, b] => format!("{a} and {b}"),
            [a, rest @ ..] => format!("{a} and {} other files", rest.len()),
        };
        Ok(match words {
            0 => format!("{verb} {what}"),
            1 => format!("{verb} {what} (+1 word)"),
            -1 => format!("{verb} {what} (−1 word)"),
            n if n > 0 => format!("{verb} {what} (+{n} words)"),
            n => format!("{verb} {what} (−{} words)", -n),
        })
    }

    fn words_in(&self, blob: Oid) -> Result<i64, Error> {
        if blob.is_zero() {
            return Ok(0);
        }
        let blob = self.repo.find_blob(blob)?;
        let text = String::from_utf8_lossy(blob.content());
        Ok(count_markdown_words(&SceneFile::parse(&text).markdown()) as i64)
    }
}

fn blob_at(tree: &Tree, path: &Path) -> Option<Oid> {
    tree.get_path(path).ok().map(|entry| entry.id())
}

#[cfg(test)]
mod tests;

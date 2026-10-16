//! Clashes waiting to be settled (DESIGN §5, Sync). Where a scene or note was changed on two
//! devices in the same paragraph, taking in keeps this vault's version in the file, the other
//! side's under `.needle/conflicts/<when>/<path>` and the version both started from under
//! `.needle/conflicts/<when>/.base/<path>`. The window compares the three (`core::merge`) to
//! show each clash, and settling one writes the choice into the kept versions here.

use std::path::{Component, Path, PathBuf};

use needle_core::merge::{BASE, CONFLICTS};
use needle_core::names::Owner;
use needle_core::scene::SceneFile;

use crate::store::Store;
use crate::{Error, Result, Vault};

/// One file's clashes from one taking-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMerge {
    /// The folder they're kept under, named for when the other side's snapshot was made:
    /// `2026-10-10-1016` (UTC).
    pub stamp: String,
    /// The file, relative to the vault, with `/` between folders.
    pub path: String,
    /// The stamp as seconds since the Unix epoch.
    pub at: i64,
    pub place: Place,
    /// The texts (without headers) of the version both started from, the other side's and the
    /// file's own as it is now.
    pub base: String,
    pub theirs: String,
    pub mine: String,
}

/// What the file is, for opening it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    Scene { project: String, scene: String },
    Note { owner: Owner, path: String },
    /// Anything else, which the window can't open to settle.
    Other,
}

impl Vault {
    /// Every scene's and note's clashes still to settle, oldest first.
    pub fn merges(&self) -> Result<Vec<PendingMerge>> {
        let conflicts = self.root.join(CONFLICTS);
        let mut merges = Vec::new();
        if !self.store.is_dir(&conflicts) {
            return Ok(merges);
        }
        let mut stamps: Vec<String> = self
            .store
            .list(&conflicts)?
            .into_iter()
            .filter(|entry| entry.is_dir)
            .filter_map(|entry| entry.path.file_name()?.to_str().map(str::to_owned))
            .collect();
        stamps.sort();
        for stamp in stamps {
            let Some(at) = seconds(&stamp) else { continue };
            let folder = conflicts.join(&stamp);
            let mut paths = Vec::new();
            files_in(self.store.as_ref(), &folder, "", &mut paths)?;
            paths.sort();
            for path in paths {
                let base = folder.join(BASE).join(&path);
                // Without the version both started from, it's a file kept whole, not a clash to
                // settle here.
                let read = |file: &Path| self.store.read_opt(file).ok().flatten();
                let (Some(base), Some(theirs), Some(mine)) = (read(&base), read(&folder.join(&path)), read(&self.root.join(&path))) else {
                    continue;
                };
                merges.push(PendingMerge {
                    place: place(&path),
                    stamp: stamp.clone(),
                    path,
                    at,
                    base: SceneFile::parse(&base).markdown(),
                    theirs: SceneFile::parse(&theirs).markdown(),
                    mine: SceneFile::parse(&mine).markdown(),
                });
            }
        }
        Ok(merges)
    }

    /// Writes a settled clash's texts into the kept versions, keeping their headers; with
    /// `None`, nothing is left to settle in that file, and both are removed.
    pub fn settle_merge(&self, stamp: &str, path: &str, texts: Option<(&str, &str)>) -> Result<()> {
        let folder = self.root.join(CONFLICTS).join(plain(stamp)?);
        let theirs = folder.join(plain(path)?);
        let base = folder.join(BASE).join(plain(path)?);
        match texts {
            Some((base_text, theirs_text)) => {
                for (file, text) in [(base, base_text), (theirs, theirs_text)] {
                    let mut kept = SceneFile::parse(&self.store.read_to_string(&file)?);
                    kept.set_markdown(text);
                    self.store.write(&file, kept.to_string().as_bytes())?;
                }
            }
            None => {
                let conflicts = self.root.join(CONFLICTS);
                for file in [base, theirs] {
                    if self.store.is_file(&file) {
                        self.store.remove_file(&file)?;
                    }
                    // Folders left empty go too, up to the conflicts folder itself.
                    let mut dir = file.parent();
                    while let Some(d) = dir.filter(|d| d.starts_with(&conflicts)) {
                        self.store.remove_dir_if_empty(d);
                        if self.store.is_dir(d) {
                            break;
                        }
                        dir = d.parent();
                    }
                }
            }
        }
        Ok(())
    }
}

/// Every file under `dir`, as paths below it, leaving out the base versions.
fn files_in(store: &dyn Store, dir: &Path, prefix: &str, found: &mut Vec<String>) -> Result<()> {
    for entry in store.list(dir)? {
        let Some(name) = entry.path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else { continue };
        let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        if entry.is_dir {
            if prefix.is_empty() && name == BASE {
                continue;
            }
            files_in(store, &entry.path, &path, found)?;
        } else {
            found.push(path);
        }
    }
    Ok(())
}

/// A relative path that stays inside the folder it's joined to.
fn plain(path: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty() || !path.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(Error::Invalid(format!("not a path in the vault: {}", path.display())));
    }
    Ok(path)
}

fn place(path: &str) -> Place {
    let parts: Vec<&str> = path.split('/').collect();
    let stem = |name: &str| name.strip_suffix(".md").map(str::to_owned);
    match parts.as_slice() {
        ["projects", project, "manuscript", file] => match stem(file) {
            Some(scene) => Place::Scene { project: (*project).to_owned(), scene },
            None => Place::Other,
        },
        [kind @ ("projects" | "worlds"), owner, "notes", rest @ ..] if !rest.is_empty() => {
            let Some(last) = stem(rest[rest.len() - 1]) else { return Place::Other };
            let mut path: Vec<&str> = rest[..rest.len() - 1].to_vec();
            path.push(&last);
            let owner = if *kind == "projects" { Owner::Project((*owner).to_owned()) } else { Owner::World((*owner).to_owned()) };
            Place::Note { owner, path: path.join("/") }
        }
        _ => Place::Other,
    }
}

/// `2026-10-10-1016` as seconds since the Unix epoch, reading it as UTC.
fn seconds(stamp: &str) -> Option<i64> {
    let number = |range: std::ops::Range<usize>| stamp.get(range)?.parse::<i64>().ok();
    if stamp.len() != 15 {
        return None;
    }
    let (year, month, day, hour, minute) = (number(0..4)?, number(5..7)?, number(8..10)?, number(11..13)?, number(13..15)?);
    // Howard Hinnant's days-from-civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn stamps_read_as_utc() {
        assert_eq!(seconds("1970-01-01-0000"), Some(0));
        assert_eq!(seconds("2026-10-10-1016"), Some(1_791_627_360));
        assert_eq!(seconds("2024-03-01-0000"), Some(1_709_251_200));
        assert_eq!(seconds("not a stamp"), None);
    }

    #[test]
    fn paths_name_scenes_and_notes() {
        assert_eq!(
            place("projects/tidewater/manuscript/night-market.md"),
            Place::Scene { project: "tidewater".into(), scene: "night-market".into() }
        );
        assert_eq!(
            place("projects/tidewater/notes/characters/mara-venn.md"),
            Place::Note { owner: Owner::Project("tidewater".into()), path: "characters/mara-venn".into() }
        );
        assert_eq!(place("worlds/aster/notes/ideas.md"), Place::Note { owner: Owner::World("aster".into()), path: "ideas".into() });
        assert_eq!(place("projects/tidewater/outline.toml"), Place::Other);
    }

    #[test]
    fn merges_are_listed_and_settled() {
        let dir = tempfile::TempDir::new().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        let path = "projects/tidewater/manuscript/night-market.md";
        let write = |rel: &str, body: &str| {
            let file = dir.path().join(rel);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, format!("+++\nid = \"sc_1\"\n+++\n\n{body}")).unwrap();
        };
        write(path, "Mine.\n");
        write(&format!("{CONFLICTS}/2026-10-10-1016/{path}"), "Theirs.\n");
        write(&format!("{CONFLICTS}/2026-10-10-1016/{BASE}/{path}"), "Base.\n");

        let merges = vault.merges().unwrap();
        assert_eq!(merges.len(), 1);
        assert_eq!(merges[0].path, path);
        assert_eq!((merges[0].base.as_str(), merges[0].theirs.as_str(), merges[0].mine.as_str()), ("Base.\n", "Theirs.\n", "Mine.\n"));
        assert_eq!(merges[0].place, Place::Scene { project: "tidewater".into(), scene: "night-market".into() });

        vault.settle_merge("2026-10-10-1016", path, Some(("Mine.\n", "Mine.\n"))).unwrap();
        let kept = fs::read_to_string(dir.path().join(CONFLICTS).join("2026-10-10-1016").join(path)).unwrap();
        assert_eq!(kept, "+++\nid = \"sc_1\"\n+++\n\nMine.\n");

        vault.settle_merge("2026-10-10-1016", path, None).unwrap();
        assert!(vault.merges().unwrap().is_empty());
        assert!(!dir.path().join(CONFLICTS).join("2026-10-10-1016").exists());
        assert!(dir.path().join(path).exists());

        assert!(vault.settle_merge("..", "x.md", None).is_err());
        assert!(vault.settle_merge("2026-10-10-1016", "../../x.md", None).is_err());
    }
}

use std::fs;
use std::path::Path;

use git2::{Repository, RepositoryInitOptions};
use needle_core::scene::SceneFile;
use tempfile::TempDir;

use super::super::{BRANCH, Vault};
use super::*;

/// One computer's vault: its folder and its history.
struct Device {
    dir: TempDir,
    vault: Vault,
}

impl Device {
    /// The first device: a vault with one scene, pushed to a new, empty repository.
    fn first(url: &str) -> Self {
        let dir = TempDir::new().unwrap();
        let vault = Vault::open_or_init(dir.path()).unwrap();
        let device = Self { dir, vault };
        device.write("night-market.md", "The market opened at dusk.\n\nMara counted the coins.\n\nTeodor watched.");
        device.vault.snapshot(None).unwrap();
        device.vault.push(url, None).unwrap();
        device
    }

    /// Another device, starting from what's in the repository.
    fn clone_of(url: &str) -> Self {
        let dir = TempDir::new().unwrap();
        Repository::clone(url, dir.path()).unwrap();
        let vault = Vault::open_or_init(dir.path()).unwrap();
        Self { dir, vault }
    }

    fn write(&self, name: &str, body: &str) {
        let path = self.dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!("+++\nid = \"sc_1\"\n+++\n\n{body}\n")).unwrap();
    }

    fn read(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.dir.path().join(name)).ok()
    }

    /// Writes, snapshots and pushes, the way the app does after a change.
    fn change(&self, url: &str, name: &str, body: &str) {
        self.write(name, body);
        self.vault.snapshot(None).unwrap();
        self.sync(url);
    }

    /// Fetches, takes in anything new, and pushes.
    fn sync(&self, url: &str) -> TakenIn {
        let taken = match self.vault.fetch(url, None).unwrap() {
            Incoming::New => self.vault.take_in("the repository").unwrap(),
            Incoming::Nothing => TakenIn::default(),
            Incoming::Unrelated => panic!("unrelated"),
        };
        self.vault.push(url, None).unwrap();
        taken
    }
}

fn bare_remote() -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    let mut options = RepositoryInitOptions::new();
    options.bare(true).initial_head(BRANCH);
    Repository::init_opts(dir.path(), &options).unwrap();
    let url = dir.path().to_str().unwrap().to_owned();
    (dir, url)
}

const SCENE: &str = "night-market.md";

#[test]
fn nothing_to_take_in_when_the_repository_is_behind_or_level() {
    let (_remote, url) = bare_remote();
    let here = Device::first(&url);
    assert_eq!(here.vault.fetch(&url, None).unwrap(), Incoming::Nothing);
    here.write(SCENE, "Written here, not pushed yet.");
    here.vault.snapshot(None).unwrap();
    assert_eq!(here.vault.fetch(&url, None).unwrap(), Incoming::Nothing);
}

#[test]
fn an_empty_repository_has_nothing_to_take_in() {
    let (_remote, url) = bare_remote();
    let dir = TempDir::new().unwrap();
    let vault = Vault::open_or_init(dir.path()).unwrap();
    assert_eq!(vault.fetch(&url, None).unwrap(), Incoming::Nothing);
    assert_eq!(vault.take_in("x").unwrap(), TakenIn::default());
}

#[test]
fn takes_in_snapshots_made_elsewhere_when_nothing_changed_here() {
    let (_remote, url) = bare_remote();
    let here = Device::first(&url);
    let there = Device::clone_of(&url);
    there.change(&url, SCENE, "Written on the phone.");
    there.change(&url, "harbor.md", "A new scene.");

    assert_eq!(here.vault.fetch(&url, None).unwrap(), Incoming::New);
    let taken = here.vault.take_in("the repository").unwrap();
    assert_eq!(taken.clashes, vec![]);
    let mut changed = taken.changed.clone();
    changed.sort();
    assert_eq!(changed, ["harbor.md", SCENE]);
    assert!(here.read(SCENE).unwrap().contains("Written on the phone."));
    assert!(here.read("harbor.md").is_some());
    // A fast-forward: the other side's snapshots are this vault's now, as they were made.
    let messages: Vec<_> = here.vault.history(Path::new(SCENE)).unwrap().into_iter().map(|v| v.message).collect();
    assert!(!messages.iter().any(|m| m.starts_with("Brought in")), "{messages:?}");
    here.vault.push(&url, None).unwrap();
}

#[test]
fn merges_changes_to_different_paragraphs_and_files() {
    let (_remote, url) = bare_remote();
    let here = Device::first(&url);
    let there = Device::clone_of(&url);
    there.change(&url, SCENE, "The market opened at dusk.\n\nMara counted the coins twice.\n\nTeodor watched.");
    there.change(&url, "harbor.md", "From the phone.");

    here.write(SCENE, "The market opened at dusk.\n\nMara counted the coins.\n\nTeodor watched, and said nothing.");
    here.vault.snapshot(None).unwrap();
    let taken = here.sync(&url);

    assert_eq!(taken.clashes, vec![]);
    let scene = here.read(SCENE).unwrap();
    assert!(scene.contains("counted the coins twice") && scene.contains("said nothing"), "{scene}");
    assert!(here.read("harbor.md").is_some());
    let latest = &here.vault.history(Path::new(SCENE)).unwrap()[0];
    assert!(latest.message.starts_with("Brought in from the repository: "), "{}", latest.message);

    // The other side then takes in the merge as a fast-forward, and both end up the same.
    there.sync(&url);
    assert_eq!(there.read(SCENE), here.read(SCENE));
}

#[test]
fn a_clash_keeps_this_version_and_puts_the_other_aside() {
    let (_remote, url) = bare_remote();
    let here = Device::first(&url);
    let there = Device::clone_of(&url);
    there.change(&url, SCENE, "The market opened at dusk.\n\nMara counted the coins on the phone.\n\nTeodor watched.");

    here.write(SCENE, "The market opened at dusk.\n\nMara counted the coins at the desk.\n\nTeodor watched.");
    here.vault.snapshot(None).unwrap();
    let taken = here.sync(&url);

    assert_eq!(taken.clashes.len(), 1);
    let clash = &taken.clashes[0];
    assert_eq!(clash.path, SCENE);
    assert!(clash.copy.starts_with(".needle/conflicts/") && clash.copy.ends_with("/night-market.md"), "{}", clash.copy);
    assert!(here.read(SCENE).unwrap().contains("at the desk"));
    assert!(here.read(&clash.copy).unwrap().contains("on the phone"));
    assert!(taken.changed.contains(&clash.copy));
    let message = &here.vault.history(Path::new(&clash.copy)).unwrap()[0].message;
    assert_eq!(message, "Brought in from the repository · 1 file to merge by hand");

    // Beside it, the version both started from, so the clash can be found again.
    let base = clash.base.as_deref().unwrap();
    assert!(base.starts_with(".needle/conflicts/") && base.contains("/.base/") && base.ends_with("/night-market.md"), "{base}");
    let base = here.read(base).unwrap();
    let theirs = here.read(&clash.copy).unwrap();
    let mine = here.read(SCENE).unwrap();
    let body = |file: &str| SceneFile::parse(file).markdown();
    let found = merge::clashes(&body(&base), &body(&mine), &body(&theirs));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].mine_text, ["Mara counted the coins at the desk."]);
    assert_eq!(found[0].theirs_text, ["Mara counted the coins on the phone."]);
}

#[test]
fn a_clash_still_takes_in_the_other_sides_changes_elsewhere_in_the_file() {
    let (_remote, url) = bare_remote();
    let here = Device::first(&url);
    let there = Device::clone_of(&url);
    // Next to each other, so git sees one clash in lines; by paragraph, only the middle clashes.
    there.change(&url, SCENE, "The market opened at dawn.\n\nMara counted the coins on the phone.\n\nTeodor watched.");
    here.write(SCENE, "The market opened at dusk.\n\nMara counted the coins at the desk.\n\nTeodor watched, silent.");
    here.vault.snapshot(None).unwrap();
    let taken = here.sync(&url);

    assert_eq!(taken.clashes.len(), 1);
    // Git on Windows may write the file with CRLF endings (core.autocrlf); the text is what counts.
    let mine = here.read(SCENE).unwrap().replace("\r\n", "\n");
    assert_eq!(mine, "+++\nid = \"sc_1\"\n+++\n\nThe market opened at dawn.\n\nMara counted the coins at the desk.\n\nTeodor watched, silent.\n");
    let theirs = here.read(&taken.clashes[0].copy).unwrap();
    assert!(theirs.contains("at dawn") && theirs.contains("on the phone") && theirs.contains("silent"), "{theirs}");
}

#[test]
fn neighbouring_paragraphs_changed_on_each_side_merge_without_a_clash() {
    let (_remote, url) = bare_remote();
    let here = Device::first(&url);
    let there = Device::clone_of(&url);
    there.change(&url, SCENE, "The market opened at dawn.\n\nMara counted the coins.\n\nTeodor watched.");
    here.write(SCENE, "The market opened at dusk.\n\nMara counted the coins twice.\n\nTeodor watched.");
    here.vault.snapshot(None).unwrap();
    let taken = here.sync(&url);

    assert_eq!(taken.clashes, vec![]);
    let scene = here.read(SCENE).unwrap();
    assert!(scene.contains("at dawn") && scene.contains("twice"), "{scene}");
    assert!(!here.dir.path().join(CONFLICTS).exists());
}

#[test]
fn a_header_changed_on_one_side_is_taken_with_the_text() {
    let (_remote, url) = bare_remote();
    let here = Device::first(&url);
    let there = Device::clone_of(&url);
    let file = "+++\nid = \"sc_1\"\nstatus = \"done\"\n+++\n\nThe market opened at dawn.\n\nMara counted the coins.\n\nTeodor watched.\n";
    fs::write(there.dir.path().join(SCENE), file).unwrap();
    there.vault.snapshot(None).unwrap();
    there.sync(&url);
    here.write(SCENE, "The market opened at dusk.\n\nMara counted the coins twice.\n\nTeodor watched.");
    here.vault.snapshot(None).unwrap();
    here.sync(&url);

    let scene = here.read(SCENE).unwrap();
    assert!(scene.contains("status = \"done\"") && scene.contains("at dawn") && scene.contains("twice"), "{scene}");
}

#[test]
fn a_file_changed_on_one_side_and_deleted_on_the_other_is_kept() {
    let (_remote, url) = bare_remote();
    let here = Device::first(&url);
    let there = Device::clone_of(&url);
    there.change(&url, SCENE, "The market opened at dusk.\n\nRewritten on the phone.\n\nTeodor watched.");

    fs::remove_file(here.dir.path().join(SCENE)).unwrap();
    here.write("other.md", "Something else, so there's a snapshot.");
    here.vault.snapshot(None).unwrap();
    let taken = here.sync(&url);
    assert_eq!(taken.clashes, vec![]);
    assert!(here.read(SCENE).unwrap().contains("Rewritten on the phone."));
}

#[test]
fn named_versions_made_elsewhere_are_adopted_without_renaming_ours() {
    let (_remote, url) = bare_remote();
    let here = Device::first(&url);
    let first = here.vault.history(Path::new(SCENE)).unwrap()[0].id.clone();
    here.vault.name_version(&first, "Ours").unwrap();
    here.vault.push(&url, None).unwrap();

    let there = Device::clone_of(&url);
    there.write(SCENE, "A draft worth naming.");
    let named = there.vault.snapshot(None).unwrap().unwrap().to_string();
    there.vault.name_version(&named, "From the phone").unwrap();
    there.vault.name_version(&first, "Renamed there").unwrap();
    there.vault.push(&url, None).unwrap();

    here.sync(&url);
    let names: Vec<_> = here.vault.history(Path::new(SCENE)).unwrap().into_iter().filter_map(|v| v.name).collect();
    assert_eq!(names, ["From the phone", "Ours"]);
}

#[test]
fn a_repository_with_another_history_is_refused() {
    let (_remote, url) = bare_remote();
    let _first = Device::first(&url);
    let dir = TempDir::new().unwrap();
    let stranger = Vault::open_or_init(dir.path()).unwrap();
    fs::write(dir.path().join("mine.md"), "A different vault.").unwrap();
    stranger.snapshot(None).unwrap();
    assert_eq!(stranger.fetch(&url, None).unwrap(), Incoming::Unrelated);
    let refused = stranger.take_in("x").unwrap_err();
    assert!(refused.message().contains("nothing was taken in or sent"));
    assert!(dir.path().join("mine.md").exists());
}

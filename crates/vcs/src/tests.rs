use std::fs;
use std::path::Path;

use git2::Repository;
use tempfile::TempDir;

use super::*;

const SCENE: &str = "night-market.md";

fn vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault = Vault::open_or_init(dir.path()).unwrap();
    (dir, vault)
}

fn write(dir: &TempDir, name: &str, body: &str) {
    fs::write(dir.path().join(name), format!("---\nid: sc_1\n---\n\n{body}\n")).unwrap();
}

fn messages(vault: &Vault, path: &str) -> Vec<String> {
    vault.history(Path::new(path)).unwrap().into_iter().map(|v| v.message).collect()
}

#[test]
fn snapshots_only_when_something_changed() {
    let (dir, vault) = vault();
    assert_eq!(vault.snapshot(None).unwrap(), None, "empty vault");
    write(&dir, SCENE, "The market opened at dusk.");
    assert!(vault.snapshot(None).unwrap().is_some());
    assert_eq!(vault.snapshot(None).unwrap(), None, "nothing new");
    write(&dir, SCENE, "The market opened at dusk, as always.");
    assert!(vault.snapshot(None).unwrap().is_some());
}

#[test]
fn messages_name_the_files_and_count_words() {
    let (dir, vault) = vault();
    write(&dir, SCENE, "The market opened at dusk.");
    vault.snapshot(None).unwrap();
    write(&dir, SCENE, "The market opened at dusk, as it always had.");
    vault.snapshot(None).unwrap();
    write(&dir, SCENE, "The market opened.");
    vault.snapshot(None).unwrap();
    write(&dir, "harbor.md", "Fog.");
    vault.snapshot(None).unwrap();
    fs::remove_file(dir.path().join("harbor.md")).unwrap();
    vault.snapshot(None).unwrap();
    write(&dir, "a.md", "One.");
    write(&dir, "b.md", "Two.");
    vault.snapshot(None).unwrap();

    assert_eq!(
        messages(&vault, SCENE),
        ["Edited night-market (−6 words)", "Edited night-market (+4 words)", "First snapshot"]
    );
    assert_eq!(messages(&vault, "harbor.md"), ["Added harbor (+1 word)"]);
    assert_eq!(messages(&vault, "a.md"), ["Edited a and b (+2 words)"]);
}

#[test]
fn history_lists_only_snapshots_that_changed_the_file() {
    let (dir, vault) = vault();
    write(&dir, SCENE, "One.");
    write(&dir, "other.md", "Other.");
    vault.snapshot(Some("both")).unwrap();
    write(&dir, "other.md", "Other, changed.");
    vault.snapshot(Some("other only")).unwrap();
    write(&dir, SCENE, "One, changed.");
    vault.snapshot(Some("scene again")).unwrap();
    assert_eq!(messages(&vault, SCENE), ["scene again", "both"]);
}

#[test]
fn reads_a_file_as_it_was() {
    let (dir, vault) = vault();
    write(&dir, SCENE, "First draft.");
    vault.snapshot(None).unwrap();
    write(&dir, SCENE, "Second draft.");
    vault.snapshot(None).unwrap();
    let oldest = vault.history(Path::new(SCENE)).unwrap().pop().unwrap();
    let text = vault.read(&oldest.id, Path::new(SCENE)).unwrap().unwrap();
    assert!(text.contains("First draft."));
    assert_eq!(vault.read(&oldest.id, Path::new("missing.md")).unwrap(), None);
}

#[test]
fn named_versions_appear_in_history_and_can_be_renamed() {
    let (dir, vault) = vault();
    write(&dir, SCENE, "Draft.");
    let id = vault.snapshot(None).unwrap().unwrap().to_string();
    vault.name_version(&id, "Sent to beta readers").unwrap();
    assert_eq!(vault.history(Path::new(SCENE)).unwrap()[0].name.as_deref(), Some("Sent to beta readers"));
    vault.name_version(&id, "Beta draft").unwrap();
    assert_eq!(vault.history(Path::new(SCENE)).unwrap()[0].name.as_deref(), Some("Beta draft"));
}

#[test]
fn reopening_keeps_the_history() {
    let (dir, vault) = vault();
    write(&dir, SCENE, "Draft.");
    vault.snapshot(None).unwrap();
    drop(vault);
    let vault = Vault::open_or_init(dir.path()).unwrap();
    assert_eq!(vault.history(Path::new(SCENE)).unwrap().len(), 1);
}

/// `cargo test --release -p needle-vcs -- --ignored --nocapture`
#[test]
#[ignore]
fn timing_on_a_million_word_vault() {
    let (dir, vault) = vault();
    let paragraph = "The market opened at dusk, as it always had, and nobody asked why. ".repeat(20);
    for i in 0..300 {
        write(&dir, &format!("scene-{i:03}.md"), &paragraph.repeat(11));
    }
    let time = |label: &str, f: &dyn Fn()| {
        let started = std::time::Instant::now();
        f();
        eprintln!("{label}: {:?}", started.elapsed());
    };
    time("first snapshot, 300 scenes", &|| {
        vault.snapshot(None).unwrap();
    });
    for round in 0..3 {
        write(&dir, "scene-150.md", &format!("{}Edit {round}.", paragraph.repeat(11)));
        time("snapshot after editing one scene", &|| {
            vault.snapshot(None).unwrap();
        });
    }
    time("history of one scene", &|| {
        vault.history(Path::new("scene-150.md")).unwrap();
    });
}

fn bare_remote() -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    Repository::init_bare(dir.path()).unwrap();
    let url = dir.path().to_str().unwrap().to_owned();
    (dir, url)
}

#[test]
fn pushes_history_and_named_versions() {
    let (dir, vault) = vault();
    let (remote_dir, url) = bare_remote();
    write(&dir, SCENE, "Draft.");
    let id = vault.snapshot(None).unwrap().unwrap();
    vault.name_version(&id.to_string(), "First").unwrap();
    vault.push(&url, None).unwrap();

    let remote = Repository::open_bare(remote_dir.path()).unwrap();
    assert_eq!(remote.refname_to_id("refs/heads/main").unwrap(), id);
    assert!(remote.refname_to_id(&format!("refs/tags/versions/{id}")).is_ok());

    write(&dir, SCENE, "Draft two.");
    let second = vault.snapshot(None).unwrap().unwrap();
    vault.push(&url, None).unwrap();
    assert_eq!(remote.refname_to_id("refs/heads/main").unwrap(), second);
}

#[test]
fn never_overwrites_snapshots_made_elsewhere() {
    let (dir, vault) = vault();
    let (_remote_dir, url) = bare_remote();
    write(&dir, SCENE, "Draft.");
    vault.snapshot(None).unwrap();
    vault.push(&url, None).unwrap();

    // Another computer pushes a snapshot this vault hasn't seen.
    let other_dir = TempDir::new().unwrap();
    Repository::clone(&url, other_dir.path()).unwrap();
    let other = Vault::open_or_init(other_dir.path()).unwrap();
    write(&other_dir, SCENE, "Written elsewhere.");
    other.snapshot(None).unwrap();
    other.push(&url, None).unwrap();

    write(&dir, SCENE, "Written here.");
    vault.snapshot(None).unwrap();
    assert!(vault.push(&url, None).is_err());
}

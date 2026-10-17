//! Saving what's written on the phone (DESIGN §9). Edits go into the vault's snapshot like any
//! write, buffered; the buffer is kept in the phone's storage as drafts after every pause, so
//! nothing is lost if the page is closed, and is sent to GitHub as one commit now and then.
//!
//! Before sending, each changed file is compared with GitHub's copy. If that moved on (the
//! desktop backed up meanwhile), the two are merged a paragraph at a time like the desktop's
//! sync does (`core::merge`); where both changed the same paragraph, the phone's version stays
//! in the file and the other is kept under `.needle/conflicts/` for the desktop to settle.
//!
//! Nothing here touches the network or the page, so it's tested natively.

use std::path::{Path, PathBuf};

use needle_core::merge::{self, BASE, CONFLICTS};
use needle_core::scene::SceneFile;
use needle_core::words::count_markdown_words;
use needle_vault::store::{Change, Snapshot, Store};
use serde::{Deserialize, Serialize};

/// A change not yet on GitHub, as the phone keeps it between visits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub path: String,
    /// The blob sha of the version it's based on; None for a new file.
    pub sha: Option<String>,
    /// That version's text.
    pub base: Option<String>,
    pub text: String,
}

/// What bringing a change up to date with GitHub did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// GitHub's copy is the one it was based on.
    Same,
    /// GitHub's copy had changed elsewhere, and the two merged.
    Merged,
    /// Both changed the same paragraph: the phone's stays, the other is kept to settle.
    Clashed,
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The snapshot's buffered writes, to keep.
pub fn drafts(snapshot: &Snapshot) -> Vec<Draft> {
    snapshot
        .changes()
        .into_iter()
        .filter_map(|(path, change)| match change {
            Change::Write(bytes) => Some(Draft {
                sha: snapshot.sha(&path),
                base: snapshot.original(&path).map(|b| text(&b)),
                path: path.to_string_lossy().replace('\\', "/"),
                text: text(&bytes),
            }),
            // The phone doesn't remove files yet.
            Change::Remove => None,
        })
        .collect()
}

/// Puts kept drafts back into a snapshot just read from GitHub, bringing each up to date with
/// what's there now. Returns what happened to each, by path.
pub fn restore(snapshot: &Snapshot, drafts: &[Draft], stamp: &str) -> Vec<(String, Outcome)> {
    drafts
        .iter()
        .map(|draft| {
            let path = Path::new(&draft.path);
            let now = snapshot.sha(path).map(|sha| (sha, snapshot.original(path).map(|b| text(&b)).unwrap_or_default()));
            snapshot.based_on(path, draft.sha.as_deref(), draft.base.clone().map(String::into_bytes));
            // Writes to a snapshot can't fail.
            let _ = snapshot.write(path, draft.text.as_bytes());
            let outcome = reconcile(snapshot, path, now.as_ref().map(|(sha, text)| (sha.as_str(), text.as_str())), stamp);
            (draft.path.clone(), outcome)
        })
        .collect()
}

/// Brings the buffered change to `path` up to date with GitHub's copy, `current` (blob sha and
/// text, or None if it isn't there). `stamp` names the folder a clash is kept under.
pub fn reconcile(snapshot: &Snapshot, path: &Path, current: Option<(&str, &str)>, stamp: &str) -> Outcome {
    let Some(Change::Write(mine)) = snapshot.changes().into_iter().find(|(p, _)| p == path).map(|(_, c)| c) else {
        return Outcome::Same;
    };
    let based_on = snapshot.sha(path);
    let (sha, theirs) = match current {
        Some((sha, _)) if based_on.as_deref() == Some(sha) => return Outcome::Same,
        None if based_on.is_none() => return Outcome::Same,
        // Gone from GitHub since: a removal can be done again, lost writing can't be got back,
        // so the phone's version makes it again.
        None => {
            snapshot.based_on(path, None, None);
            return Outcome::Same;
        }
        Some(found) => found,
    };
    let base = snapshot.original(path).map(|b| text(&b)).unwrap_or_default();
    let merged = merge::merge_file(&base, &text(&mine), theirs);
    snapshot.based_on(path, Some(sha), Some(theirs.as_bytes().to_vec()));
    if merged.text == theirs {
        // Nothing of the phone's left to send: the other side had it all.
        let _ = snapshot.write(path, theirs.as_bytes());
        snapshot.sent(path, Some(sha), &Change::Write(theirs.as_bytes().to_vec()));
        return Outcome::Merged;
    }
    let _ = snapshot.write(path, merged.text.as_bytes());
    let Some((their_copy, base_copy)) = merged.clash else { return Outcome::Merged };
    let name = path.to_string_lossy().replace('\\', "/");
    let _ = snapshot.write(&PathBuf::from(format!("{CONFLICTS}/{stamp}/{name}")), their_copy.as_bytes());
    let _ = snapshot.write(&PathBuf::from(format!("{CONFLICTS}/{stamp}/{BASE}/{name}")), base_copy.as_bytes());
    Outcome::Clashed
}

/// The commit's message, as the desktop words its snapshots: "Edited night-market on the phone
/// (+12 words)", with how many files are left to merge by hand.
pub fn message(snapshot: &Snapshot, paths: &[PathBuf]) -> String {
    let words_in = |bytes: Option<Vec<u8>>| bytes.map(|b| count_markdown_words(&SceneFile::parse(&text(&b)).markdown()) as i64).unwrap_or(0);
    let mut names = Vec::new();
    let mut added = Vec::new();
    let mut words = 0;
    let mut clashes = 0;
    for path in paths {
        if path.starts_with(CONFLICTS) {
            if !path.components().any(|c| c.as_os_str() == BASE) {
                clashes += 1;
            }
            continue;
        }
        if path.extension().is_some_and(|e| e == "md") {
            words += words_in(snapshot.read(path).ok()) - words_in(snapshot.original(path));
        }
        names.push(path.file_stem().unwrap_or(path.as_os_str()).to_string_lossy().into_owned());
        added.push(snapshot.sha(path).is_none());
    }
    let verb = if added.len() == 1 && added[0] { "Added" } else { "Edited" };
    let what = match names.as_slice() {
        [] => "the vault".to_owned(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, rest @ ..] => format!("{a} and {} other files", rest.len()),
    };
    let mut message = match words {
        0 => format!("{verb} {what} on the phone"),
        1 => format!("{verb} {what} on the phone (+1 word)"),
        -1 => format!("{verb} {what} on the phone (−1 word)"),
        n if n > 0 => format!("{verb} {what} on the phone (+{n} words)"),
        n => format!("{verb} {what} on the phone (−{} words)", -n),
    };
    match clashes {
        0 => {}
        1 => message.push_str(" · 1 file to merge by hand"),
        n => message.push_str(&format!(" · {n} files to merge by hand")),
    }
    message
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;

    const SCENE: &str = "projects/tidewater/manuscript/night-market.md";
    const HEADER: &str = "+++\nid = \"sc_1\"\ntitle = \"The night market\"\n+++\n\n";

    fn scene(body: &str) -> String {
        format!("{HEADER}{body}")
    }

    fn snapshot(body: &str) -> Snapshot {
        let snap = Snapshot::new(SystemTime::UNIX_EPOCH);
        snap.add_path(SCENE, "sha-0");
        snap.add_contents(SCENE, scene(body).into_bytes());
        snap
    }

    fn read(snap: &Snapshot, path: &str) -> String {
        snap.read_to_string(Path::new(path)).unwrap()
    }

    #[test]
    fn a_change_is_kept_as_a_draft_with_what_it_was_based_on() {
        let snap = snapshot("The market opened at dusk.\n");
        snap.write(Path::new(SCENE), scene("The market opened at dusk, as always.\n").as_bytes()).unwrap();
        assert_eq!(
            drafts(&snap),
            [Draft {
                path: SCENE.into(),
                sha: Some("sha-0".into()),
                base: Some(scene("The market opened at dusk.\n")),
                text: scene("The market opened at dusk, as always.\n"),
            }]
        );
    }

    #[test]
    fn unchanged_on_github_it_goes_as_it_is() {
        let snap = snapshot("One.\n");
        snap.write(Path::new(SCENE), scene("One, changed.\n").as_bytes()).unwrap();
        assert_eq!(reconcile(&snap, Path::new(SCENE), Some(("sha-0", &scene("One.\n"))), "stamp"), Outcome::Same);
        assert_eq!(read(&snap, SCENE), scene("One, changed.\n"));
    }

    #[test]
    fn changed_elsewhere_in_another_paragraph_the_two_merge() {
        let snap = snapshot("One.\n\nTwo.\n");
        snap.write(Path::new(SCENE), scene("One, on the phone.\n\nTwo.\n").as_bytes()).unwrap();
        let theirs = scene("One.\n\nTwo, on the desktop.\n");
        assert_eq!(reconcile(&snap, Path::new(SCENE), Some(("sha-1", &theirs)), "stamp"), Outcome::Merged);
        assert_eq!(read(&snap, SCENE), scene("One, on the phone.\n\nTwo, on the desktop.\n"));
        assert_eq!(snap.sha(Path::new(SCENE)).as_deref(), Some("sha-1"), "it now goes on top of the desktop's");
        assert_eq!(drafts(&snap)[0].base.as_deref(), Some(theirs.as_str()));
    }

    #[test]
    fn the_same_paragraph_changed_on_both_keeps_the_phones_and_the_other_to_settle() {
        let snap = snapshot("One.\n");
        snap.write(Path::new(SCENE), scene("One, on the phone.\n").as_bytes()).unwrap();
        let theirs = scene("One, on the desktop.\n");
        assert_eq!(reconcile(&snap, Path::new(SCENE), Some(("sha-1", &theirs)), "2026-10-17-1200"), Outcome::Clashed);
        assert_eq!(read(&snap, SCENE), scene("One, on the phone.\n"));
        assert_eq!(read(&snap, &format!(".needle/conflicts/2026-10-17-1200/{SCENE}")), theirs);
        assert_eq!(read(&snap, &format!(".needle/conflicts/2026-10-17-1200/.base/{SCENE}")), scene("One.\n"));
        let paths: Vec<PathBuf> = snap.changes().into_iter().map(|(p, _)| p).collect();
        assert_eq!(message(&snap, &paths), "Edited night-market on the phone · 1 file to merge by hand");
    }

    #[test]
    fn when_the_other_side_already_has_it_nothing_is_left_to_send() {
        let snap = snapshot("One.\n");
        snap.write(Path::new(SCENE), scene("One, again.\n").as_bytes()).unwrap();
        assert_eq!(reconcile(&snap, Path::new(SCENE), Some(("sha-1", &scene("One, again.\n"))), "stamp"), Outcome::Merged);
        assert!(snap.changes().is_empty(), "{:?}", snap.changes());
    }

    #[test]
    fn drafts_come_back_after_the_page_was_closed() {
        let snap = snapshot("One.\n\nTwo.\n");
        snap.write(Path::new(SCENE), scene("One, on the phone.\n\nTwo.\n").as_bytes()).unwrap();
        let kept = drafts(&snap);

        // Opened again: GitHub hasn't moved, so the draft just goes back on top.
        let again = snapshot("One.\n\nTwo.\n");
        assert_eq!(restore(&again, &kept, "stamp"), [(SCENE.to_owned(), Outcome::Same)]);
        assert_eq!(read(&again, SCENE), scene("One, on the phone.\n\nTwo.\n"));

        // Opened again after the desktop changed the other paragraph: they merge.
        let moved = Snapshot::new(SystemTime::UNIX_EPOCH);
        moved.add_path(SCENE, "sha-1");
        moved.add_contents(SCENE, scene("One.\n\nTwo, on the desktop.\n").into_bytes());
        assert_eq!(restore(&moved, &kept, "stamp"), [(SCENE.to_owned(), Outcome::Merged)]);
        assert_eq!(read(&moved, SCENE), scene("One, on the phone.\n\nTwo, on the desktop.\n"));
    }

    #[test]
    fn messages_name_the_file_and_count_words() {
        let snap = snapshot("The market opened.\n");
        snap.write(Path::new(SCENE), scene("The market opened at dusk.\n").as_bytes()).unwrap();
        assert_eq!(message(&snap, &[PathBuf::from(SCENE)]), "Edited night-market on the phone (+2 words)");
        let new = PathBuf::from("projects/tidewater/manuscript/quay.md");
        snap.write(&new, b"+++\n+++\n\nNew.\n").unwrap();
        assert_eq!(message(&snap, &[new]), "Added quay on the phone (+1 word)");
    }
}

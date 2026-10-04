use needle_core::project::{NoteKind, ProjectKind};
use needle_vault::{Placement, Vault};
use tempfile::TempDir;

use super::*;

/// Tidewater (in the Glass Coast world) with two scenes and two notes, and Saltmarsh with one
/// scene.
fn vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault = Vault::create(&dir.path().join("Writing")).unwrap();
    let tidewater = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    vault.create_world("Glass Coast").unwrap();
    vault.update_project("tidewater", |c| c.world = Some("glass-coast".into())).unwrap();

    tidewater
        .update_header("untitled-scene", |h| {
            h.set_str("title", "The night market");
            h.set_str("status", "draft");
            h.set_str("pov", "Mara");
        })
        .unwrap();
    tidewater
        .save_body("untitled-scene", "The *ledger* had left port.\n\nNobody at the café would say where.\n")
        .unwrap();
    let ledger = tidewater
        .create_scene("The ledger", Placement::After { scene: "untitled-scene".into() })
        .unwrap();
    tidewater.save_body(&ledger.slug, "Rain on the harbour.\n").unwrap();

    let mara = tidewater.notes().create(NoteKind::Character, "Mara Venn").unwrap();
    tidewater.notes().update_header(&mara.path, |h| h.set_list("aliases", &["Mara"])).unwrap();
    tidewater.notes().save_body(&mara.path, "Harbour pilot. Lost the ledger once.\n").unwrap();
    vault
        .world("glass-coast")
        .unwrap()
        .notes()
        .create(NoteKind::Event, "The Drowning")
        .unwrap();

    let saltmarsh = vault.create_project("Saltmarsh", ProjectKind::Fiction).unwrap();
    saltmarsh.save_body("untitled-scene", "Another ledger, in another book.\n").unwrap();
    (dir, vault)
}

fn titles(hits: &[Hit]) -> Vec<&str> {
    hits.iter().map(|h| h.title.as_str()).collect()
}

fn search(index: &Index, text: &str) -> Vec<Hit> {
    index.search(&Query { text: text.into(), ..Default::default() }).unwrap()
}

#[test]
fn finds_words_by_their_start_and_ranks_titles_first() {
    let (_dir, vault) = vault();
    let mut index = Index::in_memory().unwrap();
    assert_eq!(index.sync(&vault).unwrap(), Synced { added: 5, updated: 0, removed: 0 });

    let hits = search(&index, "ledg");
    assert_eq!(titles(&hits)[0], "The ledger", "a title match comes first");
    assert_eq!(hits.len(), 4);
    assert!(search(&index, "nothing-like-this").is_empty());
    // Accents don't matter, and Markdown's markers aren't part of the text.
    assert_eq!(titles(&search(&index, "cafe")), ["The night market"]);
    assert_eq!(titles(&search(&index, "\"ledger had left\"")), ["The night market"]);
    assert!(search(&index, "\"left had ledger\"").is_empty());
}

#[test]
fn snippets_mark_the_match() {
    let (_dir, vault) = vault();
    let mut index = Index::in_memory().unwrap();
    index.sync(&vault).unwrap();
    let hit = search(&index, "port").into_iter().next().unwrap();
    assert_eq!(hit.kind, Kind::Scene);
    assert_eq!((hit.project.as_deref(), hit.key.as_str()), (Some("tidewater"), "untitled-scene"));
    let marked: Vec<&str> = hit.snippet.iter().filter(|(_, m)| *m).map(|(t, _)| t.as_str()).collect();
    assert_eq!(marked, ["port"]);
    let text: String = hit.snippet.iter().map(|(t, _)| t.as_str()).collect();
    assert!(text.starts_with("The ledger had left port. Nobody"), "{text}");
}

#[test]
fn filters_narrow_the_results() {
    let (_dir, vault) = vault();
    let mut index = Index::in_memory().unwrap();
    index.sync(&vault).unwrap();
    let find = |query: Query| titles(&index.search(&query).unwrap()).into_iter().map(str::to_owned).collect::<Vec<_>>();
    let ledger = || Query { text: "ledger".into(), ..Default::default() };

    let here = Query { projects: vec!["tidewater".into()], worlds: vec!["glass-coast".into()], ..ledger() };
    assert_eq!(find(here.clone()).len(), 3, "Saltmarsh's scene is left out");
    assert_eq!(find(Query { kind: Some(Kind::Note), ..here.clone() }), ["Mara Venn"]);
    assert_eq!(find(Query { status: Some("draft".into()), ..here.clone() }), ["The night market"]);
    // The POV is written as an alias; looking for any of the note's names finds it.
    let pov = vec![("pov".into(), vec!["Mara Venn".into(), "mara".into()])];
    assert_eq!(find(Query { names: pov, ..here.clone() }), ["The night market"]);

    // With no words, the filters alone list what they allow, scenes first.
    let notes = Query { kind: Some(Kind::Note), ..Default::default() };
    assert_eq!(find(notes), ["Mara Venn", "The Drowning"]);
    let world = Query { worlds: vec!["glass-coast".into()], ..Default::default() };
    assert_eq!(find(world), ["The Drowning"]);
}

#[test]
fn a_sync_reads_only_what_changed() {
    let (_dir, vault) = vault();
    let mut index = Index::in_memory().unwrap();
    index.sync(&vault).unwrap();
    assert_eq!(index.sync(&vault).unwrap(), Synced::default());

    let tidewater = vault.project("tidewater").unwrap();
    tidewater.save_body("untitled-scene", "The compass, not the book, was the point.\n").unwrap();
    tidewater.cut_scene("the-ledger").unwrap();
    assert_eq!(index.sync(&vault).unwrap(), Synced { added: 0, updated: 1, removed: 1 });
    assert_eq!(titles(&search(&index, "compass")), ["The night market"]);
    let hits = search(&index, "ledger");
    let mut left = titles(&hits);
    left.sort();
    assert_eq!(left, ["Mara Venn", "Untitled scene"]);
}

#[test]
fn an_index_survives_reopening() {
    let (dir, vault) = vault();
    let file = dir.path().join("cache/index.sqlite");
    {
        let mut index = Index::open(&file).unwrap();
        index.sync(&vault).unwrap();
    }
    let mut index = Index::open(&file).unwrap();
    assert_eq!(index.sync(&vault).unwrap(), Synced::default());
    assert_eq!(search(&index, "drowning").len(), 1);
}

#[test]
fn what_people_type_becomes_a_safe_query() {
    assert_eq!(match_expression("  ledg  port "), Some("\"ledg\"* \"port\"*".into()));
    assert_eq!(match_expression("“left port” Mara’s"), Some("\"left port\" \"Mara s\"*".into()));
    assert_eq!(match_expression("AND OR NOT ( ) * ^"), Some("\"AND\"* \"OR\"* \"NOT\"*".into()));
    assert_eq!(match_expression("\"unclosed phrase"), Some("\"unclosed phrase\"".into()));
    assert_eq!(match_expression("  — "), None);
}

/// Timings on the sample vault, scaled up 20 times: `cargo test -p needle-index -- --ignored --nocapture`.
#[test]
#[ignore]
fn timings_on_a_novel_sized_vault() {
    let sample = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-vault");
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("vault");
    let copy = |from: &std::path::Path, to: &std::path::Path| {
        fn walk(from: &std::path::Path, to: &std::path::Path) {
            fs::create_dir_all(to).unwrap();
            for entry in fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                let target = to.join(entry.file_name());
                if entry.path().is_dir() {
                    walk(&entry.path(), &target);
                } else {
                    fs::copy(entry.path(), target).unwrap();
                }
            }
        }
        walk(from, to);
    };
    copy(&sample, &root);
    // Twenty copies of the long chapter: about 200,000 words, two novels' worth.
    let manuscript = root.join("projects/tidewater/manuscript");
    let long = fs::read_to_string(manuscript.join("long-chapter.md")).unwrap();
    for i in 0..20 {
        fs::write(manuscript.join(format!("copy-{i}.md")), &long).unwrap();
    }
    let vault = Vault::open(&root).unwrap();
    let mut index = Index::in_memory().unwrap();
    let started = std::time::Instant::now();
    let synced = index.sync(&vault).unwrap();
    eprintln!("first sync: {synced:?} in {:?}", started.elapsed());
    let started = std::time::Instant::now();
    index.sync(&vault).unwrap();
    eprintln!("sync with nothing changed: {:?}", started.elapsed());
    let started = std::time::Instant::now();
    let hits = search(&index, "harbor bell");
    eprintln!("search: {} hits in {:?}", hits.len(), started.elapsed());
}

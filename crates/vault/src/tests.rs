use std::fs;

use needle_core::names::{Owner, Resolution};
use needle_core::outline::Chapter;
use needle_core::project::{NoteKind, ProjectKind};
use needle_core::settings::TypographyRule;
use tempfile::TempDir;

use super::*;

fn vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault = Vault::create(&dir.path().join("Writing")).unwrap();
    (dir, vault)
}

fn order(project: &Project) -> Vec<String> {
    project.outline().unwrap().scenes().map(str::to_owned).collect()
}

fn first_chapter(project: &Project) -> String {
    project.outline().unwrap().chapters[0].id.clone()
}

#[test]
fn creating_a_vault_is_idempotent_and_only_vaults_open() {
    let (dir, vault) = vault();
    assert!(Vault::is_vault(vault.root()));
    let settings = fs::read_to_string(vault.root().join(".needle/vault.toml")).unwrap();
    assert!(settings.contains("statuses"));
    Vault::create(vault.root()).unwrap();
    assert_eq!(fs::read_to_string(vault.root().join(".needle/vault.toml")).unwrap(), settings);
    assert!(Vault::open(dir.path()).is_err());
    assert_eq!(vault.settings().unwrap().statuses, ["idea", "draft", "revised", "done"]);
}

#[test]
fn typography_switches_are_saved_in_the_vault_settings() {
    let (_dir, vault) = vault();
    let path = vault.root().join(".needle/vault.toml");
    let before = fs::read_to_string(&path).unwrap();
    vault.set_typography(TypographyRule::EmDash, false).unwrap();
    assert!(!vault.settings().unwrap().typography.em_dash);
    assert_eq!(fs::read_to_string(&path).unwrap(), before.replace("em_dash = true", "em_dash = false"));
}

#[test]
fn a_new_project_is_ready_to_write_in() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    assert_eq!(project.slug, "tidewater");
    assert_eq!(order(&project), ["untitled-scene"]);

    let scene = project.scene_info("untitled-scene").unwrap();
    assert_eq!(scene.title, "Untitled scene");
    assert_eq!(scene.status, "idea");
    assert!(scene.id.starts_with("sc_"));
    let text = fs::read_to_string(project.scene_path("untitled-scene").unwrap()).unwrap();
    assert!(text.starts_with("+++\nid = \"sc_"), "{text}");

    let again = vault.create_project("Tidewater", ProjectKind::Nonfiction).unwrap();
    assert_eq!(again.slug, "tidewater-2");
    let titles: Vec<_> = vault.projects().unwrap().into_iter().map(|p| p.slug).collect();
    assert_eq!(titles, ["tidewater", "tidewater-2"]);
}

#[test]
fn scenes_get_unique_file_names_and_their_place_in_the_outline() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    let chapter = first_chapter(&project);
    project.create_scene("The Harbor", Placement::End { chapter: chapter.clone() }).unwrap();
    let second = project.create_scene("The Harbor", Placement::End { chapter }).unwrap();
    assert_eq!(second.slug, "the-harbor-2");
    project
        .create_scene("Night market", Placement::After { scene: "untitled-scene".into() })
        .unwrap();
    assert_eq!(order(&project), ["untitled-scene", "night-market", "the-harbor", "the-harbor-2"]);
    assert_eq!(project.scenes().unwrap().len(), 4);
}

#[test]
fn a_bad_placement_creates_nothing() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    assert!(project.create_scene("Lost", Placement::After { scene: "missing".into() }).is_err());
    assert!(project.create_scene("Lost", Placement::End { chapter: "ol_none".into() }).is_err());
    assert!(!project.scene_path("lost").unwrap().exists());
}

#[test]
fn saving_text_keeps_the_header() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    assert!(project.save_body("untitled-scene", "The market opened at dusk.\n").unwrap());
    assert!(!project.save_body("untitled-scene", "The market opened at dusk.\n").unwrap());
    let scene = project.scene("untitled-scene").unwrap();
    assert_eq!(scene.markdown(), "The market opened at dusk.\n");
    assert_eq!(scene.header().unwrap().title(), Some("Untitled scene"));
    assert_eq!(project.scene_info("untitled-scene").unwrap().words, 5);
}

#[test]
fn header_edits_keep_the_text_and_other_fields() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    project.save_body("untitled-scene", "Text.\n").unwrap();
    let path = project.scene_path("untitled-scene").unwrap();
    let text = fs::read_to_string(&path).unwrap().replace("status = \"idea\"", "status = \"idea\"  # for now");
    fs::write(&path, text).unwrap();

    let info = project
        .update_header("untitled-scene", |h| {
            h.set_str("title", "The night market");
            h.set_str("status", "draft");
        })
        .unwrap();
    assert_eq!((info.title.as_str(), info.status.as_str()), ("The night market", "draft"));
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("status = \"draft\"  # for now"), "{text}");
    assert!(text.ends_with("+++\n\nText.\n"), "{text}");
}

#[test]
fn a_summary_is_one_paragraph_and_an_empty_one_is_removed() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    project.save_body("untitled-scene", "Text.\n").unwrap();
    let path = project.scene_path("untitled-scene").unwrap();

    let info = project.set_summary("untitled-scene", "  Mara trades the compass\nand  learns the ledger has left port. ").unwrap();
    assert_eq!(info.summary, "Mara trades the compass and learns the ledger has left port.");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("summary = \"Mara trades the compass and learns the ledger has left port.\""), "{text}");
    assert!(text.ends_with("+++\n\nText.\n"), "{text}");

    assert_eq!(project.set_summary("untitled-scene", " \n ").unwrap().summary, "");
    assert!(!fs::read_to_string(&path).unwrap().contains("summary"));
}

#[test]
fn cutting_a_scene_moves_it_to_the_bin() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    project.save_body("untitled-scene", "Keep me somewhere.\n").unwrap();
    let cut = project.cut_scene("untitled-scene").unwrap();

    assert!(!project.scene_path("untitled-scene").unwrap().exists());
    assert!(order(&project).is_empty());
    assert!(cut.starts_with(project.root().join("cut")));
    let text = fs::read_to_string(cut).unwrap();
    assert!(text.contains("cut_at = \"20"), "{text}");
    assert!(text.contains("cut_from_scene = \"untitled-scene\""));
    assert!(text.ends_with("Keep me somewhere.\n"));
}

#[test]
fn a_cut_passage_is_a_file_of_its_own_in_the_bin() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    project.update_header("untitled-scene", |h| h.set_str("title", "The night market")).unwrap();
    let passage = Passage {
        markdown: "The smell of the harbor came in under everything.\n".into(),
        text_before: "Mara walked the length of it twice.".into(),
        text_after: " She kept the compass.".into(),
        starts_paragraph: false,
        ends_paragraph: false,
        starts_with_space: true,
        ends_with_space: false,
    };
    let item = project.cut_passage("untitled-scene", &passage).unwrap();
    assert_eq!(item.kind, CutKind::Passage);
    assert_eq!(item.title, "The night market");
    assert_eq!(item.scene.as_deref(), Some("untitled-scene"));
    assert_eq!(item.words, 9);
    assert_eq!(item.passage.as_ref(), Some(&passage));
    assert!(item.name.ends_with("-untitled-scene-passage"), "{}", item.name);

    let text = fs::read_to_string(project.root().join("cut").join(format!("{}.md", item.name))).unwrap();
    assert!(text.contains("cut_from_scene = \"untitled-scene\""), "{text}");
    assert!(text.contains("text_after = \" She kept the compass.\""), "{text}");
    assert!(text.contains("starts_paragraph = false"), "{text}");
    assert!(text.contains("starts_with_space = true"), "{text}");
    assert!(!text.contains("ends_with_space"), "{text}");
    assert!(text.ends_with("+++\n\nThe smell of the harbor came in under everything.\n"), "{text}");

    assert!(project.cut_passage("untitled-scene", &Passage::default()).is_err());
    assert!(project.cut_passage("no-such-scene", &passage).is_err());

    project.bin().remove_passage(&item.name).unwrap();
    assert!(project.bin().list().unwrap().is_empty());
}

#[test]
fn a_restored_scene_goes_back_where_it_was() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    let chapter = first_chapter(&project);
    let end = || Placement::End { chapter: chapter.clone() };
    project.create_scene("Two", end()).unwrap();
    project.create_scene("Three", end()).unwrap();
    project.save_body("two", "Second.\n").unwrap();

    // Back after the scene it followed, with its name, text and header as they were.
    let before = fs::read_to_string(project.scene_path("two").unwrap()).unwrap();
    project.cut_scene("two").unwrap();
    let [cut] = &project.bin().list().unwrap()[..] else { panic!() };
    assert_eq!((cut.kind, cut.title.as_str(), cut.words), (CutKind::Scene, "Two", 1));
    assert!(project.bin().remove_passage(&cut.name).is_err());
    let restored = project.restore_scene(&cut.name).unwrap();
    assert_eq!(restored.slug, "two");
    assert_eq!(order(&project), ["untitled-scene", "two", "three"]);
    assert_eq!(fs::read_to_string(project.scene_path("two").unwrap()).unwrap(), before);
    assert!(project.bin().list().unwrap().is_empty());

    // The first scene in its chapter goes back first.
    project.cut_scene("untitled-scene").unwrap();
    let name = project.bin().list().unwrap()[0].name.clone();
    project.restore_scene(&name).unwrap();
    assert_eq!(order(&project), ["untitled-scene", "two", "three"]);

    // If the scene it followed has gone too, it goes at the end of its chapter.
    project.cut_scene("three").unwrap();
    let three = project.bin().list().unwrap()[0].name.clone();
    project.cut_scene("two").unwrap();
    project.restore_scene(&three).unwrap();
    assert_eq!(order(&project), ["untitled-scene", "three"]);

    // With its chapter gone, it waits among the scenes the outline doesn't place, and a name
    // that's been taken meanwhile gets a number.
    let rename = |title: &str| project.edit_outline(|o| { o.chapters[0].title = title.into(); Ok(()) }).unwrap();
    rename("Arrival");
    project.cut_scene("untitled-scene").unwrap();
    let name = project.bin().list().unwrap()[0].name.clone();
    rename("Departure");
    project.create_scene("Untitled scene", end()).unwrap();
    let restored = project.restore_scene(&name).unwrap();
    assert_eq!(restored.slug, "untitled-scene-2");
    assert_eq!(order(&project), ["three", "untitled-scene"]);
    assert!(project.reading_order().unwrap().contains(&"untitled-scene-2".to_owned()));
}

#[test]
fn a_restored_note_goes_back_to_its_folder() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    let notes = project.notes();
    let mara = notes.create(NoteKind::Character, "Mara Venn").unwrap();
    notes.save_body(&mara.path, "Harbor pilot.\n").unwrap();
    let before = fs::read_to_string(notes.file_path(&mara.path).unwrap()).unwrap();
    notes.cut(&mara.path).unwrap();

    let [cut] = &project.bin().list().unwrap()[..] else { panic!() };
    assert_eq!((cut.kind, cut.title.as_str(), cut.note_kind), (CutKind::Note, "Mara Venn", Some(NoteKind::Character)));
    let restored = notes.restore(&cut.name).unwrap();
    assert_eq!(restored.path, "characters/mara-venn");
    assert_eq!(fs::read_to_string(notes.file_path(&mara.path).unwrap()).unwrap(), before);

    // A note made under the same name meanwhile keeps it.
    notes.cut(&mara.path).unwrap();
    notes.create(NoteKind::Character, "Mara Venn").unwrap();
    let name = project.bin().list().unwrap()[0].name.clone();
    assert!(project.restore_scene(&name).is_err());
    assert_eq!(notes.restore(&name).unwrap().path, "characters/mara-venn-2");
}

#[test]
fn a_projects_bin_shows_what_was_cut_from_its_world_too() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    let world = vault.create_world("Glass Coast").unwrap();
    vault.update_project(&project.slug, |c| c.world = Some(world.slug.clone())).unwrap();
    let teodor = world.notes().create(NoteKind::Character, "Old Teodor").unwrap();
    world.notes().cut(&teodor.path).unwrap();
    let project = vault.project(&project.slug).unwrap();
    project.cut_scene("untitled-scene").unwrap();

    let items = vault.bin_items(&project).unwrap();
    let titles: Vec<_> = items.iter().map(|i| (i.owner.clone(), i.title.as_str())).collect();
    assert_eq!(titles.len(), 2);
    assert!(titles.contains(&(Owner::World(world.slug.clone()), "Old Teodor")), "{titles:?}");
    assert!(items.windows(2).all(|w| w[0].cut_at >= w[1].cut_at));

    let bin = vault.bin_of(&Owner::World(world.slug.clone())).unwrap();
    let name = bin.list().unwrap()[0].name.clone();
    assert_eq!(world.notes().restore(&name).unwrap().title, "Old Teodor");
}

#[test]
fn cutting_a_note_moves_it_to_its_owners_bin() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    let mara = project.notes().create(NoteKind::Character, "Mara Venn").unwrap();
    project.notes().save_body(&mara.path, "Harbor pilot.\n").unwrap();
    let cut = project.notes().cut(&mara.path).unwrap();

    assert!(project.notes().list().unwrap().is_empty());
    assert!(cut.starts_with(project.root().join("cut")));
    let text = fs::read_to_string(cut).unwrap();
    assert!(text.contains("cut_at = \"20"), "{text}");
    assert!(text.contains("cut_from_note = \"characters/mara-venn\""), "{text}");
    assert!(text.ends_with("Harbor pilot.\n"));

    // A world keeps its notes in its own folder, so its bin is there too, and isn't a type.
    let world = vault.create_world("Glass Coast").unwrap();
    let teodor = world.notes().create(NoteKind::Character, "Old Teodor").unwrap();
    world.notes().create(NoteKind::Place, "Night Market").unwrap();
    let cut = world.notes().cut(&teodor.path).unwrap();
    assert!(cut.starts_with(world.root().join("cut")));
    let left: Vec<_> = world.notes().list().unwrap().into_iter().map(|n| n.title).collect();
    assert_eq!(left, ["Night Market"]);
}

#[test]
fn splitting_puts_the_rest_in_a_new_scene_after() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    project.update_header("untitled-scene", |h| h.set_str("title", "Market")).unwrap();
    project.save_body("untitled-scene", "One.\n\nTwo.\n").unwrap();
    let new = project.split_scene("untitled-scene", "One.\n", "Two.\n").unwrap();

    assert_eq!(new.title, "Market (continued)");
    assert_eq!(order(&project), ["untitled-scene", new.slug.as_str()]);
    assert_eq!(project.scene("untitled-scene").unwrap().markdown(), "One.\n");
    assert_eq!(project.scene(&new.slug).unwrap().markdown(), "Two.\n");
}

#[test]
fn merging_appends_the_next_scene_and_bins_it() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    project.save_body("untitled-scene", "One.\n").unwrap();
    let next = project
        .create_scene("Next", Placement::After { scene: "untitled-scene".into() })
        .unwrap();
    project.save_body(&next.slug, "Two.\n").unwrap();

    assert_eq!(project.merge_with_next("untitled-scene").unwrap(), "next");
    assert_eq!(project.scene("untitled-scene").unwrap().markdown(), "One.\n\nTwo.\n");
    assert_eq!(order(&project), ["untitled-scene"]);
    assert_eq!(fs::read_dir(project.root().join("cut")).unwrap().count(), 1);
    assert!(project.merge_with_next("untitled-scene").is_err(), "nothing left to merge");
}

#[test]
fn outline_edits_are_saved() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    project
        .edit_outline(|o| {
            o.insert_chapter(
                9,
                Chapter {
                    id: "ol_two".into(),
                    title: "Two".into(),
                    ..Default::default()
                },
            );
            Ok(())
        })
        .unwrap();
    project
        .create_scene("Later", Placement::End { chapter: "ol_two".into() })
        .unwrap();
    project.edit_outline(|o| o.move_chapter("ol_two", 0)).unwrap();
    assert_eq!(order(&project), ["later", "untitled-scene"]);
    assert!(project.edit_outline(|o| o.move_chapter("ol_missing", 0)).is_err());
}

#[test]
fn names_from_outside_cannot_escape_the_vault() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    assert!(vault.project("../outside").is_err());
    assert!(project.scene("../../project").is_err());
    assert!(project.save_body("../secret", "x").is_err());
}

#[test]
fn the_sample_vault_opens_and_every_scene_is_placed() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-vault");
    let vault = Vault::open(&root).unwrap();
    let projects = vault.projects().unwrap();
    assert_eq!(projects.len(), 1);
    let project = &projects[0];
    assert_eq!(project.config.title, "Tidewater");

    let outline = project.outline().unwrap();
    let mut placed: Vec<String> = outline.scenes().map(str::to_owned).collect();
    let mut files: Vec<String> = project.scenes().unwrap().into_iter().map(|s| s.slug).collect();
    placed.sort();
    files.sort();
    assert_eq!(placed, files);
    let market = project.scene_info("night-market").unwrap();
    assert_eq!((market.title.as_str(), market.status.as_str()), ("The night market", "draft"));

    let notes: Vec<String> = project.notes().list().unwrap().into_iter().map(|n| n.title).collect();
    assert_eq!(notes, ["Mara Venn", "Old Teodor", "Night Market", "The missing ledger"]);
    let mara = vault.note_links(project, &Owner::Project("tidewater".into()), "characters/mara-venn").unwrap();
    assert_eq!(mara.appears_in.len(), 1);
    assert_eq!(mara.linked_from.len(), 2, "both scenes");
}

#[test]
fn new_chapters_join_the_part_before_them() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    let first = first_chapter(&project);
    project.edit_outline(|o| {
        o.chapter_mut(&first)?.part = Some("Part One".into());
        Ok(())
    })
    .unwrap();
    let second = project.add_chapter("  Arrival ", Some(&first)).unwrap();
    assert_eq!((second.title.as_str(), second.part.as_deref()), ("Arrival", Some("Part One")));
    assert!(second.id.starts_with("ol_"));
    assert_eq!(project.outline().unwrap().chapters[1].id, second.id);
    assert!(project.add_chapter("Lost", Some("ol_missing")).is_err());
}

#[test]
fn notes_start_from_templates_in_their_type_folder() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    let notes = project.notes();
    let mara = notes.create(NoteKind::Character, " Mara Venn ").unwrap();
    assert_eq!(mara.path, "characters/mara-venn");
    assert_eq!((mara.title.as_str(), mara.kind), ("Mara Venn", NoteKind::Character));
    assert!(mara.id.starts_with("nt_"));
    let text = fs::read_to_string(project.root().join("notes/characters/mara-venn.md")).unwrap();
    assert_eq!(
        text,
        format!("+++\nid = \"{}\"\ntype = \"character\"\ntitle = \"Mara Venn\"\naliases = []\n+++\n", mara.id)
    );

    // Every default template is written out, ready to edit.
    let templates = vault.templates_path();
    for kind in NoteKind::ALL {
        assert!(templates.join(format!("{kind}.md")).is_file(), "{kind}");
    }

    // An edited template shapes the next note, but can't override the app's own fields.
    fs::write(
        templates.join("place.md"),
        "+++\ntitle = \"ignored\"\nregion = \"\"   # coast, hills…\n+++\n\n## What it smells like\n",
    )
    .unwrap();
    let market = notes.create(NoteKind::Place, "Night Market").unwrap();
    let text = fs::read_to_string(notes.file_path(&market.path).unwrap()).unwrap();
    assert!(text.contains("title = \"Night Market\"\nregion = \"\"   # coast, hills…\n+++\n\n## What it smells like\n"), "{text}");
    assert_eq!(notes.create(NoteKind::Place, "Night market").unwrap().path, "places/night-market-2");
}

#[test]
fn notes_are_listed_by_type_then_title() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    let notes = project.notes();
    notes.create(NoteKind::Place, "Night Market").unwrap();
    notes.create(NoteKind::Character, "old Teodor").unwrap();
    notes.create(NoteKind::Character, "Mara Venn").unwrap();
    let root = project.root().join("notes");
    // Written by hand: no header at all, and a header that isn't valid TOML.
    fs::write(root.join("characters/joss.md"), "Joss sails with Mara.\n").unwrap();
    fs::write(root.join("loose idea.md"), "+++\ntitle = unquoted\n+++\nText.\n").unwrap();
    fs::create_dir_all(root.join(".hidden")).unwrap();
    fs::write(root.join(".hidden/x.md"), "").unwrap();

    let listed: Vec<_> = notes.list().unwrap().into_iter().map(|n| (n.kind, n.title)).collect();
    assert_eq!(
        listed,
        [
            (NoteKind::Character, "joss".to_owned()),
            (NoteKind::Character, "Mara Venn".to_owned()),
            (NoteKind::Character, "old Teodor".to_owned()),
            (NoteKind::Place, "Night Market".to_owned()),
            (NoteKind::Note, "loose idea".to_owned()),
        ]
    );
}

#[test]
fn note_edits_keep_the_rest_of_the_file() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    let notes = project.notes();
    let mara = notes.create(NoteKind::Character, "Mara Venn").unwrap();
    assert!(notes.save_body(&mara.path, "Harbor pilot, thirty-four.\n").unwrap());
    let info = notes.update_header(&mara.path, |h| h.set_list("aliases", &["Mara", "the Captain"])).unwrap();
    assert_eq!(info.aliases, ["Mara", "the Captain"]);
    assert_eq!(notes.read(&mara.path).unwrap().markdown(), "Harbor pilot, thirty-four.\n");
    for bad in ["../outside", "characters/../../x", "a/b/c", ""] {
        assert!(notes.read(bad).is_err(), "{bad}");
    }
}

#[test]
fn links_reach_this_project_then_its_world_then_other_projects_by_name() {
    let (_dir, vault) = vault();
    let tidewater = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    let saltmarsh = vault.create_project("Saltmarsh", ProjectKind::Fiction).unwrap();
    let world = vault.create_world("The Glass Coast").unwrap();
    assert_eq!(world.slug, "the-glass-coast");
    let config = tidewater.root().join("project.toml");
    let text = fs::read_to_string(&config).unwrap();
    fs::write(&config, format!("{text}world = \"the-glass-coast\"\n")).unwrap();
    let tidewater = vault.project("tidewater").unwrap();

    let mara = tidewater.notes().create(NoteKind::Character, "Mara Venn").unwrap();
    tidewater.notes().update_header(&mara.path, |h| h.set_list("aliases", &["Mara"])).unwrap();
    world.notes().create(NoteKind::Character, "Mara").unwrap();
    world.notes().create(NoteKind::Event, "The Drowning").unwrap();
    saltmarsh.notes().create(NoteKind::Character, "Joss").unwrap();

    let names = vault.names(&tidewater).unwrap();
    let found = |name: &str| match names.resolve(name) {
        Resolution::Found(n) => Some((n.owner.clone(), n.note.title.clone())),
        _ => None,
    };
    let here = Owner::Project("tidewater".into());
    let shared = Owner::World("the-glass-coast".into());
    assert_eq!(found("mara"), Some((here.clone(), "Mara Venn".into())));
    assert_eq!(found("the drowning"), Some((shared, "The Drowning".into())));
    assert_eq!(found("Joss"), None);
    assert_eq!(found("saltmarsh/Joss"), Some((Owner::Project("saltmarsh".into()), "Joss".into())));

    // A project without a world doesn't see the world's notes.
    let names = vault.names(&saltmarsh).unwrap();
    assert!(matches!(names.resolve("The Drowning"), Resolution::Missing));
}

/// Tidewater (in the Glass Coast world) with Mara, Teodor and two scenes that name them, plus
/// Saltmarsh, which links into Tidewater.
fn linked_vault() -> (TempDir, Vault) {
    let (dir, vault) = vault();
    let tidewater = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    let saltmarsh = vault.create_project("Saltmarsh", ProjectKind::Fiction).unwrap();
    vault.create_world("Glass Coast").unwrap();
    let config = tidewater.root().join("project.toml");
    fs::write(&config, fs::read_to_string(&config).unwrap() + "world = \"glass-coast\"\n").unwrap();
    let tidewater = vault.project("tidewater").unwrap();

    let notes = tidewater.notes();
    let mara = notes.create(NoteKind::Character, "Mara Venn").unwrap();
    notes.update_header(&mara.path, |h| h.set_list("aliases", &["Mara"])).unwrap();
    let teodor = notes.create(NoteKind::Character, "Old Teodor").unwrap();
    notes.save_body(&teodor.path, "Owes [[Mara Venn]] nothing.\n").unwrap();
    let drowning = vault.world("glass-coast").unwrap().notes().create(NoteKind::Event, "The Drowning").unwrap();
    vault.world("glass-coast").unwrap().notes().save_body(&drowning.path, "[[Old Teodor]] saw it.\n").unwrap();

    tidewater
        .update_header("untitled-scene", |h| {
            h.set_str("pov", "Mara Venn");
            h.set_list("cast", &["Mara", "Old Teodor"]);
        })
        .unwrap();
    tidewater
        .save_body("untitled-scene", "[[Mara Venn|Mara]] met [[old teodor]].\n\nLater Mara slept. Mara Venn’s boat rocked.\n")
        .unwrap();
    let next = tidewater
        .create_scene("Next", Placement::After { scene: "untitled-scene".into() })
        .unwrap();
    tidewater.save_body(&next.slug, "[[Mara]] waited.\n").unwrap();

    saltmarsh
        .save_body("untitled-scene", "[[tidewater/Old Teodor]] and an [[Old Teodor]] of our own.\n")
        .unwrap();
    (dir, vault)
}

#[test]
fn a_note_knows_what_points_at_it() {
    let (_dir, vault) = linked_vault();
    let tidewater = vault.project("tidewater").unwrap();
    let here = Owner::Project("tidewater".into());
    let found = vault.note_links(&tidewater, &here, "characters/mara-venn").unwrap();

    let appears: Vec<_> = found.appears_in.iter().map(|a| (a.scene.slug.as_str(), a.fields.clone())).collect();
    assert_eq!(appears, [("untitled-scene", vec!["pov", "cast"])]);

    let linked: Vec<_> = found
        .linked_from
        .iter()
        .map(|b| match &b.from {
            LinkSource::Scene(s) => (s.slug.clone(), b.count),
            LinkSource::Note(n) => (n.note.title.clone(), b.count),
        })
        .collect();
    assert_eq!(linked, [("untitled-scene".into(), 1), ("next".into(), 1), ("Old Teodor".into(), 1)]);

    // "Mara Venn’s" is one mention, not also one of "Mara".
    let [mention] = found.mentioned_in.as_slice() else { panic!("{:?}", found.mentioned_in) };
    assert_eq!((mention.scene.slug.as_str(), mention.count), ("untitled-scene", 2));
    assert_eq!(
        (mention.before.as_str(), mention.name.as_str(), mention.after.as_str()),
        ("Later ", "Mara", " slept. Mara Venn’s boat rocked.")
    );

    // A world note's backlinks include world notes.
    let teodor = vault.note_links(&tidewater, &here, "characters/old-teodor").unwrap();
    assert_eq!(teodor.linked_from.len(), 2, "{:?}", teodor.linked_from);
}

#[test]
fn renaming_a_note_keeps_every_link_and_the_prose() {
    let (_dir, vault) = linked_vault();
    let here = Owner::Project("tidewater".into());
    let renamed = vault.rename_note(&here, "characters/old-teodor", "Teodor Brask").unwrap();
    assert_eq!(renamed.note.title, "Teodor Brask");
    assert_eq!(renamed.note.path, "characters/old-teodor", "the file keeps its name");

    let tidewater = vault.project("tidewater").unwrap();
    let scene = tidewater.scene("untitled-scene").unwrap();
    assert!(scene.markdown().starts_with("[[Mara Venn|Mara]] met [[Teodor Brask|old teodor]]."), "{}", scene.markdown());
    assert_eq!(scene.header().unwrap().list("cast"), ["Mara", "Teodor Brask"]);

    // Saltmarsh's prefixed link follows; its own loose "Old Teodor" isn't this note.
    let saltmarsh = vault.project("saltmarsh").unwrap();
    assert_eq!(
        saltmarsh.scene("untitled-scene").unwrap().markdown(),
        "[[tidewater/Teodor Brask|tidewater/Old Teodor]] and an [[Old Teodor]] of our own.\n"
    );
    // Opened from Tidewater, the world's note linked to Tidewater's Teodor, so it follows too.
    let world = vault.world("glass-coast").unwrap().notes();
    assert_eq!(world.read("events/the-drowning").unwrap().markdown(), "[[Teodor Brask|Old Teodor]] saw it.\n");

    let mut scenes = renamed.scenes.clone();
    scenes.sort();
    assert_eq!(scenes, [("saltmarsh".into(), "untitled-scene".into()), ("tidewater".into(), "untitled-scene".into())]);
    assert_eq!(renamed.notes, [(Owner::World("glass-coast".into()), "events/the-drowning".to_owned())]);
}

#[test]
fn a_rename_can_not_take_another_notes_title() {
    let (_dir, vault) = linked_vault();
    let here = Owner::Project("tidewater".into());
    assert!(vault.rename_note(&here, "characters/mara-venn", "old TEODOR").is_err());
    assert!(vault.rename_note(&here, "characters/mara-venn", "  ").is_err());
    // Changing only the case is fine and rewrites nothing.
    let renamed = vault.rename_note(&here, "characters/mara-venn", "MARA VENN").unwrap();
    assert_eq!(renamed.note.title, "MARA VENN");
    assert!(renamed.scenes.is_empty());
}

#[test]
fn linking_a_mention_keeps_the_words_shown() {
    let (_dir, vault) = linked_vault();
    let tidewater = vault.project("tidewater").unwrap();
    let here = Owner::Project("tidewater".into());
    assert!(vault.link_mention(&tidewater, "untitled-scene", &here, "characters/mara-venn").unwrap());
    assert_eq!(
        tidewater.scene("untitled-scene").unwrap().markdown(),
        "[[Mara Venn|Mara]] met [[old teodor]].\n\nLater [[Mara Venn|Mara]] slept. Mara Venn’s boat rocked.\n"
    );
    assert!(vault.link_mention(&tidewater, "untitled-scene", &here, "characters/mara-venn").unwrap());
    assert!(tidewater.scene("untitled-scene").unwrap().markdown().contains("[[Mara Venn]]’s boat"));
    assert!(!vault.link_mention(&tidewater, "untitled-scene", &here, "characters/mara-venn").unwrap());
}

#[test]
fn a_project_can_join_a_world_and_promote_notes_to_it() {
    let (_dir, vault) = vault();
    vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    assert!(vault.update_project("tidewater", |c| c.world = Some("nowhere".into())).is_err());
    let world = vault.create_world("Glass Coast").unwrap();
    let project = vault
        .update_project("tidewater", |c| {
            c.world = Some(world.slug.clone());
            c.kind = ProjectKind::Nonfiction;
            c.title = "  Tidewater, revised ".into();
        })
        .unwrap();
    assert_eq!(project.config.title, "Tidewater, revised");
    assert_eq!(vault.project("tidewater").unwrap().config.kind, ProjectKind::Nonfiction);

    let mara = project.notes().create(NoteKind::Character, "Mara Venn").unwrap();
    project.save_body("untitled-scene", "[[Mara Venn]] waits.\n").unwrap();
    let promoted = vault.promote_note(&project, &mara.path).unwrap();
    assert_eq!(promoted.path, "characters/mara-venn");
    assert!(project.notes().list().unwrap().is_empty());
    assert_eq!(world.notes().list().unwrap()[0].id, mara.id);

    // Links from the project now find it in the world.
    let names = vault.names(&project).unwrap();
    assert!(matches!(names.resolve("Mara Venn"), Resolution::Found(n) if n.owner == Owner::World("glass-coast".into())));

    // A second note with that title can't follow it.
    let again = project.notes().create(NoteKind::Character, "Mara Venn").unwrap();
    assert!(vault.promote_note(&project, &again.path).is_err());
    assert!(vault.update_project("tidewater", |c| c.title = " ".into()).is_err());
}

#[test]
fn a_scene_lists_its_names_and_hints_at_missing_ones() {
    let (_dir, vault) = linked_vault();
    let tidewater = vault.project("tidewater").unwrap();
    tidewater.notes().create(NoteKind::Place, "Night Market").unwrap();
    tidewater.notes().create(NoteKind::Thread, "The ledger").unwrap();
    tidewater
        .save_body("next", "[[Mara]] waited at the Night Market. Old Teodor didn't come.\n")
        .unwrap();
    vault.set_scene_names(&tidewater, "next", "places", &["Night Market".into(), "The harbor".into()]).unwrap();

    let names = vault.scene_names(&tidewater, "next").unwrap();
    let places = &names.fields.iter().find(|(f, _)| *f == "places").unwrap().1;
    let shown: Vec<_> = places.iter().map(|n| (n.name.as_str(), n.note.as_ref().map(|n| n.note.title.as_str()))).collect();
    assert_eq!(shown, [("Night Market", Some("Night Market")), ("The harbor", None)]);
    // Mara is linked and Teodor mentioned, but neither is in the cast; the market is listed.
    let hints: Vec<_> = names.hints.iter().map(|h| (h.field, h.note.note.title.as_str())).collect();
    assert_eq!(hints, [("cast", "Mara Venn"), ("cast", "Old Teodor")]);

    vault.set_scene_names(&tidewater, "next", "cast", &["Mara".into()]).unwrap();
    vault.set_scene_names(&tidewater, "next", "pov", &[" Mara Venn ".into()]).unwrap();
    let names = vault.scene_names(&tidewater, "next").unwrap();
    let hints: Vec<_> = names.hints.iter().map(|h| h.note.note.title.as_str()).collect();
    assert_eq!(hints, ["Old Teodor"], "an alias in the cast counts");
    assert_eq!(tidewater.scene("next").unwrap().header().unwrap().str("pov"), Some("Mara Venn"));
    vault.set_scene_names(&tidewater, "next", "pov", &[]).unwrap();
    assert!(!tidewater.scene("next").unwrap().header().unwrap().contains("pov"));
    assert!(vault.set_scene_names(&tidewater, "next", "mood", &[]).is_err());
}

#[test]
fn a_project_with_no_board_reads_as_an_empty_one() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    assert_eq!(project.network().unwrap(), needle_core::network::Network::default());
    assert!(!project.root().join("network.toml").exists(), "nothing is written until the board is arranged");
}

#[test]
fn the_board_survives_a_round_trip_to_disk() {
    use needle_core::network::{Mark, Network, Node, Zone};
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();

    let mut board = Network::default();
    board.nodes.insert("nt_mara".into(), Node { at: (120.0, 240.0), turn: -1.5 });
    board.zones.push(Zone { id: "zn_1".into(), name: "The harbour".into(), at: (0.0, 0.0), size: (520.0, 380.0), turn: 0.0 });
    board.marks.push(Mark::Note { id: "mk_1".into(), text: "who has it now?".into(), at: (600.0, 120.0), turn: -6.0, on: None });
    project.save_network(&board).unwrap();

    assert_eq!(project.network().unwrap(), board);
    // Reopening the project, as the app does when switching back to it, reads the same board.
    let reopened = vault.project(&project.slug).unwrap();
    assert_eq!(reopened.network().unwrap(), board);
}

#[test]
fn a_damaged_board_file_is_reported_not_ignored() {
    let (_dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    fs::write(project.root().join("network.toml"), "nodes = \"not a table\"").unwrap();
    let error = project.network().unwrap_err().to_string();
    assert!(error.contains("network.toml"), "{error}");
}

/// A project with the four kinds of card note, one relationship and one world.
fn board_vault() -> (TempDir, Vault, Project) {
    let (dir, vault) = vault();
    let project = vault.create_project("Tidewater", ProjectKind::Fiction).unwrap();
    let notes = project.notes();
    notes.create(NoteKind::Character, "Mara Venn").unwrap();
    notes.create(NoteKind::Character, "Old Teodor").unwrap();
    notes.create(NoteKind::Place, "Night Market").unwrap();
    notes.create(NoteKind::Thread, "The missing ledger").unwrap();
    notes.create(NoteKind::Event, "The ledger leaves port").unwrap();
    (dir, vault, project)
}

fn card<'a>(board: &'a Board, title: &str) -> &'a Card {
    board.cards.iter().find(|c| c.title == title).unwrap_or_else(|| panic!("no card for {title}"))
}

fn joined(board: &Board, a: &str, b: &str) -> bool {
    let (a, b) = (&card(board, a).id, &card(board, b).id);
    board.links.iter().any(|l| (l.from == *a && l.to == *b) || (l.from == *b && l.to == *a))
}

#[test]
fn the_board_shows_one_card_per_note_that_takes_one() {
    let (_dir, vault, project) = board_vault();
    // Relationships are lines, and a plain note stays off the board.
    project.notes().create(NoteKind::Note, "Things to check").unwrap();
    let board = vault.board(&project).unwrap();

    let mut titles: Vec<&str> = board.cards.iter().map(|c| c.title.as_str()).collect();
    titles.sort_unstable();
    assert_eq!(titles, ["Mara Venn", "Night Market", "Old Teodor", "The ledger leaves port", "The missing ledger"]);
    assert!(board.cards.iter().all(|c| c.from.is_empty()), "all from this project");
    assert!(board.placed_new, "nothing had been placed yet");
}

#[test]
fn a_plot_point_is_joined_to_what_its_header_names() {
    let (_dir, vault, project) = board_vault();
    project
        .notes()
        .update_header("events/the-ledger-leaves-port", |h| {
            h.set_list("involves", &["Mara Venn", "Old Teodor"]);
            h.set_list("threads", &["The missing ledger"]);
        })
        .unwrap();
    let board = vault.board(&project).unwrap();

    assert!(joined(&board, "The ledger leaves port", "Mara Venn"));
    assert!(joined(&board, "The ledger leaves port", "Old Teodor"));
    assert!(joined(&board, "The ledger leaves port", "The missing ledger"));
    assert!(!joined(&board, "Mara Venn", "Old Teodor"), "the header says nothing about those two");
}

#[test]
fn a_link_in_a_notes_text_joins_them_once_however_often_it_appears() {
    let (_dir, vault, project) = board_vault();
    let notes = project.notes();
    notes.save_body("characters/mara-venn", "She owes [[Old Teodor]] for the berth, and [[Old Teodor]] knows it.").unwrap();
    let board = vault.board(&project).unwrap();

    let ends: Vec<&Link> = board
        .links
        .iter()
        .filter(|l| [&l.from, &l.to].contains(&&card(&board, "Mara Venn").id))
        .collect();
    assert_eq!(ends.len(), 1, "one line, not one per mention: {ends:?}");
    assert!(joined(&board, "Mara Venn", "Old Teodor"));
}

#[test]
fn a_relationship_is_strung_between_its_two_ends() {
    let (_dir, vault, project) = board_vault();
    let notes = project.notes();
    notes.create(NoteKind::Relationship, "Mara and Teodor").unwrap();
    notes
        .update_header("relationships/mara-and-teodor", |h| {
            h.set_list("between", &["Mara Venn", "Old Teodor"]);
            h.set_str("label", "trusts");
            h.set_bool("directed", true);
        })
        .unwrap();
    let board = vault.board(&project).unwrap();

    assert_eq!(board.relationships.len(), 1);
    let rel = &board.relationships[0];
    assert_eq!(rel.label, "trusts");
    assert!(rel.directed);
    assert_eq!(rel.between, (card(&board, "Mara Venn").id.clone(), card(&board, "Old Teodor").id.clone()));
    assert!(board.cards.iter().all(|c| c.title != "Mara and Teodor"), "a relationship is a line, not a card");
}

#[test]
fn string_replaces_the_twine_between_the_same_two_cards() {
    let (_dir, vault, project) = board_vault();
    let notes = project.notes();
    notes.save_body("characters/mara-venn", "She owes [[Old Teodor]] for the berth.").unwrap();
    notes.create(NoteKind::Relationship, "Mara and Teodor").unwrap();
    notes
        .update_header("relationships/mara-and-teodor", |h| {
            h.set_list("between", &["Mara Venn", "Old Teodor"]);
            h.set_str("label", "trusts");
        })
        .unwrap();
    let board = vault.board(&project).unwrap();

    assert_eq!(board.relationships.len(), 1);
    assert!(!joined(&board, "Mara Venn", "Old Teodor"), "the relationship already draws this pair");
}

#[test]
fn a_relationship_with_an_end_that_isnt_up_is_simply_not_drawn() {
    let (_dir, vault, project) = board_vault();
    let notes = project.notes();
    notes.create(NoteKind::Relationship, "Mara and a stranger").unwrap();
    notes
        .update_header("relationships/mara-and-a-stranger", |h| {
            h.set_list("between", &["Mara Venn", "Nobody At All"]);
        })
        .unwrap();
    assert!(vault.board(&project).unwrap().relationships.is_empty());
}

#[test]
fn a_world_note_is_only_up_once_it_has_been_pinned() {
    let (_dir, vault, project) = board_vault();
    let world = vault.create_world("The Glass Coast").unwrap();
    let mut project = project;
    let mut config = project.config.clone();
    config.world = Some(world.slug.clone());
    project.save_config(config).unwrap();
    let harbour = world.notes().create(NoteKind::Place, "The Drowned Harbour").unwrap();

    let board = vault.board(&project).unwrap();
    assert!(board.cards.iter().all(|c| c.title != "The Drowned Harbour"), "not up until pinned");

    let mut layout = project.network().unwrap();
    layout.nodes.insert(harbour.id.clone(), needle_core::network::Node { at: (10.0, 20.0), turn: 0.0 });
    project.save_network(&layout).unwrap();

    let board = vault.board(&project).unwrap();
    let pinned = card(&board, "The Drowned Harbour");
    assert_eq!(pinned.at, (10.0, 20.0));
    assert_eq!(pinned.from, world.slug, "the card says where it came from");
}

#[test]
fn a_card_keeps_where_it_was_put_and_new_ones_get_room() {
    let (_dir, vault, project) = board_vault();
    let board = vault.board(&project).unwrap();
    let mara = card(&board, "Mara Venn").id.clone();

    let mut layout = project.network().unwrap();
    layout.nodes.insert(mara.clone(), needle_core::network::Node { at: (640.0, 480.0), turn: -2.0 });
    project.save_network(&layout).unwrap();

    let board = vault.board(&project).unwrap();
    assert_eq!(card(&board, "Mara Venn").at, (640.0, 480.0));
    assert_eq!(card(&board, "Mara Venn").turn, -2.0);
    assert!(board.placed_new, "the others still had no spot");
    // No two cards on top of each other.
    for (i, a) in board.cards.iter().enumerate() {
        for b in &board.cards[i + 1..] {
            assert!(a.at != b.at, "{} and {} are on the same spot", a.title, b.title);
        }
    }
}

#[test]
fn an_arranged_board_places_nothing_new() {
    let (_dir, vault, project) = board_vault();
    let first = vault.board(&project).unwrap();
    assert!(first.placed_new);

    let mut layout = project.network().unwrap();
    for c in &first.cards {
        layout.nodes.insert(c.id.clone(), needle_core::network::Node { at: c.at, turn: c.turn });
    }
    vault.save_board(&project, &layout).unwrap();

    let again = vault.board(&project).unwrap();
    assert!(!again.placed_new);
    assert_eq!(again.cards, first.cards, "reading it again finds the same board");
}

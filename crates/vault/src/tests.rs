use std::fs;

use needle_core::outline::Chapter;
use needle_core::project::ProjectKind;
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
}

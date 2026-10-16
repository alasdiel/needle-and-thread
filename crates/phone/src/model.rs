//! What the phone's screens show, worked out from the vault. Nothing here touches the page or
//! the network, so it's tested against the sample vault held in a snapshot.

use std::collections::HashMap;

use needle_core::names::{Owner, Resolution};
use needle_core::project::ProjectKind;
use needle_core::words::visible_text;
use needle_vault::{LinkSource, Project, SceneInfo, Vault};

use crate::render;
use crate::search::Doc;

/// A row of the outline.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Part { title: String, words: usize },
    Chapter { number: usize, title: String, words: usize },
    /// The scenes in no chapter, after the rest.
    Unplaced { words: usize },
    Scene(Scene),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub slug: String,
    pub title: String,
    pub status: String,
    /// How far along its status is, 0 to 1, or None for a status the vault doesn't list.
    pub progress: Option<f64>,
    pub words: usize,
}

fn scene_of(info: &SceneInfo, statuses: &[String]) -> Scene {
    Scene {
        slug: info.slug.clone(),
        title: info.title.clone(),
        status: info.status.clone(),
        progress: progress(&info.status, statuses),
        words: info.words,
    }
}

pub fn progress(status: &str, statuses: &[String]) -> Option<f64> {
    let index = statuses.iter().position(|s| s == status)?;
    Some(match statuses.len() {
        1 => 1.0,
        n => index as f64 / (n - 1) as f64,
    })
}

fn statuses(vault: &Vault) -> Vec<String> {
    vault.settings().map(|s| s.statuses).unwrap_or_default()
}

/// The project's outline as rows: parts, chapters and their scenes, then any scenes in none.
pub fn outline(vault: &Vault, project: &Project) -> Result<Vec<Row>, String> {
    let statuses = statuses(vault);
    let outline = project.outline().map_err(|e| e.to_string())?;
    let mut scenes: HashMap<String, Scene> =
        project.scenes().map_err(|e| e.to_string())?.iter().map(|s| (s.slug.clone(), scene_of(s, &statuses))).collect();
    let mut rows = Vec::new();
    let mut part: Option<(usize, Option<String>)> = None;
    for (number, chapter) in outline.chapters.iter().enumerate() {
        if chapter.part.is_some() && part.as_ref().map(|(_, p)| p) != Some(&chapter.part) {
            part = Some((rows.len(), chapter.part.clone()));
            rows.push(Row::Part { title: chapter.part.clone().unwrap_or_default(), words: 0 });
        } else if chapter.part.is_none() {
            part = None;
        }
        let at = rows.len();
        rows.push(Row::Chapter { number: number + 1, title: chapter.title.clone(), words: 0 });
        let mut words = 0;
        for slug in &chapter.scenes {
            if let Some(scene) = scenes.remove(slug) {
                words += scene.words;
                rows.push(Row::Scene(scene));
            }
        }
        if let Row::Chapter { words: w, .. } = &mut rows[at] {
            *w = words;
        }
        if let Some((p, _)) = &part
            && let Row::Part { words: w, .. } = &mut rows[*p]
        {
            *w += words;
        }
    }
    let mut unplaced: Vec<Scene> = scenes.into_values().collect();
    if !unplaced.is_empty() {
        unplaced.sort_by(|a, b| a.title.cmp(&b.title));
        rows.push(Row::Unplaced { words: unplaced.iter().map(|s| s.words).sum() });
        rows.extend(unplaced.into_iter().map(Row::Scene));
    }
    Ok(rows)
}

/// A scene to read.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub scene: Scene,
    pub summary: String,
    /// Its chapter's title, if it's in one.
    pub chapter: Option<String>,
    /// Its place in reading order, from 1, and how many there are.
    pub piece: usize,
    pub of: usize,
    pub html: String,
    pub previous: Option<(String, String)>,
    pub next: Option<(String, String)>,
}

pub fn reading(vault: &Vault, project: &Project, slug: &str) -> Result<Reading, String> {
    let statuses = statuses(vault);
    let info = project.scene_info(slug).map_err(|e| e.to_string())?;
    let file = project.scene(slug).map_err(|e| e.to_string())?;
    let order = project.reading_order().map_err(|e| e.to_string())?;
    let at = order.iter().position(|s| s == slug).unwrap_or(0);
    let titled = |i: Option<usize>| {
        let slug = order.get(i?)?;
        Some((slug.clone(), project.scene_info(slug).ok()?.title))
    };
    let chapter = project.outline().ok().and_then(|o| o.chapters.into_iter().find(|c| c.scenes.iter().any(|s| s == slug))).map(|c| c.title);
    Ok(Reading {
        scene: scene_of(&info, &statuses),
        summary: info.summary,
        chapter,
        piece: at + 1,
        of: order.len(),
        html: render::html(&file.markdown()),
        previous: titled(at.checked_sub(1)),
        next: titled(Some(at + 1)),
    })
}

/// A note, as the sheet and its own page show it.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteCard {
    pub owner: Owner,
    pub path: String,
    pub kind: String,
    pub title: String,
    pub aliases: Vec<String>,
    /// Its summary, or else its first paragraph, as plain text.
    pub gist: String,
    pub html: String,
    /// The scenes it's in, in reading order, with how: "POV", "Cast", "Linked"…
    pub scenes: Vec<(Scene, String)>,
}

pub fn note(vault: &Vault, project: &Project, owner: &Owner, path: &str) -> Result<NoteCard, String> {
    let statuses = statuses(vault);
    let notes = vault.notes_of(owner).map_err(|e| e.to_string())?;
    let info = notes.info(path).map_err(|e| e.to_string())?;
    let markdown = notes.read(path).map_err(|e| e.to_string())?.markdown();
    let gist = if info.summary.is_empty() {
        markdown.split("\n\n").map(str::trim).find(|p| !p.is_empty() && !p.starts_with('#')).map(visible_text).unwrap_or_default()
    } else {
        info.summary.clone()
    };
    let mut scenes: Vec<(Scene, String)> = Vec::new();
    if let Ok(links) = vault.note_links(project, owner, path) {
        for appearance in &links.appears_in {
            let how = appearance.fields.iter().map(|f| field_label(f)).collect::<Vec<_>>().join(", ");
            scenes.push((scene_of(&appearance.scene, &statuses), how));
        }
        for backlink in &links.linked_from {
            if let LinkSource::Scene(info) = &backlink.from
                && !scenes.iter().any(|(s, _)| s.slug == info.slug)
            {
                scenes.push((scene_of(info, &statuses), "Linked".to_owned()));
            }
        }
        // Back into reading order, since appearances and links were each in it separately.
        if let Ok(order) = project.reading_order() {
            scenes.sort_by_key(|(s, _)| order.iter().position(|o| *o == s.slug).unwrap_or(usize::MAX));
        }
    }
    Ok(NoteCard {
        owner: owner.clone(),
        path: path.to_owned(),
        kind: info.kind.label(project.config.kind).to_owned(),
        title: info.title,
        aliases: info.aliases,
        gist,
        html: render::html(&markdown),
        scenes,
    })
}

fn field_label(field: &str) -> String {
    match field {
        "pov" => "POV".to_owned(),
        other => {
            let mut chars = other.chars();
            chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
        }
    }
}

/// The note a link names, if there is one. Of two with the same name, the first.
pub fn resolve(vault: &Vault, project: &Project, name: &str) -> Option<(Owner, String)> {
    let names = vault.names(project).ok()?;
    match names.resolve(name) {
        Resolution::Found(n) => Some((n.owner.clone(), n.note.path.clone())),
        Resolution::Ambiguous(many) => many.first().map(|n| (n.owner.clone(), n.note.path.clone())),
        Resolution::Missing => None,
    }
}

/// A note in the Notes list.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub owner: Owner,
    pub path: String,
    pub title: String,
    /// Whose it is when it isn't this project's: the world's name.
    pub from: Option<String>,
}

/// The notes this project can link to, grouped by kind in the app's order, A to Z in each.
pub fn notes(vault: &Vault, project: &Project) -> Vec<(String, Vec<Listed>)> {
    let Ok(all) = vault.reachable_notes(project) else { return Vec::new() };
    let mut groups: Vec<(needle_core::project::NoteKind, Vec<Listed>)> = Vec::new();
    for named in all {
        let listed = Listed {
            from: match &named.owner {
                Owner::World(w) => Some(vault.world(w).map(|w| w.config.name).unwrap_or_else(|_| w.clone())),
                Owner::Project(_) => None,
            },
            owner: named.owner,
            path: named.note.path,
            title: named.note.title,
        };
        match groups.iter_mut().find(|(k, _)| *k == named.note.kind) {
            Some((_, list)) => list.push(listed),
            None => groups.push((named.note.kind, vec![listed])),
        }
    }
    groups.sort_by_key(|(k, _)| needle_core::project::NoteKind::ALL.iter().position(|a| a == k));
    groups
        .into_iter()
        .map(|(kind, mut list)| {
            list.sort_by_key(|l| l.title.to_lowercase());
            (kind_heading(kind, project.config.kind), list)
        })
        .collect()
}

fn kind_heading(kind: needle_core::project::NoteKind, project: ProjectKind) -> String {
    kind.label(project).to_owned()
}

/// What search can open.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    Scene(String),
    Note(Owner, String),
}

/// Text to search and quote: what a reader sees, without Markdown's marks.
fn plain(markdown: &str) -> String {
    visible_text(markdown)
        .lines()
        .map(|line| line.trim_start_matches(['#', '>', ' ']).replace(['*', '`'], "").replace("\\", ""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every scene of the project and every note it can reach, to search through.
pub fn documents(vault: &Vault, project: &Project) -> Vec<Doc<Found>> {
    let mut docs = Vec::new();
    for slug in project.reading_order().unwrap_or_default() {
        if let (Ok(info), Ok(file)) = (project.scene_info(&slug), project.scene(&slug)) {
            docs.push(Doc { item: Found::Scene(slug), title: info.title, text: plain(&file.markdown()) });
        }
    }
    for named in vault.reachable_notes(project).unwrap_or_default() {
        let Ok(notes) = vault.notes_of(&named.owner) else { continue };
        let Ok(file) = notes.read(&named.note.path) else { continue };
        docs.push(Doc { item: Found::Note(named.owner, named.note.path), title: named.note.title, text: plain(&file.markdown()) });
    }
    docs
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::SystemTime;

    use needle_vault::store::Snapshot;

    use super::*;

    /// The sample vault, held in a snapshot the way the phone holds what it fetched.
    fn sample() -> Vault {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-vault");
        let snapshot = Snapshot::new(SystemTime::UNIX_EPOCH);
        fn walk(dir: &Path, root: &Path, snapshot: &Snapshot) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, root, snapshot);
                } else {
                    let rel: PathBuf = path.strip_prefix(root).unwrap().to_owned();
                    snapshot.add_path(&rel, "sha");
                    snapshot.add_contents(&rel, std::fs::read(&path).unwrap());
                }
            }
        }
        walk(&root, &root, &snapshot);
        Vault::open_on(Arc::new(snapshot), Path::new("")).unwrap()
    }

    #[test]
    fn the_outline_has_chapters_and_their_scenes() {
        let vault = sample();
        let project = vault.project("tidewater").unwrap();
        let rows = outline(&vault, &project).unwrap();
        let scenes: Vec<&str> = rows.iter().filter_map(|r| if let Row::Scene(s) = r { Some(s.title.as_str()) } else { None }).collect();
        assert!(scenes.contains(&"The night market"), "{scenes:?}");
        assert!(matches!(rows.iter().find(|r| matches!(r, Row::Chapter { .. })), Some(Row::Chapter { number: 1, .. })));
        let chapter_words: usize = rows.iter().filter_map(|r| if let Row::Chapter { words, .. } = r { Some(*words) } else { None }).sum();
        let scene_words: usize = rows.iter().filter_map(|r| if let Row::Scene(s) = r { Some(s.words) } else { None }).sum();
        let unplaced: usize = rows.iter().filter_map(|r| if let Row::Unplaced { words } = r { Some(*words) } else { None }).sum();
        assert_eq!(chapter_words + unplaced, scene_words);
    }

    #[test]
    fn a_scene_reads_with_its_links_and_neighbours() {
        let vault = sample();
        let project = vault.project("tidewater").unwrap();
        let reading = reading(&vault, &project, "night-market").unwrap();
        assert_eq!(reading.scene.title, "The night market");
        assert!(reading.html.contains(r#"data-target="Mara Venn""#), "{}", reading.html);
        assert!(reading.piece >= 1 && reading.piece <= reading.of);
        assert!(reading.previous.is_some() || reading.next.is_some());
    }

    #[test]
    fn a_link_opens_its_note_with_the_scenes_its_in() {
        let vault = sample();
        let project = vault.project("tidewater").unwrap();
        let (owner, path) = resolve(&vault, &project, "Mara").expect("Mara is an alias");
        let card = note(&vault, &project, &owner, &path).unwrap();
        assert_eq!(card.title, "Mara Venn");
        assert_eq!(card.kind, "Character");
        assert!(card.scenes.iter().any(|(s, _)| s.slug == "night-market"), "{:?}", card.scenes);
        assert!(resolve(&vault, &project, "Nobody at all").is_none());
    }

    #[test]
    fn notes_are_grouped_by_kind() {
        let vault = sample();
        let project = vault.project("tidewater").unwrap();
        let groups = notes(&vault, &project);
        assert_eq!(groups.first().map(|(k, _)| k.as_str()), Some("Character"));
        assert!(groups.iter().any(|(_, list)| list.iter().any(|n| n.title == "Old Teodor")));
    }

    #[test]
    fn search_reaches_scenes_and_notes() {
        let vault = sample();
        let project = vault.project("tidewater").unwrap();
        let docs = documents(&vault, &project);
        let hits = crate::search::search(&docs, "compass");
        let hit = hits.iter().find(|h| h.item == Found::Scene("night-market".into())).expect("found");
        assert!(!hit.after.contains('*'), "Markdown marks are left out: {}", hit.after);
    }
}

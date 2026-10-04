//! Commands for notes and what points at them. A note is named by its owner (a project or world
//! folder, with `world` saying which) and its path inside it, e.g. `characters/mara-venn`.

use needle_core::names::Owner;
use needle_core::project::NoteKind;
use needle_vault::{LinkSource, NamedNote, NoteInfo};
use serde::Serialize;
use tauri::State;

use crate::state::{AppState, OrString};
use crate::workspace::SceneView;

#[derive(Serialize, Clone)]
pub struct NoteView {
    /// The project or world folder the note belongs to.
    owner: String,
    world: bool,
    path: String,
    id: String,
    kind: &'static str,
    title: String,
    aliases: Vec<String>,
    summary: String,
}

#[derive(Serialize)]
pub struct OpenedNote {
    note: NoteView,
    markdown: String,
}

#[derive(Serialize)]
pub struct NoteLinksView {
    appears_in: Vec<AppearanceView>,
    linked_from: Vec<BacklinkView>,
    mentioned_in: Vec<MentionView>,
}

#[derive(Serialize)]
pub struct AppearanceView {
    scene: SceneView,
    fields: Vec<&'static str>,
}

/// Exactly one of `scene` and `note` is set.
#[derive(Serialize)]
pub struct BacklinkView {
    scene: Option<SceneView>,
    note: Option<NoteView>,
    count: usize,
}

#[derive(Serialize)]
pub struct MentionView {
    scene: SceneView,
    count: usize,
    before: String,
    name: String,
    after: String,
}

fn note_view(owner: &Owner, note: NoteInfo) -> NoteView {
    let (slug, world) = match owner {
        Owner::Project(slug) => (slug.clone(), false),
        Owner::World(slug) => (slug.clone(), true),
    };
    NoteView {
        owner: slug,
        world,
        path: note.path,
        id: note.id,
        kind: note.kind.as_str(),
        title: note.title,
        aliases: note.aliases,
        summary: note.summary,
    }
}

fn named_view(named: NamedNote) -> NoteView {
    note_view(&named.owner, named.note)
}

fn owner(slug: String, world: bool) -> Owner {
    if world { Owner::World(slug) } else { Owner::Project(slug) }
}

#[derive(Serialize)]
pub struct ProjectNotes {
    notes: Vec<NoteView>,
    /// The project's world, if any: (folder, name).
    world: Option<(String, String)>,
}

/// Every note a link in `project` can reach (every project's, and its world's), and its world.
#[tauri::command]
pub fn project_notes(state: State<'_, AppState>, project: String) -> Result<ProjectNotes, String> {
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        let world = open.vault.world_of(&project).or_string()?;
        Ok(ProjectNotes {
            notes: open.vault.reachable_notes(&project).or_string()?.into_iter().map(named_view).collect(),
            world: world.map(|w| (w.slug, w.config.name)),
        })
    })
}

#[tauri::command]
pub fn open_note(state: State<'_, AppState>, owner: String, world: bool, path: String) -> Result<OpenedNote, String> {
    let owner = self::owner(owner, world);
    state.with(|open| {
        let notes = open.vault.notes_of(&owner).or_string()?;
        let markdown = notes.read(&path).or_string()?.markdown();
        Ok(OpenedNote {
            note: note_view(&owner, notes.info(&path).or_string()?),
            markdown,
        })
    })
}

/// Saves a note's text, keeping its header. Synchronous, like `save_scene`, so saves stay in
/// order.
#[tauri::command]
pub fn save_note(state: State<'_, AppState>, owner: String, world: bool, path: String, markdown: String) -> Result<(), String> {
    let owner = self::owner(owner, world);
    state.with(|open| {
        if open.vault.notes_of(&owner).or_string()?.save_body(&path, &markdown).or_string()? {
            open.history.edited();
        }
        Ok(())
    })
}

/// Makes a note in `project` from the vault's template for `kind`.
#[tauri::command]
pub fn create_note(state: State<'_, AppState>, project: String, kind: String, title: String) -> Result<NoteView, String> {
    let kind = NoteKind::parse(&kind).ok_or_else(|| format!("{kind:?} isn't a type of note"))?;
    state.with(|open| {
        let notes = open.vault.project(&project).or_string()?.notes();
        let note = notes.create(kind, &title).or_string()?;
        open.history.edited();
        Ok(note_view(notes.owner(), note))
    })
}

/// Retitles a note and follows every link to it.
#[tauri::command]
pub fn rename_note(state: State<'_, AppState>, owner: String, world: bool, path: String, title: String) -> Result<NoteView, String> {
    let owner = self::owner(owner, world);
    state.with(|open| {
        let renamed = open.vault.rename_note(&owner, &path, &title).or_string()?;
        open.history.edited();
        Ok(note_view(&owner, renamed.note))
    })
}

#[tauri::command]
pub fn set_note_aliases(
    state: State<'_, AppState>,
    owner: String,
    world: bool,
    path: String,
    aliases: Vec<String>,
) -> Result<NoteView, String> {
    let owner = self::owner(owner, world);
    let aliases: Vec<&str> = aliases.iter().map(|a| a.trim()).filter(|a| !a.is_empty()).collect();
    state.with(|open| {
        let notes = open.vault.notes_of(&owner).or_string()?;
        let note = notes.update_header(&path, |h| h.set_list("aliases", &aliases)).or_string()?;
        open.history.edited();
        Ok(note_view(&owner, note))
    })
}

/// What points at a note from inside `project`.
#[tauri::command]
pub fn note_links(
    state: State<'_, AppState>,
    project: String,
    owner: String,
    world: bool,
    path: String,
) -> Result<NoteLinksView, String> {
    let owner = self::owner(owner, world);
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        let links = open.vault.note_links(&project, &owner, &path).or_string()?;
        Ok(NoteLinksView {
            appears_in: links
                .appears_in
                .into_iter()
                .map(|a| AppearanceView { scene: a.scene.into(), fields: a.fields })
                .collect(),
            linked_from: links
                .linked_from
                .into_iter()
                .map(|b| match b.from {
                    LinkSource::Scene(scene) => BacklinkView { scene: Some(scene.into()), note: None, count: b.count },
                    LinkSource::Note(note) => BacklinkView { scene: None, note: Some(named_view(note)), count: b.count },
                })
                .collect(),
            mentioned_in: links
                .mentioned_in
                .into_iter()
                .map(|m| MentionView {
                    scene: m.scene.into(),
                    count: m.count,
                    before: m.before,
                    name: m.name,
                    after: m.after,
                })
                .collect(),
        })
    })
}

/// Turns the first unlinked mention of a note in `scene` into a link.
#[tauri::command]
pub fn link_mention(
    state: State<'_, AppState>,
    project: String,
    scene: String,
    owner: String,
    world: bool,
    path: String,
) -> Result<bool, String> {
    let owner = self::owner(owner, world);
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        let linked = open.vault.link_mention(&project, &scene, &owner, &path).or_string()?;
        if linked {
            open.history.edited();
        }
        Ok(linked)
    })
}

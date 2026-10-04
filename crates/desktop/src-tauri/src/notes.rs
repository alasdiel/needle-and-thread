//! Commands for notes and what points at them. A note is named by its owner (a project or world
//! folder, with `world` saying which) and its path inside it, e.g. `characters/mara-venn`.

use needle_core::names::Owner;
use needle_core::project::{NoteKind, ProjectKind};
use needle_vault::{LinkSource, NamedNote, NoteInfo, SceneNames};
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

/// Moves a project's note into the project's world.
#[tauri::command]
pub fn promote_note(state: State<'_, AppState>, project: String, path: String) -> Result<NoteView, String> {
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        let note = open.vault.promote_note(&project, &path).or_string()?;
        open.history.edited();
        let world = project.config.world.clone().unwrap_or_default();
        Ok(note_view(&Owner::World(world), note))
    })
}

#[derive(Serialize)]
pub struct ProjectSettings {
    title: String,
    kind: &'static str,
    world: Option<String>,
    /// Every world in the vault: (folder, name).
    worlds: Vec<(String, String)>,
}

#[tauri::command]
pub fn project_settings(state: State<'_, AppState>, project: String) -> Result<ProjectSettings, String> {
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        Ok(ProjectSettings {
            title: project.config.title.clone(),
            kind: match project.config.kind {
                ProjectKind::Fiction => "fiction",
                ProjectKind::Nonfiction => "nonfiction",
            },
            world: project.config.world.clone(),
            worlds: open.vault.worlds().or_string()?.into_iter().map(|w| (w.slug, w.config.name)).collect(),
        })
    })
}

/// Changes a project's title, kind and world. With `new_world`, makes that world first and puts
/// the project in it.
#[tauri::command]
pub fn update_project(
    state: State<'_, AppState>,
    project: String,
    title: String,
    kind: String,
    world: Option<String>,
    new_world: Option<String>,
) -> Result<(), String> {
    state.with(|open| {
        let world = match new_world.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
            Some(name) => Some(open.vault.create_world(name).or_string()?.slug),
            None => world.filter(|w| !w.is_empty()),
        };
        open.vault
            .update_project(&project, |c| {
                c.title = title;
                c.kind = if kind == "nonfiction" { ProjectKind::Nonfiction } else { ProjectKind::Fiction };
                c.world = world;
            })
            .or_string()?;
        open.history.edited();
        Ok(())
    })
}

#[derive(Serialize)]
pub struct SceneNamesView {
    fields: Vec<FieldView>,
    hints: Vec<HintView>,
}

#[derive(Serialize)]
pub struct FieldView {
    field: &'static str,
    names: Vec<HeaderNameView>,
}

#[derive(Serialize)]
pub struct HeaderNameView {
    name: String,
    /// None if no note has the name yet.
    note: Option<NoteView>,
}

#[derive(Serialize)]
pub struct HintView {
    field: &'static str,
    note: NoteView,
}

fn scene_names_view(names: SceneNames) -> SceneNamesView {
    SceneNamesView {
        fields: names
            .fields
            .into_iter()
            .map(|(field, names)| FieldView {
                field,
                names: names
                    .into_iter()
                    .map(|n| HeaderNameView { name: n.name, note: n.note.map(named_view) })
                    .collect(),
            })
            .collect(),
        hints: names
            .hints
            .into_iter()
            .map(|h| HintView { field: h.field, note: named_view(h.note) })
            .collect(),
    }
}

/// A scene's pov, cast, places and threads, and notes its text names that those don't list.
#[tauri::command]
pub fn scene_names(state: State<'_, AppState>, project: String, scene: String) -> Result<SceneNamesView, String> {
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        Ok(scene_names_view(open.vault.scene_names(&project, &scene).or_string()?))
    })
}

/// Sets one of a scene's name fields and returns them all again.
#[tauri::command]
pub fn set_scene_names(
    state: State<'_, AppState>,
    project: String,
    scene: String,
    field: String,
    names: Vec<String>,
) -> Result<SceneNamesView, String> {
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        open.vault.set_scene_names(&project, &scene, &field, &names).or_string()?;
        open.history.edited();
        Ok(scene_names_view(open.vault.scene_names(&project, &scene).or_string()?))
    })
}

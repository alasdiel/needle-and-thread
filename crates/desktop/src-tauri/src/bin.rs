//! Commands for the cut bin: cutting a passage from a scene, listing what's in the bin, and
//! putting things back. Scenes and notes are cut by their own commands.

use needle_core::names::Owner;
use needle_vault::{CutItem, CutKind, Passage};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::notes::{self, NoteView};
use crate::state::{AppState, OrString};
use crate::workspace::{self, Created};

#[derive(Serialize)]
pub struct CutView {
    /// The project or world folder whose bin it's in.
    owner: String,
    world: bool,
    /// Its file's name in `cut/`.
    name: String,
    /// "passage", "scene" or "note".
    kind: &'static str,
    cut_at: String,
    title: String,
    scene: Option<String>,
    chapter: Option<String>,
    note_kind: Option<&'static str>,
    words: usize,
    passage: Option<PassageView>,
}

/// A passage and what was around it, as the editor makes and restores it.
#[derive(Serialize, Deserialize)]
pub struct PassageView {
    markdown: String,
    text_before: String,
    text_after: String,
    starts_paragraph: bool,
    ends_paragraph: bool,
    #[serde(default)]
    starts_with_space: bool,
    #[serde(default)]
    ends_with_space: bool,
}

impl From<Passage> for PassageView {
    fn from(p: Passage) -> Self {
        Self {
            markdown: p.markdown,
            text_before: p.text_before,
            text_after: p.text_after,
            starts_paragraph: p.starts_paragraph,
            ends_paragraph: p.ends_paragraph,
            starts_with_space: p.starts_with_space,
            ends_with_space: p.ends_with_space,
        }
    }
}

impl From<PassageView> for Passage {
    fn from(p: PassageView) -> Self {
        Self {
            markdown: p.markdown,
            text_before: p.text_before,
            text_after: p.text_after,
            starts_paragraph: p.starts_paragraph,
            ends_paragraph: p.ends_paragraph,
            starts_with_space: p.starts_with_space,
            ends_with_space: p.ends_with_space,
        }
    }
}

fn cut_view(item: CutItem) -> CutView {
    let (owner, world) = match item.owner {
        Owner::Project(slug) => (slug, false),
        Owner::World(slug) => (slug, true),
    };
    CutView {
        owner,
        world,
        name: item.name,
        kind: match item.kind {
            CutKind::Passage => "passage",
            CutKind::Scene => "scene",
            CutKind::Note => "note",
        },
        cut_at: item.cut_at,
        title: item.title,
        scene: item.scene,
        chapter: item.chapter,
        note_kind: item.note_kind.map(|k| k.as_str()),
        words: item.words,
        passage: item.passage.map(PassageView::from),
    }
}

/// What the project's bin holds, with notes cut from its world, the most recently cut first.
#[tauri::command]
pub fn bin_items(state: State<'_, AppState>, project: String) -> Result<Vec<CutView>, String> {
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        Ok(open.vault.bin_items(&project).or_string()?.into_iter().map(cut_view).collect())
    })
}

/// Puts a passage cut from `scene` in the bin. Synchronous, like `save_scene`, so it lands
/// before the save that takes the passage out of the scene.
#[tauri::command]
pub fn cut_passage(state: State<'_, AppState>, project: String, scene: String, passage: PassageView) -> Result<CutView, String> {
    state.with(|open| {
        let item = open.vault.project(&project).or_string()?.cut_passage(&scene, &passage.into()).or_string()?;
        open.history.edited();
        Ok(cut_view(item))
    })
}

/// Takes a passage out of the bin once it's back in a scene.
#[tauri::command]
pub fn remove_from_bin(state: State<'_, AppState>, owner: String, world: bool, name: String) -> Result<(), String> {
    state.with(|open| {
        open.vault.bin_of(&notes::owner(owner, world)).or_string()?.remove_passage(&name).or_string()?;
        open.history.edited();
        Ok(())
    })
}

/// Puts a scene from the bin back in the manuscript and the outline.
#[tauri::command]
pub fn restore_scene(state: State<'_, AppState>, project: String, name: String) -> Result<Created, String> {
    let (scene, outline) = workspace::change(&state, &project, |p| p.restore_scene(&name))?;
    Ok(Created { outline, scene: scene.slug })
}

/// Puts a note from its owner's bin back among its notes.
#[tauri::command]
pub fn restore_note(state: State<'_, AppState>, owner: String, world: bool, name: String) -> Result<NoteView, String> {
    let owner = notes::owner(owner, world);
    state.with(|open| {
        let note = open.vault.notes_of(&owner).or_string()?.restore(&name).or_string()?;
        open.history.edited();
        Ok(notes::note_view(&owner, note))
    })
}

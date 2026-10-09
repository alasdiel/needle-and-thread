//! Commands for vaults, projects, the outline and scenes. Each outline-changing command returns
//! the updated outline, so the frontend never has to guess what happened.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use needle_core::project::ProjectKind;
use needle_core::settings::{TypographyRule, TypographySettings};
use needle_vault::{Placement, Project, SceneInfo, Vault};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::state::{AppState, OpenVault, OrString};

#[derive(Serialize)]
pub struct VaultView {
    path: String,
    projects: Vec<ProjectView>,
    typography: TypographySettings,
}

#[derive(Serialize, Clone)]
pub struct ProjectView {
    slug: String,
    title: String,
    kind: &'static str,
}

#[derive(Serialize)]
pub struct OutlineView {
    project: ProjectView,
    statuses: Vec<String>,
    chapters: Vec<ChapterView>,
    /// Scene files the outline doesn't place anywhere.
    unplaced: Vec<SceneView>,
}

#[derive(Serialize)]
pub struct ChapterView {
    id: String,
    title: String,
    part: Option<String>,
    summary: String,
    scenes: Vec<SceneView>,
}

#[derive(Serialize, Clone)]
pub struct SceneView {
    slug: String,
    id: String,
    title: String,
    status: String,
    summary: String,
    words: usize,
}

#[derive(Serialize)]
pub struct OpenedScene {
    scene: SceneView,
    markdown: String,
}

#[derive(Serialize)]
pub struct Created {
    pub(crate) outline: OutlineView,
    pub(crate) scene: String,
}

impl From<SceneInfo> for SceneView {
    fn from(s: SceneInfo) -> Self {
        Self {
            slug: s.slug,
            id: s.id,
            title: s.title,
            status: s.status,
            summary: s.summary,
            words: s.words,
        }
    }
}

fn project_view(project: &Project) -> ProjectView {
    ProjectView {
        slug: project.slug.clone(),
        title: project.config.title.clone(),
        kind: match project.config.kind {
            ProjectKind::Fiction => "fiction",
            ProjectKind::Nonfiction => "nonfiction",
        },
    }
}

fn vault_view(vault: &Vault) -> Result<VaultView, String> {
    Ok(VaultView {
        path: vault.root().to_string_lossy().into_owned(),
        projects: vault.projects().or_string()?.iter().map(project_view).collect(),
        typography: vault.settings().or_string()?.typography,
    })
}

fn outline_view(open: &OpenVault, project: &Project) -> Result<OutlineView, String> {
    let outline = project.outline().or_string()?;
    let mut scenes: HashMap<String, SceneView> = project
        .scenes()
        .or_string()?
        .into_iter()
        .map(|s| (s.slug.clone(), s.into()))
        .collect();
    let chapters = outline
        .chapters
        .into_iter()
        .map(|c| ChapterView {
            scenes: c.scenes.iter().filter_map(|slug| scenes.remove(slug)).collect(),
            id: c.id,
            title: c.title,
            part: c.part,
            summary: c.summary,
        })
        .collect();
    let mut unplaced: Vec<SceneView> = scenes.into_values().collect();
    unplaced.sort_by(|a, b| a.title.cmp(&b.title));
    Ok(OutlineView {
        project: project_view(project),
        statuses: open.vault.settings().or_string()?.statuses,
        chapters,
        unplaced,
    })
}

/// Runs `edit` on a project and returns its updated outline, counting it as an edit for
/// snapshots.
pub(crate) fn change<T>(
    state: &AppState,
    project: &str,
    edit: impl FnOnce(&Project) -> needle_vault::Result<T>,
) -> Result<(T, OutlineView), String> {
    state.with(|open| {
        let project = open.vault.project(project).or_string()?;
        let result = edit(&project).or_string()?;
        open.history.edited();
        Ok((result, outline_view(open, &project)?))
    })
}

// --- Vaults -----------------------------------------------------------------------------

#[tauri::command]
pub fn current_vault(state: State<'_, AppState>) -> Result<Option<VaultView>, String> {
    match state.lock().as_ref() {
        Some(open) => vault_view(&open.vault).map(Some),
        None => Ok(None),
    }
}

/// Asks for a folder and opens it as a vault. Returns None if the dialog was cancelled.
#[tauri::command]
pub async fn open_vault(app: AppHandle, state: State<'_, AppState>) -> Result<Option<VaultView>, String> {
    let Some(folder) = pick_folder(&app, "Open a vault") else {
        return Ok(None);
    };
    let vault = Vault::open(&folder).or_string()?;
    let view = vault_view(&vault)?;
    state.open(&app, vault, true)?;
    Ok(Some(view))
}

/// Asks for an empty folder (or an existing vault) and makes it the vault.
#[tauri::command]
pub async fn create_vault(app: AppHandle, state: State<'_, AppState>) -> Result<Option<VaultView>, String> {
    let Some(folder) = pick_folder(&app, "Choose an empty folder for your vault") else {
        return Ok(None);
    };
    if !Vault::is_vault(&folder) && fs::read_dir(&folder).or_string()?.next().is_some() {
        return Err("that folder isn't empty; choose or create an empty one".into());
    }
    let vault = Vault::create(&folder).or_string()?;
    let view = vault_view(&vault)?;
    state.open(&app, vault, true)?;
    Ok(Some(view))
}

/// Opens a copy of the sample vault kept in the app's data folder, creating it the first time.
#[tauri::command]
pub fn open_sample_vault(app: AppHandle, state: State<'_, AppState>) -> Result<VaultView, String> {
    let root = app.path().app_data_dir().or_string()?.join("sample-vault");
    if !Vault::is_vault(&root) {
        write_sample(&root)?;
    }
    let vault = Vault::open(&root).or_string()?;
    let view = vault_view(&vault)?;
    state.open(&app, vault, true)?;
    Ok(view)
}

/// Switches one of the automatic typography changes, for the whole vault.
#[tauri::command]
pub fn set_typography(state: State<'_, AppState>, rule: TypographyRule, on: bool) -> Result<(), String> {
    state.with(|open| open.vault.set_typography(rule, on).or_string())
}

fn pick_folder(app: &AppHandle, title: &str) -> Option<PathBuf> {
    app.dialog().file().set_title(title).blocking_pick_folder()?.into_path().ok()
}

const SAMPLE: &[(&str, &str)] = &[
    (".needle/vault.toml", include_str!("../../../../fixtures/sample-vault/.needle/vault.toml")),
    (
        "projects/tidewater/project.toml",
        include_str!("../../../../fixtures/sample-vault/projects/tidewater/project.toml"),
    ),
    (
        "projects/tidewater/outline.toml",
        include_str!("../../../../fixtures/sample-vault/projects/tidewater/outline.toml"),
    ),
    (
        "projects/tidewater/manuscript/night-market.md",
        include_str!("../../../../fixtures/sample-vault/projects/tidewater/manuscript/night-market.md"),
    ),
    (
        "projects/tidewater/manuscript/long-chapter.md",
        include_str!("../../../../fixtures/sample-vault/projects/tidewater/manuscript/long-chapter.md"),
    ),
    (
        "projects/tidewater/notes/characters/mara-venn.md",
        include_str!("../../../../fixtures/sample-vault/projects/tidewater/notes/characters/mara-venn.md"),
    ),
    (
        "projects/tidewater/notes/characters/old-teodor.md",
        include_str!("../../../../fixtures/sample-vault/projects/tidewater/notes/characters/old-teodor.md"),
    ),
    (
        "projects/tidewater/notes/places/night-market.md",
        include_str!("../../../../fixtures/sample-vault/projects/tidewater/notes/places/night-market.md"),
    ),
    (
        "projects/tidewater/notes/threads/the-missing-ledger.md",
        include_str!("../../../../fixtures/sample-vault/projects/tidewater/notes/threads/the-missing-ledger.md"),
    ),
    (
        "projects/tidewater/notes/events/the-harbor.md",
        include_str!("../../../../fixtures/sample-vault/projects/tidewater/notes/events/the-harbor.md"),
    ),
];

fn write_sample(root: &Path) -> Result<(), String> {
    for (path, contents) in SAMPLE {
        let path = root.join(path);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).or_string()?;
        }
        fs::write(path, contents).or_string()?;
    }
    Ok(())
}

// --- Projects ---------------------------------------------------------------------------

#[tauri::command]
pub fn create_project(state: State<'_, AppState>, title: String, kind: String) -> Result<ProjectView, String> {
    let kind = match kind.as_str() {
        "nonfiction" => ProjectKind::Nonfiction,
        _ => ProjectKind::Fiction,
    };
    state.with(|open| {
        let project = open.vault.create_project(&title, kind).or_string()?;
        open.history.edited();
        Ok(project_view(&project))
    })
}

#[tauri::command]
pub fn project_outline(state: State<'_, AppState>, project: String) -> Result<OutlineView, String> {
    state.with(|open| outline_view(open, &open.vault.project(&project).or_string()?))
}

// --- Scenes -----------------------------------------------------------------------------

#[tauri::command]
pub fn open_scene(state: State<'_, AppState>, project: String, scene: String) -> Result<OpenedScene, String> {
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        let file = project.scene(&scene).or_string()?;
        Ok(OpenedScene {
            scene: project.scene_info(&scene).or_string()?.into(),
            markdown: file.markdown(),
        })
    })
}

/// Saves the scene's text, keeping its header. Synchronous on purpose: Tauri runs sync
/// commands one at a time, so two quick saves can't land out of order.
#[tauri::command]
pub fn save_scene(state: State<'_, AppState>, project: String, scene: String, markdown: String) -> Result<(), String> {
    state.with(|open| {
        if open.vault.project(&project).or_string()?.save_body(&scene, &markdown).or_string()? {
            open.history.edited();
        }
        Ok(())
    })
}

/// Creates a scene after `after`, or at the end of `chapter`.
#[tauri::command]
pub fn create_scene(
    state: State<'_, AppState>,
    project: String,
    title: String,
    chapter: Option<String>,
    after: Option<String>,
) -> Result<Created, String> {
    let placement = match (after, chapter) {
        (Some(scene), _) => Placement::After { scene },
        (None, Some(chapter)) => Placement::End { chapter },
        (None, None) => return Err("say where the new scene goes".into()),
    };
    let (scene, outline) = change(&state, &project, |p| p.create_scene(&title, placement))?;
    Ok(Created { outline, scene: scene.slug })
}

#[tauri::command]
pub fn rename_scene(state: State<'_, AppState>, project: String, scene: String, title: String) -> Result<OutlineView, String> {
    let title = title.trim().to_owned();
    if title.is_empty() {
        return Err("a scene needs a title".into());
    }
    change(&state, &project, |p| p.update_header(&scene, |h| h.set_str("title", &title))).map(|(_, o)| o)
}

#[tauri::command]
pub fn set_scene_status(state: State<'_, AppState>, project: String, scene: String, status: String) -> Result<OutlineView, String> {
    change(&state, &project, |p| p.update_header(&scene, |h| h.set_str("status", &status))).map(|(_, o)| o)
}

/// Sets a scene's summary; an empty one is taken out of its header.
#[tauri::command]
pub fn set_scene_summary(state: State<'_, AppState>, project: String, scene: String, summary: String) -> Result<OutlineView, String> {
    change(&state, &project, |p| p.set_summary(&scene, &summary)).map(|(_, o)| o)
}

#[tauri::command]
pub fn cut_scene(state: State<'_, AppState>, project: String, scene: String) -> Result<OutlineView, String> {
    change(&state, &project, |p| p.cut_scene(&scene)).map(|(_, o)| o)
}

/// Splits a scene: `before` stays, `after` becomes a new scene right after it.
#[tauri::command]
pub fn split_scene(
    state: State<'_, AppState>,
    project: String,
    scene: String,
    before: String,
    after: String,
) -> Result<Created, String> {
    let (new, outline) = change(&state, &project, |p| p.split_scene(&scene, &before, &after))?;
    Ok(Created { outline, scene: new.slug })
}

#[tauri::command]
pub fn merge_scene(state: State<'_, AppState>, project: String, scene: String) -> Result<OutlineView, String> {
    change(&state, &project, |p| p.merge_with_next(&scene)).map(|(_, o)| o)
}

/// Moves a scene to `index` in `chapter`, counting positions before the move.
#[tauri::command]
pub fn move_scene(
    state: State<'_, AppState>,
    project: String,
    scene: String,
    chapter: String,
    index: usize,
) -> Result<OutlineView, String> {
    change(&state, &project, |p| {
        p.edit_outline(|o| {
            // An unplaced scene isn't in the outline yet; place it instead of moving it.
            if o.locate(&scene).is_some() {
                o.move_scene(&scene, &chapter, index)
            } else {
                o.insert_scene(&chapter, index, &scene)
            }
        })
    })
    .map(|(_, o)| o)
}

// --- Chapters ---------------------------------------------------------------------------

/// Adds a chapter after `after` (or at the end), in the same part as the chapter before it.
#[tauri::command]
pub fn add_chapter(state: State<'_, AppState>, project: String, title: String, after: Option<String>) -> Result<OutlineView, String> {
    change(&state, &project, |p| p.add_chapter(&title, after.as_deref())).map(|(_, o)| o)
}

#[tauri::command]
pub fn rename_chapter(state: State<'_, AppState>, project: String, chapter: String, title: String) -> Result<OutlineView, String> {
    change(&state, &project, |p| {
        p.edit_outline(|o| {
            o.chapter_mut(&chapter)?.title = title.trim().to_owned();
            Ok(())
        })
    })
    .map(|(_, o)| o)
}

/// Sets the chapter's part; an empty `part` takes it out of any part.
#[tauri::command]
pub fn set_chapter_part(state: State<'_, AppState>, project: String, chapter: String, part: String) -> Result<OutlineView, String> {
    let part = Some(part.trim().to_owned()).filter(|p| !p.is_empty());
    change(&state, &project, |p| {
        p.edit_outline(|o| {
            o.chapter_mut(&chapter)?.part = part;
            Ok(())
        })
    })
    .map(|(_, o)| o)
}

#[tauri::command]
pub fn move_chapter(state: State<'_, AppState>, project: String, chapter: String, index: usize) -> Result<OutlineView, String> {
    change(&state, &project, |p| p.edit_outline(|o| o.move_chapter(&chapter, index))).map(|(_, o)| o)
}

/// Removes an empty chapter.
#[tauri::command]
pub fn remove_chapter(state: State<'_, AppState>, project: String, chapter: String) -> Result<OutlineView, String> {
    change(&state, &project, |p| p.edit_outline(|o| o.remove_chapter(&chapter).map(|_| ()))).map(|(_, o)| o)
}

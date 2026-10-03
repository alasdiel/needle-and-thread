//! Spike-only commands for opening and saving a scene. They accept any path; phase 1 limits
//! them to the vault.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use needle_core::scene::SceneFile;
use serde::Serialize;
use tauri::{AppHandle, Manager};

const SAMPLES: &[(&str, &str)] = &[
    (
        "night-market",
        include_str!("../../../../fixtures/sample-vault/projects/tidewater/manuscript/night-market.md"),
    ),
    (
        "long-chapter",
        include_str!("../../../../fixtures/sample-vault/projects/tidewater/manuscript/long-chapter.md"),
    ),
];

#[derive(Serialize)]
pub struct SampleScene {
    name: String,
    path: String,
}

#[derive(Serialize)]
pub struct OpenedScene {
    path: String,
    markdown: String,
}

/// Where the spike keeps its stand-in vault, inside the app's data dir.
pub fn spike_vault(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("spike-vault"))
}

/// Copies the sample scenes into the spike vault (once, so edits survive restarts) and
/// returns their paths.
#[tauri::command]
pub fn sample_scenes(app: AppHandle) -> Result<Vec<SampleScene>, String> {
    let dir = spike_vault(&app)?.join("projects/tidewater/manuscript");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    SAMPLES
        .iter()
        .map(|(name, contents)| {
            let path = dir.join(format!("{name}.md"));
            if !path.exists() {
                fs::write(&path, contents).map_err(|e| e.to_string())?;
            }
            Ok(SampleScene {
                name: (*name).to_owned(),
                path: path.to_string_lossy().into_owned(),
            })
        })
        .collect()
}

#[tauri::command]
pub fn open_scene(path: String) -> Result<OpenedScene, String> {
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(OpenedScene {
        markdown: SceneFile::parse(&text).markdown(),
        path,
    })
}

/// Replaces the scene's body. Front matter is re-read from disk rather than remembered from
/// `open_scene`, so metadata edited elsewhere in the meantime is kept.
///
/// Synchronous on purpose: Tauri runs sync commands one at a time on the main thread, so two
/// quick saves can't land out of order.
#[tauri::command]
pub fn save_scene(path: String, markdown: String) -> Result<(), String> {
    let path = Path::new(&path);
    let existing = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut scene = SceneFile::parse(&existing);
    scene.set_markdown(&markdown);
    let updated = scene.to_string();
    if updated == existing {
        return Ok(());
    }
    write_atomically(path, updated.as_bytes()).map_err(|e| e.to_string())
}

/// Writes to a temporary file beside `path` and renames it into place, so a crash mid-write
/// leaves the old file intact rather than a truncated one.
fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("scene path has no parent directory"))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;
    // NamedTempFile is created 0600; keep the original file's permissions.
    fs::set_permissions(tmp.path(), fs::metadata(path)?.permissions())?;
    tmp.persist(path)?;
    Ok(())
}

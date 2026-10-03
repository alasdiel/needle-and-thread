//! Automatic snapshots of the open vault, and the commands behind the History panel.

use std::{
    env,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    thread,
    time::{Duration, Instant},
};

use needle_core::{scene::SceneFile, settings::SnapshotSettings, words::count_markdown_words};
use needle_vault::Vault;
use needle_vcs::{Scheduler, Vault as Repo};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::{AppState, OpenVault, OrString};

/// Tells the frontend to refresh the History panel.
const SNAPSHOT_EVENT: &str = "snapshot-taken";

pub struct History {
    repo: Mutex<Repo>,
    scheduler: Mutex<Scheduler>,
}

impl History {
    /// Opens (or starts) the vault's history and snapshots anything changed outside the app.
    pub fn open(app: &AppHandle, vault: &Vault) -> Result<Self, String> {
        let settings = vault.settings().or_string()?;
        let history = Self {
            repo: Mutex::new(Repo::open_or_init(vault.root()).or_string()?),
            scheduler: Mutex::new(scheduler(settings.snapshots)),
        };
        history.snapshot(app, None)?;
        Ok(history)
    }

    pub fn edited(&self) {
        self.scheduler().edited(Instant::now());
    }

    fn is_due(&self) -> bool {
        self.scheduler().is_due(Instant::now())
    }

    pub fn has_changes(&self) -> bool {
        self.scheduler().has_changes()
    }

    /// Snapshots any changes. Returns whether a snapshot was taken.
    pub fn snapshot(&self, app: &AppHandle, message: Option<&str>) -> Result<bool, String> {
        let taken = self.repo().snapshot(message).or_string()?.is_some();
        self.scheduler().taken();
        if taken {
            let _ = app.emit(SNAPSHOT_EVENT, ());
        }
        Ok(taken)
    }

    fn repo(&self) -> MutexGuard<'_, Repo> {
        self.repo.lock().expect("a snapshot panicked")
    }

    fn scheduler(&self) -> MutexGuard<'_, Scheduler> {
        self.scheduler.lock().expect("a snapshot panicked")
    }
}

/// Timings come from the vault's settings. `NEEDLE_SNAPSHOT_IDLE_SECS` and
/// `NEEDLE_SNAPSHOT_MAX_SECS` override them, to try the behaviour without waiting.
fn scheduler(settings: SnapshotSettings) -> Scheduler {
    let secs = |name: &str| env::var(name).ok()?.parse().ok().map(Duration::from_secs);
    Scheduler::new(
        secs("NEEDLE_SNAPSHOT_IDLE_SECS").unwrap_or(Duration::from_secs(settings.idle_minutes * 60)),
        secs("NEEDLE_SNAPSHOT_MAX_SECS").unwrap_or(Duration::from_secs(settings.max_minutes * 60)),
    )
}

/// Checks once a second whether an automatic snapshot is due.
pub fn start_timer(app: AppHandle) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(1));
        let state = app.state::<AppState>();
        let open = state.lock();
        if let Some(open) = open.as_ref()
            && open.history.is_due()
            && let Err(e) = open.history.snapshot(&app, None)
        {
            eprintln!("automatic snapshot failed: {e}");
        }
    });
}

/// A scene's path relative to the vault, as the history stores it.
fn scene_path(open: &OpenVault, project: &str, scene: &str) -> Result<PathBuf, String> {
    let path = open.vault.project(project).or_string()?.scene_path(scene).or_string()?;
    path.strip_prefix(open.vault.root()).map(PathBuf::from).or_string()
}

#[derive(Serialize)]
pub struct VersionInfo {
    id: String,
    time: i64,
    message: String,
    name: Option<String>,
    words: usize,
}

#[tauri::command]
pub fn scene_history(state: State<'_, AppState>, project: String, scene: String) -> Result<Vec<VersionInfo>, String> {
    state.with(|open| {
        let path = scene_path(open, &project, &scene)?;
        let repo = open.history.repo();
        repo.history(&path)
            .or_string()?
            .into_iter()
            .map(|v| {
                let text = repo.read(&v.id, &path).or_string()?.unwrap_or_default();
                Ok(VersionInfo {
                    words: count_markdown_words(&SceneFile::parse(&text).markdown()),
                    id: v.id,
                    time: v.time,
                    message: v.message,
                    name: v.name,
                })
            })
            .collect()
    })
}

/// The scene's text as of version `id`, without its header.
#[tauri::command]
pub fn scene_version(state: State<'_, AppState>, project: String, scene: String, id: String) -> Result<String, String> {
    state.with(|open| version_text(open, &project, &scene, &id))
}

fn version_text(open: &OpenVault, project: &str, scene: &str, id: &str) -> Result<String, String> {
    let path = scene_path(open, project, scene)?;
    let text = open
        .history
        .repo()
        .read(id, &path)
        .or_string()?
        .ok_or("that version doesn't contain this scene")?;
    Ok(SceneFile::parse(&text).markdown())
}

/// Snapshots now, e.g. when switching scenes. Returns whether anything changed.
#[tauri::command]
pub fn snapshot_now(app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    state.with(|open| open.history.snapshot(&app, None))
}

/// Puts back the scene's text from version `id` and returns it. The current header (status,
/// links) stays, and the text being replaced is snapshotted first so it's never lost.
#[tauri::command]
pub fn restore_version(
    app: AppHandle,
    state: State<'_, AppState>,
    project: String,
    scene: String,
    id: String,
    label: String,
) -> Result<String, String> {
    state.with(|open| {
        open.history.snapshot(&app, None)?;
        let markdown = version_text(open, &project, &scene, &id)?;
        open.vault.project(&project).or_string()?.save_body(&scene, &markdown).or_string()?;
        open.history.snapshot(&app, Some(&format!("Restored {scene} to {label}")))?;
        Ok(markdown)
    })
}

#[tauri::command]
pub fn name_version(app: AppHandle, state: State<'_, AppState>, id: String, name: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a version name can't be empty".into());
    }
    state.with(|open| open.history.repo().name_version(&id, name).or_string())?;
    let _ = app.emit(SNAPSHOT_EVENT, ());
    Ok(())
}

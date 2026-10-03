//! Automatic snapshots of the vault, and the commands behind the History panel.

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    thread,
    time::{Duration, Instant},
};

use needle_core::{scene::SceneFile, words::count_markdown_words};
use needle_vcs::{Scheduler, Vault};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::scenes::replace_body;

/// Tells the frontend to refresh the History panel.
const SNAPSHOT_EVENT: &str = "snapshot-taken";

pub struct History {
    root: PathBuf,
    vault: Mutex<Vault>,
    scheduler: Mutex<Scheduler>,
}

impl History {
    pub fn open(root: PathBuf) -> Result<Self, String> {
        let vault = Vault::open_or_init(&root).map_err(|e| e.to_string())?;
        Ok(Self {
            root,
            vault: Mutex::new(vault),
            scheduler: Mutex::new(scheduler_from_env()),
        })
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
        let taken = self.vault().snapshot(message).map_err(|e| e.to_string())?.is_some();
        self.scheduler().taken();
        if taken {
            let _ = app.emit(SNAPSHOT_EVENT, ());
        }
        Ok(taken)
    }

    fn relative(&self, path: &str) -> Result<PathBuf, String> {
        Path::new(path)
            .strip_prefix(&self.root)
            .map(Path::to_owned)
            .map_err(|_| format!("{path} is outside the vault"))
    }

    fn vault(&self) -> MutexGuard<'_, Vault> {
        self.vault.lock().expect("a snapshot panicked")
    }

    fn scheduler(&self) -> MutexGuard<'_, Scheduler> {
        self.scheduler.lock().expect("a snapshot panicked")
    }
}

/// `NEEDLE_SNAPSHOT_IDLE_SECS` and `NEEDLE_SNAPSHOT_MAX_SECS` shorten the 2 min / 10 min
/// defaults, to try the behaviour without waiting.
fn scheduler_from_env() -> Scheduler {
    let secs = |name: &str| env::var(name).ok()?.parse().ok().map(Duration::from_secs);
    match (secs("NEEDLE_SNAPSHOT_IDLE_SECS"), secs("NEEDLE_SNAPSHOT_MAX_SECS")) {
        (None, None) => Scheduler::default(),
        (idle, max) => Scheduler::new(
            idle.unwrap_or(Duration::from_secs(120)),
            max.unwrap_or(Duration::from_secs(600)),
        ),
    }
}

/// Checks once a second whether an automatic snapshot is due.
pub fn start_timer(app: AppHandle) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(1));
        let history = app.state::<History>();
        if history.is_due()
            && let Err(e) = history.snapshot(&app, None)
        {
            eprintln!("automatic snapshot failed: {e}");
        }
    });
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
pub fn scene_history(history: State<'_, History>, path: String) -> Result<Vec<VersionInfo>, String> {
    let relative = history.relative(&path)?;
    let vault = history.vault();
    let versions = vault.history(&relative).map_err(|e| e.to_string())?;
    versions
        .into_iter()
        .map(|v| {
            let text = vault.read(&v.id, &relative).map_err(|e| e.to_string())?.unwrap_or_default();
            Ok(VersionInfo {
                words: count_markdown_words(&SceneFile::parse(&text).markdown()),
                id: v.id,
                time: v.time,
                message: v.message,
                name: v.name,
            })
        })
        .collect()
}

/// The scene's text as of version `id`, without front matter.
#[tauri::command]
pub fn scene_version(history: State<'_, History>, path: String, id: String) -> Result<String, String> {
    let relative = history.relative(&path)?;
    let text = history
        .vault()
        .read(&id, &relative)
        .map_err(|e| e.to_string())?
        .ok_or("that version doesn't contain this scene")?;
    Ok(SceneFile::parse(&text).markdown())
}

/// Snapshots now, e.g. when switching scenes. Returns whether anything changed.
#[tauri::command]
pub fn snapshot_now(app: AppHandle, history: State<'_, History>) -> Result<bool, String> {
    history.snapshot(&app, None)
}

/// Puts back the scene's text from version `id` and returns it. The current front matter
/// (status, links) stays, and the text being replaced is snapshotted first so it's never lost.
#[tauri::command]
pub fn restore_version(
    app: AppHandle,
    history: State<'_, History>,
    path: String,
    id: String,
    label: String,
) -> Result<String, String> {
    history.snapshot(&app, None)?;
    let markdown = scene_version(history.clone(), path.clone(), id)?;
    replace_body(Path::new(&path), &markdown)?;
    let name = Path::new(&path).file_stem().unwrap_or_default().to_string_lossy();
    history.snapshot(&app, Some(&format!("Restored {name} to {label}")))?;
    Ok(markdown)
}

#[tauri::command]
pub fn name_version(app: AppHandle, history: State<'_, History>, id: String, name: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a version name can't be empty".into());
    }
    history.vault().name_version(&id, name).map_err(|e| e.to_string())?;
    let _ = app.emit(SNAPSHOT_EVENT, ());
    Ok(())
}

/// Opens (or starts) the vault's history and takes a first snapshot of anything new.
pub fn open(app: &AppHandle, root: PathBuf) -> Result<History, String> {
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let history = History::open(root)?;
    history.snapshot(app, None)?;
    Ok(history)
}

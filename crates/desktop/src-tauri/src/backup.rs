//! Backing up the open vault: pushing its history to a repository over SSH (DESIGN §5, Sync),
//! after each snapshot or when asked. Pushes run on a thread of their own, one at a time, so a
//! slow network never holds up writing or snapshots.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use needle_core::settings::BackupSettings;
use needle_vcs::Vault as Repo;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::state::{AppState, OrString};

/// Tells the frontend the backup's status changed; the payload is a `BackupStatus`.
const STATUS_EVENT: &str = "backup-status";

/// How the backup is doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum BackupStatus {
    /// Nothing pushed yet since the vault opened.
    Waiting,
    Pushing,
    /// Seconds since the Unix epoch.
    Done { at: i64 },
    Failed { at: i64, error: String },
}

struct Inner {
    settings: BackupSettings,
    status: BackupStatus,
    /// A push is running.
    running: bool,
    /// Another was asked for while it ran.
    again: bool,
}

#[derive(Clone)]
pub struct Backup {
    root: PathBuf,
    inner: Arc<Mutex<Inner>>,
}

impl Backup {
    pub fn new(root: PathBuf, settings: BackupSettings) -> Self {
        Self {
            root,
            inner: Arc::new(Mutex::new(Inner {
                settings,
                status: BackupStatus::Waiting,
                running: false,
                again: false,
            })),
        }
    }

    /// After a snapshot: pushes, if the vault backs up after each one.
    pub fn after_snapshot(&self, app: &AppHandle) {
        if self.inner().settings.is_on() {
            self.push(app);
        }
    }

    /// Pushes now, or right after the push that's running.
    pub fn push(&self, app: &AppHandle) {
        let remote = {
            let mut inner = self.inner();
            let remote = inner.settings.remote.trim().to_owned();
            if remote.is_empty() {
                return;
            }
            if inner.running {
                inner.again = true;
                return;
            }
            inner.running = true;
            remote
        };
        let (backup, app) = (self.clone(), app.clone());
        thread::spawn(move || {
            let mut remote = remote;
            loop {
                backup.set_status(&app, BackupStatus::Pushing);
                let pushed = Repo::open_or_init(&backup.root).and_then(|repo| repo.push(&remote, None));
                let at = now();
                backup.set_status(
                    &app,
                    match pushed {
                        Ok(()) => BackupStatus::Done { at },
                        Err(e) => BackupStatus::Failed {
                            at,
                            error: needle_vcs::push_problem(&e, &remote),
                        },
                    },
                );
                let mut inner = backup.inner();
                if !std::mem::take(&mut inner.again) || inner.settings.remote.trim().is_empty() {
                    inner.running = false;
                    break;
                }
                remote = inner.settings.remote.trim().to_owned();
            }
        });
    }

    pub fn settings(&self) -> BackupSettings {
        self.inner().settings.clone()
    }

    pub fn status(&self) -> BackupStatus {
        self.inner().status.clone()
    }

    fn set_settings(&self, settings: BackupSettings) {
        let mut inner = self.inner();
        if settings.remote.trim().is_empty() {
            inner.status = BackupStatus::Waiting;
        }
        inner.settings = settings;
    }

    fn set_status(&self, app: &AppHandle, status: BackupStatus) {
        self.inner().status = status.clone();
        let _ = app.emit(STATUS_EVENT, status);
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().expect("a backup panicked")
    }
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

#[derive(Serialize)]
pub struct BackupView {
    remote: String,
    after_snapshot: bool,
    status: BackupStatus,
}

#[tauri::command]
pub fn backup(state: State<'_, AppState>) -> Result<BackupView, String> {
    state.with(|open| {
        let settings = open.history.backup().settings();
        Ok(BackupView {
            remote: settings.remote,
            after_snapshot: settings.after_snapshot,
            status: open.history.backup().status(),
        })
    })
}

/// Saves where (and whether) to back up, then backs up straight away if there's an address.
#[tauri::command]
pub fn set_backup(app: AppHandle, state: State<'_, AppState>, remote: String, after_snapshot: bool) -> Result<(), String> {
    state.with(|open| {
        let settings = BackupSettings {
            remote: remote.trim().to_owned(),
            after_snapshot,
        };
        open.vault.set_backup(&settings).or_string()?;
        let backup = open.history.backup();
        backup.set_settings(settings);
        let _ = app.emit(STATUS_EVENT, backup.status());
        backup.push(&app);
        Ok(())
    })
}

/// Snapshots anything unsaved, then pushes.
#[tauri::command]
pub fn back_up_now(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.with(|open| {
        let taken = open.history.snapshot(&app, None)?;
        let backup = open.history.backup();
        // A snapshot pushes by itself when the backup is on.
        if !(taken && backup.settings().is_on()) {
            backup.push(&app);
        }
        Ok(())
    })
}

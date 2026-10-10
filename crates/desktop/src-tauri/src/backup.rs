//! Backing up the open vault: pushing its history to a repository over SSH (DESIGN §5, Sync),
//! after each snapshot or when asked. Each push fetches first, and so does a quiet check every
//! few minutes; if the repository has snapshots made elsewhere, the status says so and the
//! window takes them in (`take_in`) once what's being typed is saved, then the push goes ahead.
//! Fetches and pushes run on a thread of their own, one at a time, so a slow network never holds
//! up writing or snapshots.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use needle_core::settings::BackupSettings;
use needle_vcs::{Incoming, TakenIn, Vault as Repo};
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
    /// The repository has snapshots made elsewhere, to take in before pushing.
    Incoming,
    /// Seconds since the Unix epoch.
    Done { at: i64 },
    Failed { at: i64, error: String },
}

struct Inner {
    settings: BackupSettings,
    status: BackupStatus,
    /// A fetch or push is running.
    running: bool,
    /// What was asked for while it ran, to do next.
    again: Option<Job>,
}

/// What the backup thread is asked to do. A push includes a check, so it wins over one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Job {
    /// Fetch, to see whether anything was made elsewhere.
    Check,
    /// Fetch, then push if there's nothing to take in first.
    Push,
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
                again: None,
            })),
        }
    }

    /// After a snapshot: pushes, if the vault backs up after each one.
    pub fn after_snapshot(&self, app: &AppHandle) {
        if self.inner().settings.is_on() {
            self.push(app);
        }
    }

    /// Pushes now, or right after whatever is running.
    pub fn push(&self, app: &AppHandle) {
        self.run(app, Job::Push);
    }

    /// Looks for snapshots made elsewhere, without pushing. Quietly: a computer that's offline
    /// isn't told so every few minutes.
    pub fn check(&self, app: &AppHandle) {
        self.run(app, Job::Check);
    }

    fn run(&self, app: &AppHandle, job: Job) {
        let remote = {
            let mut inner = self.inner();
            let remote = inner.settings.remote.trim().to_owned();
            if remote.is_empty() {
                return;
            }
            if inner.running {
                inner.again = inner.again.max(Some(job));
                return;
            }
            inner.running = true;
            remote
        };
        let (backup, app) = (self.clone(), app.clone());
        thread::spawn(move || {
            let (mut remote, mut job) = (remote, job);
            loop {
                backup.fetch_then(&app, &remote, job);
                let mut inner = backup.inner();
                match inner.again.take() {
                    Some(next) if !inner.settings.remote.trim().is_empty() => {
                        job = next;
                        remote = inner.settings.remote.trim().to_owned();
                    }
                    _ => {
                        inner.running = false;
                        break;
                    }
                }
            }
        });
    }

    /// Fetches, and pushes for a `Job::Push` if nothing came that has to be taken in first.
    fn fetch_then(&self, app: &AppHandle, remote: &str, job: Job) {
        if job == Job::Push {
            self.set_status(app, BackupStatus::Pushing);
        }
        let fetched = Repo::open_or_init(&self.root).and_then(|repo| Ok((repo.fetch(remote, None)?, repo)));
        let failed = |error: String| BackupStatus::Failed { at: now(), error };
        match fetched {
            Ok((Incoming::New, _)) => self.set_status(app, BackupStatus::Incoming),
            Ok((Incoming::Unrelated, _)) => {
                self.set_status(app, failed("the repository's history isn't this vault's, so nothing was sent".to_owned()))
            }
            Ok((Incoming::Nothing, repo)) if job == Job::Push => self.set_status(
                app,
                match repo.push(remote, None) {
                    Ok(()) => BackupStatus::Done { at: now() },
                    Err(e) => failed(needle_vcs::push_problem(&e, remote)),
                },
            ),
            Ok((Incoming::Nothing, _)) => {}
            Err(e) if job == Job::Push => self.set_status(app, failed(needle_vcs::push_problem(&e, remote))),
            Err(e) => eprintln!("couldn't check {remote} for snapshots made elsewhere: {e}"),
        }
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

#[derive(Serialize)]
pub struct TakenInView {
    /// Files that changed, relative to the vault, so the window can reload what it shows.
    changed: Vec<String>,
    clashes: Vec<ClashView>,
}

#[derive(Serialize)]
pub struct ClashView {
    path: String,
    copy: String,
}

impl From<TakenIn> for TakenInView {
    fn from(taken: TakenIn) -> Self {
        Self {
            changed: taken.changed,
            clashes: taken.clashes.into_iter().map(|c| ClashView { path: c.path, copy: c.copy }).collect(),
        }
    }
}

/// Takes in the snapshots made elsewhere that the last fetch found, after snapshotting what's
/// here, then pushes the result if the vault backs up. The window calls it on the `Incoming`
/// status, once what's being typed is saved.
#[tauri::command]
pub fn take_in(app: AppHandle, state: State<'_, AppState>) -> Result<TakenInView, String> {
    state.with(|open| {
        let backup = open.history.backup();
        let settings = backup.settings();
        let remote = settings.remote.trim();
        if remote.is_empty() {
            return Ok(TakenIn::default().into());
        }
        let taken = match open.history.take_in(&app, needle_vcs::host_of(remote)) {
            Ok(taken) => taken,
            Err(error) => {
                backup.set_status(&app, BackupStatus::Failed { at: now(), error: error.clone() });
                return Err(error);
            }
        };
        if settings.is_on() {
            backup.push(&app);
        } else {
            backup.set_status(&app, BackupStatus::Waiting);
        }
        Ok(taken.into())
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

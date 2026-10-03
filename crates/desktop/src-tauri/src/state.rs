//! The open vault, shared by every command, and which vault to reopen on the next launch.

use std::fmt::Display;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use needle_vault::Vault;
use tauri::{AppHandle, Manager};

use crate::history::History;

pub struct OpenVault {
    pub vault: Vault,
    pub history: History,
}

#[derive(Default)]
pub struct AppState {
    open: Mutex<Option<OpenVault>>,
}

impl AppState {
    /// Runs `f` with the open vault, or fails if none is open.
    pub fn with<T>(&self, f: impl FnOnce(&OpenVault) -> Result<T, String>) -> Result<T, String> {
        match self.lock().as_ref() {
            Some(open) => f(open),
            None => Err("no vault is open".into()),
        }
    }

    /// Switches to `vault`, giving the previous one a final snapshot first. With `remember_it`,
    /// it also reopens on the next launch.
    pub fn open(&self, app: &AppHandle, vault: Vault, remember_it: bool) -> Result<(), String> {
        let history = History::open(app, &vault)?;
        let root = vault.root().to_owned();
        {
            let mut open = self.lock();
            if let Some(previous) = open.as_ref() {
                previous.history.snapshot(app, None)?;
            }
            *open = Some(OpenVault { vault, history });
        }
        if remember_it { remember(app, &root) } else { Ok(()) }
    }

    pub fn lock(&self) -> MutexGuard<'_, Option<OpenVault>> {
        self.open.lock().expect("a command panicked while using the vault")
    }
}

/// `.or_string()` turns any displayable error into the `String` errors commands return.
pub trait OrString<T> {
    fn or_string(self) -> Result<T, String>;
}

impl<T, E: Display> OrString<T> for Result<T, E> {
    fn or_string(self) -> Result<T, String> {
        self.map_err(|e| e.to_string())
    }
}

// Per-computer settings are small text files in the app's config folder, one per setting.

fn config_file(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    Ok(app.path().app_config_dir().or_string()?.join(name))
}

/// The setting's text, or None if it was never written.
pub fn read_config(app: &AppHandle, name: &str) -> Option<String> {
    fs::read_to_string(config_file(app, name).ok()?).ok()
}

pub fn write_config(app: &AppHandle, name: &str, contents: &str) -> Result<(), String> {
    let file = config_file(app, name)?;
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).or_string()?;
    }
    fs::write(file, contents).or_string()
}

fn remember(app: &AppHandle, root: &Path) -> Result<(), String> {
    write_config(app, "last-vault", &root.to_string_lossy())
}

/// The vault used last time, if it's still there.
pub fn last_vault(app: &AppHandle) -> Option<PathBuf> {
    let path = PathBuf::from(read_config(app, "last-vault")?.trim());
    Vault::is_vault(&path).then_some(path)
}

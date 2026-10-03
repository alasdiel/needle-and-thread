//! System, Light or Dark. It's kept per computer next to `last-vault`, not in the vault, because
//! the vault also syncs to other devices.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::state;

const SETTING: &str = "appearance";

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    /// Follow the desktop's light or dark setting.
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    fn name(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// This computer's choice; System until one is made.
#[tauri::command]
pub fn appearance(app: AppHandle) -> Appearance {
    match state::read_config(&app, SETTING).as_deref().map(str::trim) {
        Some("light") => Appearance::Light,
        Some("dark") => Appearance::Dark,
        _ => Appearance::System,
    }
}

#[tauri::command]
pub fn set_appearance(app: AppHandle, appearance: Appearance) -> Result<(), String> {
    state::write_config(&app, SETTING, appearance.name())
}

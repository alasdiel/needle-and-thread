//! Settings kept per computer, next to `last-vault`, rather than in the vault, because the vault
//! also syncs to other devices.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::state;

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
    match state::read_config(&app, "appearance").as_deref().map(str::trim) {
        Some("light") => Appearance::Light,
        Some("dark") => Appearance::Dark,
        _ => Appearance::System,
    }
}

#[tauri::command]
pub fn set_appearance(app: AppHandle, appearance: Appearance) -> Result<(), String> {
    state::write_config(&app, "appearance", appearance.name())
}

/// Whether the scene menu offers the Markdown panel, a developer aid. Off until switched on.
#[tauri::command]
pub fn markdown_panel(app: AppHandle) -> bool {
    state::read_config(&app, "markdown-panel").as_deref().map(str::trim) == Some("on")
}

#[tauri::command]
pub fn set_markdown_panel(app: AppHandle, on: bool) -> Result<(), String> {
    state::write_config(&app, "markdown-panel", if on { "on" } else { "off" })
}

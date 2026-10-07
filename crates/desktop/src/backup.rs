//! The vault's backup to GitHub: its section in Settings, and its mark beside "Saved".

use leptos::{prelude::*, task::spawn_local};
use wasm_bindgen::JsValue;

use crate::history::{clock_of, day_label};
use crate::icons::{Glyph, Icon};
use crate::tauri::{self, BackupStatus, BackupView};

/// The open vault's backup, kept up to date from the backend's events.
#[derive(Clone, Copy)]
pub struct Backup {
    pub view: RwSignal<Option<BackupView>>,
}

impl Backup {
    /// Loads the backup's settings and status, and follows its changes.
    pub fn load() -> Self {
        let backup = Self { view: RwSignal::new(None) };
        backup.refresh();
        let _ = tauri::listen("backup-status", move || backup.refresh());
        backup
    }

    fn refresh(self) {
        spawn_local(async move {
            if let Ok(view) = tauri::backup().await {
                self.view.try_set(Some(view));
            }
        });
    }

    /// The status, when there's somewhere to back up to.
    fn status(&self) -> Option<BackupStatus> {
        self.view.with(|v| v.as_ref().filter(|v| !v.remote.trim().is_empty()).map(|v| v.status.clone()))
    }
}

/// "today at 14:32", "Oct 4 at 09:05".
fn when(seconds: i64) -> String {
    let date = js_sys::Date::new(&JsValue::from_f64(seconds as f64 * 1000.0));
    let day = day_label(&date);
    let day = if matches!(day.as_str(), "Today" | "Yesterday") { day.to_lowercase() } else { day };
    format!("{day} at {}", clock_of(&date))
}

/// The Backup section of the Settings popover.
#[component]
pub fn BackupSettings(backup: Backup) -> impl IntoView {
    let error = RwSignal::new(None::<String>);
    let remote = RwSignal::new(String::new());
    // Starts from the saved address; typing changes only the box until it's saved.
    Effect::new(move |_| {
        if let Some(saved) = backup.view.with(|v| v.as_ref().map(|v| v.remote.clone())) {
            remote.set(saved);
        }
    });
    let after_snapshot = move || backup.view.with(|v| v.as_ref().is_none_or(|v| v.after_snapshot));

    let save = move |address: String, on: bool| {
        spawn_local(async move {
            let saved = tauri::set_backup(&address, on).await;
            error.try_set(saved.err());
            backup.refresh();
        });
    };
    let back_up_now = move |_| {
        spawn_local(async move {
            if let Err(e) = tauri::back_up_now().await {
                error.try_set(Some(e));
            }
        });
    };

    let status = move || {
        backup.status().map(|status| {
            let (icon, text, failed) = match status {
                BackupStatus::Waiting => (None, "Not backed up yet".to_owned(), false),
                BackupStatus::Pushing => (None, "Backing up…".to_owned(), false),
                BackupStatus::Done { at } => (Some(Glyph::Check), format!("Backed up {}", when(at)), false),
                BackupStatus::Failed { error, .. } => (Some(Glyph::Alert), format!("Couldn't back up: {error}"), true),
            };
            view! {
                <div class="backup-status">
                    <span class="backup-status-text" class:error=failed>
                        {icon.map(|glyph| view! { <Icon glyph=glyph size=14 /> })}
                        {text}
                    </span>
                    <button class="small" on:click=back_up_now>"Back up now"</button>
                </div>
            }
        })
    };

    view! {
        <h2>"Backup"</h2>
        <label class="field">
            <span class="field-label">"GitHub repository"</span>
            <input
                type="text"
                class="backup-remote"
                placeholder="git@github.com:you/novel.git"
                spellcheck="false"
                prop:value=move || remote.get()
                on:input=move |ev| remote.set(event_target_value(&ev))
                on:change=move |ev| save(event_target_value(&ev), after_snapshot())
            />
        </label>
        <label class="setting">
            <span class="setting-text">
                <span class="setting-label">"After each snapshot"</span>
                <span class="setting-description">"Sends your history to GitHub, so it's safe if this computer isn't."</span>
            </span>
            <input
                type="checkbox"
                class="switch"
                prop:checked=after_snapshot
                on:change=move |ev| save(remote.get_untracked(), event_target_checked(&ev))
            />
        </label>
        {status}
        {move || error.get().map(|e| view! { <p class="error">{format!("Couldn't save this: {e}")}</p> })}
    }
}

/// Beside "Saved": "· backed up", "· backing up…" or "· not backed up", which opens the
/// backup's settings. Nothing when there's no backup, or before the first try.
#[component]
pub fn BackupMark(backup: Backup, on_open: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    move || {
        let (text, title, failed) = match backup.status()? {
            BackupStatus::Waiting => return None,
            BackupStatus::Pushing => ("backing up…", "Sending your history to GitHub".to_owned(), false),
            BackupStatus::Done { at } => ("backed up", format!("Backed up to GitHub {}", when(at)), false),
            BackupStatus::Failed { error, .. } => ("not backed up", format!("Couldn't back up: {error}"), true),
        };
        Some(view! {
            <button type="button" class="backup-mark" class:error=failed title=title on:click=move |_| on_open()>
                {failed.then(|| view! { <Icon glyph=Glyph::Alert size=14 /> })}
                {text}
            </button>
        })
    }
}

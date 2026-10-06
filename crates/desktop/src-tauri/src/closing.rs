//! Closing the window: it stays open until the frontend has saved what's being typed, and if
//! that save fails, the user chooses whether to close anyway.

use std::{
    sync::{Mutex, MutexGuard},
    thread,
    time::{Duration, Instant},
};

use tauri::{CloseRequestApi, Emitter, Manager, State, Window};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

/// Asks the frontend to save, then call `finish_close`.
const SAVE_EVENT: &str = "save-before-close";

/// A frontend that hasn't answered by then is stuck, and the window closes without it.
const PATIENCE: Duration = Duration::from_secs(10);

#[derive(Default)]
pub struct Closing(Mutex<Stage>);

#[derive(Clone, Copy, Default, PartialEq)]
enum Stage {
    /// The page isn't listening yet, so it has nothing to save and the window just closes.
    #[default]
    Loading,
    Open,
    /// The frontend was asked to save at this moment, which also tells one request from the next.
    Saving(Instant),
    /// The save failed and the user is choosing whether to close anyway.
    Asking,
}

impl Closing {
    fn stage(&self) -> MutexGuard<'_, Stage> {
        // A panic while holding this only ever left a valid stage behind.
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Holds the close until the frontend has saved. Asking again meanwhile changes nothing.
pub fn requested(window: &Window, api: &CloseRequestApi) {
    let closing = window.state::<Closing>();
    let mut stage = closing.stage();
    if *stage == Stage::Loading {
        return;
    }
    api.prevent_close();
    if *stage != Stage::Open {
        return;
    }
    let asked = Instant::now();
    *stage = Stage::Saving(asked);
    drop(stage);
    if let Err(e) = window.emit(SAVE_EVENT, ()) {
        eprintln!("couldn't ask the page to save before closing: {e}");
    }
    let window = window.clone();
    thread::spawn(move || {
        thread::sleep(PATIENCE);
        if *window.state::<Closing>().stage() == Stage::Saving(asked) {
            eprintln!("the page didn't save within {PATIENCE:?}; closing without it");
            let _ = window.destroy();
        }
    });
}

/// The frontend is listening for `SAVE_EVENT` now, so closes wait for it from here on.
#[tauri::command]
pub fn close_listening(closing: State<'_, Closing>) {
    let mut stage = closing.stage();
    if *stage == Stage::Loading {
        *stage = Stage::Open;
    }
}

/// The frontend's answer: everything is saved, or `error` says why the latest changes aren't.
#[tauri::command]
pub fn finish_close(window: Window, closing: State<'_, Closing>, error: Option<String>) {
    let Some(error) = error else {
        let _ = window.destroy();
        return;
    };
    *closing.stage() = Stage::Asking;
    window
        .dialog()
        .message(format!("Your latest changes couldn't be saved.\n\n{error}"))
        .title("Not saved")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom("Close anyway".into(), "Keep open".into()))
        .parent(&window)
        .show(move |close| {
            if close {
                let _ = window.destroy();
            } else {
                *window.state::<Closing>().stage() = Stage::Open;
            }
        });
}

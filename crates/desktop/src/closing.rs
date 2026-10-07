//! Closing the window: the backend holds the close and asks for a save first (see
//! `src-tauri/src/closing.rs`).

use std::rc::Rc;

use leptos::{prelude::*, task::spawn_local};
use wasm_bindgen_futures::JsFuture;

use crate::tauri::{self, LocalFuture};

type Save = Rc<dyn Fn() -> LocalFuture<Result<(), String>>>;

/// What has to be saved before the window closes, while a workspace is open.
#[derive(Clone, Copy)]
pub struct BeforeClose {
    save: StoredValue<Option<Save>, LocalStorage>,
    /// Counts `set` calls, so an old workspace's cleanup can't clear a newer one's save.
    owner: StoredValue<u32>,
}

impl BeforeClose {
    /// Answers the backend's close requests for the rest of the app's life.
    pub fn provide() {
        let this = Self {
            save: StoredValue::new_local(None),
            owner: StoredValue::new(0),
        };
        provide_context(this);
        let listening = tauri::listen("save-before-close", move || {
            let save = this.save.get_value();
            spawn_local(async move {
                let saved = match save {
                    Some(save) => save().await,
                    None => Ok(()),
                };
                let _ = tauri::finish_close(saved.err().as_deref()).await;
            });
        });
        spawn_local(async move {
            if JsFuture::from(listening).await.is_ok() {
                let _ = tauri::close_listening().await;
            }
        });
    }

    /// Runs `save` before the window closes, until the calling component goes away. It fails
    /// with the reason if the latest changes couldn't be saved.
    pub fn set(save: impl Fn() -> LocalFuture<Result<(), String>> + 'static) {
        let this = expect_context::<Self>();
        this.owner.update_value(|n| *n += 1);
        let mine = this.owner.get_value();
        this.save.set_value(Some(Rc::new(save)));
        on_cleanup(move || {
            if this.owner.try_get_value() == Some(mine) {
                this.save.try_set_value(None);
            }
        });
    }
}

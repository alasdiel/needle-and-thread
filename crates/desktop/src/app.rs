use leptos::{prelude::*, task::spawn_local};

use crate::closing::BeforeClose;
use crate::settings::Prefs;
use crate::tauri::{self, VaultView};
use crate::welcome::Welcome;
use crate::workspace::Workspace;

/// Shows the welcome screen until a vault is open, then the workspace for it.
#[component]
pub fn App() -> impl IntoView {
    let vault = RwSignal::new(None::<VaultView>);
    let loaded = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    let prefs = Prefs::load();
    provide_context(prefs);
    Effect::new(move |_| prefs.appearance.get().apply());
    BeforeClose::provide();

    spawn_local(async move {
        match tauri::current_vault().await {
            Ok(current) => vault.set(current),
            Err(e) => error.set(Some(e)),
        }
        loaded.set(true);
    });

    let on_open = move |opened: VaultView| vault.set(Some(opened));
    // Only a different vault builds a fresh workspace.
    let path = Memo::new(move |_| vault.with(|v| v.as_ref().map(|v| v.path.clone())));

    view! {
        {move || error.get().map(|e| view! { <p class="error">{e}</p> })}
        {move || {
            if !loaded.get() {
                return None;
            }
            Some(match path.get() {
                None => view! { <Welcome on_open=on_open /> }.into_any(),
                Some(_) => view! { <Workspace vault=vault.get_untracked().unwrap() on_open_vault=on_open /> }.into_any(),
            })
        }}
    }
}

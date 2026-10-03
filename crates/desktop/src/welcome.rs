//! First screen when no vault is open: open one, create one, or try the sample.

use leptos::{prelude::*, task::spawn_local};

use crate::tauri::{self, LocalFuture, VaultView};

type Action = fn() -> LocalFuture<Result<Option<VaultView>, String>>;

#[component]
pub fn Welcome(on_open: impl Fn(VaultView) + Copy + Send + Sync + 'static) -> impl IntoView {
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);

    let run = move |action: Action| {
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match action().await {
                Ok(Some(vault)) => on_open(vault),
                Ok(None) => {}
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    view! {
        <main class="welcome">
            <h1>"Needle and Thread"</h1>
            <p class="muted">"Your writing lives in a vault: a folder of plain files, with its history kept alongside."</p>
            <div class="welcome-actions">
                <button disabled=move || busy.get() on:click=move |_| run(|| Box::pin(tauri::open_vault()))>
                    "Open a vault…"
                </button>
                <button disabled=move || busy.get() on:click=move |_| run(|| Box::pin(tauri::create_vault()))>
                    "Create a vault…"
                </button>
                <button
                    class="quiet"
                    disabled=move || busy.get()
                    on:click=move |_| run(|| Box::pin(async { tauri::open_sample_vault().await.map(Some) }))
                >
                    "Try the sample"
                </button>
            </div>
            {move || error.get().map(|e| view! { <p class="error">{e}</p> })}
        </main>
    }
}

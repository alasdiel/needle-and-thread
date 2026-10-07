//! Settings kept per computer by the backend, and the popover that changes them, opened from the
//! bottom of the sidebar.

use leptos::{prelude::*, task::spawn_local};

use crate::appearance::Appearance;
use crate::backup::{Backup, BackupSettings};
use crate::icons::Icon;
use crate::tauri;

/// Shared through context, since the popover changes them from deep in the workspace.
#[derive(Clone, Copy)]
pub struct Prefs {
    pub appearance: RwSignal<Appearance>,
    /// Offers the Markdown panel in the scene menu.
    pub markdown_panel: RwSignal<bool>,
}

impl Prefs {
    /// Starts from the defaults and fills in the saved values when they arrive.
    pub fn load() -> Self {
        let prefs = Self {
            appearance: RwSignal::new(Appearance::System),
            markdown_panel: RwSignal::new(false),
        };
        spawn_local(async move {
            if let Ok(saved) = tauri::appearance().await {
                prefs.appearance.set(saved);
            }
            if let Ok(on) = tauri::markdown_panel().await {
                prefs.markdown_panel.set(on);
            }
        });
        prefs
    }
}

#[component]
pub fn SettingsPanel() -> impl IntoView {
    let prefs = expect_context::<Prefs>();
    let error = RwSignal::new(None::<String>);
    // The panel may have closed by the time a save finishes.
    let saved = move |result: Result<(), String>| {
        error.try_set(result.err());
    };
    let choose = move |choice: Appearance| {
        prefs.appearance.set(choice);
        spawn_local(async move { saved(tauri::set_appearance(choice).await) });
    };
    let set_markdown_panel = move |on: bool| {
        prefs.markdown_panel.set(on);
        spawn_local(async move { saved(tauri::set_markdown_panel(on).await) });
    };

    view! {
        <div class="panel up" role="dialog" aria-label="Settings">
            <h2>"Appearance"</h2>
            <div class="segmented" role="radiogroup" aria-label="Appearance">
                {Appearance::ALL
                    .into_iter()
                    .map(|choice| {
                        view! {
                            <button
                                type="button"
                                role="radio"
                                aria-checked=move || (prefs.appearance.get() == choice).to_string()
                                class:active=move || prefs.appearance.get() == choice
                                on:click=move |_| choose(choice)
                            >
                                <Icon glyph=choice.glyph() size=15 />
                                {choice.label()}
                            </button>
                        }
                    })
                    .collect_view()}
            </div>
            {use_context::<Backup>().map(|backup| view! { <BackupSettings backup=backup /> })}
            <h2>"Developer"</h2>
            <label class="setting">
                <span class="setting-text">
                    <span class="setting-label">"Markdown panel"</span>
                    <span class="setting-description">"Adds “Show Markdown” to the scene menu, to see a scene as it's saved."</span>
                </span>
                <input
                    type="checkbox"
                    class="switch"
                    prop:checked=move || prefs.markdown_panel.get()
                    on:change=move |ev| set_markdown_panel(event_target_checked(&ev))
                />
            </label>
            {move || error.get().map(|e| view! { <p class="error">{format!("Couldn't save this: {e}")}</p> })}
        </div>
    }
}

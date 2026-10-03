//! The settings popover, opened from the bottom of the sidebar.

use leptos::{prelude::*, task::spawn_local};

use crate::appearance::Appearance;
use crate::icons::Icon;
use crate::tauri;

#[component]
pub fn SettingsPanel() -> impl IntoView {
    let appearance = expect_context::<RwSignal<Appearance>>();
    let error = RwSignal::new(None::<String>);
    let choose = move |choice: Appearance| {
        appearance.set(choice);
        spawn_local(async move {
            // The panel may have closed by the time this lands.
            error.try_set(tauri::set_appearance(choice).await.err());
        });
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
                                aria-checked=move || (appearance.get() == choice).to_string()
                                class:active=move || appearance.get() == choice
                                on:click=move |_| choose(choice)
                            >
                                <Icon glyph=choice.glyph() size=15 />
                                {choice.label()}
                            </button>
                        }
                    })
                    .collect_view()}
            </div>
            {move || error.get().map(|e| view! { <p class="error">{format!("Couldn't save this choice: {e}")}</p> })}
        </div>
    }
}

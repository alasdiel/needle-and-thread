//! The project's ⋯ popover: its title, fiction or nonfiction, and the world it shares notes with.

use leptos::{prelude::*, task::spawn_local};

use crate::tauri::{self, ProjectSettings};

/// The world select's value for "make a new world".
const NEW_WORLD: &str = "\u{0}new";

#[component]
pub fn ProjectPanel(
    project: String,
    /// Called after saving, to reload the project.
    on_saved: impl Fn() + Copy + Send + Sync + 'static,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let settings = RwSignal::new(None::<ProjectSettings>);
    let title = RwSignal::new(String::new());
    let kind = RwSignal::new("fiction".to_owned());
    let world = RwSignal::new(String::new());
    let new_world = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);

    {
        let project = project.clone();
        spawn_local(async move {
            match tauri::project_settings(&project).await {
                Ok(found) => {
                    title.try_set(found.title.clone());
                    kind.try_set(found.kind.clone());
                    world.try_set(found.world.clone().unwrap_or_default());
                    settings.try_set(Some(found));
                }
                Err(e) => {
                    error.try_set(Some(e));
                }
            }
        });
    }

    let save = move |_| {
        let project = project.clone();
        let (title, kind, world, new_world) = (title.get_untracked(), kind.get_untracked(), world.get_untracked(), new_world.get_untracked());
        spawn_local(async move {
            let (world, new_world) = if world == NEW_WORLD { (None, Some(new_world)) } else { (Some(world), None) };
            match tauri::update_project(&project, &title, &kind, world.as_deref(), new_world.as_deref()).await {
                Ok(()) => {
                    on_close();
                    on_saved();
                }
                Err(e) => {
                    error.try_set(Some(e));
                }
            }
        });
    };

    let kind_button = move |value: &'static str, label: &'static str| {
        view! {
            <button
                type="button"
                role="radio"
                aria-checked=move || (kind.get() == value).to_string()
                class:active=move || kind.get() == value
                on:click=move |_| kind.set(value.to_owned())
            >
                {label}
            </button>
        }
    };

    view! {
        <div class="panel project-panel" role="dialog" aria-label="Project">
            <h2>"Project"</h2>
            <label class="field">
                <span class="field-label">"Title"</span>
                <input prop:value=move || title.get() on:input=move |ev| title.set(event_target_value(&ev)) />
            </label>
            <div class="field">
                <span class="field-label">"Kind"</span>
                <div class="segmented two" role="radiogroup" aria-label="Kind">
                    {kind_button("fiction", "Fiction")}
                    {kind_button("nonfiction", "Nonfiction")}
                </div>
            </div>
            <label class="field">
                <span class="field-label">"World"</span>
                <select prop:value=move || world.get() on:change=move |ev| world.set(event_target_value(&ev))>
                    <option value="">"None"</option>
                    {move || {
                        settings
                            .get()
                            .map(|s| {
                                s.worlds
                                    .into_iter()
                                    .map(|(slug, name)| view! { <option value=slug>{name}</option> })
                                    .collect_view()
                            })
                    }}
                    <option value=NEW_WORLD>"New world…"</option>
                </select>
            </label>
            <Show when=move || world.get() == NEW_WORLD>
                <input
                    class="new-world"
                    aria-label="New world's name"
                    placeholder="Name, e.g. The Glass Coast"
                    prop:value=move || new_world.get()
                    on:input=move |ev| new_world.set(event_target_value(&ev))
                />
            </Show>
            <p class="setting-description">"A world's notes are shared by every project in it."</p>
            {move || error.get().map(|e| view! { <p class="error">{e}</p> })}
            <div class="actions">
                <button type="button" class="primary" on:click=save>"Save"</button>
                <button type="button" class="quiet" on:click=move |_| on_close()>"Cancel"</button>
            </div>
        </div>
    }
}

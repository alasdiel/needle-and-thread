use leptos::{prelude::*, task::spawn_local};
use needle_core::spell::Speller;

use crate::editor::{Editor, EditorHandle, Typography};
use crate::history::HistoryPanel;
use crate::spell::SpellBridge;
use crate::tauri::{self, SampleScene};
use crate::typography::TypographySettings;

#[derive(Clone, PartialEq)]
enum SaveState {
    Saved,
    Saving,
    Failed(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Panel {
    Markdown,
    History,
}

#[derive(Clone, PartialEq)]
enum SpellState {
    Loading,
    Ready,
    Failed(String),
}

#[component]
pub fn App() -> impl IntoView {
    let editor = StoredValue::new_local(None::<EditorHandle>);
    let spell_bridge = StoredValue::new_local(None::<SpellBridge>);
    let samples = RwSignal::new(Vec::<SampleScene>::new());
    let current_path = RwSignal::new(None::<String>);
    let save_state = RwSignal::new(SaveState::Saved);
    let spell_state = RwSignal::new(SpellState::Loading);
    // Whether the opened file was already in canonical form, i.e. saving it unedited would
    // change nothing.
    let round_trip_clean = RwSignal::new(None::<bool>);
    let words = RwSignal::new(0u32);
    let markdown = RwSignal::new(String::new());
    let typography = RwSignal::new(Typography::default());
    let show_typography = RwSignal::new(false);
    let panel = RwSignal::new(None::<Panel>);
    // Bumped by the backend's snapshot events, so the History panel reloads.
    let revision = RwSignal::new(0u32);
    tauri::listen("snapshot-taken", move || revision.update(|r| *r += 1));

    let on_change = move |md: String, count: u32| {
        words.set(count);
        markdown.set(md.clone());
        let Some(path) = current_path.get_untracked() else { return };
        save_state.set(SaveState::Saving);
        spawn_local(async move {
            save_state.set(match tauri::save_scene(&path, &md).await {
                Ok(()) => SaveState::Saved,
                Err(e) => SaveState::Failed(e),
            });
        });
    };

    let open = move |path: String| {
        // Flushing saves the scene being left while `current_path` still points at it; the
        // snapshot is queued after that save.
        editor.with_value(|h| {
            if let Some(h) = h {
                h.flush();
            }
        });
        if current_path.get_untracked().is_some() {
            spawn_local(async move {
                let _ = tauri::snapshot_now().await;
            });
        }
        spawn_local(async move {
            match tauri::open_scene(&path).await {
                Ok(scene) => editor.with_value(|h| {
                    let Some(h) = h else { return };
                    h.set_content(&scene.markdown);
                    let serialized = h.markdown();
                    round_trip_clean.set(Some(serialized == scene.markdown));
                    words.set(h.word_count());
                    markdown.set(serialized);
                    current_path.set(Some(scene.path));
                    save_state.set(SaveState::Saved);
                    h.focus();
                }),
                Err(e) => save_state.set(SaveState::Failed(e)),
            }
        });
    };

    let on_restore = move |md: String| {
        editor.with_value(|h| {
            if let Some(h) = h {
                h.set_content(&md);
                words.set(h.word_count());
            }
        });
        markdown.set(md);
        save_state.set(SaveState::Saved);
    };

    let toggle = move |which: Panel| panel.update(|p| *p = if *p == Some(which) { None } else { Some(which) });

    let load_spellchecker = move || {
        spawn_local(async move {
            let speller = tauri::spell_dictionary().await.and_then(|dictionary| {
                let mut speller = Speller::new(&dictionary.aff, &dictionary.dic).map_err(|e| e.to_string())?;
                for word in &dictionary.personal_words {
                    // A malformed line in the word list shouldn't disable spellcheck.
                    let _ = speller.add(word);
                }
                Ok(speller)
            });
            match speller {
                Ok(speller) => {
                    let bridge = SpellBridge::new(speller, move |word| {
                        spawn_local(async move {
                            if let Err(e) = tauri::add_to_dictionary(&word).await {
                                spell_state.set(SpellState::Failed(format!("couldn't save “{word}”: {e}")));
                            }
                        });
                    });
                    editor.with_value(|h| {
                        if let Some(h) = h {
                            h.set_spellchecker(bridge.as_object());
                        }
                    });
                    spell_bridge.set_value(Some(bridge));
                    spell_state.set(SpellState::Ready);
                }
                Err(e) => spell_state.set(SpellState::Failed(e)),
            }
        });
    };

    let on_ready = move || {
        load_spellchecker();
        spawn_local(async move {
            match tauri::sample_scenes().await {
                Ok(list) => {
                    if let Some(first) = list.first() {
                        open(first.path.clone());
                    }
                    samples.set(list);
                }
                Err(e) => save_state.set(SaveState::Failed(e)),
            }
        });
    };

    let scene_button = move |scene: SampleScene| {
        let path = scene.path.clone();
        let is_current = {
            let path = path.clone();
            move || current_path.get().as_deref() == Some(path.as_str())
        };
        view! {
            <button class:active=is_current on:click=move |_| open(path.clone())>
                {scene.name}
            </button>
        }
    };

    view! {
        <header class="topbar">
            <nav class="scenes">
                <For each=move || samples.get() key=|scene| scene.path.clone() children=scene_button />
            </nav>
            <div class="status">
                {move || {
                    round_trip_clean
                        .get()
                        .map(|clean| {
                            view! {
                                <span class="badge" class:warn=!clean>
                                    {if clean { "Round-trip clean" } else { "Normalizes on first save" }}
                                </span>
                            }
                        })
                }}
                {move || match spell_state.get() {
                    SpellState::Loading => Some(view! { <span>"Loading dictionary…"</span> }.into_any()),
                    SpellState::Ready => None,
                    SpellState::Failed(e) => {
                        Some(view! { <span class="error">{format!("Spellcheck unavailable: {e}")}</span> }.into_any())
                    }
                }}
                <span>{move || format!("{} words", words.get())}</span>
                <span class="save" class:error=move || matches!(save_state.get(), SaveState::Failed(_))>
                    {move || match save_state.get() {
                        SaveState::Saved => "Saved".to_owned(),
                        SaveState::Saving => "Saving…".to_owned(),
                        SaveState::Failed(e) => format!("Error: {e}"),
                    }}
                </span>
                <div class="popover-anchor">
                    <button class:active=move || show_typography.get() on:click=move |_| show_typography.update(|v| *v = !*v)>
                        "Typography"
                    </button>
                    <Show when=move || show_typography.get()>
                        <div class="backdrop" on:click=move |_| show_typography.set(false)></div>
                        <TypographySettings typography=typography />
                    </Show>
                </div>
                <button class:active=move || panel.get() == Some(Panel::History) on:click=move |_| toggle(Panel::History)>
                    "History"
                </button>
                <button class:active=move || panel.get() == Some(Panel::Markdown) on:click=move |_| toggle(Panel::Markdown)>
                    "Markdown"
                </button>
            </div>
        </header>
        <main class="workspace">
            <div class="page">
                <Editor handle=editor typography=typography on_change=on_change on_ready=on_ready />
            </div>
            {move || match panel.get() {
                None => None,
                Some(Panel::Markdown) => Some(view! {
                    <aside class="side-panel markdown-panel">
                        <pre>{move || markdown.get()}</pre>
                    </aside>
                }.into_any()),
                Some(Panel::History) => Some(view! {
                    <HistoryPanel path=current_path revision=revision editor=editor on_restore=on_restore />
                }.into_any()),
            }}
        </main>
    }
}

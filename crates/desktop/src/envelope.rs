//! A scene's envelope. The back of a sewing-pattern envelope lists the notions a pattern needs;
//! this lists what a scene needs: its POV, cast, places and threads, with faint hints for notes
//! the text names that aren't listed. It sits beside the scene, staying in view as the page
//! scrolls, or in a narrow window slides out from a tab on the page's edge.

use leptos::{ev, html, prelude::*, task::spawn_local};
use needle_core::project::ProjectKind;

use crate::icons::{Glyph, Icon};
use crate::notes::kind_glyph;
use crate::tauri::{FieldView, HintView, NoteKey, NoteView, SceneNamesView};

/// What each field is called, and the type of note it lists.
fn field_info(field: &str) -> (&'static str, &'static str) {
    match field {
        "pov" => ("Pov", "character"),
        "cast" => ("Cast", "character"),
        "places" => ("Places", "place"),
        _ => ("Threads", "thread"),
    }
}

#[component]
pub fn EnvelopeCard(
    #[prop(into)] names: Signal<Option<SceneNamesView>>,
    #[prop(into)] title: Signal<String>,
    #[prop(into)] summary: Signal<String>,
    /// "Piece 3", as on the scene.
    #[prop(into)]
    piece: Signal<String>,
    #[prop(into)] notes: Signal<Vec<NoteView>>,
    #[prop(into)] kind: Signal<ProjectKind>,
    /// Makes the ids of its suggestion lists unique, since the card can show in two places.
    place: &'static str,
    /// Sets a field to these names.
    on_set: impl Fn(String, Vec<String>) + Copy + Send + Sync + 'static,
    /// Saves a new summary.
    on_summary: impl Fn(String) + Copy + Send + Sync + 'static,
    on_open_note: impl Fn(NoteKey) + Copy + Send + Sync + 'static,
    /// Makes a note from its title and type.
    on_make_note: impl Fn(String, String) + Copy + Send + Sync + 'static,
    #[prop(optional, into)] on_close: Option<Callback<()>>,
) -> impl IntoView {
    // The field being added to, if any.
    let adding = RwSignal::new(None::<String>);

    let field_view = move |field: FieldView, hints: Vec<HintView>| {
        let (label, note_kind) = field_info(&field.field);
        let current: Vec<String> = field.names.iter().map(|n| n.name.clone()).collect();
        let list_id = format!("names-{place}-{}", field.field);
        let single = field.field == "pov";
        let name = field.field.clone();

        let chips = field
            .names
            .into_iter()
            .enumerate()
            .map(|(i, header)| {
                let (field, current) = (name.clone(), current.clone());
                let remove = move |_| {
                    let mut rest = current.clone();
                    rest.remove(i);
                    on_set(field.clone(), rest);
                };
                let shown = header.name.clone();
                match header.note {
                    Some(note) => {
                        let key = note.key();
                        view! {
                            <span class="chip">
                                <button class="chip-open" title=format!("Open {}", note.title) on:click=move |_| on_open_note(key.clone())>
                                    <Icon glyph=kind_glyph(&note.kind) size=13 />
                                    {shown.clone()}
                                </button>
                                <button class="chip-remove" aria-label=format!("Remove {shown}") on:click=remove>
                                    <Icon glyph=Glyph::Close size=12 />
                                </button>
                            </span>
                        }
                        .into_any()
                    }
                    None => {
                        let make = shown.clone();
                        view! {
                            <span class="chip basted">
                                <button
                                    class="chip-open"
                                    title="No note yet. Click to make one."
                                    on:click=move |_| on_make_note(make.clone(), note_kind.to_owned())
                                >
                                    {shown.clone()}
                                </button>
                                <button class="chip-remove" aria-label=format!("Remove {shown}") on:click=remove>
                                    <Icon glyph=Glyph::Close size=12 />
                                </button>
                            </span>
                        }
                        .into_any()
                    }
                }
            })
            .collect_view();

        let hint_chips = hints
            .into_iter()
            .map(|hint| {
                let (field, mut more) = (name.clone(), current.clone());
                let title = hint.note.title.clone();
                more.push(title.clone());
                view! {
                    <button
                        class="chip-hint"
                        title="Named in the text but not listed here. Click to add."
                        on:click=move |_| on_set(field.clone(), more.clone())
                    >
                        {title}
                        <b>"+"</b>
                    </button>
                }
            })
            .collect_view();

        let show_add = !single || current.is_empty();
        let (add_field, input_field) = (name.clone(), name.clone());
        let is_adding = move || adding.get().as_deref() == Some(input_field.as_str());
        let commit = {
            let (field, current) = (name.clone(), current.clone());
            move |value: String| {
                adding.set(None);
                let value = value.trim().to_owned();
                if value.is_empty() || current.contains(&value) {
                    return;
                }
                let names = if single { vec![value] } else { current.iter().cloned().chain([value]).collect() };
                on_set(field.clone(), names);
            }
        };
        let suggestions = move || {
            notes.with(|all| {
                all.iter()
                    .filter(|n| n.kind == note_kind)
                    .map(|n| view! { <option value=n.title.clone()></option> })
                    .collect_view()
            })
        };
        view! {
            <div class="envelope-field">
                <h3 class="envelope-label">{label}</h3>
                <div class="chips">
                    {chips}
                    {show_add.then(|| view! {
                        <button
                            class="chip-add"
                            aria-label=format!("Add to {}", label.to_lowercase())
                            title=format!("Add to {}", label.to_lowercase())
                            on:click=move |_| adding.set(Some(add_field.clone()))
                        >
                            <Icon glyph=Glyph::Plus size=13 />
                        </button>
                    })}
                    {hint_chips}
                </div>
                <Show when=is_adding.clone()>
                    <input
                        class="chip-input"
                        list=list_id.clone()
                        placeholder=match note_kind {
                            "place" => "A place",
                            "thread" => "A thread",
                            _ => "A character",
                        }
                        autofocus=true
                        on:change={
                            let commit = commit.clone();
                            move |ev| commit(event_target_value(&ev))
                        }
                        on:keydown=move |ev| {
                            if ev.key() == "Escape" {
                                adding.set(None);
                            }
                        }
                        on:blur=move |_| adding.set(None)
                    />
                    <datalist id=list_id.clone()>{suggestions}</datalist>
                </Show>
            </div>
        }
    };

    let fields = move || {
        let _ = kind.get();
        names.get().map(|names| {
            names
                .fields
                .into_iter()
                .map(|field| {
                    let hints = names.hints.iter().filter(|h| h.field == field.field).cloned().collect();
                    field_view(field, hints)
                })
                .collect_view()
        })
    };

    view! {
        <div class="envelope">
            {on_close.map(|close| view! {
                <button class="icon-button envelope-close" aria-label="Close" on:click=move |_| close.run(())>
                    <Icon glyph=Glyph::Close size=15 />
                </button>
            })}
            <div class="envelope-head label">
                <span>"Notions"</span>
                <span class="muted">{move || piece.get()}</span>
            </div>
            <div class="envelope-title title">{move || title.get()}</div>
            <Summary summary=summary on_save=on_summary />
            <hr class="envelope-rule" />
            {fields}
        </div>
    }
}

/// The scene's summary, under its title. It reads as printed on the card until clicked, then
/// it's a field in the same place: Enter or clicking away saves, Escape cancels. One paragraph,
/// since the header holds one string.
#[component]
fn Summary(#[prop(into)] summary: Signal<String>, on_save: impl Fn(String) + Copy + Send + Sync + 'static) -> impl IntoView {
    let editing = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    let field = NodeRef::<html::Textarea>::new();
    // Grows the field with its text, so it never scrolls inside itself.
    let fit = move || {
        if let Some(el) = field.get_untracked() {
            let _ = el.set_attribute("style", "height: auto");
            let _ = el.set_attribute("style", &format!("height: {}px", el.scroll_height() + 2));
        }
    };
    Effect::new(move |_| {
        if let Some(el) = field.get() {
            // It's measured once it's on the page.
            request_animation_frame(fit);
            let _ = el.focus();
            let end = el.value().encode_utf16().count() as u32;
            let _ = el.set_selection_range(end, end);
        }
    });
    // Blur also fires after Enter or Escape has finished, so only the first one counts.
    let finish = move |save: bool| {
        if !editing.get_untracked() {
            return;
        }
        editing.set(false);
        let text = draft.get_untracked();
        if save && text.split_whitespace().collect::<Vec<_>>().join(" ") != summary.get_untracked() {
            on_save(text);
        }
    };
    let on_key = move |ev: ev::KeyboardEvent| match ev.key().as_str() {
        "Enter" => {
            ev.prevent_default();
            finish(true);
        }
        "Escape" => finish(false),
        _ => {}
    };

    view! {
        <Show
            when=move || editing.get()
            fallback=move || {
                view! {
                    <button
                        type="button"
                        class="envelope-summary"
                        class:empty=move || summary.with(String::is_empty)
                        on:click=move |_| {
                            draft.set(summary.get_untracked());
                            editing.set(true);
                        }
                    >
                        {move || match summary.get() {
                            s if s.is_empty() => "What happens here?".to_owned(),
                            s => s,
                        }}
                    </button>
                }
            }
        >
            <textarea
                class="envelope-summary-field"
                node_ref=field
                rows="1"
                aria-label="Summary"
                placeholder="What happens here?"
                prop:value=move || draft.get()
                on:input=move |ev| {
                    draft.set(event_target_value(&ev));
                    fit();
                }
                on:keydown=on_key
                on:blur=move |_| finish(true)
            ></textarea>
        </Show>
    }
}

/// How many hints the envelope has, for the tab's badge.
pub fn hint_count(names: &Option<SceneNamesView>) -> usize {
    names.as_ref().map_or(0, |n| n.hints.len())
}

/// Spawns `fetch` and hands its result to `done` if `still` holds when it arrives.
pub fn load<T: 'static>(
    fetch: impl std::future::Future<Output = Result<T, String>> + 'static,
    still: impl Fn() -> bool + 'static,
    done: impl Fn(T) + 'static,
) {
    spawn_local(async move {
        if let Ok(value) = fetch.await
            && still()
        {
            done(value);
        }
    });
}

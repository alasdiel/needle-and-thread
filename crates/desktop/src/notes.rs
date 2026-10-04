//! Notes: the list in the sidebar's Notes tab, the head of an open note (drawn as a fabric
//! swatch), and the smaller swatch pinned beside it with what links to the note.

use leptos::{prelude::*, task::spawn_local};
use needle_core::project::{NoteKind, ProjectKind};

use crate::icons::{Glyph, Icon};
use crate::status::{self, StatusMark};
use crate::tauri::{self, NoteKey, NoteLinksView, NoteView};

pub fn project_kind(kind: &str) -> ProjectKind {
    if kind == "nonfiction" { ProjectKind::Nonfiction } else { ProjectKind::Fiction }
}

fn note_kind(kind: &str) -> NoteKind {
    NoteKind::parse(kind).unwrap_or(NoteKind::Note)
}

/// "Character", or "Plot point" / "Event" depending on the project.
pub fn kind_label(kind: &str, project: ProjectKind) -> &'static str {
    note_kind(kind).label(project)
}

pub fn kind_glyph(kind: &str) -> Glyph {
    match note_kind(kind) {
        NoteKind::Character => Glyph::Person,
        NoteKind::Place => Glyph::MapPin,
        NoteKind::Thread => Glyph::Thread,
        NoteKind::Source => Glyph::Book,
        NoteKind::Event => Glyph::Flag,
        NoteKind::Relationship => Glyph::Link,
        NoteKind::Note => Glyph::File,
    }
}

/// The pin holding a swatch to the bench.
#[component]
pub fn Pin(#[prop(optional)] class: &'static str) -> impl IntoView {
    view! {
        <svg
            class=format!("pin {class}")
            width="96"
            height="64"
            viewBox="0 0 96 64"
            aria-hidden="true"
            inner_html=r#"<path class="pin-shaft" d="M14 14 40 33"/><path class="pin-shaft" d="M54 43 84 61"/><circle class="pin-head" cx="12" cy="12" r="8"/><circle class="pin-shine" cx="9.5" cy="9.5" r="2.4"/>"#
        ></svg>
    }
}

/// The Notes tab: this project's notes grouped by type, then its world's, and a form for a
/// new one.
#[component]
pub fn NotesList(
    #[prop(into)] notes: Signal<Vec<NoteView>>,
    #[prop(into)] project: Signal<Option<String>>,
    /// The project's world, as (folder, name).
    #[prop(into)]
    world: Signal<Option<(String, String)>>,
    #[prop(into)] kind: Signal<ProjectKind>,
    #[prop(into)] current: Signal<Option<NoteKey>>,
    on_open: impl Fn(NoteKey) + Copy + Send + Sync + 'static,
    /// Receives the new note's type and title.
    on_create: impl Fn(String, String) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let creating = RwSignal::new(false);

    let row = move |note: NoteView| {
        let key = note.key();
        let is_current = {
            let key = key.clone();
            move || current.with(|c| c.as_ref() == Some(&key))
        };
        view! {
            <li class="note-row" class:current=is_current>
                <button class="note-title" title=note.summary.clone() on:click=move |_| on_open(key.clone())>
                    <Icon glyph=kind_glyph(&note.kind) size=14 />
                    <span class="note-label">{note.title.clone()}</span>
                </button>
            </li>
        }
    };

    // One section per owner: the project's notes, then the world's under its name.
    let section = move |heading: Option<String>, notes: Vec<NoteView>| {
        let project_kind = kind.get();
        let groups = NoteKind::ALL
            .into_iter()
            .filter_map(|k| {
                let mine: Vec<NoteView> = notes.iter().filter(|n| note_kind(&n.kind) == k).cloned().collect();
                (!mine.is_empty()).then(|| {
                    let count = mine.len();
                    view! {
                        <li class="note-group">
                            <div class="part-head">
                                <span>{k.plural(project_kind)}</span>
                                <span class="count">{count}</span>
                            </div>
                            <ul>{mine.into_iter().map(row).collect_view()}</ul>
                        </li>
                    }
                })
            })
            .collect_view();
        view! {
            {heading.map(|h| view! { <h3 class="notes-world">{h}</h3> })}
            <ul class="note-groups">{groups}</ul>
        }
    };

    let sections = move || {
        let here = project.get().unwrap_or_default();
        let world = world.get();
        notes.with(|all| {
            let mine: Vec<NoteView> = all.iter().filter(|n| !n.world && n.owner == here).cloned().collect();
            let shared: Vec<NoteView> = match &world {
                Some((slug, _)) => all.iter().filter(|n| n.world && n.owner == *slug).cloned().collect(),
                None => Vec::new(),
            };
            let empty = mine.is_empty() && shared.is_empty();
            view! {
                {section(None, mine)}
                {(!shared.is_empty()).then(|| section(world.map(|(_, name)| name), shared))}
                {empty.then(|| view! {
                    <p class="notes-empty">"No notes yet. Characters, places and threads go here."</p>
                })}
            }
        })
    };

    view! {
        <nav class="notes-list" aria-label="Notes">
            {sections}
            <Show
                when=move || creating.get()
                fallback=move || view! {
                    <div class="add-chapter">
                        <button class="add" on:click=move |_| creating.set(true)>
                            <Icon glyph=Glyph::Plus size=14 />
                            "New note"
                        </button>
                    </div>
                }
            >
                <NewNoteForm
                    kind=kind
                    on_create=move |k, t| {
                        creating.set(false);
                        on_create(k, t);
                    }
                    on_cancel=move || creating.set(false)
                />
            </Show>
        </nav>
    }
}

#[component]
fn NewNoteForm(
    #[prop(into)] kind: Signal<ProjectKind>,
    on_create: impl Fn(String, String) + Copy + Send + Sync + 'static,
    on_cancel: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let title = RwSignal::new(String::new());
    let note_kind = RwSignal::new("character".to_owned());
    let input = NodeRef::<leptos::html::Input>::new();
    Effect::new(move |_| {
        if let Some(el) = input.get() {
            let _ = el.focus();
        }
    });
    view! {
        <form class="new-note" on:submit=move |ev| {
            ev.prevent_default();
            on_create(note_kind.get_untracked(), title.get_untracked());
        }>
            <input
                node_ref=input
                placeholder="Name, e.g. Mara Venn"
                prop:value=move || title.get()
                on:input=move |ev| title.set(event_target_value(&ev))
                on:keydown=move |ev| {
                    if ev.key() == "Escape" {
                        on_cancel();
                    }
                }
            />
            <select aria-label="Type" on:change=move |ev| note_kind.set(event_target_value(&ev))>
                {move || {
                    let project = kind.get();
                    NoteKind::ALL
                        .into_iter()
                        .map(|k| view! { <option value=k.as_str()>{k.label(project)}</option> })
                        .collect_view()
                }}
            </select>
            <div class="actions">
                <button type="submit" class="primary">"Create"</button>
                <button type="button" class="quiet" on:click=move |_| on_cancel()>"Cancel"</button>
            </div>
        </form>
    }
}

/// What a scene's header field is called in the list.
fn field_label(field: &str) -> &str {
    match field {
        "places" => "place",
        "threads" => "thread",
        other => other,
    }
}

/// The smaller swatch beside an open note: the scenes that list it in their header, the scenes
/// and notes that link to it, and scenes that mention it without a link.
#[component]
pub fn LinksSwatch(
    #[prop(into)] project: Signal<Option<String>>,
    #[prop(into)] note: Signal<Option<NoteView>>,
    #[prop(into)] statuses: Signal<Vec<String>>,
    on_open_scene: impl Fn(String) + Copy + Send + Sync + 'static,
    on_open_note: impl Fn(NoteKey) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let links = RwSignal::new(None::<NoteLinksView>);
    let error = RwSignal::new(None::<String>);
    // Bumped after linking a mention, to fetch the lists again.
    let refresh = RwSignal::new(0u32);
    let key = Memo::new(move |_| note.with(|n| n.as_ref().map(NoteView::key)));

    Effect::new(move |_| {
        refresh.track();
        let (Some(p), Some(k)) = (project.get(), key.get()) else { return };
        spawn_local(async move {
            let result = tauri::note_links(&p, &k).await;
            // A slow answer for a note that's no longer open is dropped.
            if key.get_untracked().as_ref() != Some(&k) {
                return;
            }
            match result {
                Ok(view) => {
                    error.set(None);
                    links.set(Some(view));
                }
                Err(e) => error.set(Some(e)),
            }
        });
    });

    let link_mention = move |scene: String| {
        let (Some(p), Some(k)) = (project.get_untracked(), key.get_untracked()) else { return };
        spawn_local(async move {
            match tauri::link_mention(&p, &scene, &k).await {
                Ok(_) => refresh.update(|r| *r += 1),
                Err(e) => error.set(Some(e)),
            }
        });
    };

    let scene_row = move |scene: tauri::SceneView, note_text: String| {
        let progress = status::progress(&scene.status, &statuses.get_untracked());
        let slug = scene.slug.clone();
        view! {
            <button class="link-row" title=scene.summary.clone() on:click=move |_| on_open_scene(slug.clone())>
                <StatusMark progress=progress />
                <span class="link-row-title">{scene.title.clone()}</span>
                <span class="count">{note_text}</span>
            </button>
        }
    };

    let lists = move || {
        links.get().map(|l| {
            let empty = l.appears_in.is_empty() && l.linked_from.is_empty() && l.mentioned_in.is_empty();
            let appears = (!l.appears_in.is_empty()).then(|| {
                view! {
                    <h3 class="links-head">"Appears in"</h3>
                    {l.appears_in
                        .into_iter()
                        .map(|a| {
                            let fields = a.fields.iter().map(|f| field_label(f)).collect::<Vec<_>>().join(", ");
                            scene_row(a.scene, fields)
                        })
                        .collect_view()}
                }
            });
            let linked = (!l.linked_from.is_empty()).then(|| {
                view! {
                    <h3 class="links-head">"Linked from"</h3>
                    {l.linked_from
                        .into_iter()
                        .map(|b| match (b.scene, b.note) {
                            (Some(scene), _) => scene_row(scene, b.count.to_string()).into_any(),
                            (None, Some(n)) => {
                                let key = n.key();
                                view! {
                                    <button class="link-row" on:click=move |_| on_open_note(key.clone())>
                                        <Icon glyph=kind_glyph(&n.kind) size=13 />
                                        <span class="link-row-title">{n.title.clone()}</span>
                                        <span class="count">{b.count}</span>
                                    </button>
                                }
                                .into_any()
                            }
                            (None, None) => ().into_any(),
                        })
                        .collect_view()}
                }
            });
            let mentioned = (!l.mentioned_in.is_empty()).then(|| {
                view! {
                    <h3 class="links-head">"Mentioned, not linked"</h3>
                    {l.mentioned_in
                        .into_iter()
                        .map(|m| {
                            let (open, link) = (m.scene.slug.clone(), m.scene.slug.clone());
                            let more = m.count.saturating_sub(1);
                            view! {
                                <div class="mention">
                                    <div class="mention-head">
                                        <button class="quiet mention-scene" on:click=move |_| on_open_scene(open.clone())>
                                            {m.scene.title.clone()}
                                        </button>
                                        <button
                                            class="pill"
                                            title="Make this mention a link"
                                            on:click=move |_| link_mention(link.clone())
                                        >
                                            "Link"
                                        </button>
                                    </div>
                                    <p class="mention-text">{m.before}<b>{m.name}</b>{m.after}</p>
                                    {(more > 0).then(|| view! {
                                        <span class="mention-more">{format!("and {more} more in this scene")}</span>
                                    })}
                                </div>
                            }
                        })
                        .collect_view()}
                }
            });
            view! {
                {appears}
                {linked}
                {mentioned}
                {empty.then(|| view! { <p class="links-empty">"Nothing links here yet."</p> })}
            }
        })
    };

    view! {
        <aside class="links-holder" aria-label="Links">
            <div class="swatch links-swatch">
                <div class="links-title label">
                    <span>"Links"</span>
                    <span class="muted">{move || note.with(|n| n.as_ref().map(|n| format!("to {}", n.title)))}</span>
                </div>
                {move || error.get().map(|e| view! { <p class="error">{e}</p> })}
                {lists}
            </div>
            <Pin class="pin-blue" />
        </aside>
    }
}

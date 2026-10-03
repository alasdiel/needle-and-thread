//! The main screen: projects and the outline on the left, the open scene in the middle drawn as
//! a pattern piece, and the History or Markdown panel on the right.

use leptos::{prelude::*, task::spawn_local};
use needle_core::spell::Speller;

use crate::editor::{Editor, EditorHandle, Typography};
use crate::history::HistoryPanel;
use crate::icons::{Glyph, Icon};
use crate::outline::{self, OutlineTree};
use crate::pattern::{Grainline, Notches};
use crate::settings::{Prefs, SettingsPanel};
use crate::spell::SpellBridge;
use crate::status::{self, StatusMark};
use crate::tauri::{self, OutlineView, ProjectView, SceneView, VaultView};
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

/// Scenes in reading order: chapters first, then unplaced ones.
fn reading_order(outline: &OutlineView) -> Vec<&SceneView> {
    outline.chapters.iter().flat_map(|c| &c.scenes).chain(&outline.unplaced).collect()
}

fn find(outline: &OutlineView, slug: &str) -> Option<SceneView> {
    reading_order(outline).into_iter().find(|s| s.slug == slug).cloned()
}

/// The scene to open after `slug` goes away: the one after it, else the one before.
fn neighbour(outline: &OutlineView, slug: &str) -> Option<String> {
    let order = reading_order(outline);
    let i = order.iter().position(|s| s.slug == slug)?;
    order.get(i + 1).or_else(|| i.checked_sub(1).and_then(|j| order.get(j))).map(|s| s.slug.clone())
}

/// Where a scene sits, for the breadcrumb and the piece's label.
struct Place {
    part: Option<String>,
    /// None for a scene that isn't in the outline.
    chapter: Option<String>,
    /// The scene's number in reading order, from 1.
    number: usize,
}

fn place(outline: &OutlineView, slug: &str) -> Option<Place> {
    let mut number = 0;
    for (index, chapter) in outline.chapters.iter().enumerate() {
        for scene in &chapter.scenes {
            number += 1;
            if scene.slug == slug {
                let title = outline::chapter_title(chapter, index);
                return Some(Place { part: chapter.part.clone(), chapter: Some(title), number });
            }
        }
    }
    let i = outline.unplaced.iter().position(|s| s.slug == slug)?;
    Some(Place { part: None, chapter: None, number: number + i + 1 })
}

fn kind_label(kind: &str) -> &str {
    match kind {
        "fiction" => "Fiction",
        "nonfiction" => "Nonfiction",
        other => other,
    }
}

#[component]
pub fn Workspace(vault: VaultView, on_open_vault: impl Fn(VaultView) + Copy + Send + Sync + 'static) -> impl IntoView {
    let prefs = expect_context::<Prefs>();
    let editor = StoredValue::new_local(None::<EditorHandle>);
    let spell_bridge = StoredValue::new_local(None::<SpellBridge>);
    let projects = RwSignal::new(vault.projects.clone());
    let project = RwSignal::new(vault.projects.first().map(|p| p.slug.clone()));
    let outline = RwSignal::new(None::<OutlineView>);
    let scene = RwSignal::new(None::<SceneView>);
    let save_state = RwSignal::new(SaveState::Saved);
    let error = RwSignal::new(None::<String>);
    let spell_error = RwSignal::new(None::<String>);
    let words = RwSignal::new(0usize);
    let markdown = RwSignal::new(String::new());
    let typography = RwSignal::new(Typography::default());
    let show_typography = RwSignal::new(false);
    let show_settings = RwSignal::new(false);
    let show_scene_menu = RwSignal::new(false);
    let panel = RwSignal::new(None::<Panel>);
    // The cut line is showing, waiting for Enter or Escape.
    let cutting = RwSignal::new(false);
    // After a merge: where the merged-in scene starts, in top-level blocks, for the seam.
    let pending_seam = StoredValue::new(None::<u32>);
    let revision = RwSignal::new(0u32);
    let new_project = RwSignal::new(None::<String>);
    tauri::listen("snapshot-taken", move || {
        revision.try_update(|r| *r += 1);
    });

    let current = move || scene.with_untracked(|s| s.as_ref().map(|s| s.slug.clone()));
    let flush = move || {
        editor.with_value(|h| {
            if let Some(h) = h {
                h.flush();
            }
        })
    };
    let report = move |e: String| error.set(Some(e));

    let on_change = move |md: String, count: u32| {
        words.set(count as usize);
        markdown.set(md.clone());
        let (Some(p), Some(s)) = (project.get_untracked(), current()) else { return };
        outline.update(|o| {
            if let Some(scene) = o.as_mut().and_then(|o| {
                o.chapters.iter_mut().flat_map(|c| &mut c.scenes).chain(&mut o.unplaced).find(|x| x.slug == s)
            }) {
                scene.words = count as usize;
            }
        });
        save_state.set(SaveState::Saving);
        spawn_local(async move {
            save_state.set(match tauri::save_scene(&p, &s, &md).await {
                Ok(()) => SaveState::Saved,
                Err(e) => SaveState::Failed(e),
            });
        });
    };

    let close_scene = move || {
        scene.set(None);
        editor.with_value(|h| {
            if let Some(h) = h {
                h.set_content("");
            }
        });
        words.set(0);
        markdown.set(String::new());
    };

    let open_scene = move |slug: String| {
        let Some(p) = project.get_untracked() else { return };
        // Loading new text takes the cut line away with it.
        cutting.set(false);
        // Flushing saves the scene being left while it's still current; the snapshot is
        // queued after that save.
        flush();
        if current().is_some() {
            spawn_local(async {
                let _ = tauri::snapshot_now().await;
            });
        }
        spawn_local(async move {
            match tauri::open_scene(&p, &slug).await {
                Ok(opened) => {
                    let seam = pending_seam.try_update_value(Option::take).flatten();
                    editor.with_value(|h| {
                        if let Some(h) = h {
                            h.set_content(&opened.markdown);
                            words.set(h.word_count() as usize);
                            markdown.set(h.markdown());
                        }
                    });
                    scene.set(Some(opened.scene));
                    save_state.set(SaveState::Saved);
                    // The piece is hidden while no scene is open (as during a split or merge), and
                    // a hidden editor can't take the cursor, so wait until it shows again.
                    request_animation_frame(move || {
                        editor.with_value(|h| {
                            if let Some(h) = h {
                                h.focus();
                                if let Some(index) = seam {
                                    h.show_seam(index);
                                }
                            }
                        });
                    });
                }
                Err(e) => report(e),
            }
        });
    };

    // After any outline change: refresh the open scene's title and status, and move on if
    // the open scene is gone. The outline is set first, so views of the scene see the new one.
    let apply = move |view: OutlineView| {
        let open = current();
        let info = open.as_deref().and_then(|s| find(&view, s));
        let first = reading_order(&view).first().map(|s| s.slug.clone());
        outline.set(Some(view));
        match info {
            Some(info) => scene.set(Some(info)),
            None if open.is_some() => {
                scene.set(None);
                match first {
                    Some(first) => open_scene(first),
                    None => close_scene(),
                }
            }
            None => {}
        }
    };

    let load_project = move |slug: String| {
        flush();
        spawn_local(async move {
            match tauri::project_outline(&slug).await {
                Ok(view) => {
                    let first = reading_order(&view).first().map(|s| s.slug.clone());
                    scene.set(None);
                    outline.set(Some(view));
                    match first {
                        Some(first) => open_scene(first),
                        None => close_scene(),
                    }
                }
                Err(e) => report(e),
            }
        });
    };

    let load_spellchecker = move || {
        spawn_local(async move {
            let speller = tauri::spell_dictionary().await.and_then(|d| {
                let mut speller = Speller::new(&d.aff, &d.dic).map_err(|e| e.to_string())?;
                for word in &d.personal_words {
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
                                spell_error.set(Some(format!("couldn't save “{word}”: {e}")));
                            }
                        });
                    });
                    editor.with_value(|h| {
                        if let Some(h) = h {
                            h.set_spellchecker(bridge.as_object());
                        }
                    });
                    spell_bridge.set_value(Some(bridge));
                }
                Err(e) => spell_error.set(Some(e)),
            }
        });
    };

    let on_ready = move || {
        load_spellchecker();
        if let Some(p) = project.get_untracked() {
            load_project(p);
        }
    };

    // --- Scene actions ---------------------------------------------------------------------

    let with_scene = move |action: &dyn Fn(String, String)| {
        if let (Some(p), Some(s)) = (project.get_untracked(), current()) {
            flush();
            action(p, s);
        }
    };

    let rename = move |title: String| {
        with_scene(&move |p, s| {
            let title = title.clone();
            spawn_local(async move {
                match tauri::rename_scene(&p, &s, &title).await {
                    Ok(view) => apply(view),
                    Err(e) => report(e),
                }
            });
        });
    };

    let set_status = move |status: String| {
        with_scene(&move |p, s| {
            let status = status.clone();
            spawn_local(async move {
                match tauri::set_scene_status(&p, &s, &status).await {
                    Ok(view) => apply(view),
                    Err(e) => report(e),
                }
            });
        });
    };

    let split = move || {
        with_scene(&move |p, s| {
            let (before, after) = editor.with_value(|h| h.as_ref().map(EditorHandle::split_parts)).unwrap_or_default();
            spawn_local(async move {
                match tauri::split_scene(&p, &s, &before, &after).await {
                    Ok(created) => {
                        // The editor still holds the whole text; don't save it over the split.
                        scene.set(None);
                        outline.set(Some(created.outline));
                        open_scene(created.scene);
                    }
                    Err(e) => report(e),
                }
            });
        });
    };

    // Split shows the cut line first; Enter (or "Cut here") splits, Escape puts it away.
    let show_cut_line = move |on: bool| {
        cutting.set(on);
        editor.with_value(|h| {
            if let Some(h) = h {
                h.show_cut_line(on);
            }
        });
    };
    let on_cut_confirm = move || {
        show_cut_line(false);
        split();
    };
    let on_cut_cancel = move || show_cut_line(false);

    let merge = move || {
        with_scene(&move |p, s| {
            // The merged-in scene's text goes after this one's, so the seam goes after its blocks.
            pending_seam.set_value(editor.with_value(|h| h.as_ref().map(EditorHandle::block_count)));
            spawn_local(async move {
                match tauri::merge_scene(&p, &s).await {
                    Ok(view) => {
                        apply(view);
                        scene.set(None);
                        open_scene(s);
                    }
                    Err(e) => {
                        pending_seam.set_value(None);
                        report(e);
                    }
                }
            });
        });
    };

    let cut = move || {
        with_scene(&move |p, s| {
            let next = outline.with_untracked(|o| o.as_ref().and_then(|o| neighbour(o, &s)));
            spawn_local(async move {
                match tauri::cut_scene(&p, &s).await {
                    Ok(view) => {
                        scene.set(None);
                        outline.set(Some(view));
                        match next {
                            Some(next) => open_scene(next),
                            None => close_scene(),
                        }
                    }
                    Err(e) => report(e),
                }
            });
        });
    };

    let on_restore = move |md: String| {
        editor.with_value(|h| {
            if let Some(h) = h {
                h.set_content(&md);
                words.set(h.word_count() as usize);
            }
        });
        markdown.set(md);
        save_state.set(SaveState::Saved);
    };

    // --- Projects and vaults ---------------------------------------------------------------

    let choose_project = move |slug: String| {
        project.set(Some(slug.clone()));
        load_project(slug);
    };

    let create_project = move |title: String, kind: String| {
        new_project.set(None);
        spawn_local(async move {
            match tauri::create_project(&title, &kind).await {
                Ok(created) => {
                    let slug = created.slug.clone();
                    projects.update(|list| {
                        list.push(created);
                        list.sort_by_key(|p: &ProjectView| p.title.to_lowercase());
                    });
                    choose_project(slug);
                }
                Err(e) => report(e),
            }
        });
    };

    let open_other_vault = move |_| {
        flush();
        spawn_local(async move {
            match tauri::open_vault().await {
                Ok(Some(vault)) => on_open_vault(vault),
                Ok(None) => {}
                Err(e) => report(e),
            }
        });
    };

    // --- What the views show ---------------------------------------------------------------

    let toggle = move |which: Panel| panel.update(|p| *p = if *p == Some(which) { None } else { Some(which) });
    let target = Signal::derive(move || project.get().zip(scene.get().map(|s| s.slug)));
    let current_slug = Signal::derive(move || scene.get().map(|s| s.slug));
    let has_scene = move || scene.with(Option::is_some);
    let current_place = move || {
        let slug = scene.with(|s| s.as_ref().map(|s| s.slug.clone()))?;
        outline.with(|o| o.as_ref().and_then(|o| place(o, &slug)))
    };

    let project_title = move || {
        let slug = project.get();
        projects.with(|list| list.iter().find(|p| Some(&p.slug) == slug.as_ref()).map(|p| p.title.clone()))
    };
    let project_meta = move || {
        outline.with(|o| {
            o.as_ref().map(|o| {
                let total: usize = o.chapters.iter().map(|c| outline::words(&c.scenes)).sum();
                format!("{} · {}", kind_label(&o.project.kind), outline::format_words(total))
            })
        })
    };
    let vault_name = vault.path.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or(&vault.path).to_owned();
    let vault_title = format!("{}\nOpen another vault…", vault.path);

    let breadcrumb = move || {
        current_place().map(|place| match place.chapter {
            None => view! { <span>"Not in the outline"</span> }.into_any(),
            Some(chapter) => view! {
                {place.part.map(|part| view! {
                    <span>{part}</span>
                    <Icon glyph=Glyph::ChevronRight size=12 />
                })}
                <span>{chapter}</span>
            }
            .into_any(),
        })
    };

    let piece_label = move || {
        current_place().map(|place| match place.chapter {
            Some(chapter) => format!("Piece {} · {chapter}", place.number),
            None => format!("Piece {} · not in the outline", place.number),
        })
    };

    // Rebuilt only when the scene changes, so typing in the title isn't interrupted.
    let piece_head = move || {
        scene.get().map(|s| {
            let statuses = outline.with_untracked(|o| o.as_ref().map(|o| o.statuses.clone()).unwrap_or_default());
            let progress = status::progress(&s.status, &statuses);
            view! {
                <header class="piece-head">
                    <div class="piece-label">{piece_label}</div>
                    <input
                        class="scene-name"
                        aria-label="Scene title"
                        prop:value=s.title.clone()
                        on:change=move |ev| rename(event_target_value(&ev))
                    />
                    <div class="piece-meta">
                        <label class="status-pill" title="Status">
                            <StatusMark progress=progress />
                            <span>{s.status.clone()}</span>
                            <Icon glyph=Glyph::ChevronDown size=12 />
                            <select aria-label="Status" prop:value=s.status.clone() on:change=move |ev| set_status(event_target_value(&ev))>
                                {statuses.into_iter().map(|st| view! { <option value=st.clone()>{st.clone()}</option> }).collect_view()}
                            </select>
                        </label>
                        <span>{move || outline::format_words(words.get())}</span>
                    </div>
                </header>
            }
        })
    };

    view! {
        <div class="workspace-grid">
            <aside class="sidebar">
                <div class="sidebar-head">
                    <span class="spool">
                        <Icon glyph=Glyph::Spool size=24 />
                    </span>
                    <div class="project">
                        <div class="project-row">
                            <div class="project-picker">
                                <span class="project-title">{project_title}</span>
                                <Icon glyph=Glyph::ChevronDown size=14 />
                                <select
                                    aria-label="Project"
                                    prop:value=move || project.get().unwrap_or_default()
                                    on:change=move |ev| choose_project(event_target_value(&ev))
                                >
                                    <For each=move || projects.get() key=|p| p.slug.clone() let(p)>
                                        <option value=p.slug.clone()>{p.title.clone()}</option>
                                    </For>
                                </select>
                            </div>
                            <button
                                class="icon-button"
                                title="New project"
                                aria-label="New project"
                                on:click=move |_| new_project.set(Some(String::new()))
                            >
                                <Icon glyph=Glyph::Plus />
                            </button>
                        </div>
                        <span class="project-meta">{project_meta}</span>
                    </div>
                </div>
                <Show when=move || new_project.get().is_some()>
                    <NewProjectForm on_create=create_project on_cancel=move || new_project.set(None) />
                </Show>
                <div class="cut-rule" aria-hidden="true">
                    <Icon glyph=Glyph::Scissors size=14 />
                </div>
                <OutlineTree
                    project=project
                    outline=outline
                    current=current_slug
                    cutting=cutting
                    on_open=open_scene
                    on_outline=apply
                    on_error=report
                />
                <div class="cut-rule" aria-hidden="true">
                    <Icon glyph=Glyph::Scissors size=14 />
                </div>
                <div class="sidebar-foot">
                    <button class="quiet vault-button" title=vault_title on:click=open_other_vault>
                        <Icon glyph=Glyph::Folder size=15 />
                        <span>{vault_name}</span>
                    </button>
                    <div class="popover-anchor">
                        <button
                            class="icon-button"
                            title="Settings"
                            aria-label="Settings"
                            class:active=move || show_settings.get()
                            on:click=move |_| show_settings.update(|v| *v = !*v)
                        >
                            <Icon glyph=Glyph::Settings />
                        </button>
                        <Show when=move || show_settings.get()>
                            <div class="backdrop" on:click=move |_| show_settings.set(false)></div>
                            <SettingsPanel />
                        </Show>
                    </div>
                </div>
            </aside>

            <section class="main">
                <header class="topbar">
                    <nav class="breadcrumb" aria-label="Where this scene is">{breadcrumb}</nav>
                    <Show when=has_scene>
                        <span class="save-state" class:error=move || matches!(save_state.get(), SaveState::Failed(_))>
                            {move || match save_state.get() {
                                SaveState::Saved => view! { <Icon glyph=Glyph::Check size=14 /> "Saved" }.into_any(),
                                SaveState::Saving => "Saving…".into_any(),
                                SaveState::Failed(e) => format!("Couldn't save: {e}").into_any(),
                            }}
                        </span>
                        <div class="popover-anchor">
                            <button
                                class="icon-button typography-button"
                                title="Typography"
                                aria-label="Typography"
                                class:active=move || show_typography.get()
                                on:click=move |_| show_typography.update(|v| *v = !*v)
                            >
                                "Aa"
                            </button>
                            <Show when=move || show_typography.get()>
                                <div class="backdrop" on:click=move |_| show_typography.set(false)></div>
                                <TypographySettings typography=typography />
                            </Show>
                        </div>
                        <button
                            class="icon-button"
                            title="History"
                            aria-label="History"
                            class:active=move || panel.get() == Some(Panel::History)
                            on:click=move |_| toggle(Panel::History)
                        >
                            <Icon glyph=Glyph::History size=17 />
                        </button>
                        <div class="popover-anchor">
                            <button
                                class="icon-button"
                                title="Scene actions"
                                aria-label="Scene actions"
                                class:active=move || show_scene_menu.get()
                                on:click=move |_| show_scene_menu.update(|v| *v = !*v)
                            >
                                <Icon glyph=Glyph::More size=17 />
                            </button>
                            <Show when=move || show_scene_menu.get()>
                                <div class="backdrop" on:click=move |_| show_scene_menu.set(false)></div>
                                <div class="menu" role="menu" aria-label="Scene actions">
                                    <button role="menuitem" title="Shows a cut line at the cursor; Enter cuts there" on:click=move |_| {
                                        show_scene_menu.set(false);
                                        show_cut_line(true);
                                    }>
                                        <Icon glyph=Glyph::Scissors />
                                        "Split at the cursor"
                                    </button>
                                    <button role="menuitem" title="Add the next scene in this chapter to the end of this one" on:click=move |_| {
                                        show_scene_menu.set(false);
                                        merge();
                                    }>
                                        <Icon glyph=Glyph::Needle />
                                        "Merge with the next scene"
                                    </button>
                                    <hr />
                                    <button role="menuitem" on:click=move |_| {
                                        show_scene_menu.set(false);
                                        cut();
                                    }>
                                        <Icon glyph=Glyph::Basket />
                                        "Move to the cut bin"
                                    </button>
                                    <Show when=move || prefs.markdown_panel.get()>
                                        <hr />
                                        <button role="menuitem" on:click=move |_| {
                                            show_scene_menu.set(false);
                                            toggle(Panel::Markdown);
                                        }>
                                            {move || if panel.get() == Some(Panel::Markdown) { "Hide Markdown" } else { "Show Markdown" }}
                                        </button>
                                    </Show>
                                </div>
                            </Show>
                        </div>
                    </Show>
                </header>
                {move || error.get().map(|e| view! {
                    <div class="banner error">
                        <span>{e}</span>
                        <button class="quiet" on:click=move |_| error.set(None)>"Dismiss"</button>
                    </div>
                })}
                {move || spell_error.get().map(|e| view! {
                    <div class="banner error">
                        <span>{format!("Spellcheck unavailable: {e}")}</span>
                    </div>
                })}
                <div class="page" class:empty=move || !has_scene()>
                    // The editor stays mounted while no scene is open; the piece is only hidden.
                    <article class="pattern-piece piece">
                        <Notches />
                        <Grainline />
                        {piece_head}
                        <Editor
                            handle=editor
                            typography=typography
                            on_change=on_change
                            on_cut_confirm=on_cut_confirm
                            on_cut_cancel=on_cut_cancel
                            on_ready=on_ready
                        />
                    </article>
                    <Show when=move || !has_scene()>
                        <p class="empty-note">"No scene open. Pick one in the outline, or add one with the + on a chapter."</p>
                    </Show>
                </div>
            </section>

            {move || match panel.get() {
                Some(Panel::History) => Some(view! {
                    <HistoryPanel target=target revision=revision editor=editor on_restore=on_restore on_close=move || panel.set(None) />
                }.into_any()),
                Some(Panel::Markdown) if prefs.markdown_panel.get() => Some(view! {
                    <aside class="side-panel markdown-panel">
                        <div class="panel-head">
                            <h2>"Markdown"</h2>
                            <button class="icon-button" title="Close" aria-label="Close Markdown" on:click=move |_| panel.set(None)>
                                <Icon glyph=Glyph::Close />
                            </button>
                        </div>
                        <pre>{move || markdown.get()}</pre>
                    </aside>
                }.into_any()),
                _ => None,
            }}
        </div>
    }
}

#[component]
fn NewProjectForm(
    on_create: impl Fn(String, String) + Copy + Send + Sync + 'static,
    on_cancel: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let title = RwSignal::new(String::new());
    let kind = RwSignal::new("fiction".to_owned());
    view! {
        <form class="new-project" on:submit=move |ev| {
            ev.prevent_default();
            on_create(title.get_untracked(), kind.get_untracked());
        }>
            <input
                placeholder="Project title"
                prop:value=move || title.get()
                on:input=move |ev| title.set(event_target_value(&ev))
            />
            <select on:change=move |ev| kind.set(event_target_value(&ev))>
                <option value="fiction">"Fiction"</option>
                <option value="nonfiction">"Nonfiction"</option>
            </select>
            <div class="actions">
                <button type="submit" class="primary">"Create"</button>
                <button type="button" class="quiet" on:click=move |_| on_cancel()>"Cancel"</button>
            </div>
        </form>
    }
}

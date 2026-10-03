//! The main screen: projects and the outline on the left, the open scene in the middle,
//! History and Markdown panels on the right.

use leptos::{prelude::*, task::spawn_local};
use needle_core::spell::Speller;

use crate::editor::{Editor, EditorHandle, Typography};
use crate::history::HistoryPanel;
use crate::outline::OutlineTree;
use crate::spell::SpellBridge;
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

#[component]
pub fn Workspace(vault: VaultView, on_open_vault: impl Fn(VaultView) + Copy + Send + Sync + 'static) -> impl IntoView {
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
    let panel = RwSignal::new(None::<Panel>);
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
                    editor.with_value(|h| {
                        if let Some(h) = h {
                            h.set_content(&opened.markdown);
                            words.set(h.word_count() as usize);
                            markdown.set(h.markdown());
                            h.focus();
                        }
                    });
                    scene.set(Some(opened.scene));
                    save_state.set(SaveState::Saved);
                }
                Err(e) => report(e),
            }
        });
    };

    // After any outline change: refresh the open scene's title and status, and move on if
    // the open scene is gone.
    let apply = move |view: OutlineView| {
        let open = current();
        match open.as_deref().and_then(|s| find(&view, s)) {
            Some(info) => scene.set(Some(info)),
            None if open.is_some() => {
                let next = reading_order(&view).first().map(|s| s.slug.clone());
                scene.set(None);
                match next {
                    Some(next) => open_scene(next),
                    None => close_scene(),
                }
            }
            None => {}
        }
        outline.set(Some(view));
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

    // --- Scene bar actions -----------------------------------------------------------------

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

    let split = move |_| {
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

    let merge = move |_| {
        with_scene(&move |p, s| {
            spawn_local(async move {
                match tauri::merge_scene(&p, &s).await {
                    Ok(view) => {
                        apply(view);
                        scene.set(None);
                        open_scene(s);
                    }
                    Err(e) => report(e),
                }
            });
        });
    };

    let cut = move |_| {
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

    let toggle = move |which: Panel| panel.update(|p| *p = if *p == Some(which) { None } else { Some(which) });
    let target = Signal::derive(move || project.get().zip(scene.get().map(|s| s.slug)));
    let current_slug = Signal::derive(move || scene.get().map(|s| s.slug));

    view! {
        <div class="workspace-grid">
            <aside class="sidebar">
                <div class="sidebar-head">
                    <select
                        class="project-picker"
                        prop:value=move || project.get().unwrap_or_default()
                        on:change=move |ev| choose_project(event_target_value(&ev))
                    >
                        <For each=move || projects.get() key=|p| p.slug.clone() let(p)>
                            <option value=p.slug.clone()>{p.title.clone()}</option>
                        </For>
                    </select>
                    <button title="New project" on:click=move |_| new_project.set(Some(String::new()))>"+"</button>
                </div>
                <Show when=move || new_project.get().is_some()>
                    <NewProjectForm on_create=create_project on_cancel=move || new_project.set(None) />
                </Show>
                <OutlineTree
                    project=project
                    outline=outline
                    current=current_slug
                    on_open=open_scene
                    on_outline=apply
                    on_error=report
                />
                <div class="sidebar-foot">
                    <button class="quiet" title=vault.path.clone() on:click=open_other_vault>"Open another vault…"</button>
                </div>
            </aside>
            <section class="main">
                <header class="topbar">
                    <div class="scene-bar">
                        {move || scene.get().map(|s| {
                            let statuses = outline.with(|o| o.as_ref().map(|o| o.statuses.clone()).unwrap_or_default());
                            view! {
                                <input
                                    class="scene-name"
                                    prop:value=s.title.clone()
                                    on:change=move |ev| rename(event_target_value(&ev))
                                />
                                <select prop:value=s.status.clone() on:change=move |ev| set_status(event_target_value(&ev))>
                                    {statuses.into_iter().map(|st| view! { <option value=st.clone()>{st.clone()}</option> }).collect_view()}
                                </select>
                                <button title="Split this scene at the cursor" on:click=split>"Split"</button>
                                <button title="Add the next scene in this chapter to the end of this one" on:click=merge>"Merge next"</button>
                                <button title="Move this scene to the cut bin" on:click=cut>"Cut"</button>
                            }
                        })}
                    </div>
                    <div class="status">
                        {move || spell_error.get().map(|e| view! { <span class="error">{format!("Spellcheck unavailable: {e}")}</span> })}
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
                {move || error.get().map(|e| view! {
                    <div class="banner error">
                        <span>{e}</span>
                        <button class="quiet" on:click=move |_| error.set(None)>"Dismiss"</button>
                    </div>
                })}
                <div class="workspace">
                    <div class="page" class:empty=move || scene.with(Option::is_none)>
                        <Editor handle=editor typography=typography on_change=on_change on_ready=on_ready />
                        <Show when=move || scene.with(Option::is_none)>
                            <p class="empty-note muted">"No scene open. Pick one in the outline, or add one with “+ New scene”."</p>
                        </Show>
                    </div>
                    {move || match panel.get() {
                        None => None,
                        Some(Panel::Markdown) => Some(view! {
                            <aside class="side-panel markdown-panel">
                                <pre>{move || markdown.get()}</pre>
                            </aside>
                        }.into_any()),
                        Some(Panel::History) => Some(view! {
                            <HistoryPanel target=target revision=revision editor=editor on_restore=on_restore />
                        }.into_any()),
                    }}
                </div>
            </section>
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
                <button type="submit">"Create"</button>
                <button type="button" class="quiet" on:click=move |_| on_cancel()>"Cancel"</button>
            </div>
        </form>
    }
}

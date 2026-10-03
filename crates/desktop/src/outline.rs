//! The outline tree: parts, chapters and scenes, with drag and drop to restructure and
//! double-click to rename.

use leptos::{ev, html, prelude::*, task::spawn_local};

use crate::tauri::{self, ChapterView, LocalFuture, OutlineView, SceneView};

/// An outline command, given the project's name.
type Command = Box<dyn FnOnce(String) -> LocalFuture<Result<OutlineView, String>>>;

#[derive(Clone, Debug, PartialEq)]
enum Drag {
    Scene(String),
    Chapter(String),
}

/// What's being renamed. Parts are named by their first chapter's id.
#[derive(Clone, Debug, PartialEq)]
enum Editing {
    Scene(String),
    Chapter(String),
    Part(String),
}

struct Group {
    part: Option<String>,
    /// Chapters with their index in the outline.
    chapters: Vec<(usize, ChapterView)>,
}

fn groups(chapters: Vec<ChapterView>) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for (index, chapter) in chapters.into_iter().enumerate() {
        match groups.last_mut() {
            Some(group) if group.part.is_some() && group.part == chapter.part => group.chapters.push((index, chapter)),
            _ => groups.push(Group {
                part: chapter.part.clone(),
                chapters: vec![(index, chapter)],
            }),
        }
    }
    groups
}

fn words(scenes: &[SceneView]) -> usize {
    scenes.iter().map(|s| s.words).sum()
}

fn format_words(n: usize) -> String {
    match n {
        1 => "1 word".to_owned(),
        n if n < 1000 => format!("{n} words"),
        n => format!("{},{:03} words", n / 1000, n % 1000),
    }
}

#[component]
pub fn OutlineTree(
    #[prop(into)] project: Signal<Option<String>>,
    #[prop(into)] outline: Signal<Option<OutlineView>>,
    #[prop(into)] current: Signal<Option<String>>,
    on_open: impl Fn(String) + Copy + Send + Sync + 'static,
    /// Receives the outline after a change made here.
    on_outline: impl Fn(OutlineView) + Copy + Send + Sync + 'static,
    on_error: impl Fn(String) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let dragging = RwSignal::new(None::<Drag>);
    let drop_target = RwSignal::new(None::<String>);
    let editing = RwSignal::new(None::<Editing>);

    // Runs an outline command for the current project and hands the result on.
    let run = move |command: Command| {
        let Some(project) = project.get_untracked() else { return };
        spawn_local(async move {
            match command(project).await {
                Ok(view) => on_outline(view),
                Err(e) => on_error(e),
            }
        });
    };

    let drop_scene = move |chapter: String, index: usize| {
        if let Some(Drag::Scene(scene)) = dragging.get_untracked() {
            run(Box::new(move |p| Box::pin(async move { tauri::move_scene(&p, &scene, &chapter, index).await })));
        }
    };
    let drop_chapter = move |index: usize| {
        if let Some(Drag::Chapter(chapter)) = dragging.get_untracked() {
            run(Box::new(move |p| Box::pin(async move { tauri::move_chapter(&p, &chapter, index).await })));
        }
    };
    let end_drag = move || {
        dragging.set(None);
        drop_target.set(None);
    };
    let accepts = move |want_scene: bool| match dragging.get_untracked() {
        Some(Drag::Scene(_)) => want_scene,
        Some(Drag::Chapter(_)) => !want_scene,
        None => false,
    };

    let new_scene = move |chapter: String| {
        let Some(p) = project.get_untracked() else { return };
        spawn_local(async move {
            match tauri::create_scene(&p, "Untitled scene", Some(&chapter), None).await {
                Ok(created) => {
                    on_outline(created.outline);
                    on_open(created.scene.clone());
                    editing.set(Some(Editing::Scene(created.scene)));
                }
                Err(e) => on_error(e),
            }
        });
    };

    let scene_row = move |chapter: Option<(String, usize)>, scene: SceneView, statuses: Vec<String>| {
        let slug = scene.slug.clone();
        let key = format!("scene:{slug}");
        let status_class = match statuses.iter().position(|s| *s == scene.status) {
            Some(i) => format!("status status-{}", i.min(3)),
            None => "status".to_owned(),
        };
        let is_current = {
            let slug = slug.clone();
            move || current.get().as_deref() == Some(slug.as_str())
        };
        let is_editing = {
            let slug = slug.clone();
            move || editing.get() == Some(Editing::Scene(slug.clone()))
        };
        let title = scene.title.clone();
        // `<Show>` children are a closure that takes what it uses, so it gets its own copies.
        let (edit_slug, edit_title) = (slug.clone(), title.clone());
        view! {
            <li
                class="scene-row"
                class:current=is_current
                class:drop-before={
                    let key = key.clone();
                    move || drop_target.get().as_deref() == Some(key.as_str())
                }
                draggable="true"
                on:dragstart={
                    let slug = slug.clone();
                    move |ev: ev::DragEvent| {
                        if let Some(dt) = ev.data_transfer() {
                            let _ = dt.set_data("text/plain", &slug);
                        }
                        dragging.set(Some(Drag::Scene(slug.clone())));
                    }
                }
                on:dragend=move |_| end_drag()
                on:dragover={
                    let key = key.clone();
                    let droppable = chapter.is_some();
                    move |ev: ev::DragEvent| {
                        if droppable && accepts(true) {
                            ev.prevent_default();
                            drop_target.set(Some(key.clone()));
                        }
                    }
                }
                on:drop={
                    let chapter = chapter.clone();
                    move |ev: ev::DragEvent| {
                        ev.prevent_default();
                        if let Some((chapter, index)) = chapter.clone() {
                            drop_scene(chapter, index);
                        }
                        end_drag();
                    }
                }
            >
                <Show
                    when=is_editing
                    fallback={
                        let slug = slug.clone();
                        let title = title.clone();
                        move || {
                            let open = slug.clone();
                            let rename = slug.clone();
                            view! {
                                <button
                                    class="scene-title"
                                    on:click=move |_| on_open(open.clone())
                                    on:dblclick=move |_| editing.set(Some(Editing::Scene(rename.clone())))
                                >
                                    <span class=status_class.clone() title="Status"></span>
                                    {title.clone()}
                                </button>
                            }
                        }
                    }
                >
                    <InlineEdit
                        initial=edit_title.clone()
                        on_done={
                            let slug = edit_slug.clone();
                            move |value: Option<String>| {
                                if editing.get_untracked() != Some(Editing::Scene(slug.clone())) {
                                    return;
                                }
                                editing.set(None);
                                if let Some(title) = value {
                                    let scene = slug.clone();
                                    run(Box::new(move |p| Box::pin(async move { tauri::rename_scene(&p, &scene, &title).await })));
                                }
                            }
                        }
                    />
                </Show>
                <span class="count">{scene.words}</span>
            </li>
        }
    };

    let chapter_block = move |index: usize, chapter: ChapterView, statuses: Vec<String>, total: usize| {
        let id = chapter.id.clone();
        let header_key = format!("chapter:{id}");
        let end_key = format!("end:{id}");
        let count = chapter.scenes.len();
        let title = if chapter.title.is_empty() { format!("Chapter {}", index + 1) } else { chapter.title.clone() };
        let is_editing = {
            let id = id.clone();
            move || editing.get() == Some(Editing::Chapter(id.clone()))
        };
        let (edit_id, edit_title, remove_id) = (id.clone(), chapter.title.clone(), id.clone());
        let scenes = chapter
            .scenes
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, scene)| scene_row(Some((id.clone(), i)), scene, statuses.clone()))
            .collect_view();
        view! {
            <li class="chapter">
                <div
                    class="chapter-head"
                    class:drop-before={
                        let key = header_key.clone();
                        move || drop_target.get().as_deref() == Some(key.as_str())
                    }
                    draggable="true"
                    on:dragstart={
                        let id = id.clone();
                        move |ev: ev::DragEvent| {
                            if let Some(dt) = ev.data_transfer() {
                                let _ = dt.set_data("text/plain", &id);
                            }
                            dragging.set(Some(Drag::Chapter(id.clone())));
                        }
                    }
                    on:dragend=move |_| end_drag()
                    on:dragover={
                        let key = header_key.clone();
                        move |ev: ev::DragEvent| {
                            if accepts(true) || accepts(false) {
                                ev.prevent_default();
                                drop_target.set(Some(key.clone()));
                            }
                        }
                    }
                    on:drop={
                        let id = id.clone();
                        move |ev: ev::DragEvent| {
                            ev.prevent_default();
                            // A scene dropped on a chapter goes first in it; a chapter goes before it.
                            drop_scene(id.clone(), 0);
                            drop_chapter(index);
                            end_drag();
                        }
                    }
                >
                    <Show
                        when=is_editing
                        fallback={
                            let id = id.clone();
                            let title = title.clone();
                            move || {
                                let rename = id.clone();
                                view! {
                                    <span class="chapter-title" on:dblclick=move |_| editing.set(Some(Editing::Chapter(rename.clone())))>
                                        {title.clone()}
                                    </span>
                                }
                            }
                        }
                    >
                        <InlineEdit
                            initial=edit_title.clone()
                            on_done={
                                let id = edit_id.clone();
                                move |value: Option<String>| {
                                    if editing.get_untracked() != Some(Editing::Chapter(id.clone())) {
                                        return;
                                    }
                                    editing.set(None);
                                    if let Some(title) = value {
                                        let chapter = id.clone();
                                        run(Box::new(move |p| Box::pin(async move { tauri::rename_chapter(&p, &chapter, &title).await })));
                                    }
                                }
                            }
                        />
                    </Show>
                    <span class="count">{total}</span>
                    <span class="row-actions">
                        <button title="New scene at the end of this chapter" on:click={
                            let id = id.clone();
                            move |_| new_scene(id.clone())
                        }>"+"</button>
                        <button title="Put this chapter in a part" on:click={
                            let id = id.clone();
                            move |_| editing.set(Some(Editing::Part(id.clone())))
                        }>"Part"</button>
                        <Show when=move || count == 0>
                            <button title="Remove this empty chapter" on:click={
                                let id = remove_id.clone();
                                move |_| {
                                    let chapter = id.clone();
                                    run(Box::new(move |p| Box::pin(async move { tauri::remove_chapter(&p, &chapter).await })));
                                }
                            }>"×"</button>
                        </Show>
                    </span>
                </div>
                <ul class="scenes">
                    {scenes}
                    <li
                        class="scene-end"
                        class:drop-before={
                            let key = end_key.clone();
                            move || drop_target.get().as_deref() == Some(key.as_str())
                        }
                        on:dragover={
                            let key = end_key.clone();
                            move |ev: ev::DragEvent| {
                                if accepts(true) {
                                    ev.prevent_default();
                                    drop_target.set(Some(key.clone()));
                                }
                            }
                        }
                        on:drop={
                            let id = id.clone();
                            move |ev: ev::DragEvent| {
                                ev.prevent_default();
                                drop_scene(id.clone(), count);
                                end_drag();
                            }
                        }
                    >
                        <button class="add" on:click={
                            let id = id.clone();
                            move |_| new_scene(id.clone())
                        }>"+ New scene"</button>
                    </li>
                </ul>
            </li>
        }
    };

    let set_part = move |chapters: Vec<String>, part: String| {
        let Some(project) = project.get_untracked() else { return };
        spawn_local(async move {
            let mut last = None;
            for chapter in chapters {
                match tauri::set_chapter_part(&project, &chapter, &part).await {
                    Ok(view) => last = Some(view),
                    Err(e) => return on_error(e),
                }
            }
            if let Some(view) = last {
                on_outline(view);
            }
        });
    };

    let tree = move || {
        let view = outline.get()?;
        let statuses = view.statuses.clone();
        let chapter_count = view.chapters.len();
        let last_chapter = view.chapters.last().map(|c| c.id.clone());
        let total: usize = view.chapters.iter().map(|c| words(&c.scenes)).sum();
        let unplaced = view.unplaced.clone();

        let blocks = groups(view.chapters)
            .into_iter()
            .map(|group| {
                let ids: Vec<String> = group.chapters.iter().map(|(_, c)| c.id.clone()).collect();
                let first = ids[0].clone();
                let group_words: usize = group.chapters.iter().map(|(_, c)| words(&c.scenes)).sum();
                let chapters = group
                    .chapters
                    .into_iter()
                    .map(|(index, chapter)| {
                        let total = words(&chapter.scenes);
                        chapter_block(index, chapter, statuses.clone(), total)
                    })
                    .collect_view();
                let editing_part = {
                    let first = first.clone();
                    move || editing.get() == Some(Editing::Part(first.clone()))
                };
                let part_title = group.part.clone();
                let header = view! {
                    <Show when=editing_part fallback={
                        let part_title = part_title.clone();
                        let first = first.clone();
                        move || part_title.clone().map(|title| {
                            let first = first.clone();
                            view! {
                                <div class="part-head" on:dblclick=move |_| editing.set(Some(Editing::Part(first.clone())))>
                                    <span>{title}</span>
                                    <span class="count">{format_words(group_words)}</span>
                                </div>
                            }
                        })
                    }>
                        <div class="part-head">
                            <InlineEdit
                                initial=part_title.clone().unwrap_or_default()
                                placeholder="Part name (empty for none)"
                                on_done={
                                    let first = first.clone();
                                    let ids = ids.clone();
                                    move |value: Option<String>| {
                                        if editing.get_untracked() != Some(Editing::Part(first.clone())) {
                                            return;
                                        }
                                        editing.set(None);
                                        if let Some(part) = value {
                                            // Renaming a part renames it on all its chapters.
                                            set_part(ids.clone(), part);
                                        }
                                    }
                                }
                            />
                        </div>
                    </Show>
                };
                view! {
                    <li class="part" class:in-part=group.part.is_some()>
                        {header}
                        <ul class="chapters">{chapters}</ul>
                    </li>
                }
            })
            .collect_view();

        let unplaced_rows = unplaced
            .into_iter()
            .map(|scene| scene_row(None, scene, statuses.clone()))
            .collect_view();
        let has_unplaced = !view.unplaced.is_empty();

        Some(view! {
            <div class="outline-total">{format_words(total)}</div>
            <ul class="outline">{blocks}</ul>
            <div
                class="add-chapter"
                class:drop-before=move || drop_target.get().as_deref() == Some("chapters-end")
                on:dragover=move |ev: ev::DragEvent| {
                    if accepts(false) {
                        ev.prevent_default();
                        drop_target.set(Some("chapters-end".into()));
                    }
                }
                on:drop=move |ev: ev::DragEvent| {
                    ev.prevent_default();
                    drop_chapter(chapter_count);
                    end_drag();
                }
            >
                <button class="add" on:click=move |_| {
                    let after = last_chapter.clone();
                    run(Box::new(move |p| Box::pin(async move { tauri::add_chapter(&p, "", after.as_deref()).await })));
                }>"+ Add chapter"</button>
            </div>
            <Show when=move || has_unplaced>
                <h3 class="unplaced-head">"Not in the outline"</h3>
            </Show>
            <ul class="scenes unplaced">{unplaced_rows}</ul>
        })
    };

    view! { <nav class="outline-tree">{tree}</nav> }
}

/// A text field that commits on Enter or when it loses focus (`Some(text)`), and cancels on
/// Escape (`None`).
#[component]
fn InlineEdit(
    initial: String,
    #[prop(optional, into)] placeholder: String,
    on_done: impl Fn(Option<String>) + Clone + Send + Sync + 'static,
) -> impl IntoView {
    let input = NodeRef::<html::Input>::new();
    Effect::new(move |_| {
        if let Some(el) = input.get() {
            let _ = el.focus();
            el.select();
        }
    });
    let value = RwSignal::new(initial);
    let on_key = {
        let on_done = on_done.clone();
        move |ev: ev::KeyboardEvent| match ev.key().as_str() {
            "Enter" => on_done(Some(value.get_untracked())),
            "Escape" => on_done(None),
            _ => {}
        }
    };
    view! {
        <input
            class="inline-edit"
            node_ref=input
            placeholder=placeholder
            prop:value=move || value.get()
            on:input=move |ev| value.set(event_target_value(&ev))
            on:keydown=on_key
            on:blur=move |_| on_done(Some(value.get_untracked()))
            // Keep clicks and drags inside the field from reaching the row.
            on:click=|ev| ev.stop_propagation()
            draggable="false"
        />
    }
}

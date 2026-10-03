//! History panel: a scene's snapshots, what changed since each one, and restoring or naming them.

use leptos::{prelude::*, task::spawn_local};
use needle_core::diff::{self, Kind, Segment};
use wasm_bindgen::JsValue;

use crate::editor::EditorHandle;
use crate::icons::{Glyph, Icon};
use crate::outline::format_words;
use crate::tauri::{self, VersionInfo};

#[component]
pub fn HistoryPanel(
    /// The open scene, as (project, scene).
    #[prop(into)] target: Signal<Option<(String, String)>>,
    /// Bumped whenever a snapshot is taken or named.
    #[prop(into)] revision: Signal<u32>,
    editor: StoredValue<Option<EditorHandle>, LocalStorage>,
    /// Receives the restored text.
    on_restore: impl Fn(String) + Copy + Send + Sync + 'static,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let versions = RwSignal::new(Vec::<VersionInfo>::new());
    let selected = RwSignal::new(None::<VersionInfo>);
    let changes = RwSignal::new(None::<Vec<Segment>>);
    let naming = RwSignal::new(false);
    let name_input = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);

    Effect::new(move |_| {
        revision.track();
        let Some((project, scene)) = target.get() else { return };
        spawn_local(async move {
            match tauri::scene_history(&project, &scene).await {
                Ok(list) => versions.set(list),
                Err(e) => error.set(Some(e)),
            }
        });
    });

    Effect::new(move |_| {
        target.track();
        selected.set(None);
    });

    let flush_editor = move || {
        editor.with_value(|h| {
            if let Some(h) = h {
                h.flush();
            }
        })
    };

    let select = move |version: VersionInfo| {
        let Some((project, scene)) = target.get_untracked() else { return };
        let current = editor.with_value(|h| h.as_ref().map(EditorHandle::markdown)).unwrap_or_default();
        let id = version.id.clone();
        changes.set(None);
        naming.set(false);
        name_input.set(version.name.clone().unwrap_or_default());
        selected.set(Some(version));
        spawn_local(async move {
            match tauri::scene_version(&project, &scene, &id).await {
                Ok(old) => changes.set(Some(diff::words(&old, &current))),
                Err(e) => error.set(Some(e)),
            }
        });
    };

    let restore = move |_| {
        let (Some((project, scene)), Some(version)) = (target.get_untracked(), selected.get_untracked()) else { return };
        flush_editor();
        let label = time_label(version.time);
        spawn_local(async move {
            match tauri::restore_version(&project, &scene, &version.id, &label).await {
                Ok(markdown) => {
                    selected.set(None);
                    on_restore(markdown);
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    let save_name = move || {
        let Some(version) = selected.get_untracked() else { return };
        let name = name_input.get_untracked();
        spawn_local(async move {
            match tauri::name_version(&version.id, &name).await {
                Ok(()) => {
                    naming.set(false);
                    selected.update(|v| {
                        if let Some(v) = v {
                            v.name = Some(name.trim().to_owned());
                        }
                    });
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    let snapshot_now = move |_| {
        flush_editor();
        spawn_local(async move {
            if let Err(e) = tauri::snapshot_now().await {
                error.set(Some(e));
            }
        });
    };

    let close_button = move || {
        view! {
            <button class="icon-button" title="Close" aria-label="Close history" on:click=move |_| on_close()>
                <Icon glyph=Glyph::Close />
            </button>
        }
    };

    // Each version is a stop on a thread: the newest filled in, named ones ringed.
    let row = move |version: VersionInfo| {
        let shown = version.clone();
        view! {
            <li class="version-item" class:named=shown.name.is_some()>
                <button class="version" on:click=move |_| select(version.clone())>
                    <span class="version-dot" aria-hidden="true"></span>
                    <span class="version-time">{time_label(shown.time)}</span>
                    <span class="version-words">{format_words(shown.words)}</span>
                    {shown.name.clone().map(|name| view! { <span class="version-name">{name}</span> })}
                    <span class="version-message">{shown.message.clone()}</span>
                </button>
            </li>
        }
    };

    let list = move || {
        view! {
            <div class="panel-head">
                <h2>"History"</h2>
                <button class="small" on:click=snapshot_now>"Snapshot now"</button>
                {close_button()}
            </div>
            <div class="panel-body">
                <Show when=move || versions.with(Vec::is_empty)>
                    <p class="muted">"No snapshots of this scene yet."</p>
                </Show>
                <ol class="versions">
                    <For each=move || versions.get() key=|v| (v.id.clone(), v.name.clone()) children=row />
                </ol>
            </div>
        }
    };

    let detail = move |version: VersionInfo| {
        view! {
            <div class="panel-head">
                <button class="quiet back" on:click=move |_| selected.set(None)>
                    <Icon glyph=Glyph::Back size=15 />
                    "All versions"
                </button>
                <span class="spacer"></span>
                {close_button()}
            </div>
            <div class="panel-body">
                <div class="version-heading">
                    <h3>{time_label(version.time)}</h3>
                    <span class="muted">{format_words(version.words)}</span>
                </div>
                {move || selected.get().and_then(|v| v.name).map(|name| view! { <span class="version-name">{name}</span> })}
                <p class="muted">{version.message.clone()}</p>
                <div class="actions">
                    <button class="primary" on:click=restore>
                        <Icon glyph=Glyph::Restore size=15 />
                        "Restore this version"
                    </button>
                    <button on:click=move |_| naming.update(|n| *n = !*n)>
                        <Icon glyph=Glyph::Bookmark size=15 />
                        "Name…"
                    </button>
                </div>
                <Show when=move || naming.get()>
                    <form class="name-form" on:submit=move |ev| {
                        ev.prevent_default();
                        save_name();
                    }>
                        <input
                            type="text"
                            placeholder="e.g. Sent to beta readers"
                            prop:value=move || name_input.get()
                            on:input=move |ev| name_input.set(event_target_value(&ev))
                        />
                        <button type="submit">"Save"</button>
                    </form>
                </Show>
                <p class="legend">
                    "Since this version: " <ins>"added"</ins> " " <del>"removed"</del>
                </p>
                <div class="diff">
                    {move || match changes.get() {
                        None => view! { <p class="muted">"Loading…"</p> }.into_any(),
                        Some(segments) => segments.into_iter().map(render_segment).collect_view().into_any(),
                    }}
                </div>
            </div>
        }
    };

    view! {
        <aside class="side-panel history-panel">
            {move || match selected.get() {
                None => list().into_any(),
                Some(version) => detail(version).into_any(),
            }}
            {move || error.get().map(|e| view! { <p class="error panel-error">{e}</p> })}
        </aside>
    }
}

fn render_segment(segment: Segment) -> AnyView {
    match segment.kind {
        Kind::Added => view! { <ins>{segment.text}</ins> }.into_any(),
        Kind::Removed => view! { <del>{segment.text}</del> }.into_any(),
        Kind::Same => view! { <span>{abbreviate(&segment.text)}</span> }.into_any(),
    }
}

/// Long unchanged stretches are cut down to their ends, so the changes are easy to find.
fn abbreviate(text: &str) -> String {
    const KEEP: usize = 200;
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= KEEP * 3 {
        return text.to_owned();
    }
    let head: String = chars[..KEEP].iter().collect();
    let tail: String = chars[chars.len() - KEEP..].iter().collect();
    format!("{head} … {tail}")
}

/// "Today 14:32", or "Oct 2 09:05" for earlier days, in local time.
fn time_label(seconds: i64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let date = js_sys::Date::new(&JsValue::from_f64(seconds as f64 * 1000.0));
    let time = format!("{:02}:{:02}", date.get_hours(), date.get_minutes());
    if date.to_date_string() == js_sys::Date::new_0().to_date_string() {
        format!("Today {time}")
    } else {
        format!("{} {} {time}", MONTHS[date.get_month() as usize % 12], date.get_date())
    }
}

//! History panel: a scene's or note's snapshots by day and writing session, what changed since
//! each one, and restoring or naming a version or putting back a passage it had.

use std::collections::HashMap;

use leptos::{prelude::*, task::spawn_local};
use needle_core::diff::{self, Kind, Segment};
use needle_core::history;
use wasm_bindgen::{JsCast, JsValue};

use crate::editor::EditorHandle;
use crate::icons::{Glyph, Icon};
use crate::outline::{format_count, format_words};
use crate::tauri::{self, NoteKey, VersionInfo};

/// What the History panel follows: the open scene or note.
#[derive(Clone, Debug, PartialEq)]
pub enum HistoryTarget {
    Scene { project: String, scene: String },
    Note(NoteKey),
}

impl HistoryTarget {
    async fn versions(&self) -> Result<Vec<VersionInfo>, String> {
        match self {
            Self::Scene { project, scene } => tauri::scene_history(project, scene).await,
            Self::Note(note) => tauri::note_history(note).await,
        }
    }

    async fn version(&self, id: &str) -> Result<String, String> {
        match self {
            Self::Scene { project, scene } => tauri::scene_version(project, scene, id).await,
            Self::Note(note) => tauri::note_version(note, id).await,
        }
    }

    async fn restore(&self, id: &str, label: &str) -> Result<String, String> {
        match self {
            Self::Scene { project, scene } => tauri::restore_version(project, scene, id, label).await,
            Self::Note(note) => tauri::restore_note_version(note, id, label).await,
        }
    }

    fn noun(&self) -> &'static str {
        match self {
            Self::Scene { .. } => "scene",
            Self::Note(_) => "note",
        }
    }
}

/// A snapshot in the list, with the words it added or took out.
#[derive(Clone)]
struct Row {
    version: VersionInfo,
    change: Option<i64>,
    /// The latest snapshot, where the thread is now.
    newest: bool,
}

/// The snapshots grouped by day ("Today", "Oct 4"), then by writing session.
fn layout(versions: &[VersionInfo]) -> Vec<(String, Vec<Vec<Row>>)> {
    let times: Vec<i64> = versions.iter().map(|v| v.time).collect();
    let words: Vec<usize> = versions.iter().map(|v| v.words).collect();
    let changes = history::changes(&words);
    history::group(&times, |t| day_key(&date(t)))
        .into_iter()
        .map(|day| {
            let label = day_label(&date(times[day.sessions[0].start]));
            let sessions = day
                .sessions
                .into_iter()
                .map(|session| {
                    session
                        .map(|i| Row {
                            version: versions[i].clone(),
                            change: changes[i],
                            newest: i == 0,
                        })
                        .collect()
                })
                .collect();
            (label, sessions)
        })
        .collect()
}

/// A removed passage picked in the changes, waiting for Put back.
#[derive(Clone, Debug, PartialEq)]
struct Picked {
    /// Where it is among the changes, and its text, to check it's still there.
    index: usize,
    text: String,
    /// Where its Put back bubble goes inside the changes box, in px: under its last line.
    top: f64,
    left: f64,
}

#[component]
pub fn HistoryPanel(
    /// The open scene or note.
    #[prop(into)] target: Signal<Option<HistoryTarget>>,
    /// The open scene's or note's text now, as Markdown.
    #[prop(into)] text: Signal<String>,
    /// Bumped whenever a snapshot is taken or named.
    #[prop(into)] revision: Signal<u32>,
    editor: StoredValue<Option<EditorHandle>, LocalStorage>,
    /// Receives the restored text.
    on_restore: impl Fn(String) + Copy + Send + Sync + 'static,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let versions = RwSignal::new(Vec::<VersionInfo>::new());
    // Sessions opened (true) or closed by hand, by their first snapshot's id. Otherwise the
    // latest session is open and the others closed.
    let toggled = RwSignal::new(HashMap::<String, bool>::new());
    let selected = RwSignal::new(None::<VersionInfo>);
    // The selected version's text.
    let old = RwSignal::new(None::<String>);
    let changes = Memo::new(move |_| old.with(|old| old.as_ref().map(|old| text.with(|now| diff::words(old, now)))));
    let picked = RwSignal::new(None::<Picked>);
    let naming = RwSignal::new(false);
    let name_input = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);

    Effect::new(move |_| {
        revision.track();
        let Some(doc) = target.get() else { return };
        spawn_local(async move {
            match doc.versions().await {
                Ok(list) => _ = versions.try_set(list),
                Err(e) => _ = error.try_set(Some(e)),
            }
        });
    });

    Effect::new(move |_| {
        target.track();
        selected.set(None);
        old.set(None);
        toggled.set(HashMap::new());
    });

    // A pick is a place in the changes, so it goes when they change (or another version opens).
    Effect::new(move |_| {
        changes.track();
        picked.set(None);
    });

    let flush_editor = move || {
        editor.with_value(|h| {
            if let Some(h) = h {
                h.flush();
            }
        })
    };

    let select = move |version: VersionInfo| {
        let Some(doc) = target.get_untracked() else { return };
        // So `text` is what's on the page.
        flush_editor();
        let id = version.id.clone();
        old.set(None);
        naming.set(false);
        name_input.set(version.name.clone().unwrap_or_default());
        selected.set(Some(version));
        spawn_local(async move {
            match doc.version(&id).await {
                Ok(text) if selected.with_untracked(|s| s.as_ref().is_some_and(|s| s.id == id)) => _ = old.try_set(Some(text)),
                Ok(_) => {}
                Err(e) => _ = error.try_set(Some(e)),
            }
        });
    };

    let back_to_list = move || {
        selected.set(None);
        old.set(None);
    };

    let restore = move |_| {
        let (Some(doc), Some(version)) = (target.get_untracked(), selected.get_untracked()) else { return };
        flush_editor();
        let label = time_label(version.time);
        spawn_local(async move {
            match doc.restore(&version.id, &label).await {
                Ok(markdown) => {
                    back_to_list();
                    on_restore(markdown);
                }
                Err(e) => _ = error.try_set(Some(e)),
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

    // A removed passage was clicked: show Put back under its last line.
    let pick = move |index: usize, text: String, el: web_sys::Element| {
        let Some(changes_box) = el.closest(".diff").ok().flatten() else { return };
        let lines = el.get_client_rects();
        let Some(last) = lines.get(lines.length().saturating_sub(1)) else { return };
        let outer = changes_box.get_bounding_client_rect();
        picked.set(Some(Picked {
            index,
            text,
            top: last.bottom() - outer.top() - f64::from(changes_box.client_top()) + 6.0,
            left: last.left() - outer.left() - f64::from(changes_box.client_left()),
        }));
    };

    let put_back = move |_| {
        let Some(p) = picked.get_untracked() else { return };
        flush_editor();
        // Typing since the pick has changed the changes, and the pick with them.
        let Some(segments) = changes.get_untracked() else { return };
        if segments.get(p.index).is_none_or(|s| s.kind != Kind::Removed || s.text != p.text) {
            return;
        }
        let Some(markdown) = diff::put_back(&segments, p.index) else { return };
        editor.with_value(|h| {
            if let Some(h) = h {
                h.apply_markdown(&markdown);
                // Saves it, and shows it as unchanged here.
                h.flush();
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

    // Each snapshot is a stop on a thread: the latest filled in, named ones ringed.
    let row = move |row: Row| {
        let Row { version, change, newest } = row;
        let shown = version.clone();
        view! {
            <li class="version-item" class:named=shown.name.is_some() class:newest=newest>
                <button class="version" title=shown.message.clone() on:click=move |_| select(version.clone())>
                    <span class="version-dot" aria-hidden="true"></span>
                    <span class="version-time">{clock(shown.time)}</span>
                    <span class="version-counts">
                        {change.filter(|&n| n != 0).map(|n| view! { <span>{signed(n)}</span> })}
                        <span>{format_words(shown.words)}</span>
                    </span>
                    {shown.name.clone().map(|name| view! { <span class="version-name">{name}</span> })}
                </button>
            </li>
        }
    };

    // A session folds into one row; its named versions show even while it's closed.
    let session = move |rows: Vec<Row>| {
        if rows.len() == 1 {
            return rows.into_iter().map(row).collect_view().into_any();
        }
        let key = rows[rows.len() - 1].version.id.clone();
        let current = rows[0].newest;
        let open = {
            let key = key.clone();
            Memo::new(move |_| toggled.with(|t| t.get(&key).copied().unwrap_or(current)))
        };
        let toggle = move |_| {
            let now_open = !open.get_untracked();
            toggled.update(|t| _ = t.insert(key.clone(), now_open));
        };
        let span = format!("{} – {}", clock(rows[rows.len() - 1].version.time), clock(rows[0].version.time));
        let total = history::total(&rows.iter().map(|r| r.change).collect::<Vec<_>>()).filter(|&n| n != 0);
        let summary = match total {
            Some(n) => format!("{} snapshots · {} words", rows.len(), signed(n)),
            None => format!("{} snapshots", rows.len()),
        };
        view! {
            <li class="version-item session" class:current=current>
                <button class="version" aria-expanded=move || open.get().to_string() on:click=toggle>
                    <span class="session-mark" aria-hidden="true"></span>
                    <span class="version-time">{span}</span>
                    <span class="session-chevron">
                        {move || {
                            let glyph = if open.get() { Glyph::ChevronDown } else { Glyph::ChevronRight };
                            view! { <Icon glyph=glyph size=14 /> }
                        }}
                    </span>
                    <span class="version-summary">{summary}</span>
                </button>
            </li>
            <li class="version-item nested">
                <ol class="versions">
                    {move || {
                        let open = open.get();
                        rows.iter().filter(|r| open || r.version.name.is_some()).cloned().map(row).collect_view()
                    }}
                </ol>
            </li>
        }
        .into_any()
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
                    <p class="muted">
                        {move || format!("No snapshots of this {} yet.", target.with(|t| t.as_ref().map_or("scene", HistoryTarget::noun)))}
                    </p>
                </Show>
                {move || {
                    versions
                        .with(|v| layout(v))
                        .into_iter()
                        .map(|(day, sessions)| {
                            view! {
                                <h3 class="history-day">{day}</h3>
                                <ol class="versions">{sessions.into_iter().map(session).collect_view()}</ol>
                            }
                        })
                        .collect_view()
                }}
            </div>
        }
    };

    // A change, as it shows in the box. Removed passages that can go back are buttons.
    let segment = move |index: usize, segment: Segment, can_put_back: bool| match segment.kind {
        Kind::Added => view! { <ins>{segment.text}</ins> }.into_any(),
        Kind::Same => view! { <span>{abbreviate(&segment.text)}</span> }.into_any(),
        Kind::Removed if !can_put_back => view! { <del>{segment.text}</del> }.into_any(),
        Kind::Removed => {
            let text = segment.text.clone();
            let text_too = segment.text.clone();
            let is_picked = move || picked.with(|p| p.as_ref().is_some_and(|p| p.index == index));
            view! {
                <del
                    class="can-put-back"
                    class:picked=is_picked
                    role="button"
                    tabindex="0"
                    on:click=move |ev| {
                        ev.stop_propagation();
                        if let Some(el) = ev.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
                            pick(index, text.clone(), el);
                        }
                    }
                    on:keydown=move |ev| {
                        if ev.key() == "Enter" || ev.key() == " " {
                            ev.prevent_default();
                            if let Some(el) = ev.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
                                pick(index, text_too.clone(), el);
                            }
                        }
                    }
                >
                    {segment.text}
                </del>
            }
            .into_any()
        }
    };

    let bubble = move |p: Picked| {
        view! {
            <div class="put-back" style=format!("top: {}px", p.top)>
                <span class="put-back-indent" style=format!("width: {}px", p.left.max(0.0))></span>
                <div class="put-back-bubble" on:click=|ev| ev.stop_propagation()>
                    <span class="muted">"Back where it was"</span>
                    <button class="small" on:click=put_back>
                        <Icon glyph=Glyph::Restore size=14 />
                        "Put back"
                    </button>
                </div>
            </div>
        }
    };

    let detail = move |version: VersionInfo| {
        let can_put_any = move || changes.with(|c| c.as_ref().is_some_and(|c| (0..c.len()).any(|i| diff::can_put_back(c, i))));
        view! {
            <div class="panel-head">
                <button class="quiet back" on:click=move |_| back_to_list()>
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
                    {move || if can_put_any() { ". Click a removed passage to put it back." } else { "." }}
                </p>
                <div class="diff" on:click=move |_| picked.set(None)>
                    {move || match changes.get() {
                        None => view! { <p class="muted">"Loading…"</p> }.into_any(),
                        Some(segments) => {
                            let can: Vec<bool> = (0..segments.len()).map(|i| diff::can_put_back(&segments, i)).collect();
                            segments
                                .into_iter()
                                .enumerate()
                                .map(|(i, s)| segment(i, s, can[i]))
                                .collect_view()
                                .into_any()
                        }
                    }}
                    {move || picked.get().map(bubble)}
                </div>
            </div>
        }
    };

    view! {
        <aside
            class="side-panel history-panel"
            on:keydown=move |ev| {
                if ev.key() == "Escape" && picked.with_untracked(Option::is_some) {
                    picked.set(None);
                }
            }
        >
            {move || match selected.get() {
                None => list().into_any(),
                Some(version) => detail(version).into_any(),
            }}
            {move || error.get().map(|e| view! { <p class="error panel-error">{e}</p> })}
        </aside>
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

/// "+31" or "−17", with a true minus sign.
fn signed(n: i64) -> String {
    let size = format_count(n.unsigned_abs() as usize);
    if n < 0 { format!("−{size}") } else { format!("+{size}") }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

fn date(seconds: i64) -> js_sys::Date {
    js_sys::Date::new(&JsValue::from_f64(seconds as f64 * 1000.0))
}

/// Tells local days apart.
fn day_key(date: &js_sys::Date) -> i64 {
    i64::from(date.get_full_year()) * 10_000 + i64::from(date.get_month()) * 100 + i64::from(date.get_date())
}

/// "Today", "Yesterday", "Oct 2", or "Oct 2, 2025" in another year, in local time.
pub(crate) fn day_label(date: &js_sys::Date) -> String {
    let today = js_sys::Date::new_0();
    let yesterday = js_sys::Date::new_0();
    yesterday.set_date(today.get_date() - 1);
    if day_key(date) == day_key(&today) {
        "Today".to_owned()
    } else if day_key(date) == day_key(&yesterday) {
        "Yesterday".to_owned()
    } else if date.get_full_year() == today.get_full_year() {
        format!("{} {}", MONTHS[date.get_month() as usize % 12], date.get_date())
    } else {
        format!("{} {}, {}", MONTHS[date.get_month() as usize % 12], date.get_date(), date.get_full_year())
    }
}

/// "14:32", in local time.
pub(crate) fn clock_of(date: &js_sys::Date) -> String {
    format!("{:02}:{:02}", date.get_hours(), date.get_minutes())
}

fn clock(seconds: i64) -> String {
    clock_of(&date(seconds))
}

/// "Today 14:32", or "Oct 2 09:05" for earlier days, in local time.
fn time_label(seconds: i64) -> String {
    let date = date(seconds);
    if day_key(&date) == day_key(&js_sys::Date::new_0()) {
        format!("Today {}", clock_of(&date))
    } else {
        format!("{} {} {}", MONTHS[date.get_month() as usize % 12], date.get_date(), clock_of(&date))
    }
}

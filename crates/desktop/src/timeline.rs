//! The timeline (DESIGN §6): a project's scenes and plot points in story time.
//!
//! Story order runs down the page, one step per item, with a tape measure in the margin giving
//! each one's time and how long since the one before. Lanes are columns, by POV, thread or
//! place. Each scene is a small pattern piece carrying its piece number, its place in reading
//! order, so a flashback's number stands out; a plot point is a scrap of note cloth. Anything
//! placed by order alone has a dashed edge. What isn't placed waits in the tray at the foot.
//!
//! Click an item to set its time in the panel; double-click to open it. The panel takes a column
//! of its own, so in a narrow window the chosen piece is scrolled back into view beside it.

use leptos::prelude::*;
use leptos::task::spawn_local;

use needle_core::project::{NoteKind, ProjectKind};

use crate::icons::{Glyph, Icon};
use crate::tauri::{self, NoteKey, TimelineItemView, TimelineView, WhenInput};

/// The tape measure's width, a lane's width, the lane names' row, and one step of story order.
const TAPE: f64 = 92.0;
const LANE: f64 = 204.0;
const HEAD: f64 = 40.0;
const STEP: f64 = 100.0;
/// Within a step: the gap's words, then the piece.
const PIECE_TOP: f64 = 26.0;
const PIECE_HEIGHT: f64 = 66.0;
/// How many lane colours there are in the stylesheet (`lane-0`…).
const COLOURS: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaneBy {
    Pov,
    Thread,
    Place,
}

/// Which lane each item on the timeline is in.
struct Layout {
    lanes: Vec<String>,
    /// For each position in story order: (item, lane).
    rows: Vec<(usize, usize)>,
}

fn layout(t: &TimelineView, by: LaneBy, kind: ProjectKind) -> Layout {
    let none = match by {
        LaneBy::Pov => "No POV".to_owned(),
        LaneBy::Thread => format!("No {}", NoteKind::Thread.label(kind).to_lowercase()),
        LaneBy::Place => "No place".to_owned(),
    };
    let events = NoteKind::Event.plural(kind).to_owned();
    let lane_of = |item: &TimelineItemView| -> String {
        let first = |list: &[String]| list.first().cloned();
        let named = match by {
            LaneBy::Pov if !item.is_scene() => return events.clone(),
            LaneBy::Pov => item.pov.clone(),
            LaneBy::Thread => first(&item.threads),
            LaneBy::Place => first(&item.places),
        };
        named.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| none.clone())
    };
    // Lanes in the order they first come up in the story, with the catch-alls last.
    let mut lanes: Vec<String> = Vec::new();
    for &i in &t.order {
        let lane = lane_of(&t.items[i]);
        if !lanes.contains(&lane) {
            lanes.push(lane);
        }
    }
    lanes.sort_by_key(|l| (*l == none) as u8 * 2 + (*l == events) as u8);
    let rows = t.order.iter().map(|&i| (i, lanes.iter().position(|l| *l == lane_of(&t.items[i])).unwrap_or(0))).collect();
    Layout { lanes, rows }
}

/// Whether a scene's piece number is out of step with story order, the way a flashback is:
/// a smaller number comes after it in the story.
fn out_of_order(t: &TimelineView, position: usize) -> bool {
    let Some(here) = t.items[t.order[position]].reading else { return false };
    t.order[position + 1..].iter().filter_map(|&i| t.items[i].reading).any(|later| later < here)
}

#[component]
pub fn Timeline(
    #[prop(into)] project: Signal<String>,
    #[prop(into)] kind: Signal<ProjectKind>,
    on_open_scene: impl Fn(String) + Copy + Send + Sync + 'static,
    on_open_note: impl Fn(NoteKey) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let timeline = RwSignal::new(None::<TimelineView>);
    let error = RwSignal::new(None::<String>);
    let lanes_by = RwSignal::new(LaneBy::Pov);
    // The chosen item, by id, whose time the panel edits.
    let chosen = RwSignal::new(None::<String>);

    Effect::new(move |_| {
        let slug = project.get();
        chosen.set(None);
        spawn_local(async move {
            match tauri::project_timeline(&slug).await {
                Ok(found) => {
                    timeline.try_set(Some(found));
                    error.try_set(None);
                }
                Err(e) => {
                    error.try_set(Some(e));
                }
            }
        });
    });

    // The panel takes room from the timeline, so the chosen piece is scrolled back into view
    // beside it once it's open.
    Effect::new(move |_| {
        if chosen.with(Option::is_none) {
            return;
        }
        request_animation_frame(|| {
            let piece = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.query_selector(".timeline-scroll .time-piece.chosen").ok().flatten());
            if let Some(piece) = piece {
                let options = web_sys::ScrollIntoViewOptions::new();
                options.set_block(web_sys::ScrollLogicalPosition::Nearest);
                options.set_inline(web_sys::ScrollLogicalPosition::Nearest);
                piece.scroll_into_view_with_scroll_into_view_options(&options);
            }
        });
    });

    let open = move |item: &TimelineItemView| {
        if item.is_scene() {
            on_open_scene(item.path.clone());
        } else {
            on_open_note(item.note_key());
        }
    };

    let piece = move |item: TimelineItemView, flashback: bool, style: Option<String>| {
        let id = item.id.clone();
        let chosen_id = id.clone();
        let opened = item.clone();
        let mark = match &item.reading {
            Some(n) => n.to_string(),
            None if item.is_scene() => "·".to_owned(),
            None => "◆".to_owned(),
        };
        let below = match (&item.problem, &item.time, item.loose) {
            (Some(problem), _, _) => view! { <span class="piece-problem">{problem.clone()}</span> }.into_any(),
            (None, Some(time), _) => view! { <span class="piece-time">{time.clone()}</span> }.into_any(),
            (None, None, true) => view! { <span class="piece-time">{loose_words(&item)}</span> }.into_any(),
            (None, None, false) => view! { <span class="piece-time">"No time yet"</span> }.into_any(),
        };
        view! {
            <button
                class="time-piece"
                class:event=!item.is_scene()
                class:loose=item.loose
                class:troubled=item.problem.is_some()
                class:chosen=move || chosen.with(|c| c.as_deref() == Some(chosen_id.as_str()))
                style=style
                title=item.summary.clone()
                on:click=move |_| chosen.set(Some(id.clone()))
                on:dblclick=move |_| open(&opened)
            >
                <span class="piece-number" class:flashback=flashback>{mark}</span>
                <span class="piece-title">{item.title.clone()}</span>
                {below}
            </button>
        }
    };

    let board = move || {
        let t = timeline.get()?;
        let lay = layout(&t, lanes_by.get(), kind.get());
        let n = lay.rows.len();
        let width = TAPE + lay.lanes.len() as f64 * LANE + 24.0;
        let height = HEAD + n as f64 * STEP + 24.0;
        // Each lane's thread, from its first piece to its last.
        let threads = (0..lay.lanes.len())
            .filter_map(|lane| {
                let rows: Vec<usize> = lay.rows.iter().enumerate().filter(|(_, (_, l))| *l == lane).map(|(r, _)| r).collect();
                let (first, last) = (*rows.first()?, *rows.last()?);
                let x = TAPE + lane as f64 * LANE + LANE / 2.0 - 1.0;
                let top = HEAD + first as f64 * STEP + PIECE_TOP + PIECE_HEIGHT / 2.0;
                let length = (last - first) as f64 * STEP;
                let style = format!("left: {x}px; top: {top}px; height: {length}px");
                Some(view! { <div class=format!("lane-thread lane-{}", lane % COLOURS) style=style></div> })
            })
            .collect_view();
        let lane_names = lay
            .lanes
            .iter()
            .enumerate()
            .map(|(i, name)| {
                view! { <div class=format!("lane-name lane-{}", i % COLOURS)><i></i>{name.clone()}</div> }
            })
            .collect_view();
        let marks = lay
            .rows
            .iter()
            .enumerate()
            .map(|(row, &(i, lane))| {
                let item = t.items[i].clone();
                let top = HEAD + row as f64 * STEP;
                let style = format!("left: {}px; top: {}px", TAPE + lane as f64 * LANE + 12.0, top + PIECE_TOP);
                let flashback = out_of_order(&t, row);
                view! { <div class=format!("lane-{}", lane % COLOURS)>{piece(item, flashback, Some(style))}</div> }
            })
            .collect_view();
        // The tape stays at the left as the lanes scroll sideways, so its marks are on it: each
        // piece's time level with the piece, and the time between written in the gap.
        let ticks = lay
            .rows
            .iter()
            .enumerate()
            .map(|(row, &(i, _))| {
                let item = &t.items[i];
                let top = row as f64 * STEP;
                let tick = item.time.clone().map(|time| {
                    let style = format!("top: {}px", top + PIECE_TOP);
                    view! { <div class="tape-tick" style=style>{time}</div> }
                });
                let gap = item.gap.clone().map(|gap| {
                    let style = format!("top: {}px", top + 3.0);
                    view! { <div class="tape-gap" style=style>{gap}</div> }
                });
                view! { {tick} {gap} }
            })
            .collect_view();
        let empty = (n == 0).then(|| {
            view! {
                <p class="timeline-empty">
                    "Nothing has a time yet. Choose something in the tray to give it one."
                </p>
            }
        });
        let inner = format!("width: {width}px; height: {height}px");
        let tape = format!("height: {}px", height - HEAD);
        Some(view! {
            <div class="timeline-inner" style=inner>
                <div class="timeline-lanes">{lane_names}</div>
                <div class="tape-measure" style=tape>{ticks}</div>
                {threads}
                {marks}
                {empty}
            </div>
        })
    };

    let tray = move || {
        let t = timeline.get()?;
        let waiting: Vec<TimelineItemView> =
            t.items.iter().enumerate().filter(|(i, _)| !t.order.contains(i)).map(|(_, item)| item.clone()).collect();
        if waiting.is_empty() {
            return None;
        }
        let count = waiting.len();
        let pieces = waiting.into_iter().map(|item| piece(item, false, None)).collect_view();
        Some(view! {
            <div class="timeline-tray">
                <p class="panel-heading">{format!("Not placed yet · {count}")}</p>
                <div class="tray-pieces">{pieces}</div>
            </div>
        })
    };

    let panel = move || {
        let id = chosen.get()?;
        let t = timeline.get()?;
        let item = t.items.iter().find(|i| i.id == id)?.clone();
        let titles: Vec<String> = t.items.iter().filter(|i| i.id != id).map(|i| i.title.clone()).collect();
        Some(view! {
            <WhenPanel
                item=item
                titles=titles
                kind=kind.get_untracked()
                project=project.get_untracked()
                on_close=move || chosen.set(None)
                on_open=open
                on_changed=move |found| timeline.set(Some(found))
            />
        })
    };

    let lane_button = move |which: LaneBy, label: &'static str| {
        view! {
            <button
                class:active=move || lanes_by.get() == which
                aria-pressed=move || (lanes_by.get() == which).to_string()
                on:click=move |_| lanes_by.set(which)
            >
                {label}
            </button>
        }
    };
    let thread_word = move || NoteKind::Thread.label(kind.get());

    view! {
        <section class="timeline" aria-label="Timeline" on:keydown=move |ev| {
            if ev.key() == "Escape" {
                chosen.set(None);
            }
        }>
            <div class="timeline-main">
            <div class="timeline-bar">
                <span class="panel-heading">"Lanes"</span>
                <div class="segmented">
                    {lane_button(LaneBy::Pov, "POV")}
                    {move || lane_button(LaneBy::Thread, thread_word())}
                    {lane_button(LaneBy::Place, "Place")}
                </div>
                {move || timeline.with(|t| t.as_ref().and_then(|t| t.calendar.clone())).map(|name| view! {
                    <span class="timeline-calendar">{name}</span>
                })}
            </div>
            {move || error.get().map(|e| view! { <div class="banner error"><span>{e}</span></div> })}
            <div class="timeline-scroll">{board}</div>
            {tray}
            </div>
            {panel}
        </section>
    }
}

/// What a piece placed by order alone says instead of a time.
fn loose_words(item: &TimelineItemView) -> String {
    let w = &item.when;
    match w.kind.as_str() {
        "order" if !w.after.is_empty() && !w.before.is_empty() => format!("between {} and {}", w.after, w.before),
        "order" if !w.after.is_empty() => format!("after {}", w.after),
        "order" => format!("before {}", w.before),
        "from" => format!("{} from {}", if w.offset.is_empty() { "0" } else { &w.offset }, w.from),
        _ => String::new(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Date,
    From,
    Order,
    Clear,
}

/// The side panel for one scene or plot point: when it happens, and opening it.
#[component]
fn WhenPanel(
    item: TimelineItemView,
    titles: Vec<String>,
    kind: ProjectKind,
    project: String,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
    on_open: impl Fn(&TimelineItemView) + Copy + Send + Sync + 'static,
    on_changed: impl Fn(TimelineView) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let w = item.when.clone();
    let mode = RwSignal::new(match w.kind.as_str() {
        "from" => Mode::From,
        "order" => Mode::Order,
        _ => Mode::Date,
    });
    let text = RwSignal::new(w.text.clone());
    let from = RwSignal::new(w.from.clone());
    let offset = RwSignal::new(w.offset.clone());
    let after = RwSignal::new(w.after.clone());
    let before = RwSignal::new(w.before.clone());
    let problem = RwSignal::new(None::<String>);
    let saving = RwSignal::new(false);

    let heading = if item.is_scene() {
        let piece = item.reading.map_or("Scene".to_owned(), |n| format!("Scene {n}"));
        match &item.pov {
            Some(pov) => format!("{piece} · {pov}"),
            None => piece,
        }
    } else if item.world {
        format!("{} · from the world", NoteKind::Event.label(kind))
    } else {
        NoteKind::Event.label(kind).to_owned()
    };
    let what = if item.is_scene() { "scene" } else { "plot point" };

    let set = {
        let item = item.clone();
        move || {
            let when = match mode.get_untracked() {
                Mode::Date => WhenInput::Text { text: text.get_untracked() },
                Mode::From => WhenInput::From { from: from.get_untracked(), offset: offset.get_untracked() },
                Mode::Order => {
                    let some = |s: String| (!s.trim().is_empty()).then_some(s);
                    WhenInput::Order { after: some(after.get_untracked()), before: some(before.get_untracked()) }
                }
                Mode::Clear => WhenInput::Clear,
            };
            let item = item.clone();
            let project = project.clone();
            saving.set(true);
            spawn_local(async move {
                match tauri::set_when(&project, &item, &when).await {
                    Ok(found) => {
                        problem.try_set(None);
                        on_changed(found);
                    }
                    Err(e) => {
                        problem.try_set(Some(e));
                    }
                }
                saving.try_set(false);
            });
        }
    };
    let set_on_enter = set.clone();
    let on_enter = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Enter" {
            ev.prevent_default();
            set_on_enter();
        }
    };

    let mode_button = move |which: Mode, label: &'static str| {
        view! {
            <button
                class:active=move || mode.get() == which
                aria-pressed=move || (mode.get() == which).to_string()
                on:click=move |_| {
                    mode.set(which);
                    problem.set(None);
                }
            >
                {label}
            </button>
        }
    };
    let field = move |label: &'static str, value: RwSignal<String>, hint: &'static str, list: bool| {
        let on_enter = on_enter.clone();
        view! {
            <label class="popover-field">
                <span class="panel-heading">{label}</span>
                <input
                    type="text"
                    list=list.then_some("timeline-names")
                    prop:value=move || value.get()
                    on:input=move |ev| value.set(event_target_value(&ev))
                    on:keydown=on_enter
                />
                {(!hint.is_empty()).then(|| view! { <span class="field-hint">{hint}</span> })}
            </label>
        }
    };
    let date_hint = "14 March 1998 19:00, 1998-03-14, Day 4…";
    let fields = move || match mode.get() {
        Mode::Date => field("Date", text, date_hint, false).into_any(),
        Mode::From => view! {
            {field("From", from, "", true)}
            {field("How long after", offset, "+6h, +3d, −2h, +1y 2mo…", false)}
        }
        .into_any(),
        Mode::Order => view! {
            {field("After", after, "", true)}
            {field("Before", before, "Either, or both", true)}
        }
        .into_any(),
        Mode::Clear => view! { <p class="field-hint">{format!("The {what} goes back to the tray.")}</p> }.into_any(),
    };
    let names = titles.into_iter().map(|t| view! { <option value=t></option> }).collect_view();
    let status = match (&item.problem, &item.time, item.loose) {
        (Some(p), _, _) => view! { <p class="when-problem">{p.clone()}</p> }.into_any(),
        (None, Some(time), _) => view! { <p class="when-now">{time.clone()}</p> }.into_any(),
        (None, None, true) => view! { <p class="when-now">{loose_words(&item)}</p> }.into_any(),
        (None, None, false) => view! { <p class="when-now muted">"Not placed yet"</p> }.into_any(),
    };
    let cast = item.cast.clone();
    let opened = item.clone();
    let open_label = if item.is_scene() { "Open the scene" } else { "Open the plot point" };

    view! {
        <aside class="board-panel when-panel" aria-label="When it happens">
            <button class="board-panel-close quiet" title="Close" on:click=move |_| on_close()>
                <Icon glyph=Glyph::Close size=16 />
            </button>
            <span class="panel-kind">{heading}</span>
            <h3 class="panel-title">{item.title.clone()}</h3>
            {status}
            <span class="panel-heading">"When"</span>
            <div class="segmented four">
                {mode_button(Mode::Date, "Date")}
                {mode_button(Mode::From, "From")}
                {mode_button(Mode::Order, "Between")}
                {mode_button(Mode::Clear, "Not yet")}
            </div>
            {fields}
            <datalist id="timeline-names">{names}</datalist>
            {move || problem.get().map(|p| view! { <p class="when-problem">{format!("Couldn't set that: {p}")}</p> })}
            <div class="panel-actions">
                <button class="small" disabled=move || saving.get() on:click={
                    let set = set.clone();
                    move |_| set()
                }>"Set"</button>
                <button class="small quiet" on:click=move |_| on_open(&opened)>{open_label}</button>
            </div>
            {(!cast.is_empty()).then(|| view! {
                <span class="panel-heading">"In it"</span>
                <div class="when-cast">{cast.into_iter().map(|c| view! { <span class="chip">{c}</span> }).collect_view()}</div>
            })}
        </aside>
    }
}

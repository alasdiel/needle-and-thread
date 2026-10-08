//! The network board (DESIGN §7): a cork board of pinned cards, strung with red thread.
//!
//! Cards are HTML, so they can be paper with a shadow; the string, pins and twine are one SVG
//! behind them. Both sit in the same layer, which carries the pan and zoom as a CSS transform,
//! so the webview can move the whole board without redrawing it (spike 3).
//!
//! Over all of it go the writer's own marks: handwritten notes, and marker rings and arrows.
//! They're the only handwriting on the board, because the writer made them.

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use needle_core::id::make_id;
use needle_core::network::{Mark, Point, arrow_path, ring_path};
use needle_core::project::ProjectKind;

use crate::icons::{Glyph, Icon};
use crate::notes;
use crate::tauri::{self, BoardView, CardView, LayoutView, NoteKey, PinnedView};

/// Each kind of note is a different piece of paper, so kinds are told apart by shape before
/// colour. Sizes are in board units, which are CSS pixels at zoom 1.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Paper {
    Polaroid,
    Postcard,
    IndexCard,
    Folder,
}

impl Paper {
    fn of(kind: &str) -> Self {
        match kind {
            "place" => Self::Postcard,
            "event" => Self::IndexCard,
            "thread" => Self::Folder,
            _ => Self::Polaroid,
        }
    }

    fn size(self) -> (f64, f64) {
        match self {
            Self::Polaroid => (96.0, 116.0),
            Self::Postcard => (128.0, 84.0),
            Self::IndexCard => (136.0, 88.0),
            Self::Folder => (132.0, 84.0),
        }
    }

    fn class(self) -> &'static str {
        match self {
            Self::Polaroid => "card polaroid",
            Self::Postcard => "card postcard",
            Self::IndexCard => "card index-card",
            Self::Folder => "card folder",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Self::Polaroid => Glyph::Person,
            Self::Postcard => Glyph::MapPin,
            Self::IndexCard => Glyph::Flag,
            Self::Folder => Glyph::Spool,
        }
    }
}

/// Where a card's pin goes, which is also where its string is tied: just inside the top edge.
fn pin_of(card: &CardView) -> (f64, f64) {
    let (w, h) = Paper::of(&card.kind).size();
    let (x, y) = card.at;
    (x + w / 2.0 - 14.0, y - h / 2.0 + 9.0)
}

#[derive(Clone, Copy)]
struct Viewport {
    x: RwSignal<f64>,
    y: RwSignal<f64>,
    zoom: RwSignal<f64>,
}

/// What a press on the board does. Move is the board as it always was; the other two are for
/// marking it up, and stay chosen until another is picked or Escape is pressed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Move,
    Write,
    Marker,
}

#[derive(Clone)]
enum Grab {
    Card { index: usize, dx: f64, dy: f64, moved: bool },
    Board { from_x: f64, from_y: f64, x: f64, y: f64 },
    /// A mark being moved: where the press began, and the mark as it was then.
    Mark { start: Point, was: Mark, moved: bool },
    /// A marker stroke being drawn. An arrow remembers the card it started on.
    Ring { start: Point },
    Arrow { start: Point, card: Option<String> },
}

#[component]
pub fn NetworkBoard(
    /// The project whose board this is.
    #[prop(into)]
    project: Signal<String>,
    /// Fiction or nonfiction, which decides what a thread card's tab is called.
    #[prop(into)]
    kind: Signal<ProjectKind>,
    on_open_note: impl Fn(NoteKey) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let board = RwSignal::new(None::<BoardView>);
    let board_el = NodeRef::<leptos::html::Div>::new();
    let error = RwSignal::new(None::<String>);
    // A card's spot while it's being dragged. The board's own copy is the truth once the drag
    // ends and the arrangement is saved.
    let cards = RwSignal::new(Vec::<CardView>::new());
    let marks = RwSignal::new(Vec::<Mark>::new());
    let view_at = Viewport { x: RwSignal::new(120.0), y: RwSignal::new(120.0), zoom: RwSignal::new(1.0) };
    let grab = StoredValue::new(None::<Grab>);
    let tool = RwSignal::new(Tool::Move);
    let selected = RwSignal::new(None::<String>);
    // The handwritten note being written, and the stroke being drawn, before they're let go.
    let editing = RwSignal::new(None::<String>);
    let drawing = RwSignal::new(None::<Mark>);

    Effect::new(move |_| {
        let slug = project.get();
        spawn_local(async move {
            match tauri::project_board(&slug).await {
                Ok(found) => {
                    cards.set(found.cards.clone());
                    marks.set(found.marks.clone());
                    board.set(Some(found));
                    error.set(None);
                    show_it_all(board_el, cards, view_at);
                }
                Err(e) => error.set(Some(e)),
            }
        });
    });

    let save = move || {
        let slug = project.get_untracked();
        let found = board.get_untracked();
        let layout = LayoutView {
            nodes: cards.with_untracked(|cards| {
                cards.iter().map(|c| PinnedView { id: c.id.clone(), at: c.at, turn: c.turn }).collect()
            }),
            zones: found.as_ref().map(|b| b.zones.clone()).unwrap_or_default(),
            marks: marks.get_untracked(),
        };
        spawn_local(async move {
            if let Err(e) = tauri::save_board(&slug, &layout).await {
                error.try_set(Some(e));
            }
        });
    };

    // Rubs out one mark, which is the only way a mark leaves the board.
    let rub_out = move |id: String| {
        marks.update(|m| m.retain(|mark| mark.id() != id));
        selected.set(None);
        save();
    };

    // Lets go of the note being written: an empty one is thrown away rather than left as a
    // blank scrap, and anything else is kept as typed.
    let finish_writing = move |text: Option<String>| {
        let Some(id) = editing.get_untracked() else { return };
        editing.set(None);
        let text = text.map(|t| t.trim_end().to_owned());
        marks.update(|m| match text.as_deref() {
            Some("") => m.retain(|mark| mark.id() != id),
            Some(new) => {
                if let Some(Mark::Note { text, .. }) = m.iter_mut().find(|mark| mark.id() == id) {
                    *text = new.to_owned();
                }
            }
            // Cancelled: a note that never had words goes; an old one keeps its words.
            None => m.retain(|mark| mark.id() != id || !matches!(mark, Mark::Note { text, .. } if text.is_empty())),
        });
        save();
    };

    let start_writing = move |id: String| {
        selected.set(None);
        editing.set(Some(id.clone()));
        // The note is drawn as an editable box on the next frame, so it's focused then, with
        // the caret after whatever's already written.
        request_animation_frame(move || {
            let Some(el) = document().get_element_by_id(&format!("mark-edit-{id}")) else { return };
            if let Some(el) = el.dyn_ref::<web_sys::HtmlElement>() {
                let _ = el.focus();
            }
            if let Ok(Some(selection)) = window().get_selection() {
                let _ = selection.select_all_children(&el);
                let _ = selection.collapse_to_end();
            }
        });
    };

    // A pointer's spot on the board itself. Events give it from the window's corner, and the
    // board starts right of the sidebar and below the top bar, so that offset comes off first.
    let on_board = move |cx: f64, cy: f64| match board_el.get_untracked() {
        Some(el) => {
            let r = el.get_bounding_client_rect();
            (cx - r.left(), cy - r.top())
        }
        None => (cx, cy),
    };

    // The same spot in board units, which are what cards and marks are placed in.
    let to_board = move |cx: f64, cy: f64| {
        let (x, y) = on_board(cx, cy);
        let z = view_at.zoom.get_untracked();
        ((x - view_at.x.get_untracked()) / z, (y - view_at.y.get_untracked()) / z)
    };

    let on_down = move |ev: web_sys::PointerEvent| {
        if ev.button() != 0 {
            return;
        }
        let target = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
        let closest = |selector: &str| target.as_ref().and_then(|el| el.closest(selector).ok().flatten());
        // A note being written takes its own clicks, so the caret goes where it's put.
        if closest(".mark-editing").is_some() {
            return;
        }
        // Without this the webview starts its own text selection, which cancels the pointer
        // mid-drag and leaves a card stuck to the cursor.
        ev.prevent_default();
        if let Some(el) = document().active_element().and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok()) {
            // Clicking away from a note being written lets go of it.
            let _ = el.blur();
        }
        let (cx, cy) = (ev.client_x() as f64, ev.client_y() as f64);
        let at = to_board(cx, cy);
        let held_card = closest("[data-card]")
            .and_then(|el| el.get_attribute("data-card"))
            .and_then(|i| i.parse::<usize>().ok());
        let held_mark = closest("[data-mark]").and_then(|el| el.get_attribute("data-mark"));

        let new = match tool.get_untracked() {
            Tool::Move => match (held_mark, held_card) {
                (Some(id), _) => {
                    selected.set(Some(id.clone()));
                    let was = marks.with_untracked(|m| m.iter().find(|mark| mark.id() == id).cloned());
                    was.map(|was| Grab::Mark { start: at, was, moved: false })
                }
                (None, Some(index)) => {
                    selected.set(None);
                    let card_at = cards.with_untracked(|c| c[index].at);
                    Some(Grab::Card { index, dx: card_at.0 - at.0, dy: card_at.1 - at.1, moved: false })
                }
                (None, None) => {
                    selected.set(None);
                    Some(Grab::Board { from_x: cx, from_y: cy, x: view_at.x.get_untracked(), y: view_at.y.get_untracked() })
                }
            },
            Tool::Write => {
                let on_note = held_mark.filter(|id| {
                    marks.with_untracked(|m| m.iter().any(|mark| mark.id() == id && matches!(mark, Mark::Note { .. })))
                });
                let id = match on_note {
                    Some(id) => id,
                    None => {
                        // Written onto a card, a note is measured from it so it travels with it.
                        let on = held_card.map(|i| cards.with_untracked(|c| (c[i].id.clone(), c[i].at)));
                        let spot = on.as_ref().map_or(at, |(_, card)| (at.0 - card.0, at.1 - card.1));
                        let id = new_id();
                        marks.update(|m| {
                            m.push(Mark::Note {
                                id: id.clone(),
                                text: String::new(),
                                at: spot,
                                turn: tilt(),
                                on: on.map(|(card, _)| card),
                            })
                        });
                        id
                    }
                };
                start_writing(id);
                None
            }
            Tool::Marker => {
                selected.set(None);
                match held_card {
                    Some(i) => Some(Grab::Arrow { start: at, card: Some(cards.with_untracked(|c| c[i].id.clone())) }),
                    None => Some(Grab::Ring { start: at }),
                }
            }
        };
        grab.set_value(new);
        if let Some(el) = ev.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
            let _ = el.set_pointer_capture(ev.pointer_id());
        }
    };

    let on_move = move |ev: web_sys::PointerEvent| {
        let (cx, cy) = (ev.client_x() as f64, ev.client_y() as f64);
        let at = to_board(cx, cy);
        match grab.get_value() {
            Some(Grab::Card { index, dx, dy, .. }) => {
                cards.update(|cards| cards[index].at = (at.0 + dx, at.1 + dy));
                grab.set_value(Some(Grab::Card { index, dx, dy, moved: true }));
            }
            Some(Grab::Board { from_x, from_y, x, y }) => {
                view_at.x.set(x + cx - from_x);
                view_at.y.set(y + cy - from_y);
            }
            Some(Grab::Mark { start, was, .. }) => {
                let moved = was.shifted((at.0 - start.0, at.1 - start.1));
                marks.update(|m| {
                    if let Some(mark) = m.iter_mut().find(|mark| mark.id() == was.id()) {
                        *mark = moved;
                    }
                });
                grab.set_value(Some(Grab::Mark { start, was, moved: true }));
            }
            Some(Grab::Ring { start }) => drawing.set(Some(ring_between(start, at))),
            Some(Grab::Arrow { start, card }) => {
                let from = cards.with_untracked(|c| edge_of(c, card.as_deref(), start, at));
                drawing.set(Some(Mark::Arrow { id: String::new(), from, to: at, bend: 0.0 }));
            }
            None => {}
        }
    };

    // A cancelled pointer is not a click: let go of the card where it is, and open nothing.
    let on_cancel = move |_: web_sys::PointerEvent| {
        if let Some(Grab::Card { moved: true, .. } | Grab::Mark { moved: true, .. }) = grab.get_value() {
            save();
        }
        drawing.set(None);
        grab.set_value(None);
    };

    let on_up = move |ev: web_sys::PointerEvent| {
        let at = to_board(ev.client_x() as f64, ev.client_y() as f64);
        match grab.get_value() {
            // A card that was only clicked, not dragged, opens its note instead.
            Some(Grab::Card { index, moved, .. }) => {
                if moved {
                    save();
                } else {
                    let key = cards.with_untracked(|c| {
                        let card = &c[index];
                        NoteKey { owner: card.owner.clone(), world: card.world, path: card.path.clone() }
                    });
                    on_open_note(key);
                }
            }
            Some(Grab::Mark { was, moved: true, .. }) => {
                // A note let go over a card goes onto that card; let go on the cork, it's loose.
                if let Mark::Note { .. } = was {
                    marks.update(|m| {
                        let Some(mark) = m.iter_mut().find(|mark| mark.id() == was.id()) else { return };
                        cards.with_untracked(|c| settle_note(mark, c, at));
                    });
                }
                save();
            }
            Some(Grab::Ring { start }) => {
                let ring = ring_between(start, at);
                drawing.set(None);
                // A tap with the marker isn't a ring.
                if let Mark::Ring { at, size, turn, .. } = ring
                    && size.0.abs() > 16.0
                    && size.1.abs() > 16.0
                {
                    marks.update(|m| m.push(Mark::Ring { id: new_id(), at, size, turn }));
                    save();
                }
            }
            Some(Grab::Arrow { start, card }) => {
                drawing.set(None);
                let arrow = cards.with_untracked(|c| {
                    let end_card = card_at(c, at).map(|i| c[i].id.clone()).filter(|id| Some(id) != card.as_ref());
                    let to = edge_of(c, end_card.as_deref(), at, start);
                    let from = edge_of(c, card.as_deref(), start, to);
                    (from, to)
                });
                let (from, to) = arrow;
                if (to.0 - from.0).hypot(to.1 - from.1) > 24.0 {
                    let bend = marks.with_untracked(|m| bend_for(m, from, to));
                    marks.update(|m| m.push(Mark::Arrow { id: new_id(), from, to, bend }));
                    save();
                }
            }
            _ => {}
        }
        grab.set_value(None);
    };

    let on_wheel = move |ev: web_sys::WheelEvent| {
        ev.prevent_default();
        let z = view_at.zoom.get_untracked();
        let next = (z * (-ev.delta_y() * 0.0015).exp()).clamp(0.2, 2.5);
        let scale = next / z;
        // Zoom about the spot under the pointer, so what you're looking at stays put.
        let (cx, cy) = on_board(ev.client_x() as f64, ev.client_y() as f64);
        view_at.x.set(cx - (cx - view_at.x.get_untracked()) * scale);
        view_at.y.set(cy - (cy - view_at.y.get_untracked()) * scale);
        view_at.zoom.set(next);
    };

    // Delete or Backspace rubs out the chosen mark; Escape puts the marker down.
    let keys = window_event_listener(leptos::ev::keydown, move |ev| {
        // Backspace in the search box, or anywhere else you're typing, is typing.
        let typing = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()).is_some_and(|el| {
            matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT") || el.is_content_editable()
        });
        if typing || editing.get_untracked().is_some() {
            return;
        }
        match ev.key().as_str() {
            "Delete" | "Backspace" => {
                if let Some(id) = selected.get_untracked() {
                    ev.prevent_default();
                    rub_out(id);
                }
            }
            "Escape" => {
                selected.try_set(None);
                tool.try_set(Tool::Move);
            }
            _ => {}
        }
    });
    on_cleanup(move || keys.remove());

    // The string, twine and pins, drawn behind the cards. Its box grows with the board, so an
    // empty board doesn't carry a huge one.
    let threads = move || {
        let found = board.get()?;
        let cards = cards.get();
        let spot = |id: &String| cards.iter().find(|c| c.id == *id).map(pin_of);
        let (minx, miny, maxx, maxy) = bounds(&cards)?;
        let (w, h) = (maxx - minx, maxy - miny);

        let twine: Vec<_> = found
            .links
            .iter()
            .filter_map(|l| Some((spot(&l.from)?, spot(&l.to)?)))
            .map(|(a, b)| view! { <path class="twine" d=sag(a, b)></path> })
            .collect();
        let strings: Vec<_> = found
            .relationships
            .iter()
            .filter_map(|r| Some((spot(&r.from)?, spot(&r.to)?)))
            .map(|(a, b)| {
                let d = sag(a, b);
                view! {
                    <path class="string-shadow" d=d.clone()></path>
                    <path class="string" d=d></path>
                }
            })
            .collect();
        Some(view! {
            <svg
                class="board-threads"
                width=w
                height=h
                viewBox=format!("{minx} {miny} {w} {h}")
                style:left=format!("{minx}px")
                style:top=format!("{miny}px")
                aria-hidden="true"
            >
                {twine}
                {strings}
            </svg>
        })
    };

    // The pins, over the cards: a pin holds its card to the board, so it can't be behind it.
    let pins = move || {
        let cards = cards.get();
        let (minx, miny, maxx, maxy) = bounds(&cards)?;
        let (w, h) = (maxx - minx, maxy - miny);
        let heads: Vec<_> = cards
            .iter()
            .map(|card| {
                let (x, y) = pin_of(card);
                let kind = card.kind.clone();
                view! {
                    <g class="pin" data-kind=kind>
                        <ellipse class="pin-shadow" cx=x + 3.0 cy=y + 5.0 rx=6 ry=3.5></ellipse>
                        <circle class="pin-head" cx=x cy=y r=6.5></circle>
                        <circle class="pin-shine" cx=x - 2.0 cy=y - 2.2 r=2></circle>
                    </g>
                }
            })
            .collect();
        Some(view! {
            <svg
                class="board-threads board-pins"
                width=w
                height=h
                viewBox=format!("{minx} {miny} {w} {h}")
                style:left=format!("{minx}px")
                style:top=format!("{miny}px")
                aria-hidden="true"
            >
                {heads}
            </svg>
        })
    };

    // A relationship's label, on a strip of masking tape halfway along its string.
    let labels = move || {
        let found = board.get()?;
        let cards = cards.get();
        let spot = |id: &String| cards.iter().find(|c| c.id == *id).map(pin_of);
        Some(
            found
                .relationships
                .iter()
                .filter(|r| !r.label.is_empty())
                .filter_map(|r| {
                    let (a, b) = (spot(&r.from)?, spot(&r.to)?);
                    let (x, y) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0 + sag_of(a, b));
                    let text = if r.directed { format!("{} \u{2192}", r.label) } else { r.label.clone() };
                    Some(view! {
                        <div class="tape" style:left=format!("{x}px") style:top=format!("{y}px")>
                            {text}
                        </div>
                    })
                })
                .collect_view(),
        )
    };

    let card_views = move || {
        cards
            .get()
            .into_iter()
            .enumerate()
            .map(|(i, card)| {
                let paper = Paper::of(&card.kind);
                // A thread is an argument in a nonfiction project, so the folder's tab says so.
                let tab = notes::kind_label(&card.kind, kind.get()).to_lowercase();
                let (w, h) = paper.size();
                let (x, y) = card.at;
                let from = card.from.clone();
                view! {
                    <div
                        class=paper.class()
                        data-card=i
                        style:left=format!("{}px", x - w / 2.0)
                        style:top=format!("{}px", y - h / 2.0)
                        style:width=format!("{w}px")
                        style:height=format!("{h}px")
                        style:transform=format!("rotate({}deg)", card.turn)
                        title=card.title.clone()
                        data-tab=tab
                    >
                        <span class="card-mark" aria-hidden="true">
                            <Icon glyph=paper.glyph() size=if paper == Paper::Polaroid { 40 } else { 18 } />
                        </span>
                        <span class="card-title">{card.title.clone()}</span>
                        <Show when=move || !from.is_empty()>
                            <span class="card-from">{card.from.clone()}</span>
                        </Show>
                    </div>
                }
            })
            .collect_view()
    };

    // Rings and arrows, over everything: you circle cards by drawing on top of them. Each has
    // a wide invisible stroke under it, so it can be picked up without aiming at a thin line.
    let strokes = move || {
        let cards = cards.get();
        let marks = marks.get();
        let preview = drawing.get();
        let chosen = selected.get();
        let (minx, miny, maxx, maxy) = mark_bounds(&cards, marks.iter().chain(preview.iter()))?;
        let (w, h) = (maxx - minx, maxy - miny);
        let stroke = |mark: &Mark, preview: bool| {
            let class = if preview {
                "marker preview"
            } else if chosen.as_deref() == Some(mark.id()) {
                "marker chosen"
            } else {
                "marker"
            };
            let id = mark.id().to_owned();
            match mark {
                Mark::Ring { at, size, .. } => {
                    let d = ring_path(*at, *size);
                    Some(view! {
                        <g class=class>
                            <path class="marker-hit" data-mark=id d=d.clone()></path>
                            <path class="marker-ink" d=d></path>
                        </g>
                    }
                    .into_any())
                }
                Mark::Arrow { from, to, bend, .. } => {
                    let (shaft, head) = arrow_path(*from, *to, *bend);
                    Some(view! {
                        <g class=class>
                            <path class="marker-hit" data-mark=id d=shaft.clone()></path>
                            <path class="marker-ink" d=shaft></path>
                            <path class="marker-ink" d=head></path>
                        </g>
                    }
                    .into_any())
                }
                Mark::Note { .. } => None,
            }
        };
        let drawn: Vec<_> = marks.iter().filter_map(|m| stroke(m, false)).chain(preview.iter().filter_map(|m| stroke(m, true))).collect();
        Some(view! {
            <svg
                class="board-threads board-marks"
                width=w
                height=h
                viewBox=format!("{minx} {miny} {w} {h}")
                style:left=format!("{minx}px")
                style:top=format!("{miny}px")
                aria-hidden="true"
            >
                {drawn}
            </svg>
        })
    };

    // Handwritten notes. The one being written is an editable box; Enter lets go of it,
    // Shift+Enter starts a new line, and Escape throws away a new one.
    let written = move || {
        let cards = cards.get();
        let chosen = selected.get();
        let now_editing = editing.get();
        marks
            .get()
            .into_iter()
            .filter_map(|mark| {
                let Mark::Note { id, text, turn, on, .. } = &mark else { return None };
                let (x, y) = note_spot(&mark, &cards);
                // Sitting on a card's paper rather than the cork, which takes dark ink to read. A
                // note can belong to a card and still sit beside it, so it's where the note's
                // middle is that counts, not which card it travels with.
                let on_card = on.is_some() && card_at(&cards, (x, y)).is_some();
                let place = format!("left: {x:.1}px; top: {y:.1}px; transform: translate(-50%, -50%) rotate({turn:.1}deg)");
                if now_editing.as_deref() == Some(id.as_str()) {
                    let text = text.clone();
                    return Some(
                        view! {
                            <div
                                id=format!("mark-edit-{id}")
                                class=if on_card { "mark-note mark-editing on-card" } else { "mark-note mark-editing" }
                                style=place
                                contenteditable="plaintext-only"
                                spellcheck="false"
                                on:blur=move |ev| {
                                    let text = ev
                                        .target()
                                        .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                                        .map(|el| el.inner_text());
                                    finish_writing(text);
                                }
                                on:keydown=move |ev: web_sys::KeyboardEvent| {
                                    ev.stop_propagation();
                                    match ev.key().as_str() {
                                        "Enter" if !ev.shift_key() => {
                                            ev.prevent_default();
                                            if let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                                                let _ = el.blur();
                                            }
                                        }
                                        "Escape" => {
                                            ev.prevent_default();
                                            finish_writing(None);
                                        }
                                        _ => {}
                                    }
                                }
                            >
                                {text}
                            </div>
                        }
                        .into_any(),
                    );
                }
                let class = match (chosen.as_deref() == Some(id.as_str()), on_card) {
                    (true, true) => "mark-note chosen on-card",
                    (true, false) => "mark-note chosen",
                    (false, true) => "mark-note on-card",
                    (false, false) => "mark-note",
                };
                let id = id.clone();
                let open_it = id.clone();
                Some(
                    view! {
                        <div
                            class=class
                            data-mark=id
                            style=place
                            on:dblclick=move |_| start_writing(open_it.clone())
                        >
                            {text.clone()}
                        </div>
                    }
                    .into_any(),
                )
            })
            .collect_view()
    };

    let empty = move || board.with(|b| b.as_ref().is_some_and(|b| b.cards.is_empty()));

    let tool_button = move |which: Tool, label: &'static str, hint: &'static str| {
        view! {
            <button
                class="small"
                class:active=move || tool.get() == which
                title=hint
                aria-pressed=move || (tool.get() == which).to_string()
                on:click=move |_| {
                    tool.set(which);
                    selected.set(None);
                }
            >
                {label}
            </button>
        }
    };

    let board_class = move || match tool.get() {
        Tool::Move => "board",
        Tool::Write => "board tool-write",
        Tool::Marker => "board tool-marker",
    };

    view! {
        <div
            class=board_class
            node_ref=board_el
            on:pointerdown=on_down
            on:pointermove=on_move
            on:pointerup=on_up
            on:pointercancel=on_cancel
            on:wheel=on_wheel
        >
            {move || error.get().map(|e| view! { <p class="error board-error">{e}</p> })}
            <Show when=empty>
                <p class="board-empty">
                    {move || {
                        let k = kind.get();
                        format!(
                            "Nothing is pinned up yet. Make a character, place, {} or {}, and it appears here.",
                            notes::kind_label("event", k).to_lowercase(),
                            notes::kind_label("thread", k).to_lowercase(),
                        )
                    }}
                </p>
            </Show>
            <div
                class="board-layer"
                style:transform=move || {
                    format!(
                        "translate({:.1}px, {:.1}px) scale({:.4})",
                        view_at.x.get(),
                        view_at.y.get(),
                        view_at.zoom.get(),
                    )
                }
            >
                {threads}
                {card_views}
                {pins}
                {labels}
                {strokes}
                {written}
            </div>
            // The tools sit over the cork, outside the layer, so they don't pan or zoom away.
            <div class="board-tools" on:pointerdown=|ev| ev.stop_propagation()>
                {tool_button(Tool::Move, "Move", "Move cards and marks, and open a card")}
                {tool_button(Tool::Write, "Write", "Click to write a note; on a card, it moves with the card")}
                {tool_button(Tool::Marker, "Marker", "Drag around cards to circle them, or from a card to point")}
                <Show when=move || selected.get().is_some()>
                    <button
                        class="small quiet"
                        title="Or press Delete"
                        on:click=move |_| {
                            if let Some(id) = selected.get_untracked() {
                                rub_out(id);
                            }
                        }
                    >
                        "Rub out"
                    </button>
                </Show>
            </div>
        </div>
    }
}

/// A fresh mark id. There's no clock or random source of Rust's own in the webview.
fn new_id() -> String {
    make_id("mk", (js_sys::Math::random() * (1u64 << 50) as f64) as u64)
}

/// A slight lean for a new note, the way nobody writes quite level.
fn tilt() -> f64 {
    ((js_sys::Math::random() - 0.5) * 6.0 * 10.0).round() / 10.0
}

/// The ring a marker drag from `a` to `b` draws: the box between them.
fn ring_between(a: Point, b: Point) -> Mark {
    Mark::Ring {
        id: String::new(),
        at: ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
        size: ((b.0 - a.0).abs(), (b.1 - a.1).abs()),
        turn: 0.0,
    }
}

/// Which card, if any, `at` is on. The last one drawn is on top, so it's checked first.
fn card_at(cards: &[CardView], at: Point) -> Option<usize> {
    cards.iter().rposition(|card| {
        let (w, h) = Paper::of(&card.kind).size();
        (at.0 - card.at.0).abs() <= w / 2.0 && (at.1 - card.at.1).abs() <= h / 2.0
    })
}

/// Where a line from the middle of card `id` towards `toward` leaves the card, so an arrow
/// starts and stops at its edge instead of over its title. Off any card, it's `at` itself.
fn edge_of(cards: &[CardView], id: Option<&str>, at: Point, toward: Point) -> Point {
    let Some(card) = id.and_then(|id| cards.iter().find(|c| c.id == id)) else { return at };
    let (w, h) = Paper::of(&card.kind).size();
    let (cx, cy) = card.at;
    let (dx, dy) = (toward.0 - cx, toward.1 - cy);
    if dx == 0.0 && dy == 0.0 {
        return card.at;
    }
    const GAP: f64 = 6.0;
    let reach = ((w / 2.0 + GAP) / dx.abs()).min((h / 2.0 + GAP) / dy.abs());
    // `toward` inside the card itself: there's no edge to stop at before it.
    if reach >= 1.0 {
        return toward;
    }
    (cx + dx * reach, cy + dy * reach)
}

/// How far to bow a new arrow so it doesn't lie along one already drawn between the same two
/// spots: the first goes straight, the next bows one way, the one after the other.
fn bend_for(marks: &[Mark], from: Point, to: Point) -> f64 {
    let near = |a: Point, b: Point| (a.0 - b.0).hypot(a.1 - b.1) < 40.0;
    let alongside = marks
        .iter()
        .filter(|m| match m {
            Mark::Arrow { from: f, to: t, .. } => (near(*f, from) && near(*t, to)) || (near(*f, to) && near(*t, from)),
            _ => false,
        })
        .count();
    match alongside {
        0 => 0.0,
        n => 28.0 * n.div_ceil(2) as f64 * if n % 2 == 1 { 1.0 } else { -1.0 },
    }
}

/// Where a handwritten note sits on the board. One on a card is measured from that card; if
/// the card has gone, the note stays where its numbers put it rather than vanishing.
fn note_spot(mark: &Mark, cards: &[CardView]) -> Point {
    match mark {
        Mark::Note { at, on: Some(card), .. } => match cards.iter().find(|c| c.id == *card) {
            Some(card) => (card.at.0 + at.0, card.at.1 + at.1),
            None => *at,
        },
        Mark::Note { at, .. } => *at,
        _ => (0.0, 0.0),
    }
}

/// After a note is dragged and let go at `dropped`: on a card, it's put on that card; on the
/// cork, it comes loose. Either way it stays exactly where it was let go.
fn settle_note(mark: &mut Mark, cards: &[CardView], dropped: Point) {
    let spot = note_spot(mark, cards);
    let Mark::Note { at, on, .. } = mark else { return };
    match card_at(cards, dropped) {
        Some(i) => {
            *on = Some(cards[i].id.clone());
            *at = (spot.0 - cards[i].at.0, spot.1 - cards[i].at.1);
        }
        None => {
            *on = None;
            *at = spot;
        }
    }
}

/// The box marks are drawn in: the cards and every mark's points, with room for a ring's
/// wobble and an arrow's bow.
fn mark_bounds<'a>(cards: &[CardView], marks: impl Iterator<Item = &'a Mark>) -> Option<(f64, f64, f64, f64)> {
    let mut points: Vec<Point> = Vec::new();
    for mark in marks {
        match mark {
            Mark::Ring { at, size, .. } => {
                points.push((at.0 - size.0 / 2.0, at.1 - size.1 / 2.0));
                points.push((at.0 + size.0 / 2.0, at.1 + size.1 / 2.0));
            }
            Mark::Arrow { from, to, .. } => points.extend([*from, *to]),
            Mark::Note { .. } => {}
        }
    }
    if points.is_empty() {
        return None;
    }
    let mut b = card_bounds(cards).unwrap_or((points[0].0, points[0].1, points[0].0, points[0].1));
    for (x, y) in points {
        b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
    }
    const ROOM: f64 = 120.0;
    Some((b.0 - ROOM, b.1 - ROOM, b.2 + ROOM, b.3 + ROOM))
}

/// Opens the board on whatever is pinned to it, rather than at some corner of the cork: the
/// content is centred, and zoomed out far enough to fit if it's bigger than the window.
fn show_it_all(board_el: NodeRef<leptos::html::Div>, cards: RwSignal<Vec<CardView>>, view_at: Viewport) {
    let Some(el) = board_el.get_untracked() else { return };
    let (w, h) = (el.client_width() as f64, el.client_height() as f64);
    let Some((minx, miny, maxx, maxy)) = card_bounds(&cards.get_untracked()) else { return };
    const MARGIN: f64 = 48.0;
    let fits = ((w - MARGIN * 2.0) / (maxx - minx).max(1.0)).min((h - MARGIN * 2.0) / (maxy - miny).max(1.0));
    let zoom = fits.clamp(0.2, 1.0);
    view_at.zoom.set(zoom);
    view_at.x.set(w / 2.0 - (minx + maxx) / 2.0 * zoom);
    view_at.y.set(h / 2.0 - (miny + maxy) / 2.0 * zoom);
}

/// How far a string dips between two pins. Real string sags with its own length.
fn sag_of(a: (f64, f64), b: (f64, f64)) -> f64 {
    0.07 * ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
}

/// A string between two pins, as a quadratic curve dipping below the straight line.
fn sag(a: (f64, f64), b: (f64, f64)) -> String {
    let dip = sag_of(a, b);
    let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0 + dip * 2.0);
    format!("M{:.1},{:.1} Q{:.1},{:.1} {:.1},{:.1}", a.0, a.1, mx, my, b.0, b.1)
}

/// The box the strings and pins are drawn in: the cards, plus room for a string's sag.
fn bounds(cards: &[CardView]) -> Option<(f64, f64, f64, f64)> {
    let (minx, miny, maxx, maxy) = card_bounds(cards)?;
    const ROOM: f64 = 400.0;
    Some((minx - ROOM, miny - ROOM, maxx + ROOM, maxy + ROOM))
}

/// The box the cards themselves fill.
fn card_bounds(cards: &[CardView]) -> Option<(f64, f64, f64, f64)> {
    let mut box_of: Option<(f64, f64, f64, f64)> = None;
    for card in cards {
        let (w, h) = Paper::of(&card.kind).size();
        let (x, y) = card.at;
        let (minx, miny, maxx, maxy) = (x - w / 2.0, y - h / 2.0, x + w / 2.0, y + h / 2.0);
        box_of = Some(match box_of {
            None => (minx, miny, maxx, maxy),
            Some(b) => (b.0.min(minx), b.1.min(miny), b.2.max(maxx), b.3.max(maxy)),
        });
    }
    box_of
}

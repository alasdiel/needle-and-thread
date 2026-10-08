//! The network board (DESIGN §7): a cork board of pinned cards, strung with red thread.
//!
//! Cards are HTML, so they can be paper with a shadow; the string, pins and twine are one SVG
//! behind them. Both sit in the same layer, which carries the pan and zoom as a CSS transform,
//! so the webview can move the whole board without redrawing it (spike 3).
//!
//! Under the cards go zones, sheets of kraft paper the writer lays down and names. Over all of
//! it go the writer's own marks: handwritten notes, and marker rings and arrows. They're the
//! only handwriting on the board, because the writer made them.
//!
//! The board is also where some notes are made: tying a string between two cards writes a
//! relationship note, and double-clicking the cork starts a plot point. Clicking a card or a
//! string opens it in the side panel.

use std::collections::HashSet;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use needle_core::id::make_id;
use needle_core::network::{Mark, Point, Zone, arrow_path, ring_path};
use needle_core::project::{NoteKind, ProjectKind};

use crate::icons::{Glyph, Icon};
use crate::notes;
use crate::tauri::{self, BoardView, CardView, LayoutView, NoteKey, NoteView, PinnedView, RelationshipEdit, RelationshipView};

/// Each kind of note is a different piece of paper, so kinds are told apart by shape before
/// colour. Sizes are in board units, which are CSS pixels at zoom 1.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Paper {
    Polaroid,
    Postcard,
    IndexCard,
    Folder,
    /// A plain sheet, for anything else the writer pins up (a source, a note).
    Scrap,
}

impl Paper {
    fn of(kind: &str) -> Self {
        match kind {
            "character" => Self::Polaroid,
            "place" => Self::Postcard,
            "event" => Self::IndexCard,
            "thread" => Self::Folder,
            _ => Self::Scrap,
        }
    }

    fn size(self) -> (f64, f64) {
        match self {
            Self::Polaroid => (96.0, 116.0),
            Self::Postcard => (128.0, 84.0),
            Self::IndexCard => (136.0, 88.0),
            Self::Folder => (132.0, 84.0),
            Self::Scrap => (120.0, 84.0),
        }
    }

    fn class(self) -> &'static str {
        match self {
            Self::Polaroid => "card polaroid",
            Self::Postcard => "card postcard",
            Self::IndexCard => "card index-card",
            Self::Folder => "card folder",
            Self::Scrap => "card scrap",
        }
    }

    fn glyph(self, kind: &str) -> Glyph {
        match self {
            Self::Polaroid => Glyph::Person,
            Self::Postcard => Glyph::MapPin,
            Self::IndexCard => Glyph::Flag,
            Self::Folder => Glyph::Spool,
            Self::Scrap => notes::kind_glyph(kind),
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

/// What a press on the board does. Move is the board as it always was; the others are for
/// building on it and marking it up, and stay chosen until another is picked or Escape is
/// pressed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Move,
    /// Ties a string from one card to another: a new relationship.
    Tie,
    Write,
    Marker,
    Zone,
}

/// What's been clicked on. A card or a string opens the side panel; a mark can be rubbed out
/// and a zone taken down.
#[derive(Clone, PartialEq, Eq)]
enum Chosen {
    Card(String),
    Relationship(String),
    Mark(String),
    Zone(String),
}

/// Something put on the board that's waiting for words: a string for its label, or a new plot
/// point for its title.
#[derive(Clone, PartialEq)]
enum Pending {
    Tie { from: String, to: String },
    Card { at: Point },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Popover {
    PinUp,
    Show,
}

/// What the board is showing. Nothing here is saved: it's a way of looking, not an arrangement.
#[derive(Clone, Default, PartialEq)]
struct Filters {
    /// Kinds of card left off, by `bucket`.
    hidden: Vec<&'static str>,
    no_twine: bool,
    /// Only this thread's card and what's tied or linked to it.
    thread: Option<String>,
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
    /// A string being pulled from a card.
    Tie { from: String },
    /// A zone being laid down, moved by its tape, or resized by its corner.
    NewZone { start: Point },
    Zone { start: Point, was: Zone, moved: bool },
    ZoneSize { start: Point, was: Zone },
}

/// The smallest a zone can be, so it can't vanish into a speck while being resized.
const MIN_ZONE: f64 = 60.0;

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
    let zones = RwSignal::new(Vec::<Zone>::new());
    let view_at = Viewport { x: RwSignal::new(120.0), y: RwSignal::new(120.0), zoom: RwSignal::new(1.0) };
    let grab = StoredValue::new(None::<Grab>);
    // The last press, to tell a double press from two single ones: when, where, and on what.
    let last_press = StoredValue::new(None::<(f64, f64, f64, String)>);
    let tool = RwSignal::new(Tool::Move);
    let chosen = RwSignal::new(None::<Chosen>);
    // The handwritten note being written, and the stroke being drawn, before they're let go.
    let editing = RwSignal::new(None::<String>);
    let drawing = RwSignal::new(None::<Mark>);
    // A string being pulled (from its pin to the pointer), and a zone being laid down.
    let tying = RwSignal::new(None::<(Point, Point)>);
    let new_zone = RwSignal::new(None::<Zone>);
    let naming = RwSignal::new(None::<String>);
    let pending = RwSignal::new(None::<Pending>);
    let popover = RwSignal::new(None::<Popover>);
    let filters = RwSignal::new(Filters::default());

    // Reads the board. The first time it's framed to fit; after a change made from it (a new
    // string, a card pinned up) it stays where you were looking.
    let load = move |fit: bool| {
        let slug = project.get_untracked();
        spawn_local(async move {
            match tauri::project_board(&slug).await {
                Ok(found) => {
                    cards.try_set(found.cards.clone());
                    marks.try_set(found.marks.clone());
                    zones.try_set(found.zones.clone());
                    board.try_set(Some(found));
                    error.try_set(None);
                    if fit {
                        show_it_all(board_el, cards, view_at);
                    }
                }
                Err(e) => {
                    error.try_set(Some(e));
                }
            }
        });
    };

    Effect::new(move |_| {
        project.track();
        chosen.set(None);
        load(true);
    });

    let save = move || {
        let slug = project.get_untracked();
        let layout = LayoutView {
            nodes: cards.with_untracked(|cards| {
                cards.iter().map(|c| PinnedView { id: c.id.clone(), at: c.at, turn: c.turn }).collect()
            }),
            zones: zones.get_untracked(),
            marks: marks.get_untracked(),
        };
        spawn_local(async move {
            if let Err(e) = tauri::save_board(&slug, &layout).await {
                error.try_set(Some(e));
            }
        });
    };

    // Runs a change to the notes behind the board, then reads the board again to show it.
    let then_reload = move |result: Result<(), String>| match result {
        Ok(()) => load(false),
        Err(e) => {
            error.try_set(Some(e));
        }
    };

    // Rubs out one mark, which is the only way a mark leaves the board.
    let rub_out = move |id: String| {
        marks.update(|m| m.retain(|mark| mark.id() != id));
        chosen.set(None);
        save();
    };

    // Takes a zone's sheet down. The cards on it stay where they are.
    let take_down_zone = move |id: String| {
        zones.update(|z| z.retain(|zone| zone.id != id));
        chosen.set(None);
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
        chosen.set(None);
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

    // Lets go of a zone's name: kept as typed, or as it was if the naming was cancelled. A
    // zone is up either way; drawing it was the point, and a name can come later.
    let finish_naming = move |name: Option<String>| {
        let Some(id) = naming.get_untracked() else { return };
        naming.set(None);
        if let Some(name) = name {
            zones.update(|z| {
                if let Some(zone) = z.iter_mut().find(|zone| zone.id == id) {
                    zone.name = name.trim().to_owned();
                }
            });
        }
        save();
    };

    let start_naming = move |id: String| {
        chosen.set(Some(Chosen::Zone(id.clone())));
        naming.set(Some(id));
        focus_soon("zone-naming");
    };

    // A string let go on a second card waits for its label; this ties it once it has one (or
    // has been left without). It reads from the first card to the second.
    let tie = move |from: String, to: String, label: String| {
        pending.set(None);
        let slug = project.get_untracked();
        spawn_local(async move {
            match tauri::add_relationship(&slug, &from, &to, &label, true).await {
                Ok(note) => {
                    load(false);
                    chosen.try_set(Some(Chosen::Relationship(note.id)));
                }
                Err(e) => {
                    error.try_set(Some(e));
                }
            }
        });
    };

    let add_plot_point = move |at: Point, title: String| {
        pending.set(None);
        let title = title.trim().to_owned();
        if title.is_empty() {
            return;
        }
        let slug = project.get_untracked();
        spawn_local(async move { then_reload(tauri::add_card(&slug, "event", &title, at).await.map(|_| ())) });
    };

    let edit_string = move |key: NoteKey, edit: RelationshipEdit| {
        spawn_local(async move { then_reload(tauri::edit_relationship(&key, &edit).await.map(|_| ())) });
    };

    let cut_string = move |key: NoteKey| {
        chosen.set(None);
        spawn_local(async move { then_reload(tauri::cut_note(&key).await) });
    };

    let take_down_card = move |id: String| {
        chosen.set(None);
        let slug = project.get_untracked();
        spawn_local(async move { then_reload(tauri::unpin_card(&slug, &id).await) });
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

    // The middle of what's in view, in board units: where a card pinned up from the list goes.
    let in_view = move || match board_el.get_untracked() {
        Some(el) => {
            let r = el.get_bounding_client_rect();
            to_board(r.left() + r.width() / 2.0, r.top() + r.height() / 2.0)
        }
        None => (0.0, 0.0),
    };

    let pin_up = move |id: String| {
        popover.set(None);
        let slug = project.get_untracked();
        let at = in_view();
        spawn_local(async move { then_reload(tauri::pin_card(&slug, &id, at).await) });
    };

    let on_down = move |ev: web_sys::PointerEvent| {
        if ev.button() != 0 {
            return;
        }
        let target = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
        let closest = |selector: &str| target.as_ref().and_then(|el| el.closest(selector).ok().flatten());
        // Words being written take their own clicks, so the caret goes where it's put.
        if closest(".mark-editing, .board-input, .label-input").is_some() {
            return;
        }
        // Without this the webview starts its own text selection, which cancels the pointer
        // mid-drag and leaves a card stuck to the cursor.
        ev.prevent_default();
        if let Some(el) = document().active_element().and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok()) {
            // Clicking away from words being written lets go of them.
            let _ = el.blur();
        }
        popover.set(None);
        let (cx, cy) = (ev.client_x() as f64, ev.client_y() as f64);
        let at = to_board(cx, cy);
        let attr = |selector: &str, name: &str| closest(selector).and_then(|el| el.get_attribute(name));
        let held_card = attr("[data-card]", "data-card").and_then(|i| i.parse::<usize>().ok());
        let held_mark = attr("[data-mark]", "data-mark");
        let held_zone = attr("[data-zone]", "data-zone");
        let held_corner = attr("[data-zone-size]", "data-zone-size");
        let held_string = attr("[data-rel]", "data-rel");
        let zone_of = |id: &str| zones.with_untracked(|z| z.iter().find(|zone| zone.id == id).cloned());

        // A double press, told apart by hand: the board holds the pointer from the first press,
        // so the webview's own double-click would land on the board rather than on the card.
        let what = match (&held_mark, held_card, &held_zone) {
            (Some(id), _, _) => format!("mark {id}"),
            (None, Some(i), _) => format!("card {i}"),
            (None, None, Some(id)) => format!("zone {id}"),
            _ if held_corner.is_some() || held_string.is_some() => "other".to_owned(),
            _ => "cork".to_owned(),
        };
        let now = ev.time_stamp();
        let double = last_press
            .get_value()
            .is_some_and(|(then, x, y, before)| now - then < 450.0 && (cx - x).hypot(cy - y) < 8.0 && before == what);
        last_press.set_value((!double).then(|| (now, cx, cy, what.clone())));
        if double && tool.get_untracked() == Tool::Move {
            grab.set_value(None);
            if let Some(index) = held_card {
                let key = cards.with_untracked(|c| c[index].key());
                on_open_note(key);
            } else if let Some(id) = held_mark {
                let is_note = marks.with_untracked(|m| m.iter().any(|mark| mark.id() == id && matches!(mark, Mark::Note { .. })));
                if is_note {
                    start_writing(id);
                }
            } else if let Some(id) = held_zone {
                start_naming(id);
            } else if what == "cork" {
                chosen.set(None);
                pending.set(Some(Pending::Card { at }));
                focus_soon("board-pending");
            }
            return;
        }

        let pan = Grab::Board { from_x: cx, from_y: cy, x: view_at.x.get_untracked(), y: view_at.y.get_untracked() };
        let new = match tool.get_untracked() {
            Tool::Move => {
                if let Some(id) = held_mark {
                    chosen.set(Some(Chosen::Mark(id.clone())));
                    let was = marks.with_untracked(|m| m.iter().find(|mark| mark.id() == id).cloned());
                    was.map(|was| Grab::Mark { start: at, was, moved: false })
                } else if let Some(index) = held_card {
                    let card_at = cards.with_untracked(|c| c[index].at);
                    Some(Grab::Card { index, dx: card_at.0 - at.0, dy: card_at.1 - at.1, moved: false })
                } else if let Some(was) = held_corner.and_then(|id| zone_of(&id)) {
                    Some(Grab::ZoneSize { start: at, was })
                } else if let Some(was) = held_zone.and_then(|id| zone_of(&id)) {
                    chosen.set(Some(Chosen::Zone(was.id.clone())));
                    Some(Grab::Zone { start: at, was, moved: false })
                } else if let Some(id) = held_string {
                    chosen.set(Some(Chosen::Relationship(id)));
                    None
                } else {
                    chosen.set(None);
                    Some(pan)
                }
            }
            Tool::Tie => match held_card {
                Some(i) => {
                    chosen.set(None);
                    Some(Grab::Tie { from: cards.with_untracked(|c| c[i].id.clone()) })
                }
                // Off a card there's nothing to tie, so the cork still pans.
                None => Some(pan),
            },
            Tool::Zone => {
                chosen.set(None);
                Some(Grab::NewZone { start: at })
            }
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
                        let id = new_id("mk");
                        marks.update(|m| {
                            m.push(Mark::Note {
                                id: id.clone(),
                                text: String::new(),
                                at: spot,
                                turn: tilt(3.0),
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
                chosen.set(None);
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
            Some(Grab::Tie { from }) => {
                let pin = cards.with_untracked(|c| c.iter().find(|c| c.id == from).map(pin_of));
                tying.set(pin.map(|pin| (pin, at)));
            }
            Some(Grab::NewZone { start }) => new_zone.set(Some(zone_between(start, at))),
            Some(Grab::Zone { start, was, .. }) => {
                let spot = (was.at.0 + at.0 - start.0, was.at.1 + at.1 - start.1);
                zones.update(|z| {
                    if let Some(zone) = z.iter_mut().find(|zone| zone.id == was.id) {
                        zone.at = spot;
                    }
                });
                grab.set_value(Some(Grab::Zone { start, was, moved: true }));
            }
            Some(Grab::ZoneSize { start, was }) => {
                // The corner is what's dragged, so the opposite corner stays put.
                let size = ((was.size.0 + at.0 - start.0).max(MIN_ZONE), (was.size.1 + at.1 - start.1).max(MIN_ZONE));
                let spot = (was.at.0 + (size.0 - was.size.0) / 2.0, was.at.1 + (size.1 - was.size.1) / 2.0);
                zones.update(|z| {
                    if let Some(zone) = z.iter_mut().find(|zone| zone.id == was.id) {
                        (zone.at, zone.size) = (spot, size);
                    }
                });
            }
            None => {}
        }
    };

    // A cancelled pointer is not a click: let go of the card where it is, and open nothing.
    let on_cancel = move |_: web_sys::PointerEvent| {
        if let Some(Grab::Card { moved: true, .. } | Grab::Mark { moved: true, .. } | Grab::Zone { moved: true, .. } | Grab::ZoneSize { .. }) =
            grab.get_value()
        {
            save();
        }
        drawing.set(None);
        tying.set(None);
        new_zone.set(None);
        grab.set_value(None);
    };

    let on_up = move |ev: web_sys::PointerEvent| {
        let at = to_board(ev.client_x() as f64, ev.client_y() as f64);
        match grab.get_value() {
            // A card that was only clicked, not dragged, is chosen, which opens it in the panel.
            Some(Grab::Card { index, moved, .. }) => {
                if moved {
                    save();
                } else {
                    let id = cards.with_untracked(|c| c[index].id.clone());
                    chosen.set(Some(Chosen::Card(id)));
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
                    marks.update(|m| m.push(Mark::Ring { id: new_id("mk"), at, size, turn }));
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
                    marks.update(|m| m.push(Mark::Arrow { id: new_id("mk"), from, to, bend }));
                    save();
                }
            }
            Some(Grab::Tie { from }) => {
                tying.set(None);
                // Let go anywhere but on another card, and the string drops: nothing is tied.
                let to = cards.with_untracked(|c| card_at(c, at).map(|i| c[i].id.clone()));
                if let Some(to) = to.filter(|to| *to != from) {
                    pending.set(Some(Pending::Tie { from, to }));
                    focus_soon("board-pending");
                }
            }
            Some(Grab::NewZone { start }) => {
                new_zone.set(None);
                let zone = zone_between(start, at);
                // A click with the tool isn't a zone.
                if zone.size.0 >= MIN_ZONE / 2.0 && zone.size.1 >= MIN_ZONE / 2.0 {
                    let size = (zone.size.0.max(MIN_ZONE), zone.size.1.max(MIN_ZONE));
                    let zone = Zone { id: new_id("zn"), size, turn: tilt(1.0), ..zone };
                    let id = zone.id.clone();
                    zones.update(|z| z.push(zone));
                    save();
                    start_naming(id);
                }
            }
            Some(Grab::Zone { moved: true, .. } | Grab::ZoneSize { .. }) => save(),
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

    // Delete or Backspace rubs out the chosen mark or takes down the chosen zone; Escape puts
    // the tool down and lets go of whatever was chosen.
    let keys = window_event_listener(leptos::ev::keydown, move |ev| {
        // Backspace in the search box, or anywhere else you're typing, is typing.
        let typing = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()).is_some_and(|el| {
            matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT") || el.is_content_editable()
        });
        if typing || editing.get_untracked().is_some() {
            return;
        }
        match ev.key().as_str() {
            "Delete" | "Backspace" => match chosen.get_untracked() {
                Some(Chosen::Mark(id)) => {
                    ev.prevent_default();
                    rub_out(id);
                }
                Some(Chosen::Zone(id)) => {
                    ev.prevent_default();
                    take_down_zone(id);
                }
                // A card or a string is a note, which isn't something to lose to a stray key.
                _ => {}
            },
            "Escape" => {
                chosen.try_set(None);
                pending.try_set(None);
                popover.try_set(None);
                tool.try_set(Tool::Move);
            }
            _ => {}
        }
    });
    on_cleanup(move || keys.remove());

    // Which cards the filters leave showing, by id.
    let shown = Memo::new(move |_| {
        let f = filters.get();
        let near_thread: Option<HashSet<String>> = f.thread.as_ref().map(|thread| {
            let mut near = HashSet::from([thread.clone()]);
            board.with(|b| {
                let Some(b) = b else { return };
                let pairs = b.links.iter().map(|l| (&l.from, &l.to)).chain(b.relationships.iter().map(|r| (&r.from, &r.to)));
                for (a, z) in pairs {
                    if a == thread {
                        near.insert(z.clone());
                    } else if z == thread {
                        near.insert(a.clone());
                    }
                }
            });
            near
        });
        cards.with(|cards| {
            cards
                .iter()
                .filter(|c| !f.hidden.contains(&bucket(&c.kind)) && near_thread.as_ref().is_none_or(|near| near.contains(&c.id)))
                .map(|c| c.id.clone())
                .collect::<HashSet<String>>()
        })
    });

    // Every label strung on the board so far, to offer while a new one is typed.
    let labels_used = Signal::derive(move || {
        let mut all: Vec<String> = board.with(|b| {
            b.iter()
                .flat_map(|b| &b.relationships)
                .flat_map(|r| std::iter::once(r.label.clone()).chain(r.changes.iter().map(|c| c.label.clone())))
                .map(|l| l.trim().to_owned())
                .filter(|l| !l.is_empty())
                .collect()
        });
        all.sort_by_key(|l| l.to_lowercase());
        all.dedup_by_key(|l| l.to_lowercase());
        all
    });

    let chosen_string = move || match chosen.get() {
        Some(Chosen::Relationship(id)) => Some(id),
        _ => None,
    };

    // The string, twine and pins, drawn behind the cards. Its box grows with the board, so an
    // empty board doesn't carry a huge one.
    let threads = move || {
        let found = board.get()?;
        let cards = cards.get();
        let shown = shown.get();
        let chosen = chosen_string();
        let spot = |id: &String| cards.iter().find(|c| c.id == *id && shown.contains(id)).map(pin_of);
        let (minx, miny, maxx, maxy) = bounds(&cards)?;
        let (w, h) = (maxx - minx, maxy - miny);

        let twine: Vec<_> = if filters.with(|f| f.no_twine) {
            Vec::new()
        } else {
            found
                .links
                .iter()
                .filter_map(|l| Some((spot(&l.from)?, spot(&l.to)?)))
                .map(|(a, b)| view! { <path class="twine" d=sag(a, b)></path> })
                .collect()
        };
        let strings: Vec<_> = found
            .relationships
            .iter()
            .filter_map(|r| Some((r, spot(&r.from)?, spot(&r.to)?)))
            .map(|(r, a, b)| {
                let d = sag(a, b);
                let class = match (chosen.as_deref() == Some(r.id.as_str()), r.ends.is_some()) {
                    (true, true) => "string chosen ended",
                    (true, false) => "string chosen",
                    (false, true) => "string ended",
                    (false, false) => "string",
                };
                view! {
                    <path class="string-shadow" d=d.clone()></path>
                    <path class=class d=d.clone()></path>
                    // A wide invisible line over it, so a string can be clicked without aiming.
                    <path class="string-hit" data-rel=r.id.clone() d=d></path>
                }
            })
            .collect();
        // A string being pulled, and one let go on a card that's waiting for its label.
        let pulled = tying.get().map(|(a, b)| view! { <path class="string pulling" d=sag(a, b)></path> });
        let waiting = match pending.get() {
            Some(Pending::Tie { from, to }) => {
                Some((spot(&from)?, spot(&to)?)).map(|(a, b)| view! { <path class="string pulling" d=sag(a, b)></path> })
            }
            _ => None,
        };
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
                {pulled}
                {waiting}
            </svg>
        })
    };

    // The pins, over the cards: a pin holds its card to the board, so it can't be behind it.
    let pins = move || {
        let cards = cards.get();
        let shown = shown.get();
        let (minx, miny, maxx, maxy) = bounds(&cards)?;
        let (w, h) = (maxx - minx, maxy - miny);
        let heads: Vec<_> = cards
            .iter()
            .filter(|card| shown.contains(&card.id))
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

    // A relationship's label, on a strip of masking tape halfway along its string. It says how
    // things stand at the end of the book; the panel has what came before.
    let labels = move || {
        let found = board.get()?;
        let cards = cards.get();
        let shown = shown.get();
        let chosen = chosen_string();
        let spot = |id: &String| cards.iter().find(|c| c.id == *id && shown.contains(id)).map(pin_of);
        Some(
            found
                .relationships
                .iter()
                .filter(|r| !r.label_now().is_empty())
                .filter_map(|r| {
                    let (x, y) = middle_of_string(spot(&r.from)?, spot(&r.to)?);
                    let text = if r.directed { format!("{} \u{2192}", r.label_now()) } else { r.label_now().to_owned() };
                    let class = match (chosen.as_deref() == Some(r.id.as_str()), r.ends.is_some()) {
                        (true, true) => "tape chosen ended",
                        (true, false) => "tape chosen",
                        (false, true) => "tape ended",
                        (false, false) => "tape",
                    };
                    Some(view! {
                        <div class=class data-rel=r.id.clone() style:left=format!("{x}px") style:top=format!("{y}px")>
                            {text}
                        </div>
                    })
                })
                .collect_view(),
        )
    };

    let card_views = move || {
        let shown = shown.get();
        let chosen_card = match chosen.get() {
            Some(Chosen::Card(id)) => Some(id),
            _ => None,
        };
        cards
            .get()
            .into_iter()
            .enumerate()
            .filter(|(_, card)| shown.contains(&card.id))
            .map(|(i, card)| {
                let paper = Paper::of(&card.kind);
                // A thread is an argument in a nonfiction project, so the folder's tab says so.
                let tab = notes::kind_label(&card.kind, kind.get()).to_lowercase();
                let (w, h) = paper.size();
                let (x, y) = card.at;
                let from = card.from.clone();
                let class = if chosen_card.as_deref() == Some(card.id.as_str()) {
                    format!("{} chosen", paper.class())
                } else {
                    paper.class().to_owned()
                };
                view! {
                    <div
                        class=class
                        data-card=i
                        data-kind=card.kind.clone()
                        style:left=format!("{}px", x - w / 2.0)
                        style:top=format!("{}px", y - h / 2.0)
                        style:width=format!("{w}px")
                        style:height=format!("{h}px")
                        style:transform=format!("rotate({}deg)", card.turn)
                        title=card.title.clone()
                        data-tab=tab
                    >
                        <span class="card-mark" aria-hidden="true">
                            <Icon glyph=paper.glyph(&card.kind) size=if paper == Paper::Polaroid { 40 } else { 18 } />
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

    // Zones, under everything else: sheets of kraft paper with a strip of tape for a name. The
    // tape is the handle (drag it to move the sheet, double-click it to rename); pressing the
    // paper itself pans the board, so a board covered in zones can still be moved around.
    let zone_views = move || {
        let now_naming = naming.get();
        let chosen_zone = match chosen.get() {
            Some(Chosen::Zone(id)) => Some(id),
            _ => None,
        };
        zones
            .get()
            .into_iter()
            .chain(new_zone.get())
            .map(|zone| {
                let (w, h) = zone.size;
                let place = format!(
                    "left: {:.1}px; top: {:.1}px; width: {w:.1}px; height: {h:.1}px; transform: rotate({:.1}deg)",
                    zone.at.0 - w / 2.0,
                    zone.at.1 - h / 2.0,
                    zone.turn
                );
                let is_chosen = chosen_zone.as_deref() == Some(zone.id.as_str());
                let class = match (zone.id.is_empty(), is_chosen) {
                    (true, _) => "zone preview",
                    (false, true) => "zone chosen",
                    (false, false) => "zone",
                };
                let id = zone.id.clone();
                let tape = if now_naming.as_deref() == Some(zone.id.as_str()) {
                    view! {
                        <TextInput
                            id="zone-naming"
                            class="zone-tape"
                            initial=zone.name.clone()
                            placeholder="Name this zone"
                            on_done=finish_naming
                        />
                    }
                    .into_any()
                } else {
                    view! {
                        <span class="zone-tape" class:unnamed=zone.name.is_empty() data-zone=id.clone() title="Drag to move it; double-click to rename it">
                            {zone.name.clone()}
                        </span>
                    }
                    .into_any()
                };
                view! {
                    <div class=class style=place>
                        {tape}
                        {is_chosen.then(|| view! { <span class="zone-size" data-zone-size=id.clone() title="Drag to resize"></span> })}
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
        let chosen = match chosen.get() {
            Some(Chosen::Mark(id)) => Some(id),
            _ => None,
        };
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
        let shown = shown.get();
        let chosen = match chosen.get() {
            Some(Chosen::Mark(id)) => Some(id),
            _ => None,
        };
        let now_editing = editing.get();
        marks
            .get()
            .into_iter()
            .filter_map(|mark| {
                let Mark::Note { id, text, turn, on, .. } = &mark else { return None };
                // A note on a card the filters have hidden goes with it.
                if on.as_ref().is_some_and(|card| cards.iter().any(|c| c.id == *card) && !shown.contains(card)) {
                    return None;
                }
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
                Some(
                    view! {
                        <div class=class data-mark=id.clone() style=place>
                            {text.clone()}
                        </div>
                    }
                    .into_any(),
                )
            })
            .collect_view()
    };

    // Words waiting to be typed for something just put on the board: a string's label on its
    // tape, or a new plot point's title on a blank index card.
    let pending_view = move || {
        let waiting = pending.get()?;
        Some(match waiting {
            Pending::Tie { from, to } => {
                let pins = cards.with_untracked(|c| {
                    let pin = |id: &str| c.iter().find(|card| card.id == id).map(pin_of);
                    Some((pin(&from)?, pin(&to)?))
                });
                let (x, y) = pins.map_or((0.0, 0.0), |(a, b)| middle_of_string(a, b));
                let done = Callback::new(move |label: Option<String>| match label {
                    Some(label) => tie(from.clone(), to.clone(), label),
                    None => pending.set(None),
                });
                view! {
                    <div class="pending-tie" style:left=format!("{x}px") style:top=format!("{y}px")>
                        <TextInput
                            id="board-pending"
                            class="tape-input"
                            initial=String::new()
                            placeholder="What is it? e.g. owes"
                            suggestions=labels_used
                            on_done=done
                        />
                    </div>
                }
                .into_any()
            }
            Pending::Card { at } => {
                let (w, h) = Paper::IndexCard.size();
                let placeholder = format!("A new {}", notes::kind_label("event", kind.get_untracked()).to_lowercase());
                let done = Callback::new(move |title: Option<String>| match title {
                    Some(title) => add_plot_point(at, title),
                    None => pending.set(None),
                });
                view! {
                    <div
                        class="card index-card pending-card"
                        style:left=format!("{}px", at.0 - w / 2.0)
                        style:top=format!("{}px", at.1 - h / 2.0)
                        style:width=format!("{w}px")
                        style:height=format!("{h}px")
                    >
                        <span class="card-mark" aria-hidden="true">
                            <Icon glyph=Glyph::Flag size=18 />
                        </span>
                        <TextInput
                            id="board-pending"
                            class="card-title-input"
                            initial=String::new()
                            placeholder=placeholder
                            on_done=done
                        />
                    </div>
                }
                .into_any()
            }
        })
    };

    // The side panel, for whatever card or string is chosen.
    let panel = move || {
        let picked = chosen.get()?;
        let found = board.get()?;
        let title_of = |id: &str| cards.with_untracked(|c| c.iter().find(|card| card.id == id).map(|card| card.title.clone())).unwrap_or_default();
        let project_kind = kind.get_untracked();
        match picked {
            Chosen::Card(id) => {
                let card = cards.with_untracked(|c| c.iter().find(|card| card.id == id).cloned())?;
                let strings: Vec<RelationshipView> = found.relationships.iter().filter(|r| r.from == id || r.to == id).cloned().collect();
                let what = match card.from.as_str() {
                    "" => notes::kind_label(&card.kind, project_kind).to_owned(),
                    from => format!("{} · from {from}", notes::kind_label(&card.kind, project_kind)),
                };
                let key = card.key();
                let pinned = card.pinned;
                let card_id = card.id.clone();
                let list = strings
                    .into_iter()
                    .map(|r| {
                        let outgoing = r.from == id;
                        let other = title_of(if outgoing { &r.to } else { &r.from });
                        let arrow = match (r.directed, outgoing) {
                            (false, _) => "\u{2194}",
                            (true, true) => "\u{2192}",
                            (true, false) => "\u{2190}",
                        };
                        let label = match r.label_now() {
                            "" => "string".to_owned(),
                            label => label.to_owned(),
                        };
                        let rel = r.id.clone();
                        view! {
                            <li>
                                <button class="panel-row" on:click=move |_| chosen.set(Some(Chosen::Relationship(rel.clone())))>
                                    <span class="panel-string-label">{label}</span>
                                    <span class="panel-arrow">{arrow}</span>
                                    <span>{other}</span>
                                </button>
                            </li>
                        }
                    })
                    .collect_view();
                let has_strings = found.relationships.iter().any(|r| r.from == id || r.to == id);
                Some(
                    view! {
                        <p class="panel-kind">{what}</p>
                        <h3 class="panel-title">{card.title.clone()}</h3>
                        <div class="panel-actions">
                            <button class="small" on:click=move |_| on_open_note(key.clone())>"Open"</button>
                            {pinned.then(|| view! {
                                <button class="small quiet" title="The note itself stays" on:click=move |_| take_down_card(card_id.clone())>
                                    "Take down"
                                </button>
                            })}
                        </div>
                        <p class="panel-heading">"Strings"</p>
                        {if has_strings {
                            view! { <ul class="panel-list">{list}</ul> }.into_any()
                        } else {
                            view! { <p class="panel-hint">"None yet. With String, drag from this card to another."</p> }.into_any()
                        }}
                    }
                    .into_any(),
                )
            }
            Chosen::Relationship(id) => {
                let r = found.relationships.iter().find(|r| r.id == id).cloned()?;
                let (from, to) = (title_of(&r.from), title_of(&r.to));
                let key = r.key();
                let label_was = r.label.clone();
                let (relabel_key, round_key, open_key, cut_key) = (key.clone(), key.clone(), key.clone(), key.clone());
                let (one_way_key, both_key) = (key.clone(), key);
                let directed = r.directed;
                let along = r.begins.is_some() || !r.changes.is_empty() || r.ends.is_some();
                let start_label = if r.changes.is_empty() { "Label" } else { "Label at the start" };
                let history = along.then(|| {
                    let start = r.begins.as_ref().map_or("From the start".to_owned(), |b| format!("From {b}"));
                    let first = if r.label.is_empty() { "no label".to_owned() } else { r.label.clone() };
                    let changes = r
                        .changes
                        .iter()
                        .map(|c| {
                            let label = if c.label.is_empty() { "no label".to_owned() } else { c.label.clone() };
                            view! { <li><span class="panel-when">{format!("At {}", c.at)}</span>{label}</li> }
                        })
                        .collect_view();
                    let ends = r.ends.clone().map(|e| view! { <li><span class="panel-when">{format!("Ends at {e}")}</span></li> });
                    view! {
                        <p class="panel-heading">"Along the way"</p>
                        <ol class="panel-history">
                            <li><span class="panel-when">{start}</span>{first}</li>
                            {changes}
                            {ends}
                        </ol>
                    }
                });
                let relabel = Callback::new(move |label: Option<String>| {
                    if let Some(label) = label.filter(|l| l.trim() != label_was) {
                        edit_string(relabel_key.clone(), RelationshipEdit { label: Some(label), ..Default::default() });
                    }
                });
                let one_way = move |_| {
                    if !directed {
                        edit_string(one_way_key.clone(), RelationshipEdit { directed: Some(true), ..Default::default() });
                    }
                };
                let both_ways = move |_| {
                    if directed {
                        edit_string(both_key.clone(), RelationshipEdit { directed: Some(false), ..Default::default() });
                    }
                };
                Some(
                    view! {
                        <p class="panel-kind">"Relationship"</p>
                        <h3 class="panel-title">
                            {from}
                            <span class="panel-arrow">{if directed { " \u{2192} " } else { " \u{2194} " }}</span>
                            {to}
                        </h3>
                        <label class="panel-field">
                            <span class="panel-heading">{start_label}</span>
                            <TextInput
                                id="string-label"
                                class="panel-input"
                                initial=r.label.clone()
                                placeholder="e.g. owes"
                                suggestions=labels_used
                                on_done=relabel
                            />
                        </label>
                        <div class="panel-actions" role="group" aria-label="Which way it reads">
                            <button
                                class="small"
                                class:active=directed
                                aria-pressed=directed.to_string()
                                on:click=one_way
                            >
                                "One way"
                            </button>
                            <button
                                class="small"
                                class:active=!directed
                                aria-pressed=(!directed).to_string()
                                on:click=both_ways
                            >
                                "Both ways"
                            </button>
                            {directed.then(|| view! {
                                <button
                                    class="small quiet"
                                    title="Read it the other way"
                                    on:click=move |_| edit_string(round_key.clone(), RelationshipEdit { reverse: true, ..Default::default() })
                                >
                                    "Turn round"
                                </button>
                            })}
                        </div>
                        {history}
                        <div class="panel-actions panel-foot">
                            <button class="small" on:click=move |_| on_open_note(open_key.clone())>"Open note"</button>
                            <button class="small quiet" on:click=move |_| cut_string(cut_key.clone())>"Move to the cut bin"</button>
                        </div>
                    }
                    .into_any(),
                )
            }
            _ => None,
        }
        .map(|inside| {
            view! {
                <aside class="board-panel" on:pointerdown=|ev| ev.stop_propagation() on:wheel=|ev| ev.stop_propagation()>
                    <button class="board-panel-close quiet" title="Close" on:click=move |_| chosen.set(None)>
                        <Icon glyph=Glyph::Close size=16 />
                    </button>
                    {inside}
                </aside>
            }
        })
    };

    // Which kinds of card are on the board at all, so the Show list only offers those.
    let kinds_up = move || {
        let mut up: Vec<&'static str> = cards.with(|c| c.iter().map(|card| bucket(&card.kind)).collect());
        up.sort_by_key(|b| BUCKETS.iter().position(|x| x == b));
        up.dedup();
        up
    };

    let show_filter = move || {
        (popover.get() == Some(Popover::Show)).then(|| {
            let project_kind = kind.get_untracked();
            let kinds = kinds_up()
                .into_iter()
                .map(|b| {
                    let name = NoteKind::parse(b).map_or("Other notes", |k| k.plural(project_kind));
                    view! {
                        <label class="popover-check">
                            <input
                                type="checkbox"
                                prop:checked=move || filters.with(|f| !f.hidden.contains(&b))
                                on:change=move |_| filters.update(|f| {
                                    if let Some(i) = f.hidden.iter().position(|h| *h == b) {
                                        f.hidden.remove(i);
                                    } else {
                                        f.hidden.push(b);
                                    }
                                })
                            />
                            {name}
                        </label>
                    }
                })
                .collect_view();
            let threads: Vec<(String, String)> =
                cards.with_untracked(|c| c.iter().filter(|card| card.kind == "thread").map(|card| (card.id.clone(), card.title.clone())).collect());
            let thread_word = NoteKind::Thread.label(project_kind);
            view! {
                <div class="board-popover" on:pointerdown=|ev| ev.stop_propagation() on:wheel=|ev| ev.stop_propagation()>
                    <p class="panel-heading">"Show"</p>
                    {kinds}
                    <label class="popover-check">
                        <input
                            type="checkbox"
                            prop:checked=move || filters.with(|f| !f.no_twine)
                            on:change=move |_| filters.update(|f| f.no_twine = !f.no_twine)
                        />
                        "Automatic links"
                    </label>
                    {(!threads.is_empty()).then(|| view! {
                        <label class="popover-field">
                            <span class="panel-heading">{format!("Only one {}", thread_word.to_lowercase())}</span>
                            <select on:change=move |ev| {
                                let value = event_target_value(&ev);
                                filters.update(|f| f.thread = (!value.is_empty()).then_some(value));
                            }>
                                <option value="" selected=move || filters.with(|f| f.thread.is_none())>"Everything"</option>
                                {threads.into_iter().map(|(id, title)| {
                                    let this = id.clone();
                                    view! {
                                        <option value=id selected=move || filters.with(|f| f.thread.as_deref() == Some(this.as_str()))>{title}</option>
                                    }
                                }).collect_view()}
                            </select>
                        </label>
                    })}
                </div>
            }
        })
    };

    let pin_picker = move || {
        (popover.get() == Some(Popover::PinUp)).then(|| {
            let up: HashSet<String> = cards.with_untracked(|c| c.iter().map(|card| card.id.clone()).collect());
            view! { <PinPicker project=project.get_untracked() up=up kind=kind.get_untracked() on_pick=pin_up /> }
        })
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
                    chosen.set(None);
                    pending.set(None);
                    popover.set(None);
                }
            >
                {label}
            </button>
        }
    };

    let popover_button = move |which: Popover, label: &'static str, hint: &'static str, on: Signal<bool>| {
        view! {
            <button
                class="small"
                class:active=move || popover.get() == Some(which) || on.get()
                title=hint
                aria-expanded=move || (popover.get() == Some(which)).to_string()
                on:click=move |_| popover.update(|p| *p = if *p == Some(which) { None } else { Some(which) })
            >
                {label}
            </button>
        }
    };

    let filtering = Signal::derive(move || filters.with(|f| *f != Filters::default()));

    let board_class = move || match tool.get() {
        Tool::Move => "board",
        Tool::Tie => "board tool-tie",
        Tool::Write => "board tool-write",
        Tool::Marker => "board tool-marker",
        Tool::Zone => "board tool-zone",
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
                        let event = notes::kind_label("event", k).to_lowercase();
                        format!(
                            "Nothing is pinned up yet. Make a character, place, {event} or {}, and it appears here, or double-click the cork to add a {event}.",
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
                {zone_views}
                {threads}
                {card_views}
                {pins}
                {labels}
                {strokes}
                {written}
                {pending_view}
            </div>
            // The tools sit over the cork, outside the layer, so they don't pan or zoom away.
            <div class="board-tools" on:pointerdown=|ev| ev.stop_propagation()>
                {tool_button(Tool::Move, "Move", "Move cards and marks; click a card or string to see it, double-click to open it")}
                {tool_button(Tool::Tie, "String", "Drag from one card to another to tie them together")}
                {tool_button(Tool::Write, "Write", "Click to write a note; on a card, it moves with the card")}
                {tool_button(Tool::Marker, "Marker", "Drag around cards to circle them, or from a card to point")}
                {tool_button(Tool::Zone, "Zone", "Drag out a sheet of paper to group cards on, then name it")}
                <span class="board-tools-gap"></span>
                {popover_button(Popover::PinUp, "Pin up", "Pin up a note that isn't on the board", Signal::derive(|| false))}
                {popover_button(Popover::Show, "Show", "Choose what the board shows", filtering)}
                {move || match chosen.get() {
                    Some(Chosen::Mark(id)) => Some(view! {
                        <button class="small quiet" title="Or press Delete" on:click=move |_| rub_out(id.clone())>"Rub out"</button>
                    }.into_any()),
                    Some(Chosen::Zone(id)) => Some(view! {
                        <button class="small quiet" title="Or press Delete. The cards on it stay" on:click=move |_| take_down_zone(id.clone())>
                            "Take down"
                        </button>
                    }.into_any()),
                    _ => None,
                }}
            </div>
            {pin_picker}
            {show_filter}
            {panel}
        </div>
    }
}

/// A one-line text box for words put on the board: a string's label, a zone's name, a new
/// card's title. Enter keeps what's typed, Escape gives up, and clicking away keeps it too.
/// With `suggestions`, it offers matching ones from the list as you type.
#[component]
fn TextInput(
    id: &'static str,
    class: &'static str,
    initial: String,
    #[prop(into)] placeholder: String,
    #[prop(optional, into)] suggestions: Option<Signal<Vec<String>>>,
    #[prop(into)] on_done: Callback<Option<String>>,
) -> impl IntoView {
    let text = RwSignal::new(initial);
    // Enter or Escape finishes, and then the box loses focus, which would finish it again.
    // Finishing usually takes the box away, so by the time that blur comes this may be gone:
    // a box that's gone has finished.
    let finished = StoredValue::new(false);
    let finish = move |value: Option<String>| {
        if finished.try_get_value() == Some(false) {
            finished.set_value(true);
            on_done.run(value);
        }
    };
    let offered = move || {
        let typed = text.get().trim().to_lowercase();
        suggestions
            .map(|s| s.get())
            .unwrap_or_default()
            .into_iter()
            .filter(|l| {
                let l = l.to_lowercase();
                l != typed && l.contains(&typed)
            })
            .take(6)
            .collect::<Vec<_>>()
    };
    view! {
        <span class="label-input">
            <input
                id=id
                class=format!("board-input {class}")
                type="text"
                autocomplete="off"
                spellcheck="false"
                placeholder=placeholder
                prop:value=move || text.get()
                on:input=move |ev| text.set(event_target_value(&ev))
                on:focus=move |_| finished.set_value(false)
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    ev.stop_propagation();
                    match ev.key().as_str() {
                        "Enter" => {
                            ev.prevent_default();
                            finish(Some(text.get_untracked()));
                        }
                        "Escape" => {
                            ev.prevent_default();
                            finish(None);
                        }
                        _ => {}
                    }
                }
                on:blur=move |_| {
                    if let Some(typed) = text.try_get_untracked() {
                        finish(Some(typed));
                    }
                }
            />
            <Show when=move || !offered().is_empty()>
                <span class="label-suggestions">
                    <For each=offered key=|l| l.clone() let:label>
                        {
                            let pick = label.clone();
                            view! {
                                // Taken on the press, before the box loses focus to the click.
                                <button
                                    class="small quiet"
                                    on:pointerdown=move |ev| {
                                        ev.prevent_default();
                                        ev.stop_propagation();
                                        text.set(pick.clone());
                                        finish(Some(pick.clone()));
                                    }
                                >
                                    {label}
                                </button>
                            }
                        }
                    </For>
                </span>
            </Show>
        </span>
    }
}

/// The list of notes that could be pinned up: anything reachable from this project that isn't
/// on the board already and isn't a relationship (those are strings, not cards).
#[component]
fn PinPicker(project: String, up: HashSet<String>, kind: ProjectKind, on_pick: impl Fn(String) + Copy + Send + Sync + 'static) -> impl IntoView {
    let notes_found = RwSignal::new(None::<Result<Vec<NoteView>, String>>);
    let query = RwSignal::new(String::new());
    let here = project.clone();
    spawn_local(async move {
        let found = tauri::project_notes(&project).await.map(|n| n.notes);
        notes_found.try_set(Some(found));
    });
    focus_soon("pin-query");
    let rows = move || {
        let q = query.get().trim().to_lowercase();
        let found = notes_found.get()?;
        Some(match found {
            Err(e) => view! { <p class="error">{e}</p> }.into_any(),
            Ok(found) => {
                let mut fits: Vec<NoteView> = found
                    .into_iter()
                    .filter(|n| n.kind != "relationship" && !up.contains(&n.id))
                    .filter(|n| q.is_empty() || n.title.to_lowercase().contains(&q) || n.aliases.iter().any(|a| a.to_lowercase().contains(&q)))
                    .collect();
                fits.sort_by_key(|n| n.title.to_lowercase());
                if fits.is_empty() {
                    let hint = if q.is_empty() { "Everything is up already." } else { "Nothing by that name that isn't up already." };
                    return Some(view! { <p class="panel-hint">{hint}</p> }.into_any());
                }
                let rows = fits
                    .into_iter()
                    .take(40)
                    .map(|n| {
                        let from = if !n.world && n.owner == here { String::new() } else { format!(" · {}", n.owner) };
                        let what = format!("{}{from}", notes::kind_label(&n.kind, kind));
                        let id = n.id.clone();
                        view! {
                            <li>
                                <button class="panel-row" on:click=move |_| on_pick(id.clone())>
                                    <Icon glyph=notes::kind_glyph(&n.kind) size=15 />
                                    <span>{n.title.clone()}</span>
                                    <span class="panel-when">{what}</span>
                                </button>
                            </li>
                        }
                    })
                    .collect_view();
                view! { <ul class="panel-list">{rows}</ul> }.into_any()
            }
        })
    };
    view! {
        <div class="board-popover pin-picker" on:pointerdown=|ev| ev.stop_propagation() on:wheel=|ev| ev.stop_propagation()>
            <input
                id="pin-query"
                class="panel-input"
                type="search"
                placeholder="Find a note to pin up"
                prop:value=move || query.get()
                on:input=move |ev| query.set(event_target_value(&ev))
                on:keydown=|ev: web_sys::KeyboardEvent| ev.stop_propagation()
            />
            {rows}
        </div>
    }
}

/// A fresh id for a mark (`mk`) or a zone (`zn`). There's no clock or random source of Rust's
/// own in the webview.
fn new_id(prefix: &str) -> String {
    make_id(prefix, (js_sys::Math::random() * (1u64 << 50) as f64) as u64)
}

/// A slight lean, up to `most` degrees either way: a note the way nobody writes quite level,
/// a zone's sheet the way nobody pins one quite straight.
fn tilt(most: f64) -> f64 {
    ((js_sys::Math::random() - 0.5) * 2.0 * most * 10.0).round() / 10.0
}

/// The sheet a zone drag from `a` to `b` lays down: the box between them.
fn zone_between(a: Point, b: Point) -> Zone {
    Zone {
        id: String::new(),
        name: String::new(),
        at: ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
        size: ((b.0 - a.0).abs(), (b.1 - a.1).abs()),
        turn: 0.0,
    }
}

/// The kinds of card the Show list can leave off, in the order it lists them. Everything that
/// isn't one of the four papers is "other".
const BUCKETS: [&str; 5] = ["character", "place", "event", "thread", "other"];

fn bucket(kind: &str) -> &'static str {
    BUCKETS.iter().find(|b| **b == kind).copied().unwrap_or("other")
}

/// Focuses the element with `id` on the next frame, once it's been drawn.
fn focus_soon(id: &'static str) {
    request_animation_frame(move || {
        if let Some(el) = document().get_element_by_id(id).and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok()) {
            let _ = el.focus();
        }
    });
}

/// Halfway along a string, where its tape goes: the middle of the two pins, down by its sag.
fn middle_of_string(a: Point, b: Point) -> Point {
    ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0 + sag_of(a, b))
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

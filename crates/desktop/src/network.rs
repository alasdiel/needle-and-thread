//! The network board (DESIGN §7): a cork board of pinned cards, strung with red thread.
//!
//! Cards are HTML, so they can be paper with a shadow; the string, pins and twine are one SVG
//! behind them. Both sit in the same layer, which carries the pan and zoom as a CSS transform,
//! so the webview can move the whole board without redrawing it (spike 3).

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::icons::{Glyph, Icon};
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

#[derive(Clone, Copy)]
enum Grab {
    Card { index: usize, dx: f64, dy: f64, moved: bool },
    Board { from_x: f64, from_y: f64, x: f64, y: f64 },
}

#[component]
pub fn NetworkBoard(
    /// The project whose board this is.
    #[prop(into)]
    project: Signal<String>,
    on_open_note: impl Fn(NoteKey) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let board = RwSignal::new(None::<BoardView>);
    let board_el = NodeRef::<leptos::html::Div>::new();
    let error = RwSignal::new(None::<String>);
    // A card's spot while it's being dragged. The board's own copy is the truth once the drag
    // ends and the arrangement is saved.
    let cards = RwSignal::new(Vec::<CardView>::new());
    let view_at = Viewport { x: RwSignal::new(120.0), y: RwSignal::new(120.0), zoom: RwSignal::new(1.0) };
    let grab = StoredValue::new(None::<Grab>);

    Effect::new(move |_| {
        let slug = project.get();
        spawn_local(async move {
            match tauri::project_board(&slug).await {
                Ok(found) => {
                    cards.set(found.cards.clone());
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
            marks: found.as_ref().map(|b| b.marks.clone()).unwrap_or_default(),
        };
        spawn_local(async move {
            if let Err(e) = tauri::save_board(&slug, &layout).await {
                error.set(Some(e));
            }
        });
    };

    let to_board = move |cx: f64, cy: f64| {
        let z = view_at.zoom.get_untracked();
        ((cx - view_at.x.get_untracked()) / z, (cy - view_at.y.get_untracked()) / z)
    };

    let on_down = move |ev: web_sys::PointerEvent| {
        if ev.button() != 0 {
            return;
        }
        // Without this the webview starts its own text selection, which cancels the pointer
        // mid-drag and leaves a card stuck to the cursor.
        ev.prevent_default();
        let (cx, cy) = (ev.client_x() as f64, ev.client_y() as f64);
        let held = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|el| el.closest("[data-card]").ok().flatten())
            .and_then(|el| el.get_attribute("data-card"))
            .and_then(|i| i.parse::<usize>().ok());
        let new = match held {
            Some(index) => {
                let (bx, by) = to_board(cx, cy);
                let at = cards.with_untracked(|c| c[index].at);
                Grab::Card { index, dx: at.0 - bx, dy: at.1 - by, moved: false }
            }
            None => Grab::Board { from_x: cx, from_y: cy, x: view_at.x.get_untracked(), y: view_at.y.get_untracked() },
        };
        grab.set_value(Some(new));
        if let Some(el) = ev.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
            let _ = el.set_pointer_capture(ev.pointer_id());
        }
    };

    let on_move = move |ev: web_sys::PointerEvent| {
        let (cx, cy) = (ev.client_x() as f64, ev.client_y() as f64);
        match grab.get_value() {
            Some(Grab::Card { index, dx, dy, .. }) => {
                let (bx, by) = to_board(cx, cy);
                cards.update(|cards| cards[index].at = (bx + dx, by + dy));
                grab.set_value(Some(Grab::Card { index, dx, dy, moved: true }));
            }
            Some(Grab::Board { from_x, from_y, x, y }) => {
                view_at.x.set(x + cx - from_x);
                view_at.y.set(y + cy - from_y);
            }
            None => {}
        }
    };

    // A cancelled pointer is not a click: let go of the card where it is, and open nothing.
    let on_cancel = move |_: web_sys::PointerEvent| {
        if let Some(Grab::Card { moved: true, .. }) = grab.get_value() {
            save();
        }
        grab.set_value(None);
    };

    let on_up = move |_: web_sys::PointerEvent| {
        // A card that was only clicked, not dragged, opens its note instead.
        if let Some(Grab::Card { index, moved, .. }) = grab.get_value() {
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
        grab.set_value(None);
    };

    let on_wheel = move |ev: web_sys::WheelEvent| {
        ev.prevent_default();
        let z = view_at.zoom.get_untracked();
        let next = (z * (-ev.delta_y() * 0.0015).exp()).clamp(0.2, 2.5);
        let scale = next / z;
        let (cx, cy) = (ev.client_x() as f64, ev.client_y() as f64);
        view_at.x.set(cx - (cx - view_at.x.get_untracked()) * scale);
        view_at.y.set(cy - (cy - view_at.y.get_untracked()) * scale);
        view_at.zoom.set(next);
    };

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

    let empty = move || board.with(|b| b.as_ref().is_some_and(|b| b.cards.is_empty()));

    view! {
        <div
            class="board"
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
                    "Nothing is pinned up yet. Make a character, place, plot point or thread, and it appears here."
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
            </div>
        </div>
    }
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

//! Search: the Ctrl+K box over the page and the sidebar's Search tab. Both show one shared
//! query and its results, so a search started in the box can be kept in the sidebar.

use std::time::Duration;

use leptos::{html, prelude::*, task::spawn_local};

use crate::icons::{Glyph, Icon};
use crate::notes::kind_glyph;
use crate::status::{self, StatusMark};
use crate::tauri::{self, HitView, NoteView, SearchQuery};

/// How long typing must pause before searching.
const PAUSE: Duration = Duration::from_millis(120);

#[derive(Clone, Copy)]
pub struct Search {
    pub text: RwSignal<String>,
    /// This project and its world, rather than the whole vault.
    pub this_project: RwSignal<bool>,
    /// "scene", "note" or "cut" (the cut bin).
    pub kind: RwSignal<Option<&'static str>>,
    pub status: RwSignal<Option<String>>,
    /// The title of the POV character or thread to filter by.
    pub pov: RwSignal<Option<String>>,
    pub thread: RwSignal<Option<String>>,
    pub results: RwSignal<Vec<HitView>>,
    pub error: RwSignal<Option<String>>,
    /// The result the keyboard is on, in the box.
    pub selected: RwSignal<usize>,
}

impl Search {
    pub fn new() -> Self {
        Self {
            text: RwSignal::new(String::new()),
            this_project: RwSignal::new(true),
            kind: RwSignal::new(None),
            status: RwSignal::new(None),
            pov: RwSignal::new(None),
            thread: RwSignal::new(None),
            results: RwSignal::new(Vec::new()),
            error: RwSignal::new(None),
            selected: RwSignal::new(0),
        }
    }

    /// The words to highlight in a scene or note opened from the results.
    pub fn terms(&self) -> Vec<String> {
        self.text.get_untracked().split_whitespace().map(str::to_owned).collect()
    }

    /// Searches again whenever the query or project changes, once typing pauses. `notes` gives
    /// each POV character's and thread's other names.
    pub fn watch(self, project: Signal<Option<String>>, notes: Signal<Vec<NoteView>>) {
        let generation = StoredValue::new(0u32);
        Effect::new(move |_| {
            let names = |title: Option<String>| -> Vec<String> {
                let Some(title) = title else { return Vec::new() };
                notes.with_untracked(|all| {
                    let aliases = all.iter().find(|n| n.title == title).map(|n| n.aliases.clone()).unwrap_or_default();
                    std::iter::once(title).chain(aliases).collect()
                })
            };
            let query = SearchQuery {
                text: self.text.get(),
                everywhere: !self.this_project.get(),
                kind: self.kind.get(),
                status: self.status.get(),
                note_type: None,
                pov: names(self.pov.get()),
                thread: names(self.thread.get()),
            };
            let Some(project) = project.get() else { return };
            generation.update_value(|g| *g += 1);
            let mine = generation.get_value();
            let nothing = query.text.trim().is_empty() && query.kind.is_none() && query.status.is_none() && query.pov.is_empty() && query.thread.is_empty();
            if nothing {
                self.results.set(Vec::new());
                self.error.set(None);
                return;
            }
            set_timeout(
                move || {
                    if generation.get_value() != mine {
                        return;
                    }
                    spawn_local(async move {
                        let found = tauri::search(&project, &query).await;
                        // A newer search has started; this answer is stale.
                        if generation.try_get_value() != Some(mine) {
                            return;
                        }
                        match found {
                            Ok(hits) => {
                                self.selected.set(0);
                                self.error.set(None);
                                self.results.set(hits);
                            }
                            Err(e) => self.error.set(Some(e)),
                        }
                    });
                },
                PAUSE,
            );
        });
    }
}

/// A filter chip that toggles.
#[component]
fn Toggle(#[prop(into)] on: Signal<bool>, on_click: impl Fn() + Send + Sync + 'static, children: Children) -> impl IntoView {
    view! {
        <button type="button" class="search-chip" aria-pressed=move || on.get().to_string() class:on=on on:click=move |_| on_click()>
            {children()}
        </button>
    }
}

/// A filter chip that picks one of `choices` (or "any"), through an invisible select over it.
#[component]
fn Pick(
    label: &'static str,
    value: RwSignal<Option<String>>,
    #[prop(into)] choices: Signal<Vec<String>>,
) -> impl IntoView {
    view! {
        <label class="search-chip" class:on=move || value.with(Option::is_some)>
            <span>{move || value.get().unwrap_or_else(|| label.to_owned())}</span>
            <Icon glyph=Glyph::ChevronDown size=12 />
            <select
                aria-label=label
                prop:value=move || value.get().unwrap_or_default()
                on:change=move |ev| {
                    let picked = event_target_value(&ev);
                    value.set((!picked.is_empty()).then_some(picked));
                }
            >
                <option value="">{format!("Any {}", label.to_lowercase())}</option>
                {move || choices.get().into_iter().map(|c| view! { <option value=c.clone()>{c.clone()}</option> }).collect_view()}
            </select>
        </label>
    }
}

#[component]
pub fn SearchFilters(
    search: Search,
    #[prop(into)] statuses: Signal<Vec<String>>,
    #[prop(into)] characters: Signal<Vec<String>>,
    #[prop(into)] threads: Signal<Vec<String>>,
) -> impl IntoView {
    let toggle_kind = move |kind: &'static str| search.kind.update(|k| *k = if *k == Some(kind) { None } else { Some(kind) });
    view! {
        <div class="search-filters">
            <Toggle on=search.this_project on_click=move || search.this_project.update(|v| *v = !*v)>"This project"</Toggle>
            <Toggle on=Signal::derive(move || search.kind.get() == Some("scene")) on_click=move || toggle_kind("scene")>"Scenes"</Toggle>
            <Toggle on=Signal::derive(move || search.kind.get() == Some("note")) on_click=move || toggle_kind("note")>"Notes"</Toggle>
            <Toggle on=Signal::derive(move || search.kind.get() == Some("cut")) on_click=move || toggle_kind("cut")>"Cut bin"</Toggle>
            <Pick label="Status" value=search.status choices=statuses />
            <Pick label="Pov" value=search.pov choices=characters />
            <Pick label="Thread" value=search.thread choices=threads />
        </div>
    }
}

/// Results with their place in the keyboard's order.
type Numbered = Vec<(usize, HitView)>;

/// Scenes, notes, then what's in the cut bin, each in the order search ranked them, with each
/// one's place in that combined order (for the keyboard).
fn grouped(hits: &[HitView]) -> [(&'static str, Numbered); 3] {
    let mut place = 0;
    ["scene", "note", "cut"].map(|kind| {
        let rows: Numbered = hits
            .iter()
            .filter(|h| h.kind == kind)
            .map(|h| {
                place += 1;
                (place - 1, h.clone())
            })
            .collect();
        let name = match kind {
            "scene" => "Scenes",
            "note" => "Notes",
            _ => "Cut bin",
        };
        (name, rows)
    })
}

/// The results in the order the keyboard moves through them.
pub fn in_order(hits: &[HitView]) -> Vec<HitView> {
    grouped(hits).into_iter().flat_map(|(_, rows)| rows).map(|(_, h)| h).collect()
}

/// The results, grouped into scenes and notes.
#[component]
pub fn SearchResults(
    search: Search,
    /// Under each title: a scene's chapter, a note's type.
    subtitle: impl Fn(&HitView) -> String + Copy + Send + Sync + 'static,
    #[prop(into)] statuses: Signal<Vec<String>>,
    /// Marks the result that's open, in the sidebar.
    is_current: impl Fn(&HitView) -> bool + Copy + Send + Sync + 'static,
    /// Marks the keyboard's result, in the box.
    #[prop(default = false)]
    keyboard: bool,
    on_open: impl Fn(HitView) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let row = move |index: usize, hit: HitView| {
        let icon = match hit.kind.as_str() {
            "scene" => {
                let progress = status::progress(hit.status.as_deref().unwrap_or_default(), &statuses.get_untracked());
                view! { <StatusMark progress=progress /> }.into_any()
            }
            "cut" => view! { <Icon glyph=Glyph::Basket size=13 /> }.into_any(),
            _ => view! { <Icon glyph=kind_glyph(hit.note_type.as_deref().unwrap_or("note")) size=13 /> }.into_any(),
        };
        let current = {
            let hit = hit.clone();
            move || is_current(&hit)
        };
        let snippet = hit
            .snippet
            .iter()
            .map(|(text, matched)| if *matched { view! { <mark>{text.clone()}</mark> }.into_any() } else { text.clone().into_any() })
            .collect_view();
        let sub = subtitle(&hit);
        let tooltip = sub.clone();
        view! {
            <button
                type="button"
                class="search-hit"
                title=tooltip
                class:current=current
                class:selected=move || keyboard && search.selected.get() == index
                on:mouseenter=move |_| {
                    if keyboard {
                        search.selected.set(index);
                    }
                }
                on:click=move |_| on_open(hit.clone())
            >
                <span class="search-hit-head">
                    {icon}
                    <span class="search-hit-title">{hit.title.clone()}</span>
                    <span class="search-hit-sub">{sub}</span>
                </span>
                <span class="search-hit-snippet">{snippet}</span>
            </button>
        }
    };

    let group = move |name: &'static str, rows: Numbered| {
        (!rows.is_empty()).then(|| {
            let count = rows.len();
            view! {
                <div class="search-group">
                    <span>{name}</span>
                    <span class="count">{count}</span>
                </div>
                {rows.into_iter().map(|(i, hit)| row(i, hit)).collect_view()}
            }
        })
    };

    view! {
        <div class="search-results">
            {move || search.error.get().map(|e| view! { <p class="error">{e}</p> })}
            {move || {
                let groups = search.results.with(|hits| grouped(hits));
                let none = groups.iter().all(|(_, rows)| rows.is_empty()) && !search.text.with(|t| t.trim().is_empty());
                view! {
                    {groups.into_iter().map(|(name, rows)| group(name, rows)).collect_view()}
                    {none.then(|| view! { <p class="search-none">"Nothing matches."</p> })}
                }
            }}
        </div>
    }
}

/// The Ctrl+K box over the page.
#[component]
pub fn SearchPalette(
    search: Search,
    open: RwSignal<bool>,
    #[prop(into)] statuses: Signal<Vec<String>>,
    #[prop(into)] characters: Signal<Vec<String>>,
    #[prop(into)] threads: Signal<Vec<String>>,
    subtitle: impl Fn(&HitView) -> String + Copy + Send + Sync + 'static,
    on_open: impl Fn(HitView) + Copy + Send + Sync + 'static,
    /// Ctrl+Enter: keep the results in the sidebar.
    on_list: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let input = NodeRef::<html::Input>::new();
    Effect::new(move |_| {
        if let Some(el) = input.get() {
            let _ = el.focus();
            el.select();
        }
    });
    let close = move || open.set(false);
    let keydown = move |ev: leptos::ev::KeyboardEvent| {
        let count = search.results.with(Vec::len);
        match ev.key().as_str() {
            "Escape" => close(),
            "ArrowDown" | "ArrowUp" if count > 0 => {
                ev.prevent_default();
                let step = if ev.key() == "ArrowDown" { 1 } else { count - 1 };
                search.selected.update(|s| *s = (*s + step) % count);
            }
            "Enter" if ev.ctrl_key() || ev.meta_key() => {
                ev.prevent_default();
                close();
                on_list();
            }
            "Enter" => {
                ev.prevent_default();
                let hit = search.results.with(|hits| in_order(hits).get(search.selected.get_untracked()).cloned());
                if let Some(hit) = hit {
                    close();
                    on_open(hit);
                }
            }
            _ => {}
        }
    };
    view! {
        <div class="palette-scrim" on:click=move |_| close()></div>
        <div class="palette" role="dialog" aria-label="Search">
            <div class="palette-field">
                <Icon glyph=Glyph::Search size=20 />
                <input
                    node_ref=input
                    aria-label="Search"
                    placeholder="Search scenes and notes"
                    prop:value=move || search.text.get()
                    on:input=move |ev| search.text.set(event_target_value(&ev))
                    on:keydown=keydown
                />
                <kbd>"Esc"</kbd>
            </div>
            <SearchFilters search=search statuses=statuses characters=characters threads=threads />
            <div class="palette-results">
                <SearchResults
                    search=search
                    subtitle=subtitle
                    statuses=statuses
                    is_current=|_| false
                    keyboard=true
                    on_open=move |hit| {
                        close();
                        on_open(hit);
                    }
                />
            </div>
            <div class="palette-keys">
                <span><b>"↑↓"</b>" choose"</span>
                <span><b>"Enter"</b>" open"</span>
                <span><b>"Ctrl+Enter"</b>" list in the sidebar"</span>
            </div>
        </div>
    }
}

/// The sidebar's Search tab.
#[component]
pub fn SearchSidebar(
    search: Search,
    /// Set to focus the field, e.g. after Ctrl+Enter in the box.
    focus: RwSignal<u32>,
    #[prop(into)] statuses: Signal<Vec<String>>,
    #[prop(into)] characters: Signal<Vec<String>>,
    #[prop(into)] threads: Signal<Vec<String>>,
    subtitle: impl Fn(&HitView) -> String + Copy + Send + Sync + 'static,
    is_current: impl Fn(&HitView) -> bool + Copy + Send + Sync + 'static,
    on_open: impl Fn(HitView) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let input = NodeRef::<html::Input>::new();
    Effect::new(move |_| {
        focus.track();
        if let Some(el) = input.get_untracked() {
            let _ = el.focus();
        }
    });
    view! {
        <div class="search-side">
            <label class="search-field">
                <Icon glyph=Glyph::Search size=16 />
                <input
                    node_ref=input
                    aria-label="Search"
                    placeholder="Search"
                    prop:value=move || search.text.get()
                    on:input=move |ev| search.text.set(event_target_value(&ev))
                />
                <kbd>"Ctrl+K"</kbd>
            </label>
            <SearchFilters search=search statuses=statuses characters=characters threads=threads />
            <nav class="search-list" aria-label="Search results">
                <SearchResults search=search subtitle=subtitle statuses=statuses is_current=is_current on_open=on_open />
            </nav>
        </div>
    }
}

//! The cut bin's panel: what was cut from the project (and notes cut from its world), the most
//! recent first and grouped by day. Each piece is a scrap of the page with its own Restore.

use leptos::prelude::*;
use needle_core::project::ProjectKind;
use needle_core::words::plain_text;
use wasm_bindgen::JsValue;

use crate::history::{clock_of, day_label};
use crate::icons::{Glyph, Icon};
use crate::notes::{kind_glyph, kind_label};
use crate::outline::format_words;
use crate::tauri::CutView;

/// When something was cut, in local time: its day ("Today", "Yesterday", "Oct 2") and time.
fn when(cut_at: &str) -> (String, String) {
    let date = js_sys::Date::new(&JsValue::from_str(cut_at));
    if date.get_time().is_nan() {
        return ("Earlier".to_owned(), String::new());
    }
    (day_label(&date), clock_of(&date))
}

#[component]
pub fn BinPanel(
    #[prop(into)] items: Signal<Vec<CutView>>,
    #[prop(into)] kind: Signal<ProjectKind>,
    /// A scene's title now, if it's still in the project.
    scene_title: impl Fn(&str) -> Option<String> + Copy + Send + Sync + 'static,
    on_restore: impl Fn(CutView) + Copy + Send + Sync + 'static,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    // Days in the order the items come, newest first.
    let days = move || {
        let mut days: Vec<(String, Vec<(CutView, String)>)> = Vec::new();
        for item in items.get() {
            let (day, time) = when(&item.cut_at);
            match days.last_mut() {
                Some((last, list)) if *last == day => list.push((item, time)),
                _ => days.push((day, vec![(item, time)])),
            }
        }
        days
    };

    let piece = move |(item, time): (CutView, String)| {
        let at = |text: String| if time.is_empty() { text } else { format!("{text} · {time}") };
        let words = format_words(item.words);
        let body = match item.kind.as_str() {
            "passage" => {
                let from = item.scene.as_deref().and_then(scene_title).unwrap_or_else(|| item.title.clone());
                let text = item.passage.as_ref().map(|p| plain_text(&p.markdown)).unwrap_or_default();
                view! {
                    <p class="bin-excerpt">{text.trim().to_owned()}</p>
                    <div class="bin-meta">
                        <span>{at(format!("From {from}"))}</span>
                        <span class="bin-words">{words}</span>
                    </div>
                }
                .into_any()
            }
            "note" => {
                let note_kind = item.note_kind.clone().unwrap_or_else(|| "note".to_owned());
                view! {
                    <div class="bin-title">
                        <Icon glyph=kind_glyph(&note_kind) size=15 />
                        <span>{item.title.clone()}</span>
                    </div>
                    <div class="bin-meta">
                        <span>{at(kind_label(&note_kind, kind.get()).to_owned())}</span>
                        <span class="bin-words">{words}</span>
                    </div>
                }
                .into_any()
            }
            _ => {
                let label = match &item.chapter {
                    Some(chapter) => format!("Scene · from {chapter}"),
                    None => "Scene".to_owned(),
                };
                view! {
                    <div class="bin-title">
                        <Icon glyph=Glyph::File size=15 />
                        <span>{item.title.clone()}</span>
                    </div>
                    <div class="bin-meta">
                        <span>{at(label)}</span>
                        <span class="bin-words">{words}</span>
                    </div>
                }
                .into_any()
            }
        };
        view! {
            <li class="bin-piece">
                {body}
                <div class="bin-actions">
                    <button class="small" on:click=move |_| on_restore(item.clone())>
                        <Icon glyph=Glyph::Restore size=14 />
                        "Restore"
                    </button>
                </div>
            </li>
        }
    };

    view! {
        <aside class="side-panel bin-panel">
            <div class="panel-head">
                <span class="panel-icon">
                    <Icon glyph=Glyph::Basket size=17 />
                </span>
                <h2>"Cut bin"</h2>
                <button class="icon-button" title="Close" aria-label="Close the cut bin" on:click=move |_| on_close()>
                    <Icon glyph=Glyph::Close />
                </button>
            </div>
            <div class="panel-body">
                <Show when=move || items.with(Vec::is_empty)>
                    <p class="muted bin-empty">"The bin is empty. Ctrl+Shift+X cuts selected text into it."</p>
                </Show>
                {move || {
                    days()
                        .into_iter()
                        .map(|(day, list)| {
                            view! {
                                <h3 class="bin-day">{day}</h3>
                                <ul class="bin-pieces">{list.into_iter().map(piece).collect_view()}</ul>
                            }
                        })
                        .collect_view()
                }}
            </div>
        </aside>
    }
}

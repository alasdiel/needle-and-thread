//! The desktop's ProseMirror island (editor/src/index.ts) on the phone, loaded as
//! `window.NeedleEditor` before the wasm module starts. It's mounted with `touch`: the
//! keyboard's own spellcheck and autocorrect, no menus over the text, and the formatting bar
//! (`write.rs`) driving it instead of shortcuts.

use std::{cell::RefCell, rc::Rc};

use js_sys::{Array, Object, Reflect};
use leptos::{html, prelude::*};
use needle_core::names::{NameIndex, Resolution};
use needle_core::project::ProjectKind;
use needle_core::settings::TypographySettings;
use needle_vault::NamedNote;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    pub type EditorHandle;

    #[wasm_bindgen(js_namespace = NeedleEditor, js_name = mount)]
    fn mount(el: &web_sys::HtmlElement, markdown: &str, options: &Object) -> EditorHandle;

    /// Expects the object `LinkBridge` builds.
    #[wasm_bindgen(method, js_name = setLinkResolver)]
    pub fn set_link_resolver(this: &EditorHandle, resolver: &Object);

    /// Runs a bar button: "bold", "italic", "heading", "quote", "list", "link" or "sceneBreak".
    #[wasm_bindgen(method)]
    pub fn format(this: &EditorHandle, name: &str) -> bool;

    #[wasm_bindgen(method, js_name = pickSuggestion)]
    pub fn pick_suggestion(this: &EditorHandle, index: u32);

    /// Changes the text to `markdown`, touching only what differs and leaving the cursor where
    /// it was; Undo doesn't take it back. For text merged with another device's.
    #[wasm_bindgen(method, js_name = replaceMarkdown)]
    pub fn replace_markdown(this: &EditorHandle, markdown: &str) -> bool;

    #[wasm_bindgen(method)]
    pub fn focus(this: &EditorHandle);

    /// Lets go of the cursor, which puts the keyboard away.
    #[wasm_bindgen(method)]
    pub fn blur(this: &EditorHandle);

    /// Reports pending changes through `on_change` now instead of after the pause.
    #[wasm_bindgen(method)]
    pub fn flush(this: &EditorHandle);

    #[wasm_bindgen(method)]
    fn destroy(this: &EditorHandle);
}

fn set(target: &Object, key: &str, value: &JsValue) {
    // Only fails for frozen objects and proxies; these are plain fresh objects.
    let _ = Reflect::set(target, &key.into(), value);
}

fn object(fields: &[(&str, JsValue)]) -> JsValue {
    let object = Object::new();
    for (key, value) in fields {
        set(&object, key, value);
    }
    object.into()
}

fn options(typography: TypographySettings) -> Object {
    let rules = Object::new();
    set(&rules, "doubleQuotes", &typography.double_quotes.into());
    set(&rules, "singleQuotes", &typography.single_quotes.into());
    set(&rules, "emDash", &typography.em_dash.into());
    set(&rules, "ellipsis", &typography.ellipsis.into());
    let options = Object::new();
    set(&options, "typography", &rules);
    set(&options, "touch", &true.into());
    options
}

/// Mounts the editor on `markdown` once its element exists and stores it in `handle`.
/// `on_change` receives the Markdown and word count after typing pauses; `on_formats` the
/// formats on at the cursor, for the bar; `on_suggestions` the notes to offer while a link is
/// typed (label and detail), or None when there are none.
#[component]
pub fn Editor(
    handle: StoredValue<Option<EditorHandle>, LocalStorage>,
    markdown: String,
    typography: TypographySettings,
    links: Rc<LinkBridge>,
    on_change: impl Fn(String, u32) + 'static,
    on_formats: impl Fn(Vec<String>) + 'static,
    on_suggestions: impl Fn(Option<Vec<(String, String)>>) + 'static,
) -> impl IntoView {
    let node_ref = NodeRef::<html::Div>::new();
    let on_change = StoredValue::new_local(Closure::<dyn FnMut(String, u32)>::new(on_change));
    let on_formats = StoredValue::new_local(Closure::<dyn FnMut(Array)>::new(move |on: Array| {
        on_formats(on.iter().filter_map(|v| v.as_string()).collect());
    }));
    let on_suggestions = StoredValue::new_local(Closure::<dyn FnMut(JsValue)>::new(move |choices: JsValue| {
        let list = choices.dyn_ref::<Array>().map(|a| {
            a.iter()
                .map(|c| {
                    let get = |key: &str| Reflect::get(&c, &key.into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
                    (get("label"), get("detail"))
                })
                .collect()
        });
        on_suggestions(list);
    }));
    let links = StoredValue::new_local(links);

    Effect::new(move |_| {
        let Some(el) = node_ref.get() else { return };
        if handle.with_value(Option::is_some) {
            return;
        }
        let options = options(typography);
        on_change.with_value(|f| set(&options, "onChange", f.as_ref()));
        on_formats.with_value(|f| set(&options, "onFormats", f.as_ref()));
        on_suggestions.with_value(|f| set(&options, "onSuggestions", f.as_ref()));
        let editor = mount(&el, &markdown, &options);
        links.with_value(|l| editor.set_link_resolver(l.as_object()));
        handle.set_value(Some(editor));
    });

    on_cleanup(move || {
        // `destroy` flushes through `on_change`, so the closures must outlive it.
        handle.update_value(|h| {
            if let Some(h) = h.take() {
                h.destroy();
            }
        });
        on_change.dispose();
        on_formats.dispose();
        on_suggestions.dispose();
        links.dispose();
    });

    view! { <div class="editor-host" node_ref=node_ref></div> }
}

/// How many suggestions to offer while typing a link: as many as fit in the bar, roughly.
const SUGGESTIONS: usize = 6;

/// The notes a project can reach, as the `LinkResolver` the editor expects (editor/src/links.ts):
/// how each link stands, and suggestions while typing one. On a phone a tap on a link only moves
/// the cursor, so opening and making notes from the editor aren't offered.
pub struct LinkBridge {
    object: Object,
    _resolve: Closure<dyn Fn(String) -> JsValue>,
    _suggest: Closure<dyn Fn(String) -> Array>,
    _open: Closure<dyn Fn(String, u32)>,
    _kinds: Closure<dyn Fn() -> Array>,
    _create: Closure<dyn Fn(String, String)>,
}

impl LinkBridge {
    pub fn new(index: NameIndex<NamedNote>, kind: ProjectKind) -> Self {
        let names = Rc::new(RefCell::new(index));
        let label = move |note: &NamedNote| note.note.kind.label(kind).to_owned();

        let resolve = Closure::<dyn Fn(String) -> JsValue>::new({
            let names = names.clone();
            move |target: String| {
                let names = names.borrow();
                let describe = |notes: Vec<&NamedNote>| -> Array {
                    notes.into_iter().map(|n| object(&[("title", n.note.title.as_str().into()), ("kind", label(n).into())])).collect()
                };
                let (state, notes) = match names.resolve(&target) {
                    Resolution::Found(note) => ("found", describe(vec![note])),
                    Resolution::Ambiguous(notes) => ("ambiguous", describe(notes)),
                    Resolution::Missing => ("loose", Array::new()),
                };
                object(&[("state", state.into()), ("notes", notes.into())])
            }
        });

        let suggest = Closure::<dyn Fn(String) -> Array>::new(move |query: String| {
            names
                .borrow()
                .suggest(&query, SUGGESTIONS)
                .into_iter()
                .map(|s| {
                    object(&[
                        ("title", s.item.note.title.as_str().into()),
                        ("name", s.name.into()),
                        ("alias", s.alias.into()),
                        ("kind", label(s.item).into()),
                    ])
                })
                .collect()
        });

        let open = Closure::<dyn Fn(String, u32)>::new(|_: String, _: u32| {});
        let kinds = Closure::<dyn Fn() -> Array>::new(Array::new);
        let create = Closure::<dyn Fn(String, String)>::new(|_: String, _: String| {});

        let object = Object::new();
        set(&object, "resolve", resolve.as_ref());
        set(&object, "suggest", suggest.as_ref());
        set(&object, "open", open.as_ref());
        set(&object, "kinds", kinds.as_ref());
        set(&object, "create", create.as_ref());
        Self { object, _resolve: resolve, _suggest: suggest, _open: open, _kinds: kinds, _create: create }
    }

    pub fn as_object(&self) -> &Object {
        &self.object
    }
}

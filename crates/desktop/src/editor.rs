//! Rust side of the ProseMirror island (`editor/src/index.ts`), which the page loads as
//! `window.NeedleEditor` before the wasm module starts.

use js_sys::{Object, Reflect};
use leptos::{html, prelude::*};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    pub type EditorHandle;

    #[wasm_bindgen(js_namespace = NeedleEditor, js_name = mount)]
    fn mount(el: &web_sys::HtmlElement, markdown: &str, options: &Object) -> EditorHandle;

    /// Loads a new document. Call `flush` first, or the last second of typing is dropped.
    #[wasm_bindgen(method, js_name = setContent)]
    pub fn set_content(this: &EditorHandle, markdown: &str);

    #[wasm_bindgen(method, js_name = setOptions)]
    fn set_options(this: &EditorHandle, options: &Object);

    /// Expects the object built by `crate::spell::SpellBridge`.
    #[wasm_bindgen(method, js_name = setSpellchecker)]
    pub fn set_spellchecker(this: &EditorHandle, checker: &Object);

    #[wasm_bindgen(method, js_name = getMarkdown)]
    pub fn markdown(this: &EditorHandle) -> String;

    #[wasm_bindgen(method, js_name = wordCount)]
    pub fn word_count(this: &EditorHandle) -> u32;

    #[wasm_bindgen(method)]
    pub fn focus(this: &EditorHandle);

    /// Reports pending changes through `on_change` now instead of after the debounce.
    #[wasm_bindgen(method)]
    pub fn flush(this: &EditorHandle);

    #[wasm_bindgen(method)]
    fn destroy(this: &EditorHandle);

    #[wasm_bindgen(method, js_name = splitParts)]
    fn split_parts_js(this: &EditorHandle) -> JsValue;
}

impl EditorHandle {
    /// The text before and after the cursor, as Markdown.
    pub fn split_parts(&self) -> (String, String) {
        let parts = self.split_parts_js();
        let get = |key: &str| Reflect::get(&parts, &key.into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
        (get("before"), get("after"))
    }
}

/// Automatic typography changes, each switchable on its own. Mirrors `Typography` in
/// editor/src/inputrules.ts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Typography {
    pub double_quotes: bool,
    pub single_quotes: bool,
    pub em_dash: bool,
    pub ellipsis: bool,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            double_quotes: true,
            single_quotes: true,
            em_dash: true,
            ellipsis: true,
        }
    }
}

fn options(typography: Typography) -> Object {
    let rules = Object::new();
    set(&rules, "doubleQuotes", &typography.double_quotes.into());
    set(&rules, "singleQuotes", &typography.single_quotes.into());
    set(&rules, "emDash", &typography.em_dash.into());
    set(&rules, "ellipsis", &typography.ellipsis.into());
    let options = Object::new();
    set(&options, "typography", &rules);
    options
}

pub(crate) fn set(target: &Object, key: &str, value: &JsValue) {
    // Only fails for frozen objects and proxies; these are plain fresh objects.
    let _ = Reflect::set(target, &key.into(), value);
}

/// Mounts the editor once its element exists, stores it in `handle` so the parent can drive
/// it, then calls `on_ready`. `on_change` receives the Markdown and word count after typing
/// pauses.
#[component]
pub fn Editor(
    handle: StoredValue<Option<EditorHandle>, LocalStorage>,
    #[prop(into)] typography: Signal<Typography>,
    on_change: impl Fn(String, u32) + 'static,
    on_ready: impl Fn() + 'static,
) -> impl IntoView {
    let node_ref = NodeRef::<html::Div>::new();
    let on_change = StoredValue::new_local(Closure::<dyn FnMut(String, u32)>::new(on_change));

    Effect::new(move |_| {
        let Some(el) = node_ref.get() else { return };
        if handle.with_value(Option::is_some) {
            return;
        }
        let options = options(typography.get_untracked());
        on_change.with_value(|f| set(&options, "onChange", f.as_ref()));
        handle.set_value(Some(mount(&el, "", &options)));
        on_ready();
    });

    Effect::new(move |_| {
        let typography = typography.get();
        handle.with_value(|h| {
            if let Some(h) = h {
                h.set_options(&options(typography));
            }
        });
    });

    on_cleanup(move || {
        // `destroy` flushes through `on_change`, so the closure must outlive it.
        handle.update_value(|h| {
            if let Some(h) = h.take() {
                h.destroy();
            }
        });
        on_change.dispose();
    });

    view! { <div class="editor-host" node_ref=node_ref></div> }
}

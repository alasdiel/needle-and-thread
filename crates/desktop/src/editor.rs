//! Rust side of the ProseMirror island (`editor/src/index.ts`), which the page loads as
//! `window.NeedleEditor` before the wasm module starts.

use js_sys::{Object, Reflect};
use leptos::{html, prelude::*};
use wasm_bindgen::prelude::*;

use crate::tauri::Passage;

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

    /// Expects the object built by `crate::links::LinkBridge`.
    #[wasm_bindgen(method, js_name = setLinkResolver)]
    pub fn set_link_resolver(this: &EditorHandle, resolver: &Object);

    /// Checks every link again, after notes changed.
    #[wasm_bindgen(method, js_name = refreshLinks)]
    pub fn refresh_links(this: &EditorHandle);

    /// Highlights words starting with any of `terms` (empty clears it); with `reveal`, scrolls
    /// to the first one.
    #[wasm_bindgen(method, js_name = setHighlights)]
    pub fn set_highlights(this: &EditorHandle, terms: &js_sys::Array, reveal: bool);

    /// Checks every word again, after the spellchecker learned new ones.
    #[wasm_bindgen(method, js_name = recheckSpelling)]
    pub fn recheck_spelling(this: &EditorHandle);

    #[wasm_bindgen(method, js_name = getMarkdown)]
    pub fn markdown(this: &EditorHandle) -> String;

    #[wasm_bindgen(method, js_name = wordCount)]
    pub fn word_count(this: &EditorHandle) -> u32;

    /// Top-level blocks in the document, 0 when it's empty.
    #[wasm_bindgen(method, js_name = blockCount)]
    pub fn block_count(this: &EditorHandle) -> u32;

    /// Shows or hides the cut line at the cursor. While it shows, Enter and Escape call the
    /// editor's `on_cut_confirm` and `on_cut_cancel`.
    #[wasm_bindgen(method, js_name = showCutLine)]
    pub fn show_cut_line(this: &EditorHandle, on: bool);

    /// Draws a fading seam before top-level block `index` and puts the cursor there.
    #[wasm_bindgen(method, js_name = showSeam)]
    pub fn show_seam(this: &EditorHandle, index: u32);

    #[wasm_bindgen(method)]
    pub fn focus(this: &EditorHandle);

    /// Reports pending changes through `on_change` now instead of after the debounce.
    #[wasm_bindgen(method)]
    pub fn flush(this: &EditorHandle);

    #[wasm_bindgen(method)]
    fn destroy(this: &EditorHandle);

    #[wasm_bindgen(method, js_name = splitParts)]
    fn split_parts_js(this: &EditorHandle) -> JsValue;

    /// Offers "Cut to bin" and its shortcut, or not: on for scenes, off for notes.
    #[wasm_bindgen(method, js_name = setBinEnabled)]
    pub fn set_bin_enabled(this: &EditorHandle, on: bool);

    #[wasm_bindgen(method, js_name = restorePassage)]
    fn restore_passage_js(this: &EditorHandle, passage: &JsValue) -> bool;

    #[wasm_bindgen(js_namespace = NeedleEditor, js_name = hasSpot)]
    fn has_spot_js(markdown: &str, passage: &JsValue) -> bool;
}

fn passage_value(passage: &Passage) -> JsValue {
    serde_wasm_bindgen::to_value(passage).unwrap_or(JsValue::NULL)
}

/// Whether a scene's `markdown` still has the place `passage` was cut from.
pub fn has_spot(markdown: &str, passage: &Passage) -> bool {
    has_spot_js(markdown, &passage_value(passage))
}

impl EditorHandle {
    /// Puts a passage from the bin back where it came from if that place is still there, and
    /// otherwise at the cursor, then selects it. Returns whether it found the place.
    pub fn restore_passage(&self, passage: &Passage) -> bool {
        self.restore_passage_js(&passage_value(passage))
    }

    /// The text before and after the cursor, as Markdown.
    pub fn split_parts(&self) -> (String, String) {
        let parts = self.split_parts_js();
        let get = |key: &str| Reflect::get(&parts, &key.into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
        (get("before"), get("after"))
    }
}

/// The vault's automatic typography changes, as the editor's options take them.
pub use needle_core::settings::TypographySettings as Typography;

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
/// pauses; `on_cut_confirm` and `on_cut_cancel` answer the cut line; `on_cut_to_bin` receives a
/// passage just taken out of the text, to store in the bin.
#[component]
pub fn Editor(
    handle: StoredValue<Option<EditorHandle>, LocalStorage>,
    #[prop(into)] typography: Signal<Typography>,
    on_change: impl Fn(String, u32) + 'static,
    on_cut_confirm: impl Fn() + 'static,
    on_cut_cancel: impl Fn() + 'static,
    on_cut_to_bin: impl Fn(Passage) + 'static,
    on_ready: impl Fn() + 'static,
) -> impl IntoView {
    let node_ref = NodeRef::<html::Div>::new();
    let on_change = StoredValue::new_local(Closure::<dyn FnMut(String, u32)>::new(on_change));
    let on_cut_confirm = StoredValue::new_local(Closure::<dyn FnMut()>::new(on_cut_confirm));
    let on_cut_cancel = StoredValue::new_local(Closure::<dyn FnMut()>::new(on_cut_cancel));
    let on_cut_to_bin = StoredValue::new_local(Closure::<dyn FnMut(JsValue)>::new(move |value: JsValue| {
        match serde_wasm_bindgen::from_value(value) {
            Ok(passage) => on_cut_to_bin(passage),
            Err(e) => leptos::logging::error!("a cut passage didn't come through: {e}"),
        }
    }));

    Effect::new(move |_| {
        let Some(el) = node_ref.get() else { return };
        if handle.with_value(Option::is_some) {
            return;
        }
        let options = options(typography.get_untracked());
        on_change.with_value(|f| set(&options, "onChange", f.as_ref()));
        on_cut_confirm.with_value(|f| set(&options, "onCutConfirm", f.as_ref()));
        on_cut_cancel.with_value(|f| set(&options, "onCutCancel", f.as_ref()));
        on_cut_to_bin.with_value(|f| set(&options, "onCutToBin", f.as_ref()));
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
        // `destroy` flushes through `on_change`, so the closures must outlive it.
        handle.update_value(|h| {
            if let Some(h) = h.take() {
                h.destroy();
            }
        });
        on_change.dispose();
        on_cut_confirm.dispose();
        on_cut_cancel.dispose();
        on_cut_to_bin.dispose();
    });

    view! { <div class="editor-host" node_ref=node_ref></div> }
}

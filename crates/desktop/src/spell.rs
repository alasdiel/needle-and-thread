//! Exposes a `needle_core::spell::Speller` to the editor as the `Spellchecker` object it
//! expects (editor/src/spellcheck.ts).

use std::{cell::RefCell, rc::Rc};

use js_sys::{Array, Object};
use needle_core::spell::Speller;
use wasm_bindgen::prelude::*;

use crate::editor::set;

/// Owns the closures the editor calls; they stop working once this is dropped.
pub struct SpellBridge {
    object: Object,
    _check: Closure<dyn Fn(String) -> bool>,
    _suggest: Closure<dyn Fn(String) -> Array>,
    _add: Closure<dyn Fn(String)>,
}

impl SpellBridge {
    /// `on_add` is called after a word is accepted, to persist it.
    pub fn new(speller: Speller, on_add: impl Fn(String) + 'static) -> Self {
        let speller = Rc::new(RefCell::new(speller));

        let check = Closure::<dyn Fn(String) -> bool>::new({
            let speller = speller.clone();
            move |word: String| speller.borrow().check(&word)
        });
        let suggest = Closure::<dyn Fn(String) -> Array>::new({
            let speller = speller.clone();
            move |word: String| speller.borrow().suggest(&word).into_iter().map(JsValue::from).collect()
        });
        let add = Closure::<dyn Fn(String)>::new(move |word: String| {
            if speller.borrow_mut().add(&word).is_ok() {
                on_add(word);
            }
        });

        let object = Object::new();
        set(&object, "check", check.as_ref());
        set(&object, "suggest", suggest.as_ref());
        set(&object, "add", add.as_ref());
        Self {
            object,
            _check: check,
            _suggest: suggest,
            _add: add,
        }
    }

    pub fn as_object(&self) -> &Object {
        &self.object
    }
}

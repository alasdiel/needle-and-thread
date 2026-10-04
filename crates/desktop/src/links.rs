//! Exposes the notes a project can reach to the editor as the `LinkResolver` it expects
//! (editor/src/links.ts): how each link stands, suggestions while typing one, and opening or
//! making a note from a link. The lookup itself is `needle_core::names`.

use std::{cell::RefCell, rc::Rc};

use js_sys::{Array, Object};
use needle_core::names::{NameIndex, Owner, Resolution};
use needle_core::project::{NoteKind, ProjectKind};
use wasm_bindgen::prelude::*;

use crate::editor::set;
use crate::notes::kind_label;
use crate::tauri::{NoteKey, NoteView};

/// How many suggestions to offer while typing a link.
const SUGGESTIONS: usize = 8;

struct Names {
    index: NameIndex<NoteView>,
    kind: ProjectKind,
}

/// Owns the closures the editor calls; they stop working once this is dropped.
pub struct LinkBridge {
    object: Object,
    names: Rc<RefCell<Names>>,
    _resolve: Closure<dyn Fn(String) -> JsValue>,
    _suggest: Closure<dyn Fn(String) -> Array>,
    _open: Closure<dyn Fn(String, u32)>,
    _kinds: Closure<dyn Fn() -> Array>,
    _create: Closure<dyn Fn(String, String)>,
}

fn object(fields: &[(&str, JsValue)]) -> JsValue {
    let object = Object::new();
    for (key, value) in fields {
        set(&object, key, value);
    }
    object.into()
}

impl LinkBridge {
    /// `on_open` opens a note; `on_create` makes one, from its title and type.
    pub fn new(on_open: impl Fn(NoteKey) + 'static, on_create: impl Fn(String, String) + 'static) -> Self {
        let names = Rc::new(RefCell::new(Names {
            index: NameIndex::new("", None),
            kind: ProjectKind::Fiction,
        }));

        let resolve = Closure::<dyn Fn(String) -> JsValue>::new({
            let names = names.clone();
            move |target: String| {
                let names = names.borrow();
                let describe = |notes: Vec<&NoteView>| -> Array {
                    notes
                        .into_iter()
                        .map(|n| {
                            object(&[
                                ("title", n.title.as_str().into()),
                                ("kind", kind_label(&n.kind, names.kind).into()),
                            ])
                        })
                        .collect()
                };
                let (state, notes) = match names.index.resolve(&target) {
                    Resolution::Found(note) => ("found", describe(vec![note])),
                    Resolution::Ambiguous(notes) => ("ambiguous", describe(notes)),
                    Resolution::Missing => ("loose", Array::new()),
                };
                object(&[("state", state.into()), ("notes", notes.into())])
            }
        });

        let suggest = Closure::<dyn Fn(String) -> Array>::new({
            let names = names.clone();
            move |query: String| {
                let names = names.borrow();
                names
                    .index
                    .suggest(&query, SUGGESTIONS)
                    .into_iter()
                    .map(|s| {
                        object(&[
                            ("title", s.item.title.as_str().into()),
                            ("name", s.name.into()),
                            ("alias", s.alias.into()),
                            ("kind", kind_label(&s.item.kind, names.kind).into()),
                        ])
                    })
                    .collect()
            }
        });

        let open = Closure::<dyn Fn(String, u32)>::new({
            let names = names.clone();
            move |target: String, index: u32| {
                let key = match names.borrow().index.resolve(&target) {
                    Resolution::Found(note) => Some(note.key()),
                    Resolution::Ambiguous(notes) => notes.get(index as usize).map(|n| n.key()),
                    Resolution::Missing => None,
                };
                if let Some(key) = key {
                    on_open(key);
                }
            }
        });

        let kinds = Closure::<dyn Fn() -> Array>::new({
            let names = names.clone();
            move || {
                let kind = names.borrow().kind;
                NoteKind::ALL
                    .into_iter()
                    .map(|k| JsValue::from(Array::of2(&k.as_str().into(), &k.label(kind).into())))
                    .collect()
            }
        });

        let create = Closure::<dyn Fn(String, String)>::new(move |title: String, kind: String| on_create(title, kind));

        let object = Object::new();
        set(&object, "resolve", resolve.as_ref());
        set(&object, "suggest", suggest.as_ref());
        set(&object, "open", open.as_ref());
        set(&object, "kinds", kinds.as_ref());
        set(&object, "create", create.as_ref());
        Self {
            object,
            names,
            _resolve: resolve,
            _suggest: suggest,
            _open: open,
            _kinds: kinds,
            _create: create,
        }
    }

    /// The notes links in `project` can reach, as `tauri::project_notes` lists them.
    pub fn set_notes(&self, project: &str, world: Option<&str>, notes: &[NoteView], kind: ProjectKind) {
        let mut index = NameIndex::new(project, world);
        for note in notes {
            let owner = if note.world { Owner::World(note.owner.clone()) } else { Owner::Project(note.owner.clone()) };
            index.add(&owner, &note.title, &note.aliases, note.clone());
        }
        *self.names.borrow_mut() = Names { index, kind };
    }

    pub fn as_object(&self) -> &Object {
        &self.object
    }
}

/// The words in note titles and aliases, for spellcheck to accept: "Mara Venn" gives "Mara"
/// and "Venn".
pub fn name_words(notes: &[NoteView]) -> Vec<String> {
    let mut words: Vec<String> = notes
        .iter()
        .flat_map(|n| std::iter::once(&n.title).chain(&n.aliases))
        .flat_map(|name| name.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’')))
        .map(|word| word.trim_matches(['\'', '’']).to_owned())
        .filter(|word| word.chars().count() > 1)
        .collect();
    words.sort();
    words.dedup();
    words
}

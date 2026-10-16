//! Clashes waiting to be settled (DESIGN §5, Sync): paragraphs changed both here and on another
//! device. The open scene or note shows each in place, with the other device's version under
//! it (the editor's `merge.ts`); this works out where they are from what's typed, and what
//! settling one leaves (`core::merge`).

use js_sys::{Array, Object};
use needle_core::diff::{self, Kind};
use needle_core::merge::{self, Choice, Clash};
use wasm_bindgen::JsValue;

use crate::editor::set;
use crate::history::time_label;
use crate::tauri::{MergeView, NoteKey};

/// What's open, to match clashes against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    Scene { project: String, scene: String },
    Note(NoteKey),
}

impl Place {
    pub fn of(merge: &MergeView) -> Option<Self> {
        match (&merge.project, &merge.scene) {
            (Some(project), Some(scene)) => Some(Self::Scene { project: project.clone(), scene: scene.clone() }),
            _ => merge.note_key().map(Self::Note),
        }
    }
}

/// A clash shown in the open scene or note, with the editor's id for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub id: u32,
    /// Which kept versions it's from.
    pub stamp: String,
    pub path: String,
    pub at: i64,
    pub clash: Clash,
}

/// The clashes in `place`, whose text is now `markdown`, in order through the text.
pub fn shown(all: &[MergeView], place: &Place, markdown: &str) -> Vec<Shown> {
    let mut shown: Vec<Shown> = all
        .iter()
        .filter(|m| Place::of(m).as_ref() == Some(place))
        .flat_map(|m| {
            merge::clashes(&m.base, markdown, &m.theirs).into_iter().map(|clash| Shown {
                id: 0,
                stamp: m.stamp.clone(),
                path: m.path.clone(),
                at: m.at,
                clash,
            })
        })
        .collect();
    shown.sort_by_key(|s| (s.clash.mine.start, s.clash.mine.end));
    for (id, s) in shown.iter_mut().enumerate() {
        s.id = id as u32;
    }
    shown
}

/// How many clashes a file has as it's saved.
pub fn count(merge: &MergeView) -> usize {
    merge::clashes(&merge.base, &merge.mine, &merge.theirs).len()
}

/// The boxes the editor's `setMerges` takes.
pub fn boxes(shown: &[Shown]) -> JsValue {
    let boxes = Array::new();
    for s in shown {
        let segments = Array::new();
        for segment in diff::words(&s.clash.mine_text.join("\n\n"), &s.clash.theirs_text.join("\n\n")) {
            let piece = Object::new();
            let kind = match segment.kind {
                Kind::Same => "same",
                Kind::Added => "added",
                Kind::Removed => "removed",
            };
            set(&piece, "kind", &kind.into());
            set(&piece, "text", &segment.text.into());
            segments.push(&piece);
        }
        let item = Object::new();
        set(&item, "id", &s.id.into());
        set(&item, "start", &(s.clash.mine.start as u32).into());
        set(&item, "end", &(s.clash.mine.end as u32).into());
        let when = match s.clash.theirs_text.is_empty() {
            true => format!("The other device removed this · {}", time_label(s.at)),
            false => format!("The other device · {}", time_label(s.at)),
        };
        set(&item, "when", &when.into());
        set(&item, "segments", &segments);
        boxes.push(&item);
    }
    boxes.into()
}

pub fn choice(name: &str) -> Option<Choice> {
    match name {
        "theirs" => Some(Choice::Theirs),
        "both" => Some(Choice::Both),
        "mine" => Some(Choice::Mine),
        _ => None,
    }
}

/// What settling `shown` by `choice` leaves, with the open file's text now `markdown`: the
/// clash is found again first, in case typing moved it since it was drawn.
pub fn settle(all: &[MergeView], shown: &Shown, markdown: &str, choice: Choice) -> Option<(MergeView, merge::Settled)> {
    let record = all.iter().find(|m| m.stamp == shown.stamp && m.path == shown.path)?.clone();
    let clash = merge::clashes(&record.base, markdown, &record.theirs)
        .into_iter()
        .find(|c| c.base == shown.clash.base && c.theirs == shown.clash.theirs)?;
    let settled = merge::settle(&record.base, markdown, &record.theirs, &clash, choice);
    Some((record, settled))
}

/// The banner's words for `n` clashes in the open file.
pub fn banner(n: usize) -> String {
    match n {
        1 => "One paragraph here was changed on another device too.".to_owned(),
        n => format!("{n} places here were changed on another device too."),
    }
}

//! Commands for clashes waiting to be settled: where a paragraph was changed both here and on
//! another device (DESIGN §5, Sync). The window works out the clashes and what settling one
//! leaves (`core::merge`); these read and write the versions kept beside the file.

use needle_core::names::Owner;
use needle_vault::{PendingMerge, Place};
use serde::Serialize;
use tauri::State;

use crate::state::{AppState, OrString};

#[derive(Serialize)]
pub struct MergeView {
    stamp: String,
    path: String,
    /// When the other side's snapshot was made, in seconds since the Unix epoch.
    at: i64,
    /// For a scene, its project and name in `manuscript/`.
    project: Option<String>,
    scene: Option<String>,
    /// For a note, whose it is and its path among their notes.
    owner: Option<String>,
    world: bool,
    note: Option<String>,
    base: String,
    theirs: String,
    mine: String,
}

impl From<PendingMerge> for MergeView {
    fn from(merge: PendingMerge) -> Self {
        let mut view = Self {
            stamp: merge.stamp,
            path: merge.path,
            at: merge.at,
            project: None,
            scene: None,
            owner: None,
            world: false,
            note: None,
            base: merge.base,
            theirs: merge.theirs,
            mine: merge.mine,
        };
        match merge.place {
            Place::Scene { project, scene } => (view.project, view.scene) = (Some(project), Some(scene)),
            Place::Note { owner, path } => {
                (view.owner, view.world) = match owner {
                    Owner::Project(slug) => (Some(slug), false),
                    Owner::World(slug) => (Some(slug), true),
                };
                view.note = Some(path);
            }
            Place::Other => {}
        }
        view
    }
}

/// Every scene's and note's clashes still to settle, oldest first.
#[tauri::command]
pub fn merges(state: State<'_, AppState>) -> Result<Vec<MergeView>, String> {
    state.with(|open| Ok(open.vault.merges().or_string()?.into_iter().map(MergeView::from).collect()))
}

/// Keeps what settling a clash left of the base and the other side's version; with neither,
/// the file has nothing left to settle and both are removed.
#[tauri::command]
pub fn settle_merge(state: State<'_, AppState>, stamp: String, path: String, base: Option<String>, theirs: Option<String>) -> Result<(), String> {
    state.with(|open| {
        let texts = base.as_deref().zip(theirs.as_deref());
        open.vault.settle_merge(&stamp, &path, texts).or_string()?;
        open.history.edited();
        Ok(())
    })
}

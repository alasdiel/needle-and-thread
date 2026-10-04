//! The search index of the open vault, and the search command.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use needle_index::{Hit, Index, Kind, Query};
use needle_vault::Vault;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::state::{AppState, OrString};

/// The open vault's index, kept in the app's cache folder: one file per vault, never inside
/// the vault, so git and sync don't see it.
pub struct Search {
    index: Mutex<Index>,
}

impl Search {
    pub fn open(app: &AppHandle, vault: &Vault) -> Result<Self, String> {
        let file = index_file(app, vault.root())?;
        // It's a cache: one that can't be opened is thrown away and built again.
        let index = Index::open(&file).or_else(|_| {
            let _ = fs::remove_file(&file);
            Index::open(&file)
        });
        Ok(Self {
            index: Mutex::new(index.or_string()?),
        })
    }

    fn index(&self) -> MutexGuard<'_, Index> {
        self.index.lock().expect("a search panicked")
    }
}

fn index_file(app: &AppHandle, root: &Path) -> Result<PathBuf, String> {
    // FNV-1a of the vault's path, so each vault has its own index.
    let hash = root
        .to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    Ok(app.path().app_cache_dir().or_string()?.join("index").join(format!("{hash:016x}.sqlite")))
}

#[derive(Serialize)]
pub struct HitView {
    kind: &'static str,
    project: Option<String>,
    world: Option<String>,
    /// A scene's file name, or a note's path among its owner's notes.
    key: String,
    title: String,
    status: Option<String>,
    note_type: Option<String>,
    /// (text, is the match) pieces.
    snippet: Vec<(String, bool)>,
}

impl From<Hit> for HitView {
    fn from(hit: Hit) -> Self {
        Self {
            kind: match hit.kind {
                Kind::Scene => "scene",
                Kind::Note => "note",
            },
            project: hit.project,
            world: hit.world,
            key: hit.key,
            title: hit.title,
            status: hit.status,
            note_type: hit.note_type,
            snippet: hit.snippet,
        }
    }
}

/// Searches `project` and its world, or with `everywhere`, the whole vault. `kind` is "scene"
/// or "note"; `pov` and `thread` are a note's names (title and aliases) that the header must
/// use.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn search(
    state: State<'_, AppState>,
    project: String,
    text: String,
    everywhere: bool,
    kind: Option<String>,
    status: Option<String>,
    note_type: Option<String>,
    pov: Vec<String>,
    thread: Vec<String>,
) -> Result<Vec<HitView>, String> {
    state.with(|open| {
        let mut query = Query {
            text,
            kind: kind.as_deref().map(|k| if k == "note" { Kind::Note } else { Kind::Scene }),
            status: status.filter(|s| !s.is_empty()),
            note_type: note_type.filter(|t| !t.is_empty()),
            ..Default::default()
        };
        if !everywhere {
            let project = open.vault.project(&project).or_string()?;
            query.worlds = project.config.world.iter().cloned().collect();
            query.projects = vec![project.slug];
        }
        for (field, names) in [("pov", pov), ("threads", thread)] {
            if !names.is_empty() {
                query.names.push((field.to_owned(), names));
            }
        }
        let mut index = open.search.index();
        index.sync(&open.vault).or_string()?;
        Ok(index.search(&query).or_string()?.into_iter().map(HitView::from).collect())
    })
}

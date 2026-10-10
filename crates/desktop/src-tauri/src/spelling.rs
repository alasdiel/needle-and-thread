//! Supplies the Hunspell dictionary and the vault's personal word list to the frontend, which
//! does the actual checking (needle_core::spell).

use std::{
    collections::BTreeSet,
    fs,
    io::{self, Write},
    path::Path,
};

use needle_core::spell::is_addable;
use serde::Serialize;
use tauri::State;

use crate::state::{AppState, OrString};

// The US English dictionary, built in (see /dictionaries), so spelling works the same on every
// machine whether or not it has Hunspell dictionaries installed.
const AFF: &str = include_str!("../../../../dictionaries/en_US.aff");
const DIC: &str = include_str!("../../../../dictionaries/en_US.dic");

#[derive(Serialize)]
pub struct SpellDictionary {
    aff: &'static str,
    dic: &'static str,
    personal_words: Vec<String>,
}

#[tauri::command]
pub fn spell_dictionary(state: State<'_, AppState>) -> Result<SpellDictionary, String> {
    let personal_words = match state.lock().as_ref() {
        Some(open) => personal_words(&open.vault.dictionary_path()).or_string()?,
        None => Vec::new(),
    };
    Ok(SpellDictionary {
        aff: AFF,
        dic: DIC,
        personal_words,
    })
}

/// Adds `word` to the vault's personal dictionary, one word per line.
#[tauri::command]
pub fn add_to_dictionary(state: State<'_, AppState>, word: String) -> Result<(), String> {
    if !is_addable(&word) {
        return Err(format!("{word:?} is not a single word"));
    }
    state.with(|open| {
        let path = open.vault.dictionary_path();
        if personal_words(&path).or_string()?.contains(&word) {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).or_string()?;
        }
        let mut file = fs::OpenOptions::new().create(true).append(true).open(&path).or_string()?;
        writeln!(file, "{word}").or_string()?;
        open.history.edited();
        Ok(())
    })
}

fn personal_words(path: &Path) -> io::Result<Vec<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text
            .lines()
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

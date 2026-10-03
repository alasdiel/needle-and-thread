//! Supplies the Hunspell dictionary and the vault's personal word list to the frontend, which
//! does the actual checking (needle_core::spell).

use std::{
    collections::BTreeSet,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use needle_core::spell::is_addable;
use serde::Serialize;
use tauri::AppHandle;

use crate::scenes::spike_vault;

const LANGUAGE: &str = "en_US";

// Where Linux distributions install Hunspell dictionaries. Windows will need one bundled.
const DICTIONARY_DIRS: &[&str] = &["/usr/share/hunspell", "/usr/share/myspell/dicts", "/usr/share/myspell"];

#[derive(Serialize)]
pub struct SpellDictionary {
    aff: String,
    dic: String,
    personal_words: Vec<String>,
}

#[tauri::command]
pub fn spell_dictionary(app: AppHandle) -> Result<SpellDictionary, String> {
    let dir = DICTIONARY_DIRS
        .iter()
        .map(Path::new)
        .find(|dir| dir.join(format!("{LANGUAGE}.dic")).exists())
        .ok_or_else(|| format!("no {LANGUAGE} Hunspell dictionary found (install hunspell-en_us)"))?;
    let read = |ext: &str| fs::read_to_string(dir.join(format!("{LANGUAGE}.{ext}"))).map_err(|e| e.to_string());
    Ok(SpellDictionary {
        aff: read("aff")?,
        dic: read("dic")?,
        personal_words: personal_words(&personal_dictionary(&app)?).map_err(|e| e.to_string())?,
    })
}

/// Adds `word` to the vault's personal dictionary, one word per line.
#[tauri::command]
pub fn add_to_dictionary(app: AppHandle, word: String) -> Result<(), String> {
    if !is_addable(&word) {
        return Err(format!("{word:?} is not a single word"));
    }
    let path = personal_dictionary(&app)?;
    if personal_words(&path).map_err(|e| e.to_string())?.contains(&word) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    writeln!(file, "{word}").map_err(|e| e.to_string())
}

fn personal_dictionary(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(spike_vault(app)?.join(".needle/dictionary.txt"))
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

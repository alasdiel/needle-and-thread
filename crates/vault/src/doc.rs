//! Reading and editing Markdown files with a TOML header, which scenes and notes both are.

use std::fs;
use std::io;
use std::path::Path;

use needle_core::header::Header;
use needle_core::scene::SceneFile;

use crate::files::write_atomically;
use crate::{Error, Result};

/// Reads a scene or note. `name` says what's missing if the file isn't there ("scene x").
pub(crate) fn read(path: &Path, name: &str) -> Result<SceneFile> {
    let text = fs::read_to_string(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => Error::NotFound(format!("no {name}")),
        _ => Error::Io(e),
    })?;
    Ok(SceneFile::parse(&text))
}

/// Replaces the text after the header, keeping the header. Returns whether the file changed.
pub(crate) fn save_body(path: &Path, name: &str, markdown: &str) -> Result<bool> {
    let mut file = read(path, name)?;
    let before = file.to_string();
    file.set_markdown(markdown);
    let after = file.to_string();
    if after == before {
        return Ok(false);
    }
    write_atomically(path, after.as_bytes())?;
    Ok(true)
}

/// Edits the header in place; fields `edit` doesn't touch keep their formatting.
pub(crate) fn update_header(path: &Path, name: &str, edit: impl FnOnce(&mut Header)) -> Result<SceneFile> {
    let mut file = read(path, name)?;
    let mut header = file.header()?;
    edit(&mut header);
    let before = file.to_string();
    file.set_header(&header);
    let after = file.to_string();
    if after != before {
        write_atomically(path, after.as_bytes())?;
    }
    Ok(file)
}

/// Changes a scene or note with `edit` and writes it back if anything changed. Returns whether
/// it did.
pub(crate) fn edit(path: &Path, name: &str, edit: impl FnOnce(&mut SceneFile) -> Result<()>) -> Result<bool> {
    let mut file = read(path, name)?;
    let before = file.to_string();
    edit(&mut file)?;
    let after = file.to_string();
    if after == before {
        return Ok(false);
    }
    write_atomically(path, after.as_bytes())?;
    Ok(true)
}

//! Notes (docs/DESIGN.md §4): characters, places, threads, sources, plot points, relationships
//! and anything else, one Markdown file each. A project keeps them in `notes/`, a world directly
//! in its folder, in one folder per type. Like a scene, a note keeps the file name it was made
//! with: links find notes by title, so retitling one never moves it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use needle_core::header::Header;
use needle_core::id::{make_id, slugify};
use needle_core::names::Owner;
use needle_core::project::NoteKind;
use needle_core::scene::SceneFile;

use crate::doc;
use crate::files::{checked_name, unique_file, utc_iso, utc_stamp, write_atomically};
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteInfo {
    /// Where the note is among its owner's notes, without `.md`: `characters/mara-venn`.
    /// Never changes once created.
    pub path: String,
    pub id: String,
    pub kind: NoteKind,
    pub title: String,
    pub aliases: Vec<String>,
    pub summary: String,
}

/// The notes of one project or world.
#[derive(Debug, Clone)]
pub struct Notes {
    owner: Owner,
    root: PathBuf,
    /// The owner's cut bin. A world's sits beside its type folders, so listing skips it.
    bin: PathBuf,
    templates: PathBuf,
}

/// Header fields each type of note starts with, when the vault has no template for it yet.
/// They're written to `.needle/templates/` the first time a note is made, to edit from there.
const DEFAULT_TEMPLATES: [(NoteKind, &str); 7] = [
    (NoteKind::Character, "+++\naliases = []\n+++\n"),
    (NoteKind::Place, "+++\naliases = []\n+++\n"),
    (NoteKind::Thread, "+++\n+++\n"),
    (NoteKind::Source, "+++\nauthor = \"\"\nyear = \"\"\nurl = \"\"\npages = \"\"\n+++\n"),
    (NoteKind::Event, "+++\ninvolves = []\nthreads = []\nscenes = []\n+++\n"),
    (NoteKind::Relationship, "+++\nbetween = []\nlabel = \"\"\n+++\n"),
    (NoteKind::Note, "+++\n+++\n"),
];

/// The fields the app fills in on every new note, ahead of the template's.
const OWN_FIELDS: [&str; 3] = ["id", "type", "title"];

impl Notes {
    pub(crate) fn new(owner: Owner, root: PathBuf, bin: PathBuf, templates: PathBuf) -> Self {
        Self { owner, root, bin, templates }
    }

    pub fn owner(&self) -> &Owner {
        &self.owner
    }

    /// The file for a note path from the frontend, rejected if it could reach outside.
    pub fn file_path(&self, path: &str) -> Result<PathBuf> {
        let (folder, name) = match path.split_once('/') {
            Some((folder, name)) => (Some(checked_name(folder)?), name),
            None => (None, path),
        };
        let mut file = self.root.clone();
        file.extend(folder);
        file.push(format!("{}.md", checked_name(name)?));
        Ok(file)
    }

    /// Every note's path, without reading any of them, in no particular order.
    pub fn paths(&self) -> Result<Vec<String>> {
        let mut paths = Vec::new();
        if !self.root.is_dir() {
            return Ok(paths);
        }
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path == self.bin {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()).filter(|n| !n.starts_with('.')) else {
                continue;
            };
            if path.is_dir() {
                for inner in fs::read_dir(&path)? {
                    if let Some(stem) = markdown_stem(&inner?.path()) {
                        paths.push(format!("{name}/{stem}"));
                    }
                }
            } else if let Some(stem) = markdown_stem(&path) {
                paths.push(stem.to_owned());
            }
        }
        Ok(paths)
    }

    /// Every note, sorted by type and then title. A note with a header that can't be read is
    /// still listed, under its file name, so a typo never hides it.
    pub fn list(&self) -> Result<Vec<NoteInfo>> {
        let mut notes = self.paths()?.iter().map(|path| self.info(path)).collect::<Result<Vec<_>>>()?;
        let rank = |kind: NoteKind| NoteKind::ALL.iter().position(|k| *k == kind);
        notes.sort_by_cached_key(|n| (rank(n.kind), n.title.to_lowercase(), n.path.clone()));
        Ok(notes)
    }

    pub fn read(&self, path: &str) -> Result<SceneFile> {
        doc::read(&self.file_path(path)?, &format!("note {path}"))
    }

    pub fn info(&self, path: &str) -> Result<NoteInfo> {
        Ok(info(path, &self.read(path)?))
    }

    /// Makes a note from the vault's template for `kind`, in that type's folder.
    pub fn create(&self, kind: NoteKind, title: &str) -> Result<NoteInfo> {
        let title = match title.trim() {
            "" => "Untitled note",
            title => title,
        };
        let template = self.template(kind)?;
        let mut header = Header::new();
        header.set_str("id", &make_id("nt", fastrand::u64(..)));
        header.set_str("type", kind.as_str());
        header.set_str("title", title);
        let mut rest = template.header()?;
        for key in OWN_FIELDS {
            rest.remove(key);
        }
        let header = Header::parse(&format!("{header}{rest}"))?;

        let mut note = SceneFile::parse("");
        note.set_header(&header);
        let body = template.markdown();
        if !body.trim().is_empty() {
            note.set_markdown(&body);
        }
        let file = unique_file(&self.root.join(kind.folder()), &slugify(title), "md");
        write_atomically(&file, note.to_string().as_bytes())?;
        let stem = markdown_stem(&file).unwrap_or_default();
        Ok(info(&format!("{}/{stem}", kind.folder()), &note))
    }

    /// Moves a note to its owner's cut bin (`cut/`), noting when and where it was. Nothing is
    /// deleted; links to it show as missing until a note has that name again.
    pub fn cut(&self, path: &str) -> Result<PathBuf> {
        let file = self.file_path(path)?;
        let mut note = self.read(path)?;
        let now = SystemTime::now();
        let mut header = note.header()?;
        header.set_str("cut_at", &utc_iso(now));
        header.set_str("cut_from_note", path);
        note.set_header(&header);

        // Copy, then remove: a crash in between leaves two copies, never none.
        let stem = path.rsplit_once('/').map_or(path, |(_, stem)| stem);
        let cut_path = unique_file(&self.bin, &format!("{}-{stem}", utc_stamp(now)), "md");
        write_atomically(&cut_path, note.to_string().as_bytes())?;
        fs::remove_file(file)?;
        Ok(cut_path)
    }

    /// Changes a note with `edit`, writing it back if anything changed.
    pub(crate) fn edit(&self, path: &str, edit: impl FnOnce(&mut SceneFile) -> Result<()>) -> Result<bool> {
        doc::edit(&self.file_path(path)?, &format!("note {path}"), edit)
    }

    /// Replaces a note's text, keeping its header. Returns whether the file changed.
    pub fn save_body(&self, path: &str, markdown: &str) -> Result<bool> {
        doc::save_body(&self.file_path(path)?, &format!("note {path}"), markdown)
    }

    /// Edits a note's header in place; fields `edit` doesn't touch keep their formatting.
    pub fn update_header(&self, path: &str, edit: impl FnOnce(&mut Header)) -> Result<NoteInfo> {
        let note = doc::update_header(&self.file_path(path)?, &format!("note {path}"), edit)?;
        Ok(info(path, &note))
    }

    /// The template for `kind`, after writing out any default templates the vault lacks.
    fn template(&self, kind: NoteKind) -> Result<SceneFile> {
        for (default_kind, text) in DEFAULT_TEMPLATES {
            let file = self.templates.join(format!("{}.md", default_kind.as_str()));
            if !file.exists() {
                write_atomically(&file, text.as_bytes())?;
            }
        }
        doc::read(&self.templates.join(format!("{}.md", kind.as_str())), &format!("template for {kind}"))
    }
}

fn markdown_stem(path: &Path) -> Option<&str> {
    if path.extension()? != "md" {
        return None;
    }
    path.file_stem()?.to_str()
}

/// What the list shows for a note. The type comes from the header, or else from the folder.
fn info(path: &str, note: &SceneFile) -> NoteInfo {
    let header = note.header().unwrap_or_default();
    let (folder, stem) = path.rsplit_once('/').unwrap_or(("", path));
    let text = |key: &str| header.str(key).unwrap_or_default().trim().to_owned();
    let title = text("title");
    NoteInfo {
        path: path.to_owned(),
        id: text("id"),
        kind: header
            .str("type")
            .and_then(NoteKind::parse)
            .or_else(|| NoteKind::from_folder(folder))
            .unwrap_or(NoteKind::Note),
        title: if title.is_empty() { stem.to_owned() } else { title },
        aliases: header.list("aliases").into_iter().filter(|a| !a.trim().is_empty()).collect(),
        summary: text("summary"),
    }
}

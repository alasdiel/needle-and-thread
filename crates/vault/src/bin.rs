//! The cut bin (docs/DESIGN.md §4): scenes, notes and passages taken out of the work, kept as
//! files in their owner's `cut/` folder until they're put back. A scene or note goes in whole,
//! with `cut_*` fields added to its header; a passage becomes a file of its own, which records
//! the scene it came from and the text either side of it.

use std::path::PathBuf;
use std::sync::Arc;

use needle_core::header::Header;
use needle_core::names::Owner;
use needle_core::project::NoteKind;
use needle_core::scene::SceneFile;
use needle_core::words::count_markdown_words;

use crate::doc;
use crate::files::checked_name;
use crate::store::{Store, unique_file};
use crate::{Error, Result};

/// The fields cutting adds to a scene's or note's header, taken out again when it's restored.
pub(crate) const CUT_FIELDS: [&str; 5] = ["cut_at", "cut_from_scene", "cut_from_chapter", "cut_after_scene", "cut_from_note"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutKind {
    Passage,
    Scene,
    Note,
}

/// A passage cut from a scene, with what it takes to put it back: the text either side of it
/// (a sentence or so, from the same paragraph or the one next to it), and whether it began or
/// ended a paragraph.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Passage {
    pub markdown: String,
    pub text_before: String,
    pub text_after: String,
    pub starts_paragraph: bool,
    pub ends_paragraph: bool,
    /// Whether it began (or ended) with a space inside its paragraph, which its Markdown can't
    /// keep.
    pub starts_with_space: bool,
    pub ends_with_space: bool,
}

/// Something in the bin, as the bin lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutItem {
    pub owner: Owner,
    /// The file's name in `cut/`, without `.md`.
    pub name: String,
    pub kind: CutKind,
    /// When it was cut, as `2026-10-03T16:15:00Z`; empty if the file doesn't say.
    pub cut_at: String,
    /// A scene's or note's title. For a passage, the title its scene had when it was cut.
    pub title: String,
    /// For a passage, the scene it came from; for a scene, the name it had.
    pub scene: Option<String>,
    /// The chapter a scene was in.
    pub chapter: Option<String>,
    pub note_kind: Option<NoteKind>,
    pub words: usize,
    /// For a passage, its text and what was around it.
    pub passage: Option<Passage>,
}

/// One project's or world's cut bin.
#[derive(Debug, Clone)]
pub struct Bin {
    owner: Owner,
    dir: PathBuf,
    store: Arc<dyn Store>,
}

impl Bin {
    pub(crate) fn new(owner: Owner, dir: PathBuf, store: Arc<dyn Store>) -> Self {
        Self { owner, dir, store }
    }

    pub fn owner(&self) -> &Owner {
        &self.owner
    }

    /// Where something in the bin is, by its name.
    pub fn file_path(&self, name: &str) -> Result<PathBuf> {
        Ok(self.dir.join(format!("{}.md", checked_name(name)?)))
    }

    pub(crate) fn read(&self, name: &str) -> Result<SceneFile> {
        doc::read(self.store.as_ref(), &self.file_path(name)?, &format!("{name} in the cut bin"))
    }

    /// Writes `file` into the bin as `stem.md` (or `stem-2.md`…) and returns its name.
    pub(crate) fn add(&self, stem: &str, file: &SceneFile) -> Result<String> {
        let path = unique_file(self.store.as_ref(), &self.dir, stem, "md");
        self.store.write(&path, file.to_string().as_bytes())?;
        Ok(path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_owned())
    }

    /// The names of everything in the bin, in no particular order.
    pub fn names(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in self.store.list(&self.dir)? {
            let path = entry.path;
            if path.extension().is_some_and(|ext| ext == "md")
                && let Some(name) = path.file_stem().and_then(|s| s.to_str()).filter(|n| !n.starts_with('.'))
            {
                names.push(name.to_owned());
            }
        }
        Ok(names)
    }

    /// Everything in the bin, the most recently cut first.
    pub fn list(&self) -> Result<Vec<CutItem>> {
        let mut items = self.names()?.iter().map(|name| self.item(name)).collect::<Result<Vec<_>>>()?;
        items.sort_by(|a, b| (&b.cut_at, &b.name).cmp(&(&a.cut_at, &a.name)));
        Ok(items)
    }

    /// What the bin shows for one file. A header that can't be read still lists the file, as a
    /// scene under its file name, so a typo never hides it.
    pub fn item(&self, name: &str) -> Result<CutItem> {
        let file = self.read(name)?;
        let header = file.header().unwrap_or_default();
        let text = |key: &str| header.str(key).map(str::to_owned);
        let markdown = file.markdown();
        let kind = if header.contains("text_before") || header.contains("text_after") {
            CutKind::Passage
        } else if header.contains("cut_from_note") {
            CutKind::Note
        } else {
            CutKind::Scene
        };
        let title = match kind {
            CutKind::Passage => text("cut_from_title").or_else(|| text("cut_from_scene")),
            _ => header.title().map(str::to_owned),
        }
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| name.to_owned());
        let from_note = text("cut_from_note").unwrap_or_default();
        Ok(CutItem {
            owner: self.owner.clone(),
            name: name.to_owned(),
            kind,
            cut_at: text("cut_at").unwrap_or_default(),
            title,
            scene: text("cut_from_scene"),
            chapter: text("cut_from_chapter").filter(|_| kind == CutKind::Scene),
            note_kind: (kind == CutKind::Note).then(|| {
                header
                    .str("type")
                    .and_then(NoteKind::parse)
                    .or_else(|| from_note.split_once('/').and_then(|(folder, _)| NoteKind::from_folder(folder)))
                    .unwrap_or(NoteKind::Note)
            }),
            words: count_markdown_words(&markdown),
            passage: (kind == CutKind::Passage).then(|| Passage {
                text_before: text("text_before").unwrap_or_default(),
                text_after: text("text_after").unwrap_or_default(),
                starts_paragraph: header.bool("starts_paragraph").unwrap_or(false),
                ends_paragraph: header.bool("ends_paragraph").unwrap_or(false),
                starts_with_space: header.bool("starts_with_space").unwrap_or(false),
                ends_with_space: header.bool("ends_with_space").unwrap_or(false),
                markdown,
            }),
        })
    }

    /// Takes a passage out of the bin, once its text is back in a scene. Scenes and notes leave
    /// the bin by being restored, so this refuses them.
    pub fn remove_passage(&self, name: &str) -> Result<()> {
        if self.item(name)?.kind != CutKind::Passage {
            return Err(Error::Invalid(format!("{name} isn't a passage")));
        }
        self.store.remove_file(&self.file_path(name)?)
    }

    /// Reads a scene or note to restore: its file with the `cut_*` fields taken out of its
    /// header, and those fields' values.
    pub(crate) fn take_out(&self, name: &str) -> Result<(SceneFile, Header)> {
        let mut file = self.read(name)?;
        let mut header = file.header()?;
        let mut cut = Header::new();
        for key in CUT_FIELDS {
            if let Some(value) = header.str(key) {
                cut.set_str(key, value);
            }
            header.remove(key);
        }
        file.set_header(&header);
        Ok((file, cut))
    }
}

/// A passage's file: its text, under a header saying where it was.
pub(crate) fn passage_file(passage: &Passage, cut_at: &str, scene: &str, title: &str) -> SceneFile {
    let mut header = Header::new();
    header.set_str("cut_at", cut_at);
    header.set_str("cut_from_scene", scene);
    header.set_str("cut_from_title", title);
    header.set_str("text_before", &passage.text_before);
    header.set_str("text_after", &passage.text_after);
    header.set_bool("starts_paragraph", passage.starts_paragraph);
    header.set_bool("ends_paragraph", passage.ends_paragraph);
    // Only written when there's a space, which is rare.
    if passage.starts_with_space {
        header.set_bool("starts_with_space", true);
    }
    if passage.ends_with_space {
        header.set_bool("ends_with_space", true);
    }
    let mut file = SceneFile::parse("");
    file.set_header(&header);
    file.set_markdown(&passage.markdown);
    file
}

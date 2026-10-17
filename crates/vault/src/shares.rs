//! Links made with Share (DESIGN §10): the vault's list of them in `.needle/shares.toml`, and
//! the text each one shares, put together from the outline.

use needle_core::share::{Covers, FORMAT, ShareList, ShareRecord, Shared, SharedPart};

use crate::{Error, Project, Result, Vault};

const SHARES: &str = ".needle/shares.toml";

impl Vault {
    /// Every link made from this vault, oldest first.
    pub fn shares(&self) -> Result<ShareList> {
        match self.store.read_opt(&self.root.join(SHARES))? {
            Some(text) => ShareList::parse(&text).map_err(Error::Invalid),
            None => Ok(ShareList::default()),
        }
    }

    fn save_shares(&self, list: &ShareList) -> Result<()> {
        self.store.write(&self.root.join(SHARES), list.to_toml().as_bytes())?;
        Ok(())
    }

    pub fn add_share(&self, record: ShareRecord) -> Result<()> {
        let mut list = self.shares()?;
        list.shares.push(record);
        self.save_shares(&list)
    }

    /// Takes a link off the list, returning it.
    pub fn remove_share(&self, id: &str) -> Result<ShareRecord> {
        let mut list = self.shares()?;
        let at = list.shares.iter().position(|s| s.id == id).ok_or_else(|| Error::NotFound(format!("no link {id}")))?;
        let record = list.shares.remove(at);
        self.save_shares(&list)?;
        Ok(record)
    }
}

impl Project {
    /// The scenes a link shares, in reading order. A chapter or part that's gone shares nothing.
    pub fn covered(&self, covers: &Covers) -> Result<Vec<String>> {
        let outline = self.outline()?;
        Ok(match covers {
            Covers::Scene { scene } => self.scene_path(scene).ok().filter(|p| self.store.is_file(p)).map(|_| scene.clone()).into_iter().collect(),
            Covers::Chapter { chapter } => outline.chapter(chapter).map(|c| c.scenes.clone()).unwrap_or_default(),
            Covers::Part { part } => outline.chapters.iter().filter(|c| c.part.as_deref() == Some(part)).flat_map(|c| c.scenes.clone()).collect(),
            Covers::Book => self.reading_order()?,
        })
    }

    /// What a link calls what it shares: the scene's, chapter's or part's name, or the book's.
    pub fn covers_title(&self, covers: &Covers) -> Result<String> {
        Ok(match covers {
            Covers::Scene { scene } => self.scene_info(scene).map(|s| s.title).unwrap_or_else(|_| scene.clone()),
            Covers::Chapter { chapter } => {
                let outline = self.outline()?;
                let at = outline.chapters.iter().position(|c| &c.id == chapter);
                at.map(|i| chapter_title(&outline.chapters[i].title, i)).unwrap_or_else(|| chapter.clone())
            }
            Covers::Part { part } => part.clone(),
            Covers::Book => self.config.title.clone(),
        })
    }

    /// The text a link shares, as it is in the files now. `version` names the snapshot that is.
    pub fn shared(&self, covers: &Covers, version: &str, updated: i64) -> Result<Shared> {
        let outline = self.outline()?;
        // Where each chapter starts, and its heading; a single scene goes without one.
        let mut starts = std::collections::HashMap::new();
        if !matches!(covers, Covers::Scene { .. }) {
            for (i, chapter) in outline.chapters.iter().enumerate() {
                if let Some(first) = chapter.scenes.first() {
                    starts.insert(first.as_str(), chapter_title(&chapter.title, i));
                }
            }
        }
        let mut parts = Vec::new();
        for scene in self.covered(covers)? {
            let Ok((info, file)) = self.read_scene(&scene) else { continue };
            parts.push(SharedPart { chapter: starts.get(scene.as_str()).cloned(), title: info.title, markdown: file.markdown(), scene });
        }
        Ok(Shared {
            v: FORMAT,
            project: self.config.title.clone(),
            title: self.covers_title(covers)?,
            parts,
            updated,
            version: version.to_owned(),
            moved: Default::default(),
        })
    }
}

fn chapter_title(title: &str, index: usize) -> String {
    if title.trim().is_empty() { format!("Chapter {}", index + 1) } else { title.to_owned() }
}

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use needle_core::header::Header;
use needle_core::id::{make_id, slugify};
use needle_core::names::Owner;
use needle_core::outline::{Chapter, Outline, OutlineError};
use needle_core::project::ProjectConfig;
use needle_core::scene::SceneFile;
use needle_core::words::count_markdown_words;

use crate::bin::{self, Bin, CutItem, CutKind, Passage};
use crate::doc;
use crate::files::{checked_name, utc_iso, utc_stamp};
use crate::notes::Notes;
use crate::store::{Store, unique_file};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Project {
    /// The project's folder name.
    pub slug: String,
    pub config: ProjectConfig,
    root: PathBuf,
    /// The vault's note templates.
    templates: PathBuf,
    store: Arc<dyn Store>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneInfo {
    /// File name in `manuscript/`, without `.md`. Never changes once created.
    pub slug: String,
    pub id: String,
    pub title: String,
    pub status: String,
    pub summary: String,
    pub words: usize,
}

/// Where a new scene goes in the outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    End { chapter: String },
    After { scene: String },
}

impl Project {
    pub(crate) fn open(root: PathBuf, templates: PathBuf, store: Arc<dyn Store>) -> Result<Self> {
        let config_path = root.join("project.toml");
        let text = store.read_to_string(&config_path).map_err(|e| match &e {
            Error::Io(e) if e.kind() == io::ErrorKind::NotFound => {
                Error::NotFound(format!("no project at {}", root.display()))
            }
            _ => e,
        })?;
        let config = ProjectConfig::parse(&text).map_err(Error::Invalid)?;
        let slug = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Self {
            slug,
            config,
            root,
            templates,
            store,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The project's own notes, in `notes/`.
    pub fn notes(&self) -> Notes {
        Notes::new(
            Owner::Project(self.slug.clone()),
            self.root.join("notes"),
            self.root.join("cut"),
            self.templates.clone(),
            self.store.clone(),
        )
    }

    /// The project's cut bin, in `cut/`. Its notes are binned there too.
    pub fn bin(&self) -> Bin {
        Bin::new(Owner::Project(self.slug.clone()), self.root.join("cut"), self.store.clone())
    }

    /// Saves new settings to `project.toml`. The file is the app's, so it's rewritten whole.
    pub fn save_config(&mut self, config: ProjectConfig) -> Result<()> {
        self.store.write(&self.root.join("project.toml"), config.to_toml().as_bytes())?;
        self.config = config;
        Ok(())
    }

    pub fn outline(&self) -> Result<Outline> {
        match self.store.read_opt(&self.root.join("outline.toml"))? {
            Some(text) => Ok(Outline::parse(&text)?),
            None => Ok(Outline::default()),
        }
    }

    pub fn save_outline(&self, outline: &Outline) -> Result<()> {
        self.store.write(&self.root.join("outline.toml"), outline.to_toml().as_bytes())
    }

    /// Reads the outline, applies `edit`, and saves it if `edit` succeeded.
    pub fn edit_outline<T>(&self, edit: impl FnOnce(&mut Outline) -> std::result::Result<T, OutlineError>) -> Result<T> {
        let mut outline = self.outline()?;
        let result = edit(&mut outline)?;
        self.save_outline(&outline)?;
        Ok(result)
    }

    /// Adds a chapter after chapter `after` (or at the end), in the same part as the chapter
    /// before it.
    pub fn add_chapter(&self, title: &str, after: Option<&str>) -> Result<Chapter> {
        self.edit_outline(|o| {
            let index = match after {
                Some(id) => o.chapters.iter().position(|c| c.id == id).map(|i| i + 1).ok_or_else(|| OutlineError::NoSuchChapter(id.to_owned()))?,
                None => o.chapters.len(),
            };
            let chapter = Chapter {
                id: make_id("ol", fastrand::u64(..)),
                title: title.trim().to_owned(),
                part: index.checked_sub(1).and_then(|i| o.chapters.get(i)).and_then(|c| c.part.clone()),
                ..Default::default()
            };
            o.insert_chapter(index, chapter.clone());
            Ok(chapter)
        })
    }

    pub fn scene_path(&self, slug: &str) -> Result<PathBuf> {
        Ok(self.root.join("manuscript").join(format!("{}.md", checked_name(slug)?)))
    }

    pub fn scene(&self, slug: &str) -> Result<SceneFile> {
        doc::read(self.store.as_ref(), &self.scene_path(slug)?, &format!("scene {slug}"))
    }

    pub fn scene_info(&self, slug: &str) -> Result<SceneInfo> {
        info(slug, &self.scene(slug)?)
    }

    /// Every scene file, whether or not the outline places it, in no particular order.
    pub fn scenes(&self) -> Result<Vec<SceneInfo>> {
        self.scene_slugs()?.iter().map(|slug| self.scene_info(slug)).collect()
    }

    fn scene_slugs(&self) -> Result<Vec<String>> {
        let mut slugs = Vec::new();
        for entry in self.store.list(&self.root.join("manuscript"))? {
            let path = entry.path;
            if path.extension().is_some_and(|ext| ext == "md")
                && let Some(slug) = path.file_stem().and_then(|s| s.to_str())
            {
                slugs.push(slug.to_owned());
            }
        }
        Ok(slugs)
    }

    /// Every scene's name in reading order, then those the outline doesn't place, by name.
    pub fn reading_order(&self) -> Result<Vec<String>> {
        let outline = self.outline()?;
        let mut order: Vec<String> = outline.scenes().map(str::to_owned).collect();
        let mut unplaced: Vec<String> = self.scene_slugs()?.into_iter().filter(|s| !order.contains(s)).collect();
        unplaced.sort();
        order.retain(|slug| self.scene_path(slug).is_ok_and(|p| self.store.is_file(&p)));
        order.extend(unplaced);
        Ok(order)
    }

    /// A scene's file and what the outline shows for it, from one read.
    pub(crate) fn read_scene(&self, slug: &str) -> Result<(SceneInfo, SceneFile)> {
        let file = self.scene(slug)?;
        Ok((info(slug, &file)?, file))
    }

    pub fn create_scene(&self, title: &str, placement: Placement) -> Result<SceneInfo> {
        self.create_scene_with_body(title, placement, "")
    }

    fn create_scene_with_body(&self, title: &str, placement: Placement, markdown: &str) -> Result<SceneInfo> {
        let title = match title.trim() {
            "" => "Untitled scene",
            title => title,
        };
        // Check the placement before creating anything.
        let mut outline = self.outline()?;
        match &placement {
            Placement::End { chapter } => outline.chapter_mut(chapter).map(|_| ())?,
            Placement::After { scene } if outline.locate(scene).is_none() => {
                return Err(OutlineError::NoSuchScene(scene.clone()).into());
            }
            Placement::After { .. } => {}
        }

        let path = unique_file(self.store.as_ref(), &self.root.join("manuscript"), &slugify(title), "md");
        let slug = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_owned();
        let mut header = Header::new();
        header.set_str("id", &make_id("sc", fastrand::u64(..)));
        header.set_str("title", title);
        header.set_str("status", "idea");
        let mut scene = SceneFile::parse("");
        scene.set_header(&header);
        if !markdown.is_empty() {
            scene.set_markdown(markdown);
        }
        self.store.write(&path, scene.to_string().as_bytes())?;

        match placement {
            Placement::End { chapter } => outline.insert_scene(&chapter, usize::MAX, &slug)?,
            Placement::After { scene } => outline.insert_scene_after(&scene, &slug)?,
        }
        self.save_outline(&outline)?;
        info(&slug, &scene)
    }

    /// Replaces a scene's text, keeping its header. Returns whether the file changed.
    pub fn save_body(&self, slug: &str, markdown: &str) -> Result<bool> {
        doc::save_body(self.store.as_ref(), &self.scene_path(slug)?, &format!("scene {slug}"), markdown)
    }

    /// Changes a scene with `edit`, writing it back if anything changed.
    pub(crate) fn edit_scene(&self, slug: &str, edit: impl FnOnce(&mut SceneFile) -> Result<()>) -> Result<bool> {
        doc::edit(self.store.as_ref(), &self.scene_path(slug)?, &format!("scene {slug}"), edit)
    }

    /// Edits a scene's header in place; fields `edit` doesn't touch keep their formatting.
    pub fn update_header(&self, slug: &str, edit: impl FnOnce(&mut Header)) -> Result<SceneInfo> {
        let scene = doc::update_header(self.store.as_ref(), &self.scene_path(slug)?, &format!("scene {slug}"), edit)?;
        info(slug, &scene)
    }

    /// Sets a scene's summary, as one paragraph (line breaks become spaces). An empty summary
    /// takes the field out of the header.
    pub fn set_summary(&self, slug: &str, summary: &str) -> Result<SceneInfo> {
        let summary = summary.split_whitespace().collect::<Vec<_>>().join(" ");
        self.update_header(slug, |h| match summary.as_str() {
            "" => h.remove("summary"),
            summary => h.set_str("summary", summary),
        })
    }

    /// Moves a scene to the cut bin (`cut/`), noting when and from where, and takes it out of
    /// the outline. Nothing is deleted.
    pub fn cut_scene(&self, slug: &str) -> Result<PathBuf> {
        let path = self.scene_path(slug)?;
        let mut scene = self.scene(slug)?;
        let mut outline = self.outline()?;
        let place = outline.locate(slug).map(|(c, i)| {
            let chapter = &outline.chapters[c];
            let title = if chapter.title.is_empty() { chapter.id.clone() } else { chapter.title.clone() };
            // Empty when it was the chapter's first scene.
            let after = i.checked_sub(1).map(|j| chapter.scenes[j].clone()).unwrap_or_default();
            (title, after)
        });

        let now = self.store.now();
        let mut header = scene.header()?;
        header.set_str("cut_at", &utc_iso(now));
        header.set_str("cut_from_scene", slug);
        if let Some((chapter, after)) = place {
            header.set_str("cut_from_chapter", &chapter);
            header.set_str("cut_after_scene", &after);
        }
        scene.set_header(&header);

        // Copy first, then update the outline, then remove: a crash midway leaves a duplicate,
        // never a loss.
        let cut_path = unique_file(self.store.as_ref(), &self.root.join("cut"), &format!("{}-{slug}", utc_stamp(now)), "md");
        self.store.write(&cut_path, scene.to_string().as_bytes())?;
        if outline.remove_scene(slug) {
            self.save_outline(&outline)?;
        }
        self.store.remove_file(&path)?;
        Ok(cut_path)
    }

    /// Moves a passage cut from `scene` into the bin, as a file of its own.
    pub fn cut_passage(&self, scene: &str, passage: &Passage) -> Result<CutItem> {
        if passage.markdown.trim().is_empty() {
            return Err(Error::Invalid("there's no text to cut".into()));
        }
        let title = self.scene_info(scene)?.title;
        let now = self.store.now();
        let file = bin::passage_file(passage, &utc_iso(now), scene, &title);
        let bin = self.bin();
        let name = bin.add(&format!("{}-{scene}-passage", utc_stamp(now)), &file)?;
        bin.item(&name)
    }

    /// Puts a scene from the bin back in the manuscript, under its old name if that's free, and
    /// back in the outline: after the scene it followed, else in its chapter (first, if it was
    /// first), else among the scenes the outline doesn't place.
    pub fn restore_scene(&self, name: &str) -> Result<SceneInfo> {
        let bin = self.bin();
        if bin.item(name)?.kind != CutKind::Scene {
            return Err(Error::Invalid(format!("{name} isn't a scene")));
        }
        let (scene, cut) = bin.take_out(name)?;
        let header = scene.header()?;
        let stem = cut
            .str("cut_from_scene")
            .filter(|s| checked_name(s).is_ok())
            .map_or_else(|| slugify(header.title().unwrap_or(name)), str::to_owned);
        let path = unique_file(self.store.as_ref(), &self.root.join("manuscript"), &stem, "md");
        let slug = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_owned();
        // Copy, then place, then remove: a crash midway leaves a duplicate, never a loss.
        self.store.write(&path, scene.to_string().as_bytes())?;

        let mut outline = self.outline()?;
        let after = cut.str("cut_after_scene");
        let chapter = cut
            .str("cut_from_chapter")
            .and_then(|name| outline.chapters.iter().find(|c| c.title == name || c.id == name))
            .map(|c| c.id.clone());
        let placed = match (after, chapter) {
            (Some(after), _) if outline.locate(after).is_some() => outline.insert_scene_after(after, &slug).map(|_| true)?,
            (Some(""), Some(chapter)) => outline.insert_scene(&chapter, 0, &slug).map(|_| true)?,
            (_, Some(chapter)) => outline.insert_scene(&chapter, usize::MAX, &slug).map(|_| true)?,
            (_, None) => false,
        };
        if placed {
            self.save_outline(&outline)?;
        }
        self.store.remove_file(&bin.file_path(name)?)?;
        info(&slug, &scene)
    }

    /// Splits a scene in two: `before` stays, `after` becomes a new scene right after it.
    pub fn split_scene(&self, slug: &str, before: &str, after: &str) -> Result<SceneInfo> {
        let original = self.scene_info(slug)?;
        let title = format!("{} (continued)", original.title);
        // The new scene is written before the original is trimmed, so the text briefly exists
        // twice rather than not at all.
        let new = self.create_scene_with_body(&title, Placement::After { scene: slug.to_owned() }, after)?;
        self.save_body(slug, before)?;
        Ok(new)
    }

    /// Appends the next scene in the same chapter to this one, and moves that scene to the cut
    /// bin so its own header and title stay recoverable. Returns the merged scene's name.
    pub fn merge_with_next(&self, slug: &str) -> Result<String> {
        let outline = self.outline()?;
        let (c, i) = outline
            .locate(slug)
            .ok_or_else(|| Error::NotFound(format!("{slug} isn't in the outline")))?;
        let next = outline.chapters[c]
            .scenes
            .get(i + 1)
            .cloned()
            .ok_or_else(|| Error::Invalid("this is the last scene in its chapter".into()))?;

        let first = self.scene(slug)?.markdown();
        let second = self.scene(&next)?.markdown();
        let combined = match (first.trim_end(), second.trim_end()) {
            ("", second) => second.to_owned(),
            (first, "") => first.to_owned(),
            (first, second) => format!("{first}\n\n{second}"),
        };
        self.save_body(slug, &combined)?;
        self.cut_scene(&next)?;
        Ok(next)
    }
}

fn info(slug: &str, scene: &SceneFile) -> Result<SceneInfo> {
    let header = scene.header()?;
    let text = |key: &str| header.str(key).unwrap_or_default().to_owned();
    Ok(SceneInfo {
        slug: slug.to_owned(),
        id: text("id"),
        title: header.title().map_or_else(|| slug.to_owned(), str::to_owned),
        status: text("status"),
        summary: text("summary"),
        words: count_markdown_words(&scene.markdown()),
    })
}

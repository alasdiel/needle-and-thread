use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use needle_core::header::Header;
use needle_core::id::{make_id, slugify};
use needle_core::outline::{Outline, OutlineError};
use needle_core::project::ProjectConfig;
use needle_core::scene::SceneFile;
use needle_core::words::count_markdown_words;

use crate::files::{checked_name, unique_file, utc_iso, utc_stamp, write_atomically};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Project {
    /// The project's folder name.
    pub slug: String,
    pub config: ProjectConfig,
    root: PathBuf,
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
    pub(crate) fn open(root: PathBuf) -> Result<Self> {
        let config_path = root.join("project.toml");
        let text = fs::read_to_string(&config_path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => Error::NotFound(format!("no project at {}", root.display())),
            _ => Error::Io(e),
        })?;
        let config = ProjectConfig::parse(&text).map_err(Error::Invalid)?;
        let slug = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Self { slug, config, root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn outline(&self) -> Result<Outline> {
        match fs::read_to_string(self.root.join("outline.toml")) {
            Ok(text) => Ok(Outline::parse(&text)?),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Outline::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_outline(&self, outline: &Outline) -> Result<()> {
        Ok(write_atomically(&self.root.join("outline.toml"), outline.to_toml().as_bytes())?)
    }

    /// Reads the outline, applies `edit`, and saves it if `edit` succeeded.
    pub fn edit_outline<T>(&self, edit: impl FnOnce(&mut Outline) -> std::result::Result<T, OutlineError>) -> Result<T> {
        let mut outline = self.outline()?;
        let result = edit(&mut outline)?;
        self.save_outline(&outline)?;
        Ok(result)
    }

    pub fn scene_path(&self, slug: &str) -> Result<PathBuf> {
        Ok(self.root.join("manuscript").join(format!("{}.md", checked_name(slug)?)))
    }

    pub fn scene(&self, slug: &str) -> Result<SceneFile> {
        let path = self.scene_path(slug)?;
        let text = fs::read_to_string(&path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => Error::NotFound(format!("no scene {slug}")),
            _ => Error::Io(e),
        })?;
        Ok(SceneFile::parse(&text))
    }

    pub fn scene_info(&self, slug: &str) -> Result<SceneInfo> {
        info(slug, &self.scene(slug)?)
    }

    /// Every scene file, whether or not the outline places it, in no particular order.
    pub fn scenes(&self) -> Result<Vec<SceneInfo>> {
        let dir = self.root.join("manuscript");
        let mut scenes = Vec::new();
        if !dir.is_dir() {
            return Ok(scenes);
        }
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "md")
                && let Some(slug) = path.file_stem().and_then(|s| s.to_str())
            {
                scenes.push(self.scene_info(slug)?);
            }
        }
        Ok(scenes)
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

        let path = unique_file(&self.root.join("manuscript"), &slugify(title), "md");
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
        write_atomically(&path, scene.to_string().as_bytes())?;

        match placement {
            Placement::End { chapter } => outline.insert_scene(&chapter, usize::MAX, &slug)?,
            Placement::After { scene } => outline.insert_scene_after(&scene, &slug)?,
        }
        self.save_outline(&outline)?;
        info(&slug, &scene)
    }

    /// Replaces a scene's text, keeping its header. Returns whether the file changed.
    pub fn save_body(&self, slug: &str, markdown: &str) -> Result<bool> {
        let path = self.scene_path(slug)?;
        let mut scene = self.scene(slug)?;
        let before = scene.to_string();
        scene.set_markdown(markdown);
        let after = scene.to_string();
        if after == before {
            return Ok(false);
        }
        write_atomically(&path, after.as_bytes())?;
        Ok(true)
    }

    /// Edits a scene's header in place; fields `edit` doesn't touch keep their formatting.
    pub fn update_header(&self, slug: &str, edit: impl FnOnce(&mut Header)) -> Result<SceneInfo> {
        let path = self.scene_path(slug)?;
        let mut scene = self.scene(slug)?;
        let mut header = scene.header()?;
        edit(&mut header);
        let before = scene.to_string();
        scene.set_header(&header);
        let after = scene.to_string();
        if after != before {
            write_atomically(&path, after.as_bytes())?;
        }
        info(slug, &scene)
    }

    /// Moves a scene to the cut bin (`cut/`), noting when and from where, and takes it out of
    /// the outline. Nothing is deleted.
    pub fn cut_scene(&self, slug: &str) -> Result<PathBuf> {
        let path = self.scene_path(slug)?;
        let mut scene = self.scene(slug)?;
        let mut outline = self.outline()?;
        let chapter = outline
            .locate(slug)
            .map(|(c, _)| &outline.chapters[c])
            .map(|c| if c.title.is_empty() { c.id.clone() } else { c.title.clone() });

        let now = SystemTime::now();
        let mut header = scene.header()?;
        header.set_str("cut_at", &utc_iso(now));
        header.set_str("cut_from_scene", slug);
        if let Some(chapter) = chapter {
            header.set_str("cut_from_chapter", &chapter);
        }
        scene.set_header(&header);

        // Copy first, then update the outline, then remove: a crash midway leaves a duplicate,
        // never a loss.
        let cut_path = unique_file(&self.root.join("cut"), &format!("{}-{slug}", utc_stamp(now)), "md");
        write_atomically(&cut_path, scene.to_string().as_bytes())?;
        if outline.remove_scene(slug) {
            self.save_outline(&outline)?;
        }
        fs::remove_file(path)?;
        Ok(cut_path)
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

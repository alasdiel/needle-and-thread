//! A vault on disk: projects, their outlines, scenes and notes, worlds, and the cut bin.
//! Everything is plain files (see docs/DESIGN.md §4); this crate is the only code that reads or
//! writes them.

mod doc;
mod error;
mod files;
mod links;
mod notes;
mod project;
mod world;

use std::fs;
use std::path::{Path, PathBuf};

use needle_core::id::{make_id, slugify};
use needle_core::names::{NameIndex, Owner};
use needle_core::outline::{Chapter, Outline};
use needle_core::project::{ProjectConfig, ProjectKind, WorldConfig};
use needle_core::settings::VaultSettings;

pub use error::{Error, Result};
pub use links::{Appearance, Backlink, LinkSource, Mention, NAME_FIELDS, NoteLinks, Renamed};
pub use notes::{NoteInfo, Notes};
pub use project::{Placement, Project, SceneInfo};
pub use world::World;

use files::{checked_name, unique_dir, write_atomically};

const SETTINGS: &str = ".needle/vault.toml";

const DEFAULT_SETTINGS: &str = r#"# Needle and Thread vault settings.
version = 1
statuses = ["idea", "draft", "revised", "done"]

[snapshots]
idle_minutes = 2
max_minutes = 10

[typography]
double_quotes = true
single_quotes = true
em_dash = true
ellipsis = true
"#;

#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
}

/// A note anywhere in the vault, with whose it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedNote {
    pub owner: Owner,
    pub note: NoteInfo,
}

impl Vault {
    pub fn is_vault(root: &Path) -> bool {
        root.join(SETTINGS).is_file()
    }

    pub fn open(root: &Path) -> Result<Self> {
        if !Self::is_vault(root) {
            return Err(Error::NotFound(format!("{} isn't a Needle and Thread vault", root.display())));
        }
        Ok(Self { root: root.to_owned() })
    }

    /// Makes `root` a vault, creating the folder if needed. An existing vault is just opened.
    pub fn create(root: &Path) -> Result<Self> {
        if Self::is_vault(root) {
            return Self::open(root);
        }
        for dir in [".needle", "projects", "worlds", "inbox"] {
            fs::create_dir_all(root.join(dir))?;
        }
        write_atomically(&root.join(SETTINGS), DEFAULT_SETTINGS.as_bytes())?;
        Ok(Self { root: root.to_owned() })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn settings(&self) -> Result<VaultSettings> {
        match fs::read_to_string(self.root.join(SETTINGS)) {
            Ok(text) => VaultSettings::parse(&text).map_err(Error::Invalid),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(VaultSettings::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Templates for new notes, one per type (`character.md`…), written out on first use.
    pub fn templates_path(&self) -> PathBuf {
        self.root.join(".needle/templates")
    }

    /// The personal spellcheck dictionary, one word per line.
    pub fn dictionary_path(&self) -> PathBuf {
        self.root.join(".needle/dictionary.txt")
    }

    /// All projects, sorted by title.
    pub fn projects(&self) -> Result<Vec<Project>> {
        let mut projects = Vec::new();
        let dir = self.root.join("projects");
        if !dir.is_dir() {
            return Ok(projects);
        }
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.join("project.toml").is_file() {
                projects.push(Project::open(path, self.templates_path())?);
            }
        }
        projects.sort_by_key(|p| (p.config.title.to_lowercase(), p.slug.clone()));
        Ok(projects)
    }

    pub fn project(&self, slug: &str) -> Result<Project> {
        Project::open(self.root.join("projects").join(checked_name(slug)?), self.templates_path())
    }

    /// Creates a project with one chapter holding one empty scene, ready to write in.
    pub fn create_project(&self, title: &str, kind: ProjectKind) -> Result<Project> {
        let title = match title.trim() {
            "" => "Untitled project",
            title => title,
        };
        let projects = self.root.join("projects");
        fs::create_dir_all(&projects)?;
        let root = unique_dir(&projects, &slugify(title));
        fs::create_dir_all(root.join("manuscript"))?;
        let config = ProjectConfig {
            title: title.to_owned(),
            kind,
            world: None,
        };
        write_atomically(&root.join("project.toml"), config.to_toml().as_bytes())?;

        let project = Project::open(root, self.templates_path())?;
        let chapter = Chapter {
            id: make_id("ol", fastrand::u64(..)),
            ..Default::default()
        };
        let first = chapter.id.clone();
        project.save_outline(&Outline {
            chapters: vec![chapter],
        })?;
        project.create_scene("Untitled scene", Placement::End { chapter: first })?;
        Ok(project)
    }

    /// All worlds, sorted by name.
    pub fn worlds(&self) -> Result<Vec<World>> {
        let mut worlds = Vec::new();
        let dir = self.root.join("worlds");
        if !dir.is_dir() {
            return Ok(worlds);
        }
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.join("world.toml").is_file() {
                worlds.push(World::open(path, self.templates_path())?);
            }
        }
        worlds.sort_by_key(|w| (w.config.name.to_lowercase(), w.slug.clone()));
        Ok(worlds)
    }

    pub fn world(&self, slug: &str) -> Result<World> {
        World::open(self.root.join("worlds").join(checked_name(slug)?), self.templates_path())
    }

    pub fn create_world(&self, name: &str) -> Result<World> {
        let name = match name.trim() {
            "" => "Untitled world",
            name => name,
        };
        let worlds = self.root.join("worlds");
        fs::create_dir_all(&worlds)?;
        let root = unique_dir(&worlds, &slugify(name));
        let config = WorldConfig { name: name.to_owned() };
        write_atomically(&root.join("world.toml"), config.to_toml().as_bytes())?;
        World::open(root, self.templates_path())
    }

    /// The world `project` belongs to, if it names one.
    pub fn world_of(&self, project: &Project) -> Result<Option<World>> {
        project.config.world.as_deref().map(|slug| self.world(slug)).transpose()
    }

    /// Every note a link in `project` can reach: every project's notes, and its world's.
    pub fn reachable_notes(&self, project: &Project) -> Result<Vec<NamedNote>> {
        let mut found = Vec::new();
        let mut add = |notes: Notes| -> Result<()> {
            let owner = notes.owner().clone();
            found.extend(notes.list()?.into_iter().map(|note| NamedNote { owner: owner.clone(), note }));
            Ok(())
        };
        for other in self.projects()? {
            add(other.notes())?;
        }
        if let Some(world) = self.world_of(project)? {
            add(world.notes())?;
        }
        Ok(found)
    }

    /// Finds notes by name the way links in `project` do.
    pub fn names(&self, project: &Project) -> Result<NameIndex<NamedNote>> {
        let notes = self.reachable_notes(project)?;
        Ok(links::index(&notes, &project.slug, project.config.world.as_deref()))
    }

    /// The notes of a project or world.
    pub fn notes_of(&self, owner: &Owner) -> Result<Notes> {
        match owner {
            Owner::Project(slug) => Ok(self.project(slug)?.notes()),
            Owner::World(slug) => Ok(self.world(slug)?.notes()),
        }
    }
}

#[cfg(test)]
mod tests;

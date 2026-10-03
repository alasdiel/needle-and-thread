//! A vault on disk: projects, their outlines and scenes, and the cut bin. Everything is plain
//! files (see docs/DESIGN.md §4); this crate is the only code that reads or writes them.

mod error;
mod files;
mod project;

use std::fs;
use std::path::{Path, PathBuf};

use needle_core::id::{make_id, slugify};
use needle_core::outline::{Chapter, Outline};
use needle_core::project::{ProjectConfig, ProjectKind};
use needle_core::settings::VaultSettings;

pub use error::{Error, Result};
pub use project::{Placement, Project, SceneInfo};

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
                projects.push(Project::open(path)?);
            }
        }
        projects.sort_by_key(|p| (p.config.title.to_lowercase(), p.slug.clone()));
        Ok(projects)
    }

    pub fn project(&self, slug: &str) -> Result<Project> {
        Project::open(self.root.join("projects").join(checked_name(slug)?))
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

        let project = Project::open(root)?;
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
}

#[cfg(test)]
mod tests;

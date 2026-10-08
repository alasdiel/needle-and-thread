use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use needle_core::names::Owner;
use needle_core::project::WorldConfig;

use crate::bin::Bin;
use crate::notes::Notes;
use crate::store::Store;
use crate::{Error, Result};

/// A world (`worlds/<folder>/`): the characters, places, history and calendar that a series
/// of projects share. Its notes sit directly in its folder, one folder per type.
#[derive(Debug, Clone)]
pub struct World {
    /// The world's folder name.
    pub slug: String,
    pub config: WorldConfig,
    root: PathBuf,
    templates: PathBuf,
    store: Arc<dyn Store>,
}

impl World {
    pub(crate) fn open(root: PathBuf, templates: PathBuf, store: Arc<dyn Store>) -> Result<Self> {
        let text = store.read_to_string(&root.join("world.toml")).map_err(|e| match &e {
            Error::Io(e) if e.kind() == io::ErrorKind::NotFound => {
                Error::NotFound(format!("no world at {}", root.display()))
            }
            _ => e,
        })?;
        let config = WorldConfig::parse(&text).map_err(Error::Invalid)?;
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

    /// The world's cut bin, where its notes go when they're cut.
    pub fn bin(&self) -> Bin {
        self.notes().bin()
    }

    pub fn notes(&self) -> Notes {
        Notes::new(
            Owner::World(self.slug.clone()),
            self.root.clone(),
            self.root.join("cut"),
            self.templates.clone(),
            self.store.clone(),
        )
    }
}

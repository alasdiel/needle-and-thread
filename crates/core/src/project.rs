//! Project settings (`project.toml`) and the kinds of notes a project holds.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub title: String,
    #[serde(default)]
    pub kind: ProjectKind,
    /// Folder name of the world this project shares notes with, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectKind {
    #[default]
    Fiction,
    Nonfiction,
}

/// A world's settings (`world.toml`). A world holds the notes and calendar that a series of
/// projects share. Fields this version doesn't know, like the calendar, are left alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldConfig {
    pub name: String,
}

impl WorldConfig {
    pub fn parse(toml: &str) -> Result<Self, String> {
        toml::from_str(toml).map_err(|e| format!("world.toml isn't valid: {}", e.message()))
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).expect("world settings serialize")
    }
}

impl ProjectConfig {
    pub fn parse(toml: &str) -> Result<Self, String> {
        toml::from_str(toml).map_err(|e| format!("project.toml isn't valid: {}", e.message()))
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).expect("project settings serialize")
    }
}

/// The `type` of a note, which also decides its folder under `notes/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteKind {
    Character,
    Place,
    Thread,
    Source,
    /// Shown as "Plot point" in fiction and "Event" in nonfiction.
    Event,
    Relationship,
    Note,
}

impl NoteKind {
    pub const ALL: [NoteKind; 7] = [
        Self::Character,
        Self::Place,
        Self::Thread,
        Self::Source,
        Self::Event,
        Self::Relationship,
        Self::Note,
    ];

    /// The value of `type` in the header.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Character => "character",
            Self::Place => "place",
            Self::Thread => "thread",
            Self::Source => "source",
            Self::Event => "event",
            Self::Relationship => "relationship",
            Self::Note => "note",
        }
    }

    pub fn folder(self) -> &'static str {
        match self {
            Self::Character => "characters",
            Self::Place => "places",
            Self::Thread => "threads",
            Self::Source => "sources",
            Self::Event => "events",
            Self::Relationship => "relationships",
            Self::Note => "other",
        }
    }

    pub fn label(self, project: ProjectKind) -> &'static str {
        match (self, project) {
            (Self::Character, _) => "Character",
            (Self::Place, _) => "Place",
            (Self::Thread, ProjectKind::Fiction) => "Thread",
            (Self::Thread, ProjectKind::Nonfiction) => "Argument",
            (Self::Source, _) => "Source",
            (Self::Event, ProjectKind::Fiction) => "Plot point",
            (Self::Event, ProjectKind::Nonfiction) => "Event",
            (Self::Relationship, _) => "Relationship",
            (Self::Note, _) => "Note",
        }
    }

    /// The heading over a group of this kind of note.
    pub fn plural(self, project: ProjectKind) -> &'static str {
        match (self, project) {
            (Self::Character, _) => "Characters",
            (Self::Place, _) => "Places",
            (Self::Thread, ProjectKind::Fiction) => "Threads",
            (Self::Thread, ProjectKind::Nonfiction) => "Arguments",
            (Self::Source, _) => "Sources",
            (Self::Event, ProjectKind::Fiction) => "Plot points",
            (Self::Event, ProjectKind::Nonfiction) => "Events",
            (Self::Relationship, _) => "Relationships",
            (Self::Note, _) => "Other notes",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    /// The kind whose notes go in `folder`, e.g. `characters`.
    pub fn from_folder(folder: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.folder() == folder)
    }
}

impl fmt::Display for NoteKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_settings_round_trip() {
        let config = ProjectConfig {
            title: "Tidewater".into(),
            kind: ProjectKind::Fiction,
            world: Some("glass-coast".into()),
        };
        let text = config.to_toml();
        assert_eq!(text, "title = \"Tidewater\"\nkind = \"fiction\"\nworld = \"glass-coast\"\n");
        assert_eq!(ProjectConfig::parse(&text).unwrap(), config);
    }

    #[test]
    fn kind_defaults_to_fiction() {
        assert_eq!(ProjectConfig::parse("title = \"Untitled\"").unwrap().kind, ProjectKind::Fiction);
    }

    #[test]
    fn note_kinds_round_trip_and_label_by_project() {
        for kind in NoteKind::ALL {
            assert_eq!(NoteKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(NoteKind::Event.label(ProjectKind::Fiction), "Plot point");
        assert_eq!(NoteKind::Thread.label(ProjectKind::Nonfiction), "Argument");
        assert_eq!(NoteKind::parse("villain"), None);
        for kind in NoteKind::ALL {
            assert_eq!(NoteKind::from_folder(kind.folder()), Some(kind));
        }
    }

    #[test]
    fn world_settings_ignore_fields_they_do_not_know() {
        let world = WorldConfig::parse("name = \"The Glass Coast\"\n\n[calendar]\nname = \"Reckoning\"\n").unwrap();
        assert_eq!(world.name, "The Glass Coast");
    }
}

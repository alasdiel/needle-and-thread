//! What points at a note, and renaming a note along with every link to it.
//!
//! Everything is found by reading the files when asked. A whole novel is about a megabyte of
//! text, so that takes milliseconds and can never be out of date; the search index (M4) can
//! take over if it ever isn't fast enough.

use std::ops::Range;

use needle_core::header::Header;
use needle_core::project::NoteKind;
use needle_core::links::{format_link, links, mentions, name_key, rewrite_links, snippet};
use needle_core::names::{NameIndex, Owner, Resolution};
use needle_core::scene::SceneFile;

use crate::{Error, NamedNote, NoteInfo, Project, Result, SceneInfo, Vault};

/// Header fields that name notes: a scene's `pov`, `cast`, `places` and `threads`, a plot
/// point's `involves`, a relationship's `between`.
pub const NAME_FIELDS: [&str; 6] = ["pov", "cast", "places", "threads", "involves", "between"];

/// The header fields that make a scene an appearance of a note.
const SCENE_FIELDS: [&str; 4] = ["pov", "cast", "places", "threads"];

/// How much text a mention's snippet shows on each side, in characters.
const SNIPPET_RADIUS: usize = 60;

/// What points at a note from one project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoteLinks {
    /// Scenes that list the note in their header, in reading order.
    pub appears_in: Vec<Appearance>,
    /// Scenes in reading order, then notes, whose text links to it.
    pub linked_from: Vec<Backlink>,
    /// Scenes that use its title or an alias without linking it, in reading order.
    pub mentioned_in: Vec<Mention>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Appearance {
    pub scene: SceneInfo,
    /// The header fields that name it, e.g. `["pov", "cast"]`.
    pub fields: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backlink {
    pub from: LinkSource,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkSource {
    Scene(SceneInfo),
    Note(NamedNote),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub scene: SceneInfo,
    pub count: usize,
    /// The first mention, as written, with the text either side of it.
    pub before: String,
    pub name: String,
    pub after: String,
}

/// What a rename changed, so anything open can be reloaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renamed {
    pub note: NoteInfo,
    /// `(project, scene)` for every scene whose file changed.
    pub scenes: Vec<(String, String)>,
    /// Every other note whose file changed.
    pub notes: Vec<(Owner, String)>,
}

/// A scene's header names (`pov`, `cast`, `places`, `threads`), each with the note it finds,
/// and notes the scene's text names that its header doesn't list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SceneNames {
    pub fields: Vec<(&'static str, Vec<HeaderName>)>,
    /// In the order the text first names them.
    pub hints: Vec<NameHint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderName {
    pub name: String,
    /// None if no note has that name (yet).
    pub note: Option<NamedNote>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameHint {
    /// The field it would go in: `cast`, `places` or `threads`.
    pub field: &'static str,
    pub note: NamedNote,
}

/// The header field each type of note is listed in, if any.
fn field_for(kind: NoteKind) -> Option<&'static str> {
    match kind {
        NoteKind::Character => Some("cast"),
        NoteKind::Place => Some("places"),
        NoteKind::Thread => Some("threads"),
        _ => None,
    }
}

impl Vault {
    /// The names in a scene's header and the notes they find, plus hints: characters, places
    /// and threads the text links to or mentions but the header doesn't list.
    pub fn scene_names(&self, project: &Project, slug: &str) -> Result<SceneNames> {
        let names = self.names(project)?;
        let file = project.scene(slug)?;
        let header = file.header()?;
        let found = |name: &str| match names.resolve(name) {
            Resolution::Found(note) => Some(note.clone()),
            _ => None,
        };
        let fields: Vec<(&'static str, Vec<HeaderName>)> = SCENE_FIELDS
            .into_iter()
            .map(|field| {
                let listed = header.list(field).into_iter().map(|name| HeaderName { note: found(&name), name }).collect();
                (field, listed)
            })
            .collect();
        let listed = |field: &str, note: &NamedNote| {
            fields.iter().any(|(f, names)| *f == field && names.iter().any(|n| n.note.as_ref() == Some(note)))
        };

        let body = file.markdown();
        let linked: Vec<(usize, NamedNote)> = links(&body)
            .into_iter()
            .filter_map(|l| found(l.target).map(|note| (l.range.start, note)))
            .collect();
        let world = project.config.world.as_deref();
        let mut hints: Vec<(usize, NameHint)> = Vec::new();
        for note in names.items() {
            let nearby = match &note.owner {
                Owner::Project(p) => *p == project.slug,
                Owner::World(w) => Some(w.as_str()) == world,
            };
            let Some(field) = field_for(note.note.kind).filter(|_| nearby) else { continue };
            if listed(field, note) {
                continue;
            }
            let first_link = linked.iter().filter(|(_, n)| n == note).map(|(at, _)| *at).min();
            let first_mention = mention_spots(&body, &note.note).first().map(|r| r.start);
            if let Some(at) = first_link.into_iter().chain(first_mention).min() {
                hints.push((at, NameHint { field, note: note.clone() }));
            }
        }
        hints.sort_by_key(|(at, _)| *at);
        Ok(SceneNames {
            fields,
            hints: hints.into_iter().map(|(_, hint)| hint).collect(),
        })
    }

    /// Sets one of a scene's name fields. `pov` takes the first name, or is removed if there's
    /// none; the lists keep the order given.
    pub fn set_scene_names(&self, project: &Project, slug: &str, field: &str, names: &[String]) -> Result<()> {
        let field = SCENE_FIELDS
            .into_iter()
            .find(|f| *f == field)
            .ok_or_else(|| Error::Invalid(format!("{field:?} isn't a scene field")))?;
        let names: Vec<&str> = names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()).collect();
        project.update_header(slug, |h| match (field, names.first()) {
            ("pov", Some(pov)) => h.set_str("pov", pov),
            ("pov", None) => h.remove("pov"),
            (field, _) => h.set_list(field, &names),
        })?;
        Ok(())
    }

    /// What points at the note `path` of `owner`, from inside `project`: its scenes, and its
    /// own and its world's notes.
    pub fn note_links(&self, project: &Project, owner: &Owner, path: &str) -> Result<NoteLinks> {
        let names = self.names(project)?;
        let target = names
            .items()
            .iter()
            .find(|n| n.owner == *owner && n.note.path == path)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("no note {path}")))?;
        let is_target = |name: &str| points_at(&names, name, &target);

        let mut found = NoteLinks::default();
        for slug in project.reading_order()? {
            let (scene, file) = project.read_scene(&slug)?;
            let header = file.header().unwrap_or_default();
            let fields: Vec<&'static str> = SCENE_FIELDS
                .into_iter()
                .filter(|field| header.list(field).iter().any(|name| is_target(name)))
                .collect();
            if !fields.is_empty() {
                found.appears_in.push(Appearance { scene: scene.clone(), fields });
            }

            let body = file.markdown();
            let count = links(&body).iter().filter(|l| is_target(l.target)).count();
            if count > 0 {
                found.linked_from.push(Backlink { from: LinkSource::Scene(scene.clone()), count });
            }

            let spots = mention_spots(&body, &target.note);
            if let Some(first) = spots.first() {
                let (before, name, after) = snippet(&body, first.clone(), SNIPPET_RADIUS);
                found.mentioned_in.push(Mention { scene, count: spots.len(), before, name, after });
            }
        }

        let world = project.config.world.as_deref();
        for named in names.items() {
            let nearby = match &named.owner {
                Owner::Project(p) => *p == project.slug,
                Owner::World(w) => Some(w.as_str()) == world,
            };
            if !nearby || *named == target {
                continue;
            }
            let body = self.notes_of(&named.owner)?.read(&named.note.path)?.markdown();
            let count = links(&body).iter().filter(|l| is_target(l.target)).count();
            if count > 0 {
                found.linked_from.push(Backlink { from: LinkSource::Note(named.clone()), count });
            }
        }
        Ok(found)
    }

    /// Turns the first unlinked mention of a note in a scene into a link that shows the same
    /// words. Returns whether there was one.
    pub fn link_mention(&self, project: &Project, scene: &str, owner: &Owner, path: &str) -> Result<bool> {
        let note = self.notes_of(owner)?.info(path)?;
        project.edit_scene(scene, |file| {
            let body = file.markdown();
            if let Some(spot) = mention_spots(&body, &note).into_iter().next() {
                let link = format_link(&note.title, Some(&body[spot.clone()]));
                file.set_markdown(&format!("{}{link}{}", &body[..spot.start], &body[spot.end..]));
            }
            Ok(())
        })
    }

    /// Retitles a note and updates every link and header that names it by its title, in every
    /// project and world. Links keep the words they show: `[[Old Teodor]]` becomes
    /// `[[Teodor Brask|Old Teodor]]`, so renaming never changes the prose.
    pub fn rename_note(&self, owner: &Owner, path: &str, title: &str) -> Result<Renamed> {
        let title = title.trim();
        if title.is_empty() {
            return Err(Error::Invalid("a note needs a title".into()));
        }
        let notes = self.notes_of(owner)?;
        let old = notes.info(path)?;
        let (old_key, new_key) = (name_key(&old.title), name_key(title));
        let mut renamed = Renamed {
            note: old.clone(),
            scenes: Vec::new(),
            notes: Vec::new(),
        };
        if new_key != old_key {
            if let Some(other) = notes.list()?.into_iter().find(|n| n.path != path && name_key(&n.title) == new_key) {
                return Err(Error::Invalid(format!("another note is already called “{}”", other.title)));
            }
            self.rewrite_names(owner, &old, title, &mut renamed)?;
        }
        renamed.note = notes.update_header(path, |h| h.set_str("title", title))?;
        Ok(renamed)
    }

    fn rewrite_names(&self, owner: &Owner, old: &NoteInfo, title: &str, renamed: &mut Renamed) -> Result<()> {
        let all = self.all_notes()?;
        let target = NamedNote { owner: owner.clone(), note: old.clone() };
        let rename = |names: &NameIndex<NamedNote>, name: &str| new_name(names, name, &target, title);

        let projects = self.projects()?;
        for project in &projects {
            let names = index(&all, &project.slug, project.config.world.as_deref());
            for slug in project.reading_order()? {
                if project.edit_scene(&slug, |file| rewrite_file(file, |n| rename(&names, n)))? {
                    renamed.scenes.push((project.slug.clone(), slug));
                }
            }
            let notes = project.notes();
            for note in notes.list()? {
                if notes.edit(&note.path, |file| rewrite_file(file, |n| rename(&names, n)))? && note.path != old.path {
                    renamed.notes.push((notes.owner().clone(), note.path));
                }
            }
        }
        for world in self.worlds()? {
            // A world note reads its links in whichever of the world's projects it's opened
            // from, so a link counts if any of them (or none) finds this note through it.
            let contexts: Vec<NameIndex<NamedNote>> = projects
                .iter()
                .filter(|p| p.config.world.as_deref() == Some(world.slug.as_str()))
                .map(|p| p.slug.as_str())
                .chain([""])
                .map(|p| index(&all, p, Some(&world.slug)))
                .collect();
            let rename_here = |n: &str| contexts.iter().find_map(|names| rename(names, n));
            let notes = world.notes();
            for note in notes.list()? {
                if notes.edit(&note.path, |file| rewrite_file(file, rename_here))? {
                    renamed.notes.push((notes.owner().clone(), note.path));
                }
            }
        }
        renamed.notes.retain(|(o, p)| !(o == owner && *p == old.path));
        Ok(())
    }

    /// Every note in the vault: each project's, then each world's.
    pub(crate) fn all_notes(&self) -> Result<Vec<NamedNote>> {
        let mut all = Vec::new();
        let worlds = self.worlds()?.into_iter().map(|w| w.notes());
        for notes in self.projects()?.into_iter().map(|p| p.notes()).chain(worlds) {
            let owner = notes.owner().clone();
            all.extend(notes.list()?.into_iter().map(|note| NamedNote { owner: owner.clone(), note }));
        }
        Ok(all)
    }
}

/// Looks names up as links in `project` (a folder name, or "" for none) do.
pub(crate) fn index(notes: &[NamedNote], project: &str, world: Option<&str>) -> NameIndex<NamedNote> {
    let mut index = NameIndex::new(project, world);
    for named in notes {
        index.add(&named.owner, &named.note.title, &named.note.aliases, named.clone());
    }
    index
}

fn points_at(names: &NameIndex<NamedNote>, name: &str, target: &NamedNote) -> bool {
    matches!(names.resolve(name), Resolution::Found(n) if n.owner == target.owner && n.note.path == target.note.path)
}

/// The renamed form of `name` if it names `target` by its title, keeping any `project/` prefix.
fn new_name(names: &NameIndex<NamedNote>, name: &str, target: &NamedNote, title: &str) -> Option<String> {
    if !points_at(names, name, target) {
        return None;
    }
    let old = name_key(&target.note.title);
    if name_key(name) == old {
        return Some(title.to_owned());
    }
    // `[[tidewater/Old Teodor]]` keeps its prefix.
    let (prefix, rest) = name.split_once('/')?;
    (name_key(rest) == old).then(|| format!("{prefix}/{title}"))
}

/// Renames links and header names in one file, with `rename` deciding each name's new form.
fn rewrite_file(file: &mut SceneFile, rename: impl Fn(&str) -> Option<String>) -> Result<()> {
    let body = file.markdown();
    let new_body = rewrite_links(&body, |link| {
        let target = rename(link.target)?;
        Some(format_link(&target, Some(link.label.unwrap_or(link.target))))
    });
    if new_body != body {
        file.set_markdown(&new_body);
    }
    // A header that isn't valid TOML is left for its writer to fix.
    if let Ok(mut header) = file.header()
        && rename_in_header(&mut header, &rename)
    {
        file.set_header(&header);
    }
    Ok(())
}

fn rename_in_header(header: &mut Header, rename: &impl Fn(&str) -> Option<String>) -> bool {
    let mut changed = false;
    for field in NAME_FIELDS {
        if let Some(value) = header.str(field) {
            if let Some(new) = rename(value) {
                header.set_str(field, &new);
                changed = true;
            }
            continue;
        }
        let items = header.list(field);
        let new_items: Vec<String> = items.iter().map(|item| rename(item).unwrap_or_else(|| item.clone())).collect();
        if new_items != items {
            header.set_list(field, &new_items);
            changed = true;
        }
    }
    changed
}

/// Where a note's title or aliases appear unlinked, longest name first where they overlap
/// ("Mara Venn" rather than "Mara" inside it).
fn mention_spots(markdown: &str, note: &NoteInfo) -> Vec<Range<usize>> {
    let mut spots: Vec<Range<usize>> = std::iter::once(&note.title)
        .chain(&note.aliases)
        .flat_map(|name| mentions(markdown, name))
        .collect();
    spots.sort_by_key(|r| (r.start, usize::MAX - r.end));
    let mut kept: Vec<Range<usize>> = Vec::new();
    for spot in spots {
        if kept.last().is_none_or(|last| spot.start >= last.end) {
            kept.push(spot);
        }
    }
    kept
}

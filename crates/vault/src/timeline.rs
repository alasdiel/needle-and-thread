//! A project's timeline (DESIGN §6): its scenes and plot points, and its world's plot points,
//! placed in story time from the `when` in their headers. Like the board, it's read from the
//! files every time it's asked for, so it's never out of date.

use std::collections::HashMap;
use std::fs;

use needle_core::calendar::Calendar;
use needle_core::header::Header;
use needle_core::links::name_key;
use needle_core::names::Owner;
use needle_core::project::NoteKind;
use needle_core::timeline::{self, Lookup, Placed, When};

use crate::{Error, Project, Result, Vault};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    Scene,
    /// A plot point (an `event` note).
    Event,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineItem {
    pub kind: ItemKind,
    pub owner: Owner,
    /// A scene's name in `manuscript/`, or a note's path among its owner's notes.
    pub path: String,
    pub id: String,
    pub title: String,
    /// A scene's place in reading order, from 1; `None` for plot points and for scenes the
    /// outline doesn't place.
    pub reading: Option<usize>,
    pub status: String,
    pub summary: String,
    /// For lanes: a scene's `pov`, and the threads, places and people in it.
    pub pov: Option<String>,
    pub threads: Vec<String>,
    pub places: Vec<String>,
    pub cast: Vec<String>,
    /// The `when` as written, for editing; empty if there's none.
    pub when: String,
    pub placed: Placed,
    /// Its time in words (`14 March 1998, 19:00`, `Day 4`), if it has one.
    pub time_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTimeline {
    /// The invented calendar's name; `None` for the real one.
    pub calendar: Option<String>,
    pub items: Vec<TimelineItem>,
    /// Item indices in story order. Items not in it wait in the tray.
    pub order: Vec<usize>,
    pub days_only: bool,
}

impl Vault {
    /// The calendar `project` uses: its own `[calendar]`, else its world's, else the real one.
    pub fn calendar(&self, project: &Project) -> Result<Calendar> {
        let read = |path: std::path::PathBuf, what: &str| -> Result<Option<Calendar>> {
            let text = fs::read_to_string(path)?;
            Calendar::from_settings(&text).map_err(|e| Error::Invalid(format!("the calendar in {what}: {e}")))
        };
        if let Some(calendar) = read(project.root().join("project.toml"), "project.toml")? {
            return Ok(calendar);
        }
        if let Some(world) = self.world_of(project)?
            && let Some(calendar) = read(world.root().join("world.toml"), "world.toml")?
        {
            return Ok(calendar);
        }
        Ok(Calendar::real())
    }

    pub fn timeline(&self, project: &Project) -> Result<ProjectTimeline> {
        let calendar = self.calendar(project)?;
        let mut items = Vec::new();
        let mut whens = Vec::new();

        let outline = project.outline()?;
        let reading: HashMap<&str, usize> = outline.scenes().enumerate().map(|(i, slug)| (slug, i + 1)).collect();
        for slug in project.reading_order()? {
            let (info, file) = project.read_scene(&slug)?;
            let header = file.header().unwrap_or_default();
            whens.push(When::from_header(&header, &calendar));
            items.push(TimelineItem {
                kind: ItemKind::Scene,
                owner: Owner::Project(project.slug.clone()),
                reading: reading.get(slug.as_str()).copied(),
                status: info.status,
                summary: info.summary,
                pov: header.str("pov").map(str::to_owned),
                threads: header.list("threads"),
                places: header.list("places"),
                cast: header.list("cast"),
                when: when_text(&header),
                id: info.id,
                title: info.title,
                path: slug,
                placed: Placed::default(),
                time_label: None,
            });
        }
        let mut owners = vec![project.notes()];
        if let Some(world) = self.world_of(project)? {
            owners.push(world.notes());
        }
        for notes in owners {
            for note in notes.list()?.into_iter().filter(|n| n.kind == NoteKind::Event) {
                let header = notes.read(&note.path)?.header().unwrap_or_default();
                whens.push(When::from_header(&header, &calendar));
                items.push(TimelineItem {
                    kind: ItemKind::Event,
                    owner: notes.owner().clone(),
                    reading: None,
                    status: String::new(),
                    summary: note.summary,
                    pov: None,
                    threads: header.list("threads"),
                    places: header.list("places"),
                    cast: header.list("involves"),
                    when: when_text(&header),
                    id: note.id,
                    title: note.title,
                    path: note.path,
                    placed: Placed::default(),
                    time_label: None,
                });
            }
        }

        let names = Names::of(&items, &aliases(self, project)?);
        let resolved = timeline::resolve(&calendar, &whens, |name| names.find(name));
        for (item, placed) in items.iter_mut().zip(resolved.items.iter()) {
            item.time_label = placed.time.map(|t| resolved.label(&calendar, t));
            item.placed = placed.clone();
        }
        Ok(ProjectTimeline { calendar: calendar.name.clone(), items, order: resolved.order, days_only: resolved.days_only })
    }
}

/// A plot point's aliases, by owner and path, so `when` can name it by one.
fn aliases(vault: &Vault, project: &Project) -> Result<HashMap<(Owner, String), Vec<String>>> {
    let mut found = HashMap::new();
    let mut owners = vec![project.notes()];
    if let Some(world) = vault.world_of(project)? {
        owners.push(world.notes());
    }
    for notes in owners {
        for note in notes.list()?.into_iter().filter(|n| n.kind == NoteKind::Event) {
            found.insert((notes.owner().clone(), note.path), note.aliases);
        }
    }
    Ok(found)
}

/// The `when` as the writer would type it again: the text itself, or a table's fields.
fn when_text(header: &Header) -> String {
    if let Some(text) = header.text("when") {
        return text;
    }
    match header.table("when") {
        Some(fields) => fields.iter().map(|(k, v)| format!("{k} = {v:?}")).collect::<Vec<_>>().join(", "),
        None => String::new(),
    }
}

/// Finding a scene or plot point by a name in a `when`. Titles come first, then plot points'
/// aliases, then scenes' file names; within each, this project's before its world's. Case,
/// spacing and apostrophes don't matter.
struct Names {
    tiers: Vec<HashMap<String, Vec<usize>>>,
}

impl Names {
    fn of(items: &[TimelineItem], aliases: &HashMap<(Owner, String), Vec<String>>) -> Self {
        let mut tiers: Vec<HashMap<String, Vec<usize>>> = vec![HashMap::new(); 6];
        for (i, item) in items.iter().enumerate() {
            let world = usize::from(matches!(item.owner, Owner::World(_)));
            tiers[world].entry(name_key(&item.title)).or_default().push(i);
            for alias in aliases.get(&(item.owner.clone(), item.path.clone())).into_iter().flatten() {
                tiers[2 + world].entry(name_key(alias)).or_default().push(i);
            }
            if item.kind == ItemKind::Scene {
                tiers[4].entry(name_key(&item.path)).or_default().push(i);
            }
        }
        Self { tiers }
    }

    fn find(&self, name: &str) -> Lookup {
        let key = name_key(name);
        for tier in &self.tiers {
            match tier.get(&key).map(Vec::as_slice) {
                Some([one]) => return Lookup::Found(*one),
                Some([_, _, ..]) => return Lookup::Ambiguous,
                _ => {}
            }
        }
        Lookup::Missing
    }
}

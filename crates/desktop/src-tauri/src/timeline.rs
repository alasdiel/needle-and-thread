//! Commands for the timeline (DESIGN §6): a project's scenes and plot points in story time,
//! and setting when something happens from the side panel.

use needle_core::names::Owner;
use needle_core::timeline::Problem;
use needle_vault::{ItemKind, ProjectTimeline, WhenEdit};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::notes::owner;
use crate::state::{AppState, OrString};

#[derive(Serialize)]
pub struct TimelineView {
    /// The invented calendar's name, or `None` for the real one.
    calendar: Option<String>,
    /// Every scene and plot point; `order` says which are on the timeline, and in what order.
    items: Vec<ItemView>,
    order: Vec<usize>,
    /// Times are days ("Day 4") rather than dates.
    days_only: bool,
}

#[derive(Serialize)]
pub struct ItemView {
    /// "scene" or "event".
    kind: &'static str,
    owner: String,
    world: bool,
    path: String,
    id: String,
    title: String,
    reading: Option<usize>,
    status: String,
    summary: String,
    pov: Option<String>,
    threads: Vec<String>,
    places: Vec<String>,
    cast: Vec<String>,
    /// The `when` as written, for editing.
    when: String,
    /// Its time in words, if it has one.
    time: Option<String>,
    /// Minutes from the calendar's start, for spacing things out; `None` when only its order
    /// is known.
    minute: Option<i64>,
    loose: bool,
    /// Why it isn't on the timeline, or is on it under protest, in words.
    problem: Option<String>,
}

fn item_kind(kind: &str) -> Result<ItemKind, String> {
    match kind {
        "scene" => Ok(ItemKind::Scene),
        "event" => Ok(ItemKind::Event),
        _ => Err(format!("{kind:?} isn't a scene or a plot point")),
    }
}

fn timeline_view(timeline: ProjectTimeline) -> TimelineView {
    let titles: Vec<String> = timeline.items.iter().map(|i| i.title.clone()).collect();
    let problem = |p: &Problem| match p {
        Problem::Unreadable(why) => why.clone(),
        Problem::Missing(name) => format!("nothing is called {name}"),
        Problem::Ambiguous(name) => format!("more than one thing is called {name}"),
        Problem::Cycle => "its time comes back round to itself".into(),
        Problem::WaitsOn(i) => format!("waits for {}, which isn't placed", titles[*i]),
        Problem::Contradiction => "it can't come both after and before those".into(),
        Problem::NoDayOne => "there's no Day 1: set day_one in the calendar".into(),
    };
    TimelineView {
        calendar: timeline.calendar,
        items: timeline
            .items
            .iter()
            .map(|item| {
                let (owner, world) = match &item.owner {
                    Owner::Project(slug) => (slug.clone(), false),
                    Owner::World(slug) => (slug.clone(), true),
                };
                ItemView {
                    kind: match item.kind {
                        ItemKind::Scene => "scene",
                        ItemKind::Event => "event",
                    },
                    owner,
                    world,
                    path: item.path.clone(),
                    id: item.id.clone(),
                    title: item.title.clone(),
                    reading: item.reading,
                    status: item.status.clone(),
                    summary: item.summary.clone(),
                    pov: item.pov.clone(),
                    threads: item.threads.clone(),
                    places: item.places.clone(),
                    cast: item.cast.clone(),
                    when: item.when.clone(),
                    time: item.time_label.clone(),
                    minute: item.placed.time.map(|t| t.minute),
                    loose: item.placed.loose,
                    problem: item.placed.problem.as_ref().map(problem),
                }
            })
            .collect(),
        order: timeline.order,
        days_only: timeline.days_only,
    }
}

#[tauri::command]
pub fn project_timeline(state: State<'_, AppState>, project: String) -> Result<TimelineView, String> {
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        Ok(timeline_view(open.vault.timeline(&project).or_string()?))
    })
}

/// A new `when`, as the side panel sends it.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WhenInput {
    Clear,
    Text { text: String },
    From { from: String, offset: String },
    Order { after: Option<String>, before: Option<String> },
}

/// Sets when a scene or plot point happens, and returns the timeline as it now stands.
#[tauri::command]
pub fn set_when(
    state: State<'_, AppState>,
    project: String,
    kind: String,
    item_owner: String,
    world: bool,
    path: String,
    when: WhenInput,
) -> Result<TimelineView, String> {
    let kind = item_kind(&kind)?;
    let edit = match when {
        WhenInput::Clear => WhenEdit::Clear,
        WhenInput::Text { text } => WhenEdit::Text(text),
        WhenInput::From { from, offset } => WhenEdit::From { from, offset },
        WhenInput::Order { after, before } => WhenEdit::Order { after, before },
    };
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        open.vault.set_when(&project, kind, &owner(item_owner, world), &path, &edit).or_string()?;
        open.history.edited();
        Ok(timeline_view(open.vault.timeline(&project).or_string()?))
    })
}

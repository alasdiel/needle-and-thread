//! Commands for the network board (DESIGN §7): reading what's on it, saving how it's
//! arranged, and the notes made or changed from it (a new plot point, a string tied between
//! two cards).
//!
//! What's on the board is read from the notes every time it's asked for, so it's never stale.
//! The arrangement is the only thing written, and it's written whole.

use needle_core::names::Owner;
use needle_core::network::{Mark, Network, Node, Point, Zone};
use needle_core::project::NoteKind;
use needle_vault::{Board, RelationshipEdit};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::notes::{NoteView, note_view, owner};
use crate::state::{AppState, OrString};

#[derive(Serialize)]
pub struct BoardView {
    cards: Vec<CardView>,
    relationships: Vec<RelationshipView>,
    links: Vec<LinkView>,
    zones: Vec<Zone>,
    marks: Vec<Mark>,
}

#[derive(Serialize)]
pub struct CardView {
    id: String,
    /// The project or world folder the note belongs to, and which of the two it is, so the card
    /// can be opened and can say where it came from.
    owner: String,
    world: bool,
    path: String,
    /// "character", "place", "event" or "thread": which paper the card is.
    kind: &'static str,
    title: String,
    /// Empty for this project's own notes.
    from: String,
    at: Point,
    turn: f64,
    /// Up only because it was pinned, so it can be taken down.
    pinned: bool,
}

#[derive(Serialize)]
pub struct RelationshipView {
    id: String,
    owner: String,
    world: bool,
    path: String,
    label: String,
    from: String,
    to: String,
    directed: bool,
    begins: Option<String>,
    ends: Option<String>,
    changes: Vec<ChangeView>,
}

#[derive(Serialize)]
pub struct ChangeView {
    at: String,
    label: String,
}

#[derive(Serialize)]
pub struct LinkView {
    from: String,
    to: String,
}

/// The arrangement as the board sends it back: every card's spot, the zones and the marks.
#[derive(Deserialize)]
pub struct LayoutView {
    nodes: Vec<PinnedView>,
    #[serde(default)]
    zones: Vec<Zone>,
    #[serde(default)]
    marks: Vec<Mark>,
}

#[derive(Deserialize)]
pub struct PinnedView {
    id: String,
    at: Point,
    #[serde(default)]
    turn: f64,
}

fn board_view(board: Board) -> BoardView {
    let owner_of = |owner: &Owner| match owner {
        Owner::Project(slug) => (slug.clone(), false),
        Owner::World(slug) => (slug.clone(), true),
    };
    BoardView {
        cards: board
            .cards
            .into_iter()
            .map(|c| {
                let (owner, world) = owner_of(&c.owner);
                CardView {
                    id: c.id,
                    owner,
                    world,
                    path: c.path,
                    kind: c.kind.as_str(),
                    title: c.title,
                    from: c.from,
                    at: c.at,
                    turn: c.turn,
                    pinned: c.pinned,
                }
            })
            .collect(),
        relationships: board
            .relationships
            .into_iter()
            .map(|r| {
                let (owner, world) = owner_of(&r.owner);
                RelationshipView {
                    id: r.id,
                    owner,
                    world,
                    path: r.path,
                    label: r.label,
                    from: r.between.0,
                    to: r.between.1,
                    directed: r.directed,
                    begins: r.begins,
                    ends: r.ends,
                    changes: r.changes.into_iter().map(|c| ChangeView { at: c.at, label: c.label }).collect(),
                }
            })
            .collect(),
        links: board.links.into_iter().map(|l| LinkView { from: l.from, to: l.to }).collect(),
        zones: board.zones,
        marks: board.marks,
    }
}

/// What's on the project's board, and where it all sits. Cards that had never been placed are
/// given room here, and that arrangement is saved at once so they stay put.
#[tauri::command]
pub fn project_board(state: State<'_, AppState>, project: String) -> Result<BoardView, String> {
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        let board = open.vault.board(&project).or_string()?;
        if board.placed_new {
            let mut layout = project.network().or_string()?;
            for card in &board.cards {
                layout.nodes.insert(card.id.clone(), Node { at: card.at, turn: card.turn });
            }
            open.vault.save_board(&project, &layout).or_string()?;
        }
        Ok(board_view(board))
    })
}

/// Saves the whole arrangement: where every card is pinned, the zones and the marks. The board
/// sends all of it, so one dragged card and one rubbed-out mark take the same path.
#[tauri::command]
pub fn save_board(state: State<'_, AppState>, project: String, layout: LayoutView) -> Result<(), String> {
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        let mut network = Network::default();
        for pinned in layout.nodes {
            network.nodes.insert(pinned.id, Node { at: pinned.at, turn: pinned.turn });
        }
        network.zones = layout.zones;
        network.marks = layout.marks;
        open.vault.save_board(&project, &network).or_string()?;
        // The board is part of the project's files, so arranging it is an edit like any other
        // and belongs in the next snapshot.
        open.history.edited();
        Ok(())
    })
}

/// Makes a note of `kind` and pins its card at `at`: double-clicking the cork adds a plot point.
#[tauri::command]
pub fn add_card(state: State<'_, AppState>, project: String, kind: String, title: String, at: Point) -> Result<NoteView, String> {
    let kind = NoteKind::parse(&kind).ok_or_else(|| format!("{kind:?} isn't a type of note"))?;
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        let note = open.vault.add_card(&project, kind, &title, at).or_string()?;
        open.history.edited();
        Ok(note_view(&Owner::Project(project.slug.clone()), note))
    })
}

/// Pins a note that isn't up on its own (a world note, another project's, a plain note).
#[tauri::command]
pub fn pin_card(state: State<'_, AppState>, project: String, id: String, at: Point) -> Result<(), String> {
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        open.vault.pin(&project, &id, at).or_string()?;
        open.history.edited();
        Ok(())
    })
}

/// Takes a pinned card down again, leaving its note alone.
#[tauri::command]
pub fn unpin_card(state: State<'_, AppState>, project: String, id: String) -> Result<(), String> {
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        open.vault.unpin(&project, &id).or_string()?;
        open.history.edited();
        Ok(())
    })
}

/// Ties a string from one card to another: a new relationship note in this project.
#[tauri::command]
pub fn add_relationship(
    state: State<'_, AppState>,
    project: String,
    from: String,
    to: String,
    label: String,
    directed: bool,
) -> Result<NoteView, String> {
    state.with(|open| {
        let project = open.vault.project(&project).or_string()?;
        let note = open.vault.add_relationship(&project, &from, &to, &label, directed).or_string()?;
        open.history.edited();
        Ok(note_view(&Owner::Project(project.slug.clone()), note))
    })
}

/// Relabels a relationship, makes it one-way or mutual, or turns it round.
#[tauri::command]
pub fn edit_relationship(
    state: State<'_, AppState>,
    owner: String,
    world: bool,
    path: String,
    label: Option<String>,
    directed: Option<bool>,
    reverse: Option<bool>,
) -> Result<NoteView, String> {
    let owner = self::owner(owner, world);
    let edit = RelationshipEdit { label, directed, reverse: reverse.unwrap_or(false) };
    state.with(|open| {
        let note = open.vault.edit_relationship(&owner, &path, &edit).or_string()?;
        open.history.edited();
        Ok(note_view(&owner, note))
    })
}

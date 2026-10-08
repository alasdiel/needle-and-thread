//! Commands for the network board (DESIGN §7): reading what's on it, and saving how it's
//! arranged.
//!
//! What's on the board is read from the notes every time it's asked for, so it's never stale.
//! The arrangement is the only thing written, and it's written whole.

use needle_core::names::Owner;
use needle_core::network::{Mark, Network, Node, Point, Zone};
use needle_vault::Board;
use serde::{Deserialize, Serialize};
use tauri::State;

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

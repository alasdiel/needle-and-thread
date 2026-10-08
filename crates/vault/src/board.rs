//! What's on a project's network board (DESIGN §7): the cards, the relationships strung
//! between them, and the thin automatic links.
//!
//! None of this is stored. The board is read from the notes themselves every time it's asked
//! for, the way backlinks are (`links.rs`), so it can never be out of date with the files.
//! Only the arrangement is saved, in `network.toml`.

use needle_core::links::links;
use needle_core::names::{Owner, Resolution};
use needle_core::network::{Network, Node, Point};
use needle_core::project::NoteKind;

use crate::links::NAME_FIELDS;
use crate::{NamedNote, Project, Result, Vault};

/// The kinds of note that get a card. Relationships are lines rather than cards, and plain
/// notes and sources stay off unless the writer pins one up themselves.
const CARD_KINDS: [NoteKind; 4] = [NoteKind::Character, NoteKind::Place, NoteKind::Event, NoteKind::Thread];

#[derive(Debug, Clone, PartialEq)]
pub struct Board {
    pub cards: Vec<Card>,
    pub relationships: Vec<Relationship>,
    /// Thin links, from `[[links]]` in a card's text and from the name fields in its header.
    pub links: Vec<Link>,
    pub zones: Vec<needle_core::network::Zone>,
    pub marks: Vec<needle_core::network::Mark>,
    /// True when cards were placed that had no spot saved, so the caller knows the arrangement
    /// is worth writing back.
    pub placed_new: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub id: String,
    pub owner: Owner,
    /// The note's path within its owner, e.g. `characters/mara-venn`.
    pub path: String,
    pub kind: NoteKind,
    pub title: String,
    /// Empty for this project's own notes; otherwise the world or project the note comes from,
    /// which the card shows.
    pub from: String,
    pub at: Point,
    pub turn: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    /// The relationship note's own id, since a relationship belongs to neither end.
    pub id: String,
    pub owner: Owner,
    pub path: String,
    pub label: String,
    /// Card ids. Reads from the first to the second when `directed`.
    pub between: (String, String),
    pub directed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub from: String,
    pub to: String,
}

impl Vault {
    /// Reads the board for a project: which cards are up, what joins them, and where it all
    /// sits. Cards never placed are given room near whatever they link to.
    pub fn board(&self, project: &Project) -> Result<Board> {
        let names = self.names(project)?;
        let layout = project.network()?;
        let world = project.config.world.as_deref();

        // This project's own notes are always up. A note from its world is up only once the
        // writer has pinned it, which is what an entry in the arrangement means.
        let mut up: Vec<&NamedNote> = Vec::new();
        for named in names.items() {
            let own = matches!(&named.owner, Owner::Project(p) if *p == project.slug);
            let kind_fits = CARD_KINDS.contains(&named.note.kind);
            let pinned = layout.nodes.contains_key(&named.note.id);
            if (own && kind_fits) || (!own && pinned) {
                up.push(named);
            }
        }
        // A stable order, so two runs place new cards the same way.
        up.sort_by(|a, b| a.note.title.to_lowercase().cmp(&b.note.title.to_lowercase()).then_with(|| a.note.id.cmp(&b.note.id)));

        let is_up = |id: &str| up.iter().any(|n| n.note.id == id);
        let find = |name: &str| match names.resolve(name) {
            Resolution::Found(note) if is_up(&note.note.id) => Some(note.note.id.clone()),
            _ => None,
        };

        // What each card points at, which is both the thin links and the hint for where a new
        // card should go.
        let mut links_found: Vec<Link> = Vec::new();
        for named in &up {
            let file = self.notes_of(&named.owner)?.read(&named.note.path)?;
            let header = file.header().unwrap_or_default();
            let body = file.markdown();
            let named_in_header = NAME_FIELDS.iter().flat_map(|field| header.list(field));
            let linked_in_text = links(&body).into_iter().map(|l| l.target.to_owned());
            for name in named_in_header.chain(linked_in_text) {
                if let Some(to) = find(&name).filter(|to| *to != named.note.id) {
                    links_found.push(Link { from: named.note.id.clone(), to });
                }
            }
        }
        dedupe_links(&mut links_found);

        let mut relationships = Vec::new();
        for named in names.items() {
            let nearby = match &named.owner {
                Owner::Project(p) => *p == project.slug,
                Owner::World(w) => Some(w.as_str()) == world,
            };
            if !nearby || named.note.kind != NoteKind::Relationship {
                continue;
            }
            let file = self.notes_of(&named.owner)?.read(&named.note.path)?;
            let header = file.header().unwrap_or_default();
            let ends: Vec<String> = header.list("between").iter().filter_map(|name| find(name)).collect();
            // A relationship with an end that isn't up, or that names the same card twice, has
            // nothing to draw; it stays in the notes and simply isn't on the board.
            let [from, to] = &ends[..] else { continue };
            if from == to {
                continue;
            }
            relationships.push(Relationship {
                id: named.note.id.clone(),
                owner: named.owner.clone(),
                path: named.note.path.clone(),
                label: header.str("label").unwrap_or_default().to_owned(),
                between: (from.clone(), to.clone()),
                directed: header.bool("directed").unwrap_or(false),
            });
        }
        relationships.sort_by(|a, b| a.id.cmp(&b.id));

        // A relationship and an automatic link between the same two cards would draw string
        // and twine along the same line, so the string wins.
        links_found.retain(|link| !relationships.iter().any(|r| joins(&r.between, link)));

        let mut layout = layout;
        let mut placed_new = false;
        let mut cards = Vec::with_capacity(up.len());
        for named in &up {
            let id = &named.note.id;
            let node = match layout.nodes.get(id) {
                Some(node) => *node,
                None => {
                    let near: Vec<String> = links_found
                        .iter()
                        .filter_map(|l| match (l.from == *id, l.to == *id) {
                            (true, _) => Some(l.to.clone()),
                            (_, true) => Some(l.from.clone()),
                            _ => None,
                        })
                        .collect();
                    let node = Node { at: layout.room_for(&near), turn: 0.0 };
                    layout.nodes.insert(id.clone(), node);
                    placed_new = true;
                    node
                }
            };
            cards.push(Card {
                id: id.clone(),
                owner: named.owner.clone(),
                path: named.note.path.clone(),
                kind: named.note.kind,
                title: named.note.title.clone(),
                from: match &named.owner {
                    Owner::Project(p) if *p == project.slug => String::new(),
                    Owner::Project(p) => p.clone(),
                    Owner::World(w) => w.clone(),
                },
                at: node.at,
                turn: node.turn,
            });
        }

        Ok(Board {
            cards,
            relationships,
            links: links_found,
            zones: layout.zones,
            marks: layout.marks,
            placed_new,
        })
    }

    /// Saves where the cards, zones and marks sit. The arrangement is the app's to write, so
    /// the file is replaced whole.
    pub fn save_board(&self, project: &Project, network: &Network) -> Result<()> {
        project.save_network(network)
    }
}

/// One line per pair, whichever way round they were found, and never two between the same two
/// cards however many times they name each other.
fn dedupe_links(found: &mut Vec<Link>) {
    let mut seen: Vec<(String, String)> = Vec::new();
    found.retain(|link| {
        let pair = if link.from <= link.to {
            (link.from.clone(), link.to.clone())
        } else {
            (link.to.clone(), link.from.clone())
        };
        let new = !seen.contains(&pair);
        if new {
            seen.push(pair);
        }
        new
    });
    found.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
}

fn joins(between: &(String, String), link: &Link) -> bool {
    (between.0 == link.from && between.1 == link.to) || (between.0 == link.to && between.1 == link.from)
}

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
use crate::{Error, NamedNote, NoteInfo, Project, Result, Vault};

/// The kinds of note that are up on their own. Relationships are lines rather than cards, and
/// plain notes and sources stay off unless the writer pins one up themselves.
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
    /// Up only because the writer pinned it (a world note, another project's, or a plain note),
    /// so it can be taken down again. The rest are up because they exist.
    pub pinned: bool,
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
    /// Where in the story it starts and stops, if the note says: plot points or scenes, by name.
    pub begins: Option<String>,
    pub ends: Option<String>,
    /// How its label changes along the way, in the order written. Until there's a timeline to
    /// slide along (M6), the board shows how things stand at the end: the last of these.
    pub changes: Vec<Change>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub at: String,
    pub label: String,
}

/// What can be changed about a relationship from the board.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelationshipEdit {
    pub label: Option<String>,
    pub directed: Option<bool>,
    /// Swaps its two ends, so a one-way relationship reads the other way.
    pub reverse: bool,
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

        // This project's characters, places, plot points and threads are always up. Anything
        // else (a world note, another project's, a plain note) is up only once the writer has
        // pinned it, which is what an entry in the arrangement means.
        let mut up: Vec<&NamedNote> = Vec::new();
        for named in names.items() {
            let pinned = layout.nodes.contains_key(&named.note.id);
            if named.note.kind != NoteKind::Relationship && (up_anyway(project, named) || pinned) {
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
            let named_field = |key: &str| header.str(key).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
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
                begins: named_field("begins"),
                ends: named_field("ends"),
                changes: header
                    .tables("changes")
                    .into_iter()
                    .filter_map(|fields| {
                        let field = |key: &str| fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
                        Some(Change { at: field("at")?, label: field("label").unwrap_or_default() })
                    })
                    .collect(),
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
                pinned: !up_anyway(project, named),
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

    /// Makes a new note of `kind` and pins it up at `at`, as double-clicking the board does for
    /// a plot point.
    pub fn add_card(&self, project: &Project, kind: NoteKind, title: &str, at: Point) -> Result<NoteInfo> {
        let note = project.notes().create(kind, title)?;
        self.pin(project, &note.id, at)?;
        Ok(note)
    }

    /// Pins a note's card up at `at`, or moves it there if it's up already.
    pub fn pin(&self, project: &Project, id: &str, at: Point) -> Result<()> {
        let mut layout = project.network()?;
        layout.nodes.insert(id.to_owned(), Node { at, turn: 0.0 });
        project.save_network(&layout)
    }

    /// Takes a pinned card down. The note itself is untouched; a note that's up because it
    /// exists (one of this project's characters, say) just comes back, so the board doesn't
    /// offer that.
    pub fn unpin(&self, project: &Project, id: &str) -> Result<()> {
        let mut layout = project.network()?;
        layout.nodes.remove(id);
        // Handwriting on the card would otherwise jump to wherever its offset puts it.
        layout.marks.retain(|mark| !matches!(mark, needle_core::network::Mark::Note { on: Some(card), .. } if card == id));
        project.save_network(&layout)
    }

    /// Strings a new relationship between two cards on the board, reading from `from` to `to`
    /// when `directed`. It's a note of its own in this project's `relationships/`, titled after
    /// its two ends, so it belongs to neither of them.
    pub fn add_relationship(&self, project: &Project, from: &str, to: &str, label: &str, directed: bool) -> Result<NoteInfo> {
        let names = self.names(project)?;
        let end = |id: &str| -> Result<(String, String)> {
            let named = names
                .items()
                .iter()
                .find(|n| n.note.id == id)
                .ok_or_else(|| Error::Invalid(format!("there's no note with id {id} to tie a string to")))?;
            let title = named.note.title.clone();
            // The name the header uses has to find this very note again. Another project's note
            // needs its project's name in front, and so does one whose title is shadowed.
            let plain_finds_it = matches!(names.resolve(&title), Resolution::Found(n) if n.note.id == id);
            let name = match &named.owner {
                Owner::Project(p) if !plain_finds_it => format!("{p}/{title}"),
                _ => title.clone(),
            };
            Ok((name, title))
        };
        let ((from_name, from_title), (to_name, to_title)) = (end(from)?, end(to)?);
        if from == to {
            return Err(Error::Invalid("a string needs two different cards".into()));
        }
        let notes = project.notes();
        let note = notes.create(NoteKind::Relationship, &format!("{from_title} and {to_title}"))?;
        notes.update_header(&note.path, |h| {
            h.set_list("between", &[from_name, to_name]);
            h.set_str("label", label.trim());
            if directed {
                h.set_bool("directed", true);
            }
        })
    }

    /// Changes a relationship's label or direction from the board, keeping the rest of its
    /// header as written.
    pub fn edit_relationship(&self, owner: &Owner, path: &str, edit: &RelationshipEdit) -> Result<NoteInfo> {
        self.notes_of(owner)?.update_header(path, |h| {
            if let Some(label) = &edit.label {
                h.set_str("label", label.trim());
            }
            match edit.directed {
                Some(true) => h.set_bool("directed", true),
                // Mutual is the default, so it's written by leaving the key out.
                Some(false) => h.remove("directed"),
                None => {}
            }
            if edit.reverse {
                let mut between = h.list("between");
                between.reverse();
                h.set_list("between", &between);
            }
        })
    }

    /// Saves where the cards, zones and marks sit. The arrangement is the app's to write, so
    /// the file is replaced whole.
    pub fn save_board(&self, project: &Project, network: &Network) -> Result<()> {
        project.save_network(network)
    }
}

/// Whether a note is up on the board without being pinned: one of the project's own notes of a
/// kind that always gets a card.
fn up_anyway(project: &Project, named: &NamedNote) -> bool {
    let own = matches!(&named.owner, Owner::Project(p) if *p == project.slug);
    own && CARD_KINDS.contains(&named.note.kind)
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

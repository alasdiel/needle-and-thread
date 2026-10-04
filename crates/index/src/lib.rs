//! The search index (docs/DESIGN.md §3): a SQLite cache of every scene's and note's title,
//! header names and text, for ranked full-text search with filters.
//!
//! It's only a cache. It lives outside the vault, is brought up to date from the files before
//! each search (a stat of every file, reading only those that changed), and can be deleted at
//! any time.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use needle_core::links::name_key;
use needle_core::project::NoteKind;
use needle_core::scene::SceneFile;
use needle_core::words::plain_text;
use needle_vault::{NAME_FIELDS, Vault};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

/// Bumped whenever the tables change; an index from another version is rebuilt.
const SCHEMA_VERSION: &str = "1";

/// Marks around matches in snippets; neither can occur in a vault's text.
const MATCH_START: char = '\u{1}';
const MATCH_END: char = '\u{2}';

#[derive(Debug)]
pub enum Error {
    Sql(rusqlite::Error),
    Vault(needle_vault::Error),
    Io(std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sql(e) => write!(f, "search index: {e}"),
            Self::Vault(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}

impl From<needle_vault::Error> for Error {
    fn from(e: needle_vault::Error) -> Self {
        Self::Vault(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Scene,
    Note,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Scene => "scene",
            Self::Note => "note",
        }
    }

    fn parse(value: &str) -> Self {
        if value == "scene" { Self::Scene } else { Self::Note }
    }
}

/// What to look for. Every filter that's set must match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// Words, each matching the start of a word ("ledg" finds "ledger"), and "exact phrases"
    /// in quotes. Empty lists everything the filters allow, by title.
    pub text: String,
    /// Projects and worlds to search, by folder name. Both empty means the whole vault.
    pub projects: Vec<String>,
    pub worlds: Vec<String>,
    pub kind: Option<Kind>,
    pub status: Option<String>,
    /// A note's type, e.g. `character`.
    pub note_type: Option<String>,
    /// Header fields that must name one of these, e.g. `pov` with a character's title and
    /// aliases.
    pub names: Vec<(String, Vec<String>)>,
    /// At most this many results; 0 means 50.
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub kind: Kind,
    pub project: Option<String>,
    pub world: Option<String>,
    /// A scene's file name in `manuscript/`, or a note's path among its owner's notes.
    pub key: String,
    pub title: String,
    pub status: Option<String>,
    pub note_type: Option<String>,
    /// The text around the best match, in pieces: (text, whether it's the match).
    pub snippet: Vec<(String, bool)>,
}

/// What a sync changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Synced {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
}

/// A scene or note file the vault has now.
struct Source {
    /// From the vault's root, with `/`: `projects/tidewater/manuscript/night-market.md`.
    path: String,
    file: PathBuf,
    kind: Kind,
    project: Option<String>,
    world: Option<String>,
    key: String,
}

/// What the index keeps of a file.
struct Parsed {
    title: String,
    status: Option<String>,
    note_type: Option<String>,
    aliases: Vec<String>,
    names: Vec<(&'static str, String)>,
    body: String,
}

pub struct Index {
    conn: Connection,
}

impl Index {
    /// Opens the index in `file`, making it if needed. One from an older version of the app
    /// is emptied, to be filled again by the next sync.
    pub fn open(file: &Path) -> Result<Self> {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        Self::start(Connection::open(file)?)
    }

    /// An index that's never saved, for tests.
    pub fn in_memory() -> Result<Self> {
        Self::start(Connection::open_in_memory()?)
    }

    fn start(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);")?;
        let version: Option<String> = conn
            .query_row("SELECT value FROM meta WHERE key = 'schema'", [], |row| row.get(0))
            .optional()?;
        if version.as_deref() != Some(SCHEMA_VERSION) {
            conn.execute_batch(
                "DROP TABLE IF EXISTS names;
                 DROP TABLE IF EXISTS text;
                 DROP TABLE IF EXISTS docs;
                 CREATE TABLE docs (
                     id INTEGER PRIMARY KEY,
                     path TEXT NOT NULL UNIQUE,
                     kind TEXT NOT NULL,
                     project TEXT,
                     world TEXT,
                     key TEXT NOT NULL,
                     title TEXT NOT NULL,
                     status TEXT,
                     note_type TEXT,
                     modified INTEGER NOT NULL,
                     size INTEGER NOT NULL
                 );
                 CREATE TABLE names (
                     doc INTEGER NOT NULL REFERENCES docs (id) ON DELETE CASCADE,
                     field TEXT NOT NULL,
                     key TEXT NOT NULL
                 );
                 CREATE INDEX names_by_doc ON names (doc);
                 CREATE VIRTUAL TABLE text USING fts5 (title, aliases, body, tokenize = 'unicode61 remove_diacritics 2');",
            )?;
            conn.execute(
                "INSERT INTO meta (key, value) VALUES ('schema', ?1) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                [SCHEMA_VERSION],
            )?;
        }
        Ok(Self { conn })
    }

    /// Brings the index up to date with the vault's files: reads new and changed ones, and
    /// forgets those that are gone.
    pub fn sync(&mut self, vault: &Vault) -> Result<Synced> {
        let mut known: HashMap<String, (i64, i64, i64)> = HashMap::new();
        {
            let mut stmt = self.conn.prepare("SELECT path, id, modified, size FROM docs")?;
            let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, (row.get(1)?, row.get(2)?, row.get(3)?))))?;
            for row in rows {
                let (path, entry) = row?;
                known.insert(path, entry);
            }
        }

        let mut synced = Synced::default();
        let tx = self.conn.transaction()?;
        let mut seen = HashSet::new();
        for source in sources(vault)? {
            seen.insert(source.path.clone());
            let Ok(meta) = fs::metadata(&source.file) else { continue };
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos() as i64);
            let size = meta.len() as i64;
            let existing = known.get(&source.path).copied();
            if existing.is_some_and(|(_, m, s)| m == modified && s == size) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&source.file) else { continue };
            let parsed = parse(&source, &text);
            let id = match existing {
                Some((id, _, _)) => {
                    tx.execute(
                        "UPDATE docs SET kind = ?2, project = ?3, world = ?4, key = ?5, title = ?6, status = ?7,
                         note_type = ?8, modified = ?9, size = ?10 WHERE id = ?1",
                        params![
                            id,
                            source.kind.as_str(),
                            source.project,
                            source.world,
                            source.key,
                            parsed.title,
                            parsed.status,
                            parsed.note_type,
                            modified,
                            size
                        ],
                    )?;
                    tx.execute("DELETE FROM text WHERE rowid = ?1", [id])?;
                    tx.execute("DELETE FROM names WHERE doc = ?1", [id])?;
                    synced.updated += 1;
                    id
                }
                None => {
                    tx.execute(
                        "INSERT INTO docs (path, kind, project, world, key, title, status, note_type, modified, size)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![
                            source.path,
                            source.kind.as_str(),
                            source.project,
                            source.world,
                            source.key,
                            parsed.title,
                            parsed.status,
                            parsed.note_type,
                            modified,
                            size
                        ],
                    )?;
                    synced.added += 1;
                    tx.last_insert_rowid()
                }
            };
            tx.execute(
                "INSERT INTO text (rowid, title, aliases, body) VALUES (?1, ?2, ?3, ?4)",
                params![id, parsed.title, parsed.aliases.join("\n"), parsed.body],
            )?;
            for (field, name) in &parsed.names {
                tx.execute("INSERT INTO names (doc, field, key) VALUES (?1, ?2, ?3)", params![id, field, name_key(name)])?;
            }
        }
        for (path, (id, _, _)) in &known {
            if !seen.contains(path) {
                tx.execute("DELETE FROM text WHERE rowid = ?1", [id])?;
                tx.execute("DELETE FROM docs WHERE id = ?1", [id])?;
                synced.removed += 1;
            }
        }
        tx.commit()?;
        Ok(synced)
    }

    /// Scenes and notes matching `query`, best first: a match in a title counts most, then an
    /// alias, then the text.
    pub fn search(&self, query: &Query) -> Result<Vec<Hit>> {
        let mut filters: Vec<String> = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        let matching = match_expression(&query.text);
        if let Some(expression) = &matching {
            filters.push("text MATCH ?".into());
            values.push(Value::Text(expression.clone()));
        }
        if !query.projects.is_empty() || !query.worlds.is_empty() {
            let marks = |n: usize| vec!["?"; n].join(", ");
            filters.push(format!(
                "(d.project IN ({}) OR d.world IN ({}))",
                marks(query.projects.len().max(1)),
                marks(query.worlds.len().max(1))
            ));
            for list in [&query.projects, &query.worlds] {
                if list.is_empty() {
                    values.push(Value::Null);
                }
                values.extend(list.iter().map(|p| Value::Text(p.clone())));
            }
        }
        if let Some(kind) = query.kind {
            filters.push("d.kind = ?".into());
            values.push(Value::Text(kind.as_str().into()));
        }
        for (column, value) in [("d.status", &query.status), ("d.note_type", &query.note_type)] {
            if let Some(value) = value {
                filters.push(format!("{column} = ?"));
                values.push(Value::Text(value.clone()));
            }
        }
        for (field, names) in &query.names {
            filters.push(format!(
                "EXISTS (SELECT 1 FROM names n WHERE n.doc = d.id AND n.field = ? AND n.key IN ({}))",
                vec!["?"; names.len().max(1)].join(", ")
            ));
            values.push(Value::Text(field.clone()));
            if names.is_empty() {
                values.push(Value::Null);
            }
            values.extend(names.iter().map(|n| Value::Text(name_key(n))));
        }
        let limit = if query.limit == 0 { 50 } else { query.limit };
        let filter = if filters.is_empty() { String::new() } else { format!("WHERE {}", filters.join(" AND ")) };
        let (snippet, order) = match matching {
            Some(_) => (
                format!("snippet(text, 2, '{MATCH_START}', '{MATCH_END}', '…', 18)"),
                "bm25(text, 10.0, 6.0, 1.0)",
            ),
            None => ("substr(text.body, 1, 160)".to_owned(), "d.kind DESC, d.title COLLATE NOCASE"),
        };
        let sql = format!(
            "SELECT d.kind, d.project, d.world, d.key, d.title, d.status, d.note_type, {snippet}
             FROM text JOIN docs d ON d.id = text.rowid {filter} ORDER BY {order} LIMIT {limit}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let hits = stmt
            .query_map(params_from_iter(values), |row| {
                Ok(Hit {
                    kind: Kind::parse(&row.get::<_, String>(0)?),
                    project: row.get(1)?,
                    world: row.get(2)?,
                    key: row.get(3)?,
                    title: row.get(4)?,
                    status: row.get(5)?,
                    note_type: row.get(6)?,
                    snippet: pieces(&row.get::<_, String>(7)?),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(hits)
    }
}

/// Every scene and note file in the vault.
fn sources(vault: &Vault) -> Result<Vec<Source>> {
    let root = vault.root();
    let relative = |file: &Path| {
        let path = file.strip_prefix(root).unwrap_or(file);
        path.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
    };
    let mut found = Vec::new();
    for project in vault.projects()? {
        for slug in project.reading_order()? {
            let file = project.scene_path(&slug)?;
            found.push(Source {
                path: relative(&file),
                file,
                kind: Kind::Scene,
                project: Some(project.slug.clone()),
                world: None,
                key: slug,
            });
        }
        let notes = project.notes();
        for path in notes.paths()? {
            let file = notes.file_path(&path)?;
            found.push(Source {
                path: relative(&file),
                file,
                kind: Kind::Note,
                project: Some(project.slug.clone()),
                world: None,
                key: path,
            });
        }
    }
    for world in vault.worlds()? {
        let notes = world.notes();
        for path in notes.paths()? {
            let file = notes.file_path(&path)?;
            found.push(Source {
                path: relative(&file),
                file,
                kind: Kind::Note,
                project: None,
                world: Some(world.slug.clone()),
                key: path,
            });
        }
    }
    Ok(found)
}

fn parse(source: &Source, text: &str) -> Parsed {
    let file = SceneFile::parse(text);
    let header = file.header().unwrap_or_default();
    let field = |key: &str| header.str(key).map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned);
    let fallback_title = source.key.rsplit('/').next().unwrap_or(&source.key).to_owned();
    let note_type = (source.kind == Kind::Note).then(|| {
        field("type").unwrap_or_else(|| {
            let folder = source.key.split_once('/').map_or("", |(folder, _)| folder);
            NoteKind::from_folder(folder).unwrap_or(NoteKind::Note).as_str().to_owned()
        })
    });
    Parsed {
        title: field("title").unwrap_or(fallback_title),
        status: if source.kind == Kind::Scene { field("status") } else { None },
        note_type,
        aliases: header.list("aliases"),
        names: NAME_FIELDS
            .into_iter()
            .flat_map(|f| header.list(f).into_iter().map(move |name| (f, name)))
            .collect(),
        body: plain_text(&file.markdown()),
    }
}

/// The FTS5 query for what someone typed: each word matches the start of a word, "quoted
/// phrases" match exactly, and every part must match. None if there's nothing to look for.
pub fn match_expression(text: &str) -> Option<String> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(|c: char| !c.is_whitespace()) {
        rest = &rest[start..];
        let quoted = rest.starts_with(['"', '“']);
        let (part, after) = if quoted {
            let inner = &rest[rest.chars().next().map_or(1, char::len_utf8)..];
            let end = inner.find(['"', '”']).unwrap_or(inner.len());
            let close = inner[end..].chars().next().map_or(0, char::len_utf8);
            (&inner[..end], &inner[end + close..])
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            (&rest[..end], &rest[end..])
        };
        rest = after;
        // Only letters and digits matter to the tokenizer; anything else could be FTS syntax.
        let words: Vec<String> = part
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
            .collect();
        if words.is_empty() {
            continue;
        }
        let phrase = format!("\"{}\"", words.join(" "));
        parts.push(if quoted { phrase } else { format!("{phrase}*") });
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Splits a snippet at its match marks into (text, is_match) pieces, joining lines.
fn pieces(snippet: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut in_match = false;
    let mut current = String::new();
    for c in snippet.chars() {
        match c {
            MATCH_START | MATCH_END => {
                if !current.is_empty() {
                    out.push((std::mem::take(&mut current), in_match));
                }
                in_match = c == MATCH_START;
            }
            '\n' => {
                if !current.ends_with(' ') && !current.is_empty() {
                    current.push(' ');
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push((current, in_match));
    }
    out
}

#[cfg(test)]
mod tests;

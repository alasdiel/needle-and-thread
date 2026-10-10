//! Typed wrappers around the backend commands in `src-tauri/src/`.

use std::{future::Future, pin::Pin};

use js_sys::{Object, Reflect};
use needle_core::network::{Mark, Point, Zone};
use needle_core::settings::TypographyRule;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use wasm_bindgen::prelude::*;

use crate::appearance::Appearance;
use crate::editor::Typography;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], catch)]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen)]
    fn listen_js(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> js_sys::Promise;
}

/// A boxed future, for passing commands around (they run on the browser's single thread).
pub type LocalFuture<T> = Pin<Box<dyn Future<Output = T>>>;

/// Calls `handler` every time the backend emits `event`, for the rest of the app's life. The
/// promise resolves once the listener is in place.
pub fn listen(event: &str, mut handler: impl FnMut() + 'static) -> js_sys::Promise {
    let closure = Closure::<dyn FnMut(JsValue)>::new(move |_payload: JsValue| handler());
    let listening = listen_js(event, &closure);
    // The listener lives as long as the window, so its closure must too.
    closure.forget();
    listening
}

/// Command arguments. Tauri matches them to the Rust parameters by name.
#[derive(Default)]
struct Args(Option<Object>);

impl Args {
    fn set(mut self, key: &str, value: JsValue) -> Self {
        let object = self.0.get_or_insert_with(Object::new);
        let _ = Reflect::set(object, &key.into(), &value);
        self
    }

    fn str(self, key: &str, value: &str) -> Self {
        self.set(key, value.into())
    }

    fn opt(self, key: &str, value: Option<&str>) -> Self {
        match value {
            Some(value) => self.str(key, value),
            None => self,
        }
    }

    fn num(self, key: &str, value: usize) -> Self {
        self.set(key, (value as f64).into())
    }
}

async fn call<R: DeserializeOwned>(cmd: &str, args: Args) -> Result<R, String> {
    let args = args.0.map_or(JsValue::UNDEFINED, JsValue::from);
    let result = invoke(cmd, args)
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
    serde_wasm_bindgen::from_value(result).map_err(|e| e.to_string())
}

fn scene_args(project: &str, scene: &str) -> Args {
    Args::default().str("project", project).str("scene", scene)
}

// --- Types ------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct VaultView {
    pub path: String,
    pub projects: Vec<ProjectView>,
    pub typography: Typography,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ProjectView {
    pub slug: String,
    pub title: String,
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct OutlineView {
    pub project: ProjectView,
    pub statuses: Vec<String>,
    pub chapters: Vec<ChapterView>,
    pub unplaced: Vec<SceneView>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ChapterView {
    pub id: String,
    pub title: String,
    pub part: Option<String>,
    pub summary: String,
    pub scenes: Vec<SceneView>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct SceneView {
    pub slug: String,
    pub id: String,
    pub title: String,
    pub status: String,
    pub summary: String,
    pub words: usize,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct OpenedScene {
    pub scene: SceneView,
    pub markdown: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Created {
    pub outline: OutlineView,
    pub scene: String,
}

/// Which note: its owner (a project or, with `world`, a world folder) and its path there.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NoteKey {
    pub owner: String,
    pub world: bool,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct NoteView {
    pub owner: String,
    pub world: bool,
    pub path: String,
    pub id: String,
    pub kind: String,
    pub title: String,
    pub aliases: Vec<String>,
    pub summary: String,
}

impl NoteView {
    pub fn key(&self) -> NoteKey {
        NoteKey {
            owner: self.owner.clone(),
            world: self.world,
            path: self.path.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ProjectNotes {
    pub notes: Vec<NoteView>,
    /// The project's world, if it has one: (folder, name).
    pub world: Option<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct OpenedNote {
    pub note: NoteView,
    pub markdown: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct NoteLinksView {
    pub appears_in: Vec<AppearanceView>,
    pub linked_from: Vec<BacklinkView>,
    pub mentioned_in: Vec<MentionView>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct AppearanceView {
    pub scene: SceneView,
    pub fields: Vec<String>,
}

/// Exactly one of `scene` and `note` is set.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BacklinkView {
    pub scene: Option<SceneView>,
    pub note: Option<NoteView>,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct MentionView {
    pub scene: SceneView,
    pub count: usize,
    pub before: String,
    pub name: String,
    pub after: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct SceneNamesView {
    pub fields: Vec<FieldView>,
    pub hints: Vec<HintView>,
}

/// One of a scene's name fields: `pov`, `cast`, `places` or `threads`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct FieldView {
    pub field: String,
    pub names: Vec<HeaderNameView>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct HeaderNameView {
    pub name: String,
    pub note: Option<NoteView>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct HintView {
    pub field: String,
    pub note: NoteView,
}

/// A search result: a scene (`kind` "scene", `key` its file name) or a note (`key` its path).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct HitView {
    pub kind: String,
    pub project: Option<String>,
    pub world: Option<String>,
    pub key: String,
    pub title: String,
    pub status: Option<String>,
    pub note_type: Option<String>,
    /// (text, is the match) pieces.
    pub snippet: Vec<(String, bool)>,
}

/// What to search for; see the `search` command.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchQuery {
    pub text: String,
    pub everywhere: bool,
    pub kind: Option<&'static str>,
    pub status: Option<String>,
    pub note_type: Option<String>,
    /// Names (title and aliases) of the POV character and thread to filter by.
    pub pov: Vec<String>,
    pub thread: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ProjectSettings {
    pub title: String,
    pub kind: String,
    pub world: Option<String>,
    /// Every world in the vault: (folder, name).
    pub worlds: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct VersionInfo {
    pub id: String,
    /// Seconds since the Unix epoch.
    pub time: i64,
    pub message: String,
    pub name: Option<String>,
    pub words: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SpellDictionary {
    pub aff: String,
    pub dic: String,
    pub personal_words: Vec<String>,
}

/// Something in the cut bin.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CutView {
    /// The project or world folder whose bin it's in.
    pub owner: String,
    pub world: bool,
    /// Its file's name in `cut/`.
    pub name: String,
    /// "passage", "scene" or "note".
    pub kind: String,
    /// When it was cut, as `2026-10-03T16:15:00Z`.
    pub cut_at: String,
    /// A scene's or note's title; for a passage, its scene's title when it was cut.
    pub title: String,
    /// The scene a passage came from.
    pub scene: Option<String>,
    pub chapter: Option<String>,
    pub note_kind: Option<String>,
    pub words: usize,
    pub passage: Option<Passage>,
}

/// A passage cut from a scene, as the editor makes it and the bin stores it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Passage {
    pub markdown: String,
    pub text_before: String,
    pub text_after: String,
    pub starts_paragraph: bool,
    pub ends_paragraph: bool,
    #[serde(default)]
    pub starts_with_space: bool,
    #[serde(default)]
    pub ends_with_space: bool,
}

// --- Vaults and projects ----------------------------------------------------------------

pub async fn current_vault() -> Result<Option<VaultView>, String> {
    call("current_vault", Args::default()).await
}

/// None if the folder picker was cancelled.
pub async fn open_vault() -> Result<Option<VaultView>, String> {
    call("open_vault", Args::default()).await
}

/// None if the folder picker was cancelled.
pub async fn create_vault() -> Result<Option<VaultView>, String> {
    call("create_vault", Args::default()).await
}

pub async fn open_sample_vault() -> Result<VaultView, String> {
    call("open_sample_vault", Args::default()).await
}

pub async fn set_typography(rule: TypographyRule, on: bool) -> Result<(), String> {
    call("set_typography", Args::default().str("rule", rule.key()).set("on", on.into())).await
}

pub async fn create_project(title: &str, kind: &str) -> Result<ProjectView, String> {
    call("create_project", Args::default().str("title", title).str("kind", kind)).await
}

pub async fn project_settings(project: &str) -> Result<ProjectSettings, String> {
    call("project_settings", Args::default().str("project", project)).await
}

/// Puts the project in `world`, or with `new_world`, in a new world of that name.
pub async fn update_project(project: &str, title: &str, kind: &str, world: Option<&str>, new_world: Option<&str>) -> Result<(), String> {
    let args = Args::default()
        .str("project", project)
        .str("title", title)
        .str("kind", kind)
        .opt("world", world)
        .opt("newWorld", new_world);
    call("update_project", args).await
}

pub async fn project_outline(project: &str) -> Result<OutlineView, String> {
    call("project_outline", Args::default().str("project", project)).await
}

// --- Scenes -----------------------------------------------------------------------------

pub async fn open_scene(project: &str, scene: &str) -> Result<OpenedScene, String> {
    call("open_scene", scene_args(project, scene)).await
}

pub async fn save_scene(project: &str, scene: &str, markdown: &str) -> Result<(), String> {
    call("save_scene", scene_args(project, scene).str("markdown", markdown)).await
}

/// Creates a scene after `after`, or else at the end of `chapter`.
pub async fn create_scene(project: &str, title: &str, chapter: Option<&str>, after: Option<&str>) -> Result<Created, String> {
    let args = Args::default().str("project", project).str("title", title).opt("chapter", chapter).opt("after", after);
    call("create_scene", args).await
}

pub async fn rename_scene(project: &str, scene: &str, title: &str) -> Result<OutlineView, String> {
    call("rename_scene", scene_args(project, scene).str("title", title)).await
}

pub async fn set_scene_status(project: &str, scene: &str, status: &str) -> Result<OutlineView, String> {
    call("set_scene_status", scene_args(project, scene).str("status", status)).await
}

pub async fn set_scene_summary(project: &str, scene: &str, summary: &str) -> Result<OutlineView, String> {
    call("set_scene_summary", scene_args(project, scene).str("summary", summary)).await
}

pub async fn cut_scene(project: &str, scene: &str) -> Result<OutlineView, String> {
    call("cut_scene", scene_args(project, scene)).await
}

pub async fn split_scene(project: &str, scene: &str, before: &str, after: &str) -> Result<Created, String> {
    call("split_scene", scene_args(project, scene).str("before", before).str("after", after)).await
}

pub async fn merge_scene(project: &str, scene: &str) -> Result<OutlineView, String> {
    call("merge_scene", scene_args(project, scene)).await
}

pub async fn move_scene(project: &str, scene: &str, chapter: &str, index: usize) -> Result<OutlineView, String> {
    call("move_scene", scene_args(project, scene).str("chapter", chapter).num("index", index)).await
}

// --- Chapters ---------------------------------------------------------------------------

pub async fn add_chapter(project: &str, title: &str, after: Option<&str>) -> Result<OutlineView, String> {
    call("add_chapter", Args::default().str("project", project).str("title", title).opt("after", after)).await
}

fn chapter_args(project: &str, chapter: &str) -> Args {
    Args::default().str("project", project).str("chapter", chapter)
}

pub async fn rename_chapter(project: &str, chapter: &str, title: &str) -> Result<OutlineView, String> {
    call("rename_chapter", chapter_args(project, chapter).str("title", title)).await
}

pub async fn set_chapter_part(project: &str, chapter: &str, part: &str) -> Result<OutlineView, String> {
    call("set_chapter_part", chapter_args(project, chapter).str("part", part)).await
}

pub async fn move_chapter(project: &str, chapter: &str, index: usize) -> Result<OutlineView, String> {
    call("move_chapter", chapter_args(project, chapter).num("index", index)).await
}

pub async fn remove_chapter(project: &str, chapter: &str) -> Result<OutlineView, String> {
    call("remove_chapter", chapter_args(project, chapter)).await
}

// --- Notes ------------------------------------------------------------------------------

fn note_args(note: &NoteKey) -> Args {
    Args::default()
        .str("owner", &note.owner)
        .set("world", note.world.into())
        .str("path", &note.path)
}

pub async fn project_notes(project: &str) -> Result<ProjectNotes, String> {
    call("project_notes", Args::default().str("project", project)).await
}

pub async fn open_note(note: &NoteKey) -> Result<OpenedNote, String> {
    call("open_note", note_args(note)).await
}

pub async fn save_note(note: &NoteKey, markdown: &str) -> Result<(), String> {
    call("save_note", note_args(note).str("markdown", markdown)).await
}

pub async fn create_note(project: &str, kind: &str, title: &str) -> Result<NoteView, String> {
    call("create_note", Args::default().str("project", project).str("kind", kind).str("title", title)).await
}

pub async fn cut_note(note: &NoteKey) -> Result<(), String> {
    call("cut_note", note_args(note)).await
}

pub async fn rename_note(note: &NoteKey, title: &str) -> Result<NoteView, String> {
    call("rename_note", note_args(note).str("title", title)).await
}

pub async fn set_note_aliases(note: &NoteKey, aliases: &[String]) -> Result<NoteView, String> {
    let list: js_sys::Array = aliases.iter().map(|a| JsValue::from_str(a)).collect();
    call("set_note_aliases", note_args(note).set("aliases", list.into())).await
}

pub async fn note_links(project: &str, note: &NoteKey) -> Result<NoteLinksView, String> {
    call("note_links", note_args(note).str("project", project)).await
}

pub async fn scene_names(project: &str, scene: &str) -> Result<SceneNamesView, String> {
    call("scene_names", scene_args(project, scene)).await
}

pub async fn set_scene_names(project: &str, scene: &str, field: &str, names: &[String]) -> Result<SceneNamesView, String> {
    let list: js_sys::Array = names.iter().map(|n| JsValue::from_str(n)).collect();
    call("set_scene_names", scene_args(project, scene).str("field", field).set("names", list.into())).await
}

/// Moves a project's note into its world.
pub async fn promote_note(project: &str, path: &str) -> Result<NoteView, String> {
    call("promote_note", Args::default().str("project", project).str("path", path)).await
}

/// Turns the first unlinked mention of the note in `scene` into a link. Returns whether there
/// was one.
pub async fn link_mention(project: &str, scene: &str, note: &NoteKey) -> Result<bool, String> {
    call("link_mention", note_args(note).str("project", project).str("scene", scene)).await
}

// --- Search -----------------------------------------------------------------------------

pub async fn search(project: &str, query: &SearchQuery) -> Result<Vec<HitView>, String> {
    let list = |names: &[String]| -> JsValue { names.iter().map(|n| JsValue::from_str(n)).collect::<js_sys::Array>().into() };
    let args = Args::default()
        .str("project", project)
        .str("text", &query.text)
        .set("everywhere", query.everywhere.into())
        .opt("kind", query.kind)
        .opt("status", query.status.as_deref())
        .opt("noteType", query.note_type.as_deref())
        .set("pov", list(&query.pov))
        .set("thread", list(&query.thread));
    call("search", args).await
}

// --- History ----------------------------------------------------------------------------

pub async fn scene_history(project: &str, scene: &str) -> Result<Vec<VersionInfo>, String> {
    call("scene_history", scene_args(project, scene)).await
}

pub async fn scene_version(project: &str, scene: &str, id: &str) -> Result<String, String> {
    call("scene_version", scene_args(project, scene).str("id", id)).await
}

pub async fn snapshot_now() -> Result<bool, String> {
    call("snapshot_now", Args::default()).await
}

/// How the vault's backup is doing; see `BackupStatus` in src-tauri/src/backup.rs.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum BackupStatus {
    Waiting,
    Pushing,
    /// The repository has snapshots made elsewhere, to take in before pushing.
    Incoming,
    /// Seconds since the Unix epoch.
    Done { at: i64 },
    Failed { at: i64, error: String },
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BackupView {
    /// The repository's SSH address; empty for no backup.
    pub remote: String,
    pub after_snapshot: bool,
    pub status: BackupStatus,
}

pub async fn backup() -> Result<BackupView, String> {
    call("backup", Args::default()).await
}

/// Saves the backup's address and switch, and backs up straight away if there's an address.
pub async fn set_backup(remote: &str, after_snapshot: bool) -> Result<(), String> {
    call("set_backup", Args::default().str("remote", remote).set("afterSnapshot", after_snapshot.into())).await
}

pub async fn back_up_now() -> Result<(), String> {
    call("back_up_now", Args::default()).await
}

/// What came in from elsewhere; see `TakenInView` in src-tauri/src/backup.rs.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct TakenInView {
    /// Files that changed, relative to the vault.
    pub changed: Vec<String>,
    pub clashes: Vec<ClashView>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ClashView {
    /// The file, still holding this computer's version.
    pub path: String,
    /// Where the other version was put.
    pub copy: String,
}

/// Snapshots, then takes in the snapshots made elsewhere that the backup found.
pub async fn take_in() -> Result<TakenInView, String> {
    call("take_in", Args::default()).await
}

/// Returns the restored text. `label` describes the version in the snapshot message.
pub async fn restore_version(project: &str, scene: &str, id: &str, label: &str) -> Result<String, String> {
    call("restore_version", scene_args(project, scene).str("id", id).str("label", label)).await
}

pub async fn note_history(note: &NoteKey) -> Result<Vec<VersionInfo>, String> {
    call("note_history", note_args(note)).await
}

pub async fn note_version(note: &NoteKey, id: &str) -> Result<String, String> {
    call("note_version", note_args(note).str("id", id)).await
}

/// Returns the restored text. `label` describes the version in the snapshot message.
pub async fn restore_note_version(note: &NoteKey, id: &str, label: &str) -> Result<String, String> {
    call("restore_note_version", note_args(note).str("id", id).str("label", label)).await
}

pub async fn name_version(id: &str, name: &str) -> Result<(), String> {
    call("name_version", Args::default().str("id", id).str("name", name)).await
}

// --- The network board ------------------------------------------------------------------

/// What's on a project's board, and where it all sits (DESIGN §7).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardView {
    pub cards: Vec<CardView>,
    pub relationships: Vec<RelationshipView>,
    pub links: Vec<LinkView>,
    pub zones: Vec<Zone>,
    pub marks: Vec<Mark>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CardView {
    pub id: String,
    pub owner: String,
    pub world: bool,
    pub path: String,
    /// "character", "place", "event" or "thread": which paper the card is.
    pub kind: String,
    pub title: String,
    /// Empty for this project's own notes; otherwise where the note came from, which the card
    /// says under its kind.
    pub from: String,
    pub at: Point,
    pub turn: f64,
    /// Up only because it was pinned, so it can be taken down.
    pub pinned: bool,
}

impl CardView {
    pub fn key(&self) -> NoteKey {
        NoteKey { owner: self.owner.clone(), world: self.world, path: self.path.clone() }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RelationshipView {
    pub id: String,
    pub owner: String,
    pub world: bool,
    pub path: String,
    pub label: String,
    pub from: String,
    pub to: String,
    pub directed: bool,
    /// Where in the story it starts and stops, and how its label changes on the way.
    pub begins: Option<String>,
    pub ends: Option<String>,
    pub changes: Vec<ChangeView>,
}

impl RelationshipView {
    pub fn key(&self) -> NoteKey {
        NoteKey { owner: self.owner.clone(), world: self.world, path: self.path.clone() }
    }

    /// How it stands at the end of the book, which is what the board shows until there's a
    /// timeline to move along: the last label it changed to, if it changed.
    pub fn label_now(&self) -> &str {
        self.changes.iter().rev().find(|c| !c.label.is_empty()).map_or(&self.label, |c| &c.label)
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ChangeView {
    pub at: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct LinkView {
    pub from: String,
    pub to: String,
}

/// The arrangement sent back when the board is rearranged.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct LayoutView {
    pub nodes: Vec<PinnedView>,
    pub zones: Vec<Zone>,
    pub marks: Vec<Mark>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PinnedView {
    pub id: String,
    pub at: Point,
    pub turn: f64,
}

/// A project's scenes and plot points in story time (DESIGN §6).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TimelineView {
    /// The invented calendar's name; `None` for the real one.
    pub calendar: Option<String>,
    pub items: Vec<TimelineItemView>,
    /// Indices into `items`, in story order. The rest wait in the tray.
    pub order: Vec<usize>,
    pub days_only: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TimelineItemView {
    /// "scene" or "event".
    pub kind: String,
    pub owner: String,
    pub world: bool,
    pub path: String,
    pub id: String,
    pub title: String,
    pub reading: Option<usize>,
    pub status: String,
    pub summary: String,
    pub pov: Option<String>,
    pub threads: Vec<String>,
    pub places: Vec<String>,
    pub cast: Vec<String>,
    pub when: WrittenView,
    pub time: Option<String>,
    pub minute: Option<i64>,
    pub loose: bool,
    pub gap: Option<String>,
    pub problem: Option<String>,
}

/// A `when` as written. `kind` is "none", "text", "from" or "order".
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct WrittenView {
    pub kind: String,
    pub text: String,
    pub from: String,
    pub offset: String,
    pub after: String,
    pub before: String,
}

impl TimelineItemView {
    pub fn is_scene(&self) -> bool {
        self.kind == "scene"
    }

    pub fn note_key(&self) -> NoteKey {
        NoteKey { owner: self.owner.clone(), world: self.world, path: self.path.clone() }
    }
}

/// A new `when`, as the timeline's panel sets it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WhenInput {
    Clear,
    Text { text: String },
    From { from: String, offset: String },
    Order { after: Option<String>, before: Option<String> },
}

/// The network board through story time (DESIGN §7): the slider's steps, and where along them
/// each card and string comes in.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct StoryView {
    pub steps: Vec<StepView>,
    /// (card id, the step it comes in at). Cards not listed are there throughout.
    pub cards: Vec<(String, usize)>,
    pub relationships: Vec<StoryRelationshipView>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct StepView {
    pub title: String,
    pub time: Option<String>,
    pub kind: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct StoryRelationshipView {
    pub id: String,
    pub begins: Option<usize>,
    pub ends: Option<usize>,
    pub changes: Vec<(usize, String)>,
    pub unplaced: Vec<String>,
}

pub async fn project_story(project: &str) -> Result<StoryView, String> {
    call("project_story", Args::default().str("project", project)).await
}

pub async fn project_timeline(project: &str) -> Result<TimelineView, String> {
    call("project_timeline", Args::default().str("project", project)).await
}

pub async fn set_when(project: &str, item: &TimelineItemView, when: &WhenInput) -> Result<TimelineView, String> {
    let when = serde_wasm_bindgen::to_value(when).map_err(|e| e.to_string())?;
    let args = Args::default()
        .str("project", project)
        .str("kind", &item.kind)
        .str("itemOwner", &item.owner)
        .set("world", item.world.into())
        .str("path", &item.path)
        .set("when", when);
    call("set_when", args).await
}

pub async fn project_board(project: &str) -> Result<BoardView, String> {
    call("project_board", Args::default().str("project", project)).await
}

pub async fn save_board(project: &str, layout: &LayoutView) -> Result<(), String> {
    let layout = serde_wasm_bindgen::to_value(layout).map_err(|e| e.to_string())?;
    call("save_board", Args::default().str("project", project).set("layout", layout)).await
}

fn point(at: Point) -> JsValue {
    js_sys::Array::of2(&at.0.into(), &at.1.into()).into()
}

pub async fn add_card(project: &str, kind: &str, title: &str, at: Point) -> Result<NoteView, String> {
    let args = Args::default().str("project", project).str("kind", kind).str("title", title).set("at", point(at));
    call("add_card", args).await
}

pub async fn pin_card(project: &str, id: &str, at: Point) -> Result<(), String> {
    call("pin_card", Args::default().str("project", project).str("id", id).set("at", point(at))).await
}

pub async fn unpin_card(project: &str, id: &str) -> Result<(), String> {
    call("unpin_card", Args::default().str("project", project).str("id", id)).await
}

pub async fn add_relationship(project: &str, from: &str, to: &str, label: &str, directed: bool) -> Result<NoteView, String> {
    let args = Args::default()
        .str("project", project)
        .str("from", from)
        .str("to", to)
        .str("label", label)
        .set("directed", directed.into());
    call("add_relationship", args).await
}

/// What to change about a relationship; anything left `None` stays as it is.
#[derive(Clone, Debug, Default)]
pub struct RelationshipEdit {
    pub label: Option<String>,
    pub directed: Option<bool>,
    pub reverse: bool,
}

pub async fn edit_relationship(note: &NoteKey, edit: &RelationshipEdit) -> Result<NoteView, String> {
    let mut args = note_args(note).opt("label", edit.label.as_deref());
    if let Some(directed) = edit.directed {
        args = args.set("directed", directed.into());
    }
    if edit.reverse {
        args = args.set("reverse", true.into());
    }
    call("edit_relationship", args).await
}

// --- The cut bin ------------------------------------------------------------------------

pub async fn bin_items(project: &str) -> Result<Vec<CutView>, String> {
    call("bin_items", Args::default().str("project", project)).await
}

pub async fn cut_passage(project: &str, scene: &str, passage: &Passage) -> Result<CutView, String> {
    let passage = serde_wasm_bindgen::to_value(passage).map_err(|e| e.to_string())?;
    call("cut_passage", scene_args(project, scene).set("passage", passage)).await
}

/// Takes a passage out of the bin, once it's back in its scene.
pub async fn remove_from_bin(item: &CutView) -> Result<(), String> {
    call("remove_from_bin", bin_args(item)).await
}

pub async fn restore_scene(project: &str, name: &str) -> Result<Created, String> {
    call("restore_scene", Args::default().str("project", project).str("name", name)).await
}

pub async fn restore_note(item: &CutView) -> Result<NoteView, String> {
    call("restore_note", bin_args(item)).await
}

fn bin_args(item: &CutView) -> Args {
    Args::default()
        .str("owner", &item.owner)
        .set("world", item.world.into())
        .str("name", &item.name)
}

// --- Settings ---------------------------------------------------------------------------

pub async fn appearance() -> Result<Appearance, String> {
    call("appearance", Args::default()).await
}

pub async fn set_appearance(appearance: Appearance) -> Result<(), String> {
    call("set_appearance", Args::default().str("appearance", appearance.name())).await
}

pub async fn markdown_panel() -> Result<bool, String> {
    call("markdown_panel", Args::default()).await
}

pub async fn set_markdown_panel(on: bool) -> Result<(), String> {
    call("set_markdown_panel", Args::default().set("on", on.into())).await
}

// --- Spelling ---------------------------------------------------------------------------

pub async fn spell_dictionary() -> Result<SpellDictionary, String> {
    call("spell_dictionary", Args::default()).await
}

pub async fn add_to_dictionary(word: &str) -> Result<(), String> {
    call("add_to_dictionary", Args::default().str("word", word)).await
}

// --- Closing ----------------------------------------------------------------------------

/// Tells the backend to hold closes until `finish_close` from now on.
pub async fn close_listening() -> Result<(), String> {
    call("close_listening", Args::default()).await
}

/// Lets the window close, or with `error`, asks whether to close without the latest changes.
pub async fn finish_close(error: Option<&str>) -> Result<(), String> {
    call("finish_close", Args::default().opt("error", error)).await
}

//! Typed wrappers around the backend commands in `src-tauri/src/`.

use std::{future::Future, pin::Pin};

use js_sys::{Object, Reflect};
use serde::{Deserialize, de::DeserializeOwned};
use wasm_bindgen::prelude::*;

use crate::appearance::Appearance;

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

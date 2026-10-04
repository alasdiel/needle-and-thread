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

/// Calls `handler` every time the backend emits `event`, for the rest of the app's life.
pub fn listen(event: &str, mut handler: impl FnMut() + 'static) {
    let closure = Closure::<dyn FnMut(JsValue)>::new(move |_payload: JsValue| handler());
    let _ = listen_js(event, &closure);
    // The listener lives as long as the window, so its closure must too.
    closure.forget();
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

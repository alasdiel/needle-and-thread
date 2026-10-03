//! Typed wrappers around the backend commands in `src-tauri/src/scenes.rs`.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], catch)]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen)]
    fn listen_js(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> js_sys::Promise;
}

/// Calls `handler` every time the backend emits `event`, for the rest of the app's life.
pub fn listen(event: &str, mut handler: impl FnMut() + 'static) {
    let closure = Closure::<dyn FnMut(JsValue)>::new(move |_payload: JsValue| handler());
    let _ = listen_js(event, &closure);
    // The listener lives as long as the window, so its closure must too.
    closure.forget();
}

async fn call<A: Serialize, R: DeserializeOwned>(cmd: &str, args: &A) -> Result<R, String> {
    let args = serde_wasm_bindgen::to_value(args).map_err(|e| e.to_string())?;
    let result = invoke(cmd, args)
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
    serde_wasm_bindgen::from_value(result).map_err(|e| e.to_string())
}

#[derive(Clone, Debug, Deserialize)]
pub struct SampleScene {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OpenedScene {
    pub path: String,
    pub markdown: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SpellDictionary {
    pub aff: String,
    pub dic: String,
    pub personal_words: Vec<String>,
}

#[derive(Serialize)]
struct WordArgs<'a> {
    word: &'a str,
}

#[derive(Serialize)]
struct OpenArgs<'a> {
    path: &'a str,
}

#[derive(Serialize)]
struct SaveArgs<'a> {
    path: &'a str,
    markdown: &'a str,
}

pub async fn sample_scenes() -> Result<Vec<SampleScene>, String> {
    call("sample_scenes", &()).await
}

pub async fn open_scene(path: &str) -> Result<OpenedScene, String> {
    call("open_scene", &OpenArgs { path }).await
}

pub async fn save_scene(path: &str, markdown: &str) -> Result<(), String> {
    call("save_scene", &SaveArgs { path, markdown }).await
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

#[derive(Serialize)]
struct VersionArgs<'a> {
    path: &'a str,
    id: &'a str,
}

#[derive(Serialize)]
struct RestoreArgs<'a> {
    path: &'a str,
    id: &'a str,
    label: &'a str,
}

#[derive(Serialize)]
struct NameArgs<'a> {
    id: &'a str,
    name: &'a str,
}

pub async fn scene_history(path: &str) -> Result<Vec<VersionInfo>, String> {
    call("scene_history", &OpenArgs { path }).await
}

pub async fn scene_version(path: &str, id: &str) -> Result<String, String> {
    call("scene_version", &VersionArgs { path, id }).await
}

pub async fn snapshot_now() -> Result<bool, String> {
    call("snapshot_now", &()).await
}

/// Returns the restored text. `label` describes the version in the snapshot message.
pub async fn restore_version(path: &str, id: &str, label: &str) -> Result<String, String> {
    call("restore_version", &RestoreArgs { path, id, label }).await
}

pub async fn name_version(id: &str, name: &str) -> Result<(), String> {
    call("name_version", &NameArgs { id, name }).await
}

pub async fn spell_dictionary() -> Result<SpellDictionary, String> {
    call("spell_dictionary", &()).await
}

pub async fn add_to_dictionary(word: &str) -> Result<(), String> {
    call("add_to_dictionary", &WordArgs { word }).await
}

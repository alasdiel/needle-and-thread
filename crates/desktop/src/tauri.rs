//! Typed wrappers around the backend commands in `src-tauri/src/scenes.rs`.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], catch)]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
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

pub async fn spell_dictionary() -> Result<SpellDictionary, String> {
    call("spell_dictionary", &()).await
}

pub async fn add_to_dictionary(word: &str) -> Result<(), String> {
    call("add_to_dictionary", &WordArgs { word }).await
}

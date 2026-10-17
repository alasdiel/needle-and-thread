//! Talking to the sharing Worker from the reader page. Everything sent is sealed here first, with
//! the link's key, which never leaves the browser.

use gloo_net::http::{Request, RequestBuilder, Response};
use needle_core::share::{self, Comment, Link, Shared};
use serde::Deserialize;

use crate::model::Opened;

/// Where the Worker is: `?worker=` in the page's address (for testing), else the one built in.
pub fn base() -> String {
    let from_query = web_sys::window().and_then(|w| w.location().search().ok()).and_then(|search| {
        search.trim_start_matches('?').split('&').find_map(|pair| pair.strip_prefix("worker=").map(|v| js_sys::decode_uri_component(v).ok().map(String::from)))?
    });
    from_query
        .or_else(|| option_env!("NEEDLE_WORKER").map(str::to_owned))
        .unwrap_or_else(|| "https://needle-and-thread.alasdiel.workers.dev".to_owned())
        .trim_end_matches('/')
        .to_owned()
}

fn url(link: &Link, path: &str) -> String {
    format!("{}/s/{}{path}", base(), link.id)
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    let crypto = web_sys::window().and_then(|w| w.crypto().ok()).expect("this browser has no crypto");
    crypto.get_random_values_with_u8_array(&mut bytes).expect("random values");
    bytes
}

/// What came back, or the reason in words.
async fn answer(request: Result<Response, gloo_net::Error>) -> Result<Response, String> {
    let response = request.map_err(|_| "Couldn’t reach the sharing service. Check the connection and try again.".to_owned())?;
    if response.ok() {
        return Ok(response);
    }
    let why = response.text().await.unwrap_or_default();
    Err(match response.status() {
        404 => "This link doesn’t work any more: it was revoked, or it was copied wrong.".to_owned(),
        _ if why.trim().is_empty() => format!("The sharing service said {}.", response.status()),
        _ => why.trim().to_owned(),
    })
}

fn author(builder: RequestBuilder, author: Option<&str>) -> RequestBuilder {
    match author {
        Some(key) => builder.header("Authorization", &format!("Bearer {key}")),
        None => builder,
    }
}

/// The shared text, unsealed.
pub async fn text(link: &Link) -> Result<Shared, String> {
    let response = answer(Request::get(&url(link, "")).send().await).await?;
    let sealed = response.binary().await.map_err(|e| e.to_string())?;
    let json = share::open(&link.key, &share::text_label(&link.id), &sealed).ok_or("This link’s key doesn’t open the text. Was it copied whole?")?;
    serde_json::from_slice(&json).map_err(|_| "This text was shared by a newer version of Needle and Thread.".to_owned())
}

#[derive(Deserialize)]
struct Listed {
    id: u64,
    body: String,
    author: bool,
    resolved: bool,
    deleted: bool,
    created: f64,
}

/// Every comment there is, opened. Ones that don't open (sealed with another key) are left out.
pub async fn comments(link: &Link) -> Result<Vec<Opened>, String> {
    let response = answer(Request::get(&url(link, "/comments")).send().await).await?;
    let listed: Vec<Listed> = response.json().await.map_err(|e| e.to_string())?;
    let label = share::comment_label(&link.id);
    Ok(listed
        .into_iter()
        .filter(|c| !c.deleted)
        .filter_map(|c| {
            let json = share::open(&link.key, &label, &share::decode(&c.body)?)?;
            Some(Opened { id: c.id, author: c.author, resolved: c.resolved, created: c.created, comment: serde_json::from_slice(&json).ok()? })
        })
        .collect())
}

/// Adds a comment: with the token the link gives every reader, or as the author.
pub async fn post(link: &Link, author_key: Option<&str>, comment: &Comment) -> Result<(), String> {
    let json = serde_json::to_vec(comment).map_err(|e| e.to_string())?;
    let sealed = share::seal(&link.key, random(), &share::comment_label(&link.id), &json);
    let builder = author(Request::post(&url(link, "/comments")).header("X-Comment-Token", &link.comment_token()), author_key);
    let request = builder.body(js_sys::Uint8Array::from(&sealed[..])).map_err(|e| e.to_string())?;
    answer(request.send().await).await.map(|_| ())
}

pub async fn resolve(link: &Link, author_key: &str, id: u64, resolved: bool) -> Result<(), String> {
    let request = author(Request::patch(&url(link, &format!("/comments/{id}"))), Some(author_key))
        .json(&serde_json::json!({ "resolved": resolved }))
        .map_err(|e| e.to_string())?;
    answer(request.send().await).await.map(|_| ())
}

pub async fn delete(link: &Link, author_key: &str, id: u64) -> Result<(), String> {
    answer(author(Request::delete(&url(link, &format!("/comments/{id}"))), Some(author_key)).send().await).await.map(|_| ())
}

pub async fn revoke(link: &Link, author_key: &str) -> Result<(), String> {
    answer(author(Request::delete(&url(link, "")), Some(author_key)).send().await).await.map(|_| ())
}

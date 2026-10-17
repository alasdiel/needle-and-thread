//! The vault's repository on GitHub (DESIGN §9). Reading: the file list is one call to the tree
//! API; contents come through GraphQL, up to a hundred files a call, and are kept in the
//! browser's Cache Storage by blob sha, so opening the app again fetches only what changed.
//! Writing: one GraphQL query for where the branch is and what the changed files hold there
//! now, then one commit of every change at once, which GitHub refuses if the branch moved in
//! between.

use gloo_net::http::{Request, Response};
use serde::Deserialize;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// The branch the desktop app backs up to.
const BRANCH: &str = "main";
/// Files fetched per GraphQL call.
const BATCH: usize = 100;
const CACHE: &str = "needle-blobs";

/// Where the vault is and how to get in: `owner/name` and a fine-grained token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Creds {
    pub repo: String,
    pub token: String,
}

impl Creds {
    fn owner_and_name(&self) -> Option<(&str, &str)> {
        self.repo.trim().split_once('/').filter(|(o, n)| !o.is_empty() && !n.is_empty() && !n.contains('/'))
    }
}

/// A file in the repository: its path and blob sha.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Blob {
    pub path: String,
    pub sha: String,
}

#[derive(Deserialize)]
struct Tree {
    tree: Vec<TreeEntry>,
    #[serde(default)]
    truncated: bool,
}

#[derive(Deserialize)]
struct TreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
}

/// Every file on the backup branch.
pub async fn files(creds: &Creds) -> Result<Vec<Blob>, String> {
    if creds.owner_and_name().is_none() {
        return Err("The repository should look like owner/name.".to_owned());
    }
    let url = format!("https://api.github.com/repos/{}/git/trees/{BRANCH}?recursive=1", creds.repo.trim());
    let response = get(creds, &url).await?;
    let tree: Tree = response.json().await.map_err(|e| e.to_string())?;
    if tree.truncated {
        return Err("This vault has more files than GitHub lists at once, which the phone can't read yet.".to_owned());
    }
    Ok(tree
        .tree
        .into_iter()
        .filter(|e| e.kind == "blob")
        .map(|e| Blob { path: e.path, sha: e.sha })
        .collect())
}

/// The contents of `blobs`, from the cache where they're there and GitHub where not. Calls
/// `progress` with how many are in hand as it goes.
pub async fn contents(creds: &Creds, blobs: &[Blob], progress: impl Fn(usize)) -> Result<Vec<(Blob, String)>, String> {
    let cache = open_cache().await;
    let mut found = Vec::with_capacity(blobs.len());
    let mut missing = Vec::new();
    for blob in blobs {
        match cached(cache.as_ref(), &blob.sha).await {
            Some(text) => found.push((blob.clone(), text)),
            None => missing.push(blob.clone()),
        }
    }
    progress(found.len());
    for batch in missing.chunks(BATCH) {
        for (blob, text) in fetch_batch(creds, batch).await? {
            store(cache.as_ref(), &blob.sha, &text).await;
            found.push((blob, text));
        }
        progress(found.len());
    }
    Ok(found)
}

/// One GraphQL call for up to a hundred blobs, by sha.
async fn fetch_batch(creds: &Creds, batch: &[Blob]) -> Result<Vec<(Blob, String)>, String> {
    let (owner, name) = creds.owner_and_name().ok_or("The repository should look like owner/name.")?;
    let fields: String = batch
        .iter()
        .enumerate()
        .map(|(i, b)| format!("f{i}: object(oid: \"{}\") {{ ... on Blob {{ text isBinary isTruncated }} }} ", b.sha))
        .collect();
    let query = format!("query {{ repository(owner: {}, name: {}) {{ {fields}}} }}", json(owner), json(name));
    let response = Request::post("https://api.github.com/graphql")
        .header("Authorization", &format!("Bearer {}", creds.token.trim()))
        .json(&serde_json::json!({ "query": query }))
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|_| "Couldn't reach GitHub. Check the connection and try again.".to_owned())?;
    let response = checked(response).await?;
    let body: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    if let Some(error) = body["errors"][0]["message"].as_str() {
        return Err(format!("GitHub said: {error}"));
    }
    let repository = &body["data"]["repository"];
    let mut out = Vec::new();
    for (i, blob) in batch.iter().enumerate() {
        let object = &repository[format!("f{i}")];
        if object["isBinary"].as_bool() == Some(true) {
            continue;
        }
        let text = match (object["text"].as_str(), object["isTruncated"].as_bool()) {
            (Some(text), Some(false) | None) => text.to_owned(),
            // Too big for GraphQL to give whole: the REST API does.
            _ => fetch_one(creds, blob).await?,
        };
        out.push((blob.clone(), text));
    }
    Ok(out)
}

#[derive(Deserialize)]
struct RestBlob {
    content: String,
}

async fn fetch_one(creds: &Creds, blob: &Blob) -> Result<String, String> {
    let url = format!("https://api.github.com/repos/{}/git/blobs/{}", creds.repo.trim(), blob.sha);
    let response = get(creds, &url).await?;
    let rest: RestBlob = response.json().await.map_err(|e| e.to_string())?;
    let clean: String = rest.content.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = js_sys::global()
        .dyn_into::<web_sys::Window>()
        .ok()
        .and_then(|w| w.atob(&clean).ok())
        .ok_or_else(|| format!("{} came back unreadable", blob.path))?;
    // atob gives one char per byte; put the bytes back together as UTF-8.
    let bytes: Vec<u8> = bytes.chars().map(|c| c as u32 as u8).collect();
    String::from_utf8(bytes).map_err(|_| format!("{} isn't text", blob.path))
}

// --- Writing ---------------------------------------------------------------------------------

/// Why a write didn't happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteError {
    /// GitHub couldn't be reached.
    Offline,
    /// The branch moved on since `current` looked: look again.
    Moved,
    /// The token can read but not write.
    ReadOnly,
    Other(String),
}

/// The backup branch as it is now, and what `paths` hold on it: blob sha and text, or None
/// where a path isn't there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Current {
    pub head: String,
    pub files: Vec<(String, Option<(String, String)>)>,
}

/// Where the branch is, and the files at `paths` there, in one call.
pub async fn current(creds: &Creds, paths: &[String]) -> Result<Current, WriteError> {
    let (owner, name) = creds.owner_and_name().ok_or_else(|| WriteError::Other("The repository should look like owner/name.".to_owned()))?;
    let fields: String = paths
        .iter()
        .enumerate()
        .map(|(i, path)| format!("f{i}: object(expression: {}) {{ ... on Blob {{ oid text isTruncated }} }} ", json(&format!("{BRANCH}:{path}"))))
        .collect();
    let query = format!(
        "query {{ repository(owner: {}, name: {}) {{ ref(qualifiedName: {}) {{ target {{ oid }} }} {fields}}} }}",
        json(owner),
        json(name),
        json(&format!("refs/heads/{BRANCH}"))
    );
    let body = graphql(creds, serde_json::json!({ "query": query })).await?;
    let repository = &body["data"]["repository"];
    let head = repository["ref"]["target"]["oid"]
        .as_str()
        .ok_or_else(|| WriteError::Other(format!("The repository has no {BRANCH} branch: back up from the desktop app first.")))?
        .to_owned();
    let mut files = Vec::with_capacity(paths.len());
    for (i, path) in paths.iter().enumerate() {
        let object = &repository[format!("f{i}")];
        let found = match (object["oid"].as_str(), object["text"].as_str(), object["isTruncated"].as_bool()) {
            (Some(sha), Some(text), Some(false) | None) => Some((sha.to_owned(), text.to_owned())),
            (Some(sha), _, _) => {
                let blob = Blob { path: path.clone(), sha: sha.to_owned() };
                Some((sha.to_owned(), fetch_one(creds, &blob).await.map_err(WriteError::Other)?))
            }
            _ => None,
        };
        files.push((path.clone(), found));
    }
    Ok(Current { head, files })
}

/// Commits `writes` (path and new contents) and `removes` on top of `head`, as one commit with
/// `message`. Returns the new blob sha of each file written.
pub async fn commit(creds: &Creds, head: &str, message: &str, writes: &[(String, Vec<u8>)], removes: &[String]) -> Result<Vec<(String, String)>, WriteError> {
    let fields: String = writes
        .iter()
        .enumerate()
        .map(|(i, (path, _))| format!("f{i}: file(path: {}) {{ oid }} ", json(path)))
        .collect();
    let query = format!("mutation($input: CreateCommitOnBranchInput!) {{ createCommitOnBranch(input: $input) {{ commit {{ oid {fields}}} }} }}");
    let input = serde_json::json!({
        "branch": { "repositoryNameWithOwner": creds.repo.trim(), "branchName": BRANCH },
        "message": { "headline": message },
        "expectedHeadOid": head,
        "fileChanges": {
            "additions": writes.iter().map(|(path, bytes)| serde_json::json!({ "path": path, "contents": base64(bytes) })).collect::<Vec<_>>(),
            "deletions": removes.iter().map(|path| serde_json::json!({ "path": path })).collect::<Vec<_>>(),
        },
    });
    let body = graphql(creds, serde_json::json!({ "query": query, "variables": { "input": input } })).await?;
    let commit = &body["data"]["createCommitOnBranch"]["commit"];
    writes
        .iter()
        .enumerate()
        .map(|(i, (path, _))| match commit[format!("f{i}")]["oid"].as_str() {
            Some(sha) => Ok((path.clone(), sha.to_owned())),
            None => Err(WriteError::Other(format!("GitHub didn't say where {path} went."))),
        })
        .collect()
}

/// One GraphQL call, with GitHub's errors put into words.
async fn graphql(creds: &Creds, body: serde_json::Value) -> Result<serde_json::Value, WriteError> {
    let response = Request::post("https://api.github.com/graphql")
        .header("Authorization", &format!("Bearer {}", creds.token.trim()))
        .json(&body)
        .map_err(|e| WriteError::Other(e.to_string()))?
        .send()
        .await
        .map_err(|_| WriteError::Offline)?;
    if response.status() == 403 {
        return Err(WriteError::ReadOnly);
    }
    let response = checked(response).await.map_err(WriteError::Other)?;
    let body: serde_json::Value = response.json().await.map_err(|e| WriteError::Other(e.to_string()))?;
    match body["errors"].get(0) {
        None => Ok(body),
        Some(error) => Err(graphql_error(error["type"].as_str().unwrap_or_default(), error["message"].as_str().unwrap_or_default())),
    }
}

fn graphql_error(kind: &str, message: &str) -> WriteError {
    if message.contains("Expected branch to point to") || kind == "STALE_DATA" {
        WriteError::Moved
    } else if kind == "FORBIDDEN" || message.contains("not accessible by") {
        WriteError::ReadOnly
    } else {
        WriteError::Other(format!("GitHub said: {message}"))
    }
}

/// Standard base64, which GitHub wants a commit's file contents in.
fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | (u32::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            out.push(if i <= chunk.len() { DIGITS[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

async fn get(creds: &Creds, url: &str) -> Result<Response, String> {
    let response = Request::get(url)
        .header("Authorization", &format!("Bearer {}", creds.token.trim()))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|_| "Couldn't reach GitHub. Check the connection and try again.".to_owned())?;
    checked(response).await
}

/// The response, or what went wrong in words.
async fn checked(response: Response) -> Result<Response, String> {
    match response.status() {
        200..=299 => Ok(response),
        401 => Err("GitHub didn't accept the token. It may have expired or been revoked.".to_owned()),
        403 | 429 => Err("GitHub refused, or asked to slow down. Try again in a few minutes.".to_owned()),
        404 => Err("No such repository, or the token can't see it. Check the name, and that the token can read its contents.".to_owned()),
        409 => Err("The repository is empty: back up from the desktop app first.".to_owned()),
        status => Err(format!("GitHub answered {status}.")),
    }
}

fn json(s: &str) -> String {
    serde_json::Value::String(s.to_owned()).to_string()
}

// --- The cache --------------------------------------------------------------------------------

fn key(sha: &str) -> String {
    // Cache Storage wants a URL; this one is never fetched.
    format!("https://needle-and-thread.invalid/blob/{sha}")
}

async fn open_cache() -> Option<web_sys::Cache> {
    let window = web_sys::window()?;
    // Absent outside secure contexts (plain http other than localhost): then nothing is kept.
    let caches = window.caches().ok()?;
    JsFuture::from(caches.open(CACHE)).await.ok()?.dyn_into().ok()
}

async fn cached(cache: Option<&web_sys::Cache>, sha: &str) -> Option<String> {
    let found = JsFuture::from(cache?.match_with_str(&key(sha))).await.ok()?;
    let response: web_sys::Response = found.dyn_into().ok()?;
    JsFuture::from(response.text().ok()?).await.ok()?.as_string()
}

async fn store(cache: Option<&web_sys::Cache>, sha: &str, text: &str) {
    let Some(cache) = cache else { return };
    let Ok(response) = web_sys::Response::new_with_opt_str(Some(text)) else { return };
    let _ = JsFuture::from(cache.put_with_str(&key(sha), &response)).await;
}

/// Forgets every cached file, when the phone is disconnected from the vault.
pub async fn clear_cache() {
    let Some(window) = web_sys::window() else { return };
    let Ok(caches) = window.caches() else { return };
    let _: Result<JsValue, JsValue> = JsFuture::from(caches.delete(CACHE)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_is_the_standard_kind() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64("Mara’s".as_bytes()), "TWFyYeKAmXM=");
    }

    #[test]
    fn github_errors_say_what_to_do() {
        assert_eq!(graphql_error("", "Expected branch to point to \"abc\" but it did not. Pull and try again."), WriteError::Moved);
        assert_eq!(graphql_error("FORBIDDEN", "Resource not accessible by personal access token"), WriteError::ReadOnly);
        assert_eq!(graphql_error("", "Something else"), WriteError::Other("GitHub said: Something else".to_owned()));
    }
}

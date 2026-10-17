//! Sharing (DESIGN §10): links made from the Shared panel, the sealed text the Worker keeps for
//! each, sent again after each snapshot that changes it (at most once a minute), and the
//! comments readers leave, placed on the words they're on as the text changes.
//!
//! Every call to the Worker happens off the window's thread: uploads on a thread of their own,
//! one at a time, and the commands that talk to it as async commands.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use needle_core::anchor::{self, Anchor};
use needle_core::scene::SceneFile;
use needle_core::share::{self, Comment, Covers, Link, MAX_COMMENT_CHARS, On, ShareRecord, Shared};
use needle_vault::Vault;
use needle_vcs::Vault as Repo;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::state::{AppState, OrString, read_config, write_config};

/// Where the Worker is. `NEEDLE_WORKER` points elsewhere, e.g. at `wrangler dev`.
const WORKER: &str = "https://needle-and-thread.alasdiel.workers.dev";
/// Where the reader page is; a link is this, `#`, and the link's id and key.
const READER: &str = "https://alasdiel.github.io/needle-and-thread/read/";
/// Tells the window a link was sent, or failed to be.
const CHANGED_EVENT: &str = "shares-changed";
/// Readers get a new version at most this often.
const EVERY: Duration = Duration::from_secs(60);
/// The per-computer file remembering which comments have been seen.
const SEEN: &str = "shares-seen.json";

fn worker() -> String {
    std::env::var("NEEDLE_WORKER").unwrap_or_else(|_| WORKER.to_owned()).trim_end_matches('/').to_owned()
}

fn reader() -> String {
    std::env::var("NEEDLE_READER").unwrap_or_else(|_| READER.to_owned())
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|e| format!("no random numbers: {e}"))?;
    Ok(bytes)
}

// --- Talking to the Worker -------------------------------------------------------------------

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(30))).build().into()
}

/// What came back, or the Worker's reason in words.
fn answer(response: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<ureq::http::Response<ureq::Body>, String> {
    let mut response = response.map_err(|e| format!("couldn't reach the sharing service ({e})"))?;
    if response.status().is_success() {
        return Ok(response);
    }
    let why = response.body_mut().read_to_string().unwrap_or_default();
    Err(if why.trim().is_empty() { format!("the sharing service said {}", response.status()) } else { why.trim().to_owned() })
}

struct Keys {
    link: Link,
    author: String,
}

fn keys(record: &ShareRecord) -> Result<Keys, String> {
    Ok(Keys { link: record.link().ok_or("this link's key in shares.toml is damaged")?, author: record.author.clone() })
}

fn upload(keys: &Keys, shared: &Shared) -> Result<(), String> {
    let json = serde_json::to_vec(shared).or_string()?;
    let sealed = share::seal(&keys.link.key, random()?, &share::text_label(&keys.link.id), &json);
    answer(
        agent()
            .put(format!("{}/s/{}", worker(), keys.link.id))
            .header("Authorization", format!("Bearer {}", keys.author))
            .header("X-Commenter", share::fingerprint(&keys.link.comment_token()))
            .send(&sealed[..]),
    )?;
    Ok(())
}

/// A comment as the Worker lists it.
#[derive(Deserialize)]
struct Listed {
    id: u64,
    body: String,
    author: bool,
    resolved: bool,
    deleted: bool,
    created: f64,
}

/// A comment, opened.
struct Opened {
    id: u64,
    author: bool,
    resolved: bool,
    created: f64,
    comment: Comment,
}

fn comments(keys: &Keys) -> Result<Vec<Opened>, String> {
    let mut response = answer(agent().get(format!("{}/s/{}/comments", worker(), keys.link.id)).call())?;
    let listed: Vec<Listed> = response.body_mut().read_json().or_string()?;
    let label = share::comment_label(&keys.link.id);
    Ok(listed
        .into_iter()
        .filter(|c| !c.deleted)
        .filter_map(|c| {
            let sealed = share::decode(&c.body)?;
            let comment = serde_json::from_slice(&share::open(&keys.link.key, &label, &sealed)?).ok()?;
            Some(Opened { id: c.id, author: c.author, resolved: c.resolved, created: c.created, comment })
        })
        .collect())
}

// --- Where comments are now ------------------------------------------------------------------

/// A scene's prose as of snapshot `version`, if that's known.
fn prose_then(repo: &Repo, vault: &Vault, project: &str, scene: &str, version: &str) -> Option<String> {
    let path = vault.project(project).ok()?.scene_path(scene).ok()?;
    let path = path.strip_prefix(vault.root()).ok()?;
    let text = repo.read(version, path).ok()??;
    Some(anchor::prose(&SceneFile::parse(&text).markdown()))
}

/// Where a comment is in `now` (scene → prose), or None if its words are gone.
fn place(repo: Option<&Repo>, vault: &Vault, project: &str, comment: &Comment, now: &HashMap<String, String>) -> Option<On> {
    let on = comment.on.as_ref()?;
    let prose = now.get(&on.scene)?;
    let then = repo.and_then(|repo| prose_then(repo, vault, project, &on.scene, &comment.version));
    let anchor = anchor::reanchor(&on.anchor, then.as_deref(), prose)?;
    Some(On { scene: on.scene.clone(), anchor })
}

// --- Sending the text after snapshots --------------------------------------------------------

#[derive(Default)]
struct Sent {
    /// What was last sent, to tell whether there's anything new.
    hash: u64,
    at: Option<Instant>,
    /// When readers last got a new version, in seconds since the Unix epoch.
    done: Option<i64>,
    error: Option<String>,
}

#[derive(Default)]
struct Inner {
    sent: HashMap<String, Sent>,
    running: bool,
    again: bool,
    /// Comments last fetched, per link, for the counts in the panel.
    counts: HashMap<String, (usize, usize)>,
}

#[derive(Clone)]
pub struct Sharing {
    vault: Vault,
    inner: Arc<Mutex<Inner>>,
}

impl Sharing {
    pub fn new(vault: Vault) -> Self {
        Self { vault, inner: Arc::default() }
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().expect("the share thread panicked")
    }

    /// After a snapshot (and on opening): sends every link whose text changed.
    pub fn after_snapshot(&self, app: &AppHandle) {
        {
            let mut inner = self.inner();
            if inner.running {
                inner.again = true;
                return;
            }
            inner.running = true;
        }
        let (sharing, app) = (self.clone(), app.clone());
        thread::spawn(move || {
            loop {
                sharing.send_all(&app);
                let mut inner = sharing.inner();
                if !std::mem::take(&mut inner.again) {
                    inner.running = false;
                    break;
                }
            }
        });
    }

    fn send_all(&self, app: &AppHandle) {
        let Ok(list) = self.vault.shares() else { return };
        if list.shares.is_empty() {
            return;
        }
        let repo = Repo::open_or_init(self.vault.root()).ok();
        let version = repo.as_ref().and_then(|r| r.latest().ok().flatten()).unwrap_or_default();
        for record in &list.shares {
            let result = self.send(repo.as_ref(), record, &version, false);
            if result != Ok(false) {
                app.emit(CHANGED_EVENT, ()).ok();
            }
        }
    }

    /// Sends a link's text if it changed since it was last sent (or if `force`), waiting out
    /// the minute since the last time first. Returns whether it sent anything.
    fn send(&self, repo: Option<&Repo>, record: &ShareRecord, version: &str, force: bool) -> Result<bool, String> {
        let result = self.try_send(repo, record, version, force);
        if let Err(error) = &result {
            self.inner().sent.entry(record.id.clone()).or_default().error = Some(error.clone());
        }
        result
    }

    fn try_send(&self, repo: Option<&Repo>, record: &ShareRecord, version: &str, force: bool) -> Result<bool, String> {
        let keys = keys(record)?;
        let project = self.vault.project(&record.project).or_string()?;
        let mut shared = project.shared(&record.covers, version, now()).or_string()?;
        // Comments the text has moved under: where they are now, for readers to see them there.
        let opened = comments(&keys)?;
        let prose: HashMap<String, String> = shared.parts.iter().map(|p| (p.scene.clone(), anchor::prose(&p.markdown))).collect();
        for c in &opened {
            if let Some(on) = &c.comment.on {
                let now = place(repo, &self.vault, &record.project, &c.comment, &prose);
                if now.as_ref() != Some(on) {
                    shared.moved.insert(c.id, now);
                }
            }
        }
        self.inner().counts.insert(record.id.clone(), counts(&record.id, &opened, &seen_ids()));
        let hash = {
            let mut hasher = DefaultHasher::new();
            serde_json::to_string(&(&shared.title, &shared.parts, &shared.moved)).unwrap_or_default().hash(&mut hasher);
            hasher.finish()
        };
        let wait = {
            let inner = self.inner();
            let last = inner.sent.get(&record.id);
            if !force && last.is_some_and(|s| s.hash == hash && s.error.is_none()) {
                return Ok(false);
            }
            last.and_then(|s| s.at).map(|at| EVERY.saturating_sub(at.elapsed())).filter(|_| !force)
        };
        if let Some(wait) = wait {
            thread::sleep(wait);
        }
        upload(&keys, &shared)?;
        self.inner().sent.insert(record.id.clone(), Sent { hash, at: Some(Instant::now()), done: Some(now()), error: None });
        Ok(true)
    }
}

/// How many comments a link has, and how many of them are new here.
fn counts(id: &str, opened: &[Opened], seen: &HashMap<String, u64>) -> (usize, usize) {
    let seen = seen.get(id).copied().unwrap_or(0);
    (opened.len(), opened.iter().filter(|c| !c.author && c.id > seen).count())
}

/// Which comments have been seen on this computer, per link: the highest id. Loaded from the
/// config folder the first time it's needed.
static SEEN_CACHE: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);

fn seen_ids() -> HashMap<String, u64> {
    SEEN_CACHE.lock().ok().and_then(|seen| seen.clone()).unwrap_or_default()
}

fn load_seen(app: &AppHandle) {
    let seen = read_config(app, SEEN).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    if let Ok(mut cache) = SEEN_CACHE.lock() {
        *cache = Some(seen);
    }
}

// --- Commands --------------------------------------------------------------------------------

/// A link, as the Shared panel shows it.
#[derive(Serialize)]
pub struct ShareView {
    id: String,
    name: String,
    covers: Covers,
    /// What it covers, in words: "Arrival", "The whole book".
    title: String,
    /// The link readers get.
    link: String,
    created: i64,
    /// When readers last got a new version, this session.
    sent: Option<i64>,
    error: Option<String>,
    comments: usize,
    fresh: usize,
}

fn view(sharing: &Sharing, vault: &Vault, record: &ShareRecord) -> ShareView {
    let title = match &record.covers {
        Covers::Book => "The whole book".to_owned(),
        covers => vault.project(&record.project).and_then(|p| p.covers_title(covers)).unwrap_or_default(),
    };
    let inner = sharing.inner();
    let sent = inner.sent.get(&record.id);
    let (comments, fresh) = inner.counts.get(&record.id).copied().unwrap_or_default();
    ShareView {
        id: record.id.clone(),
        name: record.name.clone(),
        covers: record.covers.clone(),
        title,
        link: record.link().map(|l| format!("{}#{}", reader(), l.fragment())).unwrap_or_default(),
        created: record.created,
        sent: sent.and_then(|s| s.done),
        error: sent.and_then(|s| s.error.clone()),
        comments,
        fresh,
    }
}

/// The open vault's sharing, and the vault.
fn open_sharing(state: &State<'_, AppState>) -> Result<(Sharing, Vault), String> {
    state.with(|open| Ok((open.history.sharing().clone(), open.vault.clone())))
}

/// The project's links, newest first.
#[tauri::command]
pub fn shares(state: State<'_, AppState>, project: String) -> Result<Vec<ShareView>, String> {
    let (sharing, vault) = open_sharing(&state)?;
    let list = vault.shares().or_string()?;
    Ok(list.shares.iter().rev().filter(|s| s.project == project).map(|s| view(&sharing, &vault, s)).collect())
}

/// Makes a link and sends its text, so it works as soon as it's copied.
#[tauri::command(async)]
pub fn make_share(app: AppHandle, state: State<'_, AppState>, project: String, covers: Covers, name: String) -> Result<ShareView, String> {
    let (sharing, vault) = open_sharing(&state)?;
    let record = ShareRecord::new(random()?, &name, &project, covers, now());
    let repo = Repo::open_or_init(vault.root()).ok();
    let version = repo.as_ref().and_then(|r| r.latest().ok().flatten()).unwrap_or_default();
    sharing.send(repo.as_ref(), &record, &version, true)?;
    vault.add_share(record.clone()).or_string()?;
    app.emit(CHANGED_EVENT, ()).ok();
    Ok(view(&sharing, &vault, &record))
}

/// Revokes a link: its text and comments are deleted, and it stops working.
#[tauri::command(async)]
pub fn revoke_share(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let (sharing, vault) = open_sharing(&state)?;
    let record = vault.shares().or_string()?.shares.into_iter().find(|s| s.id == id).ok_or("that link is already gone")?;
    let keys = keys(&record)?;
    let gone = answer(agent().delete(format!("{}/s/{}", worker(), keys.link.id)).header("Authorization", format!("Bearer {}", keys.author)).call());
    // Already gone from the service is as good as revoked.
    if let Err(e) = gone
        && e != "no such share"
    {
        return Err(e);
    }
    vault.remove_share(&id).or_string()?;
    sharing.inner().sent.remove(&id);
    app.emit(CHANGED_EVENT, ()).ok();
    Ok(())
}

/// Opens the reader page as the author, with the tools to reply, resolve, delete and revoke.
#[tauri::command]
pub fn open_as_author(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let (_, vault) = open_sharing(&state)?;
    let record = vault.shares().or_string()?.shares.into_iter().find(|s| s.id == id).ok_or("that link is gone")?;
    let link = record.link().ok_or("this link's key in shares.toml is damaged")?;
    open_in_browser(&format!("{}#{}", reader(), link.author_fragment(&record.author)))
}

fn open_in_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    command.spawn().map(|_| ()).map_err(|e| format!("couldn't open the browser: {e}"))
}

/// A comment, as the window shows it.
#[derive(Serialize)]
pub struct CommentView {
    share: String,
    share_name: String,
    id: u64,
    /// The name its writer gave; empty for the author.
    name: String,
    text: String,
    author: bool,
    resolved: bool,
    /// Seconds since the Unix epoch.
    created: f64,
    reply_to: Option<u64>,
    /// The scene it's on, for a comment that isn't a reply.
    scene: Option<String>,
    /// Where its words are now; none if they're gone (or for a reply).
    anchor: Option<Anchor>,
    /// The words it was made on.
    quote: String,
    fresh: bool,
}

/// Every comment on the project's links, placed on the text as it is in the files now.
#[tauri::command(async)]
pub fn share_comments(app: AppHandle, state: State<'_, AppState>, project: String) -> Result<Vec<CommentView>, String> {
    let (sharing, vault) = open_sharing(&state)?;
    if SEEN_CACHE.lock().is_ok_and(|s| s.is_none()) {
        load_seen(&app);
    }
    let seen = seen_ids();
    let records: Vec<ShareRecord> = vault.shares().or_string()?.shares.into_iter().filter(|s| s.project == project).collect();
    let repo = Repo::open_or_init(vault.root()).ok();
    let project_files = vault.project(&project).or_string()?;
    let mut prose: HashMap<String, String> = HashMap::new();
    let mut out = Vec::new();
    let mut problem = None;
    for record in &records {
        let opened = match keys(record).and_then(|k| comments(&k)) {
            Ok(opened) => opened,
            Err(e) => {
                problem = Some(e);
                continue;
            }
        };
        sharing.inner().counts.insert(record.id.clone(), counts(&record.id, &opened, &seen));
        let last_seen = seen.get(&record.id).copied().unwrap_or(0);
        for c in opened {
            let scene = c.comment.on.as_ref().map(|on| on.scene.clone());
            if let Some(scene) = &scene
                && !prose.contains_key(scene)
                && let Ok(file) = project_files.scene(scene)
            {
                prose.insert(scene.clone(), anchor::prose(&file.markdown()));
            }
            let anchor = place(repo.as_ref(), &vault, &project, &c.comment, &prose).map(|on| on.anchor);
            out.push(CommentView {
                share: record.id.clone(),
                share_name: record.name.clone(),
                id: c.id,
                name: c.comment.name,
                text: c.comment.text,
                author: c.author,
                resolved: c.resolved,
                created: c.created,
                reply_to: c.comment.reply_to,
                quote: c.comment.on.map(|on| on.anchor.quote).unwrap_or_default(),
                scene,
                anchor,
                fresh: !c.author && c.id > last_seen,
            });
        }
    }
    match problem {
        Some(e) if out.is_empty() && !records.is_empty() => Err(e),
        _ => Ok(out),
    }
}

/// Marks a link's comments up to `id` as seen on this computer.
#[tauri::command]
pub fn seen_comments(app: AppHandle, share: String, id: u64) -> Result<(), String> {
    if SEEN_CACHE.lock().is_ok_and(|s| s.is_none()) {
        load_seen(&app);
    }
    let seen = {
        let mut cache = SEEN_CACHE.lock().map_err(|_| "the seen list broke")?;
        let seen = cache.get_or_insert_with(HashMap::new);
        let last = seen.entry(share).or_default();
        *last = (*last).max(id);
        seen.clone()
    };
    let sorted: BTreeMap<_, _> = seen.into_iter().collect();
    write_config(&app, SEEN, &serde_json::to_string(&sorted).or_string()?)
}

fn record_of(vault: &Vault, share: &str) -> Result<Keys, String> {
    let record = vault.shares().or_string()?.shares.into_iter().find(|s| s.id == share).ok_or("that link is gone")?;
    keys(&record)
}

/// Replies to a comment as the author.
#[tauri::command(async)]
pub fn reply_comment(state: State<'_, AppState>, share: String, to: u64, text: String) -> Result<(), String> {
    let (_, vault) = open_sharing(&state)?;
    let keys = record_of(&vault, &share)?;
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_COMMENT_CHARS {
        return Err("a reply needs a few words, and fewer than 4,000 characters".into());
    }
    let comment = Comment { v: share::FORMAT, name: String::new(), text: text.to_owned(), on: None, version: String::new(), reply_to: Some(to) };
    let sealed = share::seal(&keys.link.key, random()?, &share::comment_label(&keys.link.id), &serde_json::to_vec(&comment).or_string()?);
    answer(agent().post(format!("{}/s/{}/comments", worker(), keys.link.id)).header("Authorization", format!("Bearer {}", keys.author)).send(&sealed[..]))?;
    Ok(())
}

#[tauri::command(async)]
pub fn resolve_comment(state: State<'_, AppState>, share: String, id: u64, resolved: bool) -> Result<(), String> {
    let (_, vault) = open_sharing(&state)?;
    let keys = record_of(&vault, &share)?;
    answer(
        agent()
            .patch(format!("{}/s/{}/comments/{id}", worker(), keys.link.id))
            .header("Authorization", format!("Bearer {}", keys.author))
            .send_json(serde_json::json!({ "resolved": resolved })),
    )?;
    Ok(())
}

#[tauri::command(async)]
pub fn delete_comment(state: State<'_, AppState>, share: String, id: u64) -> Result<(), String> {
    let (_, vault) = open_sharing(&state)?;
    let keys = record_of(&vault, &share)?;
    answer(agent().delete(format!("{}/s/{}/comments/{id}", worker(), keys.link.id)).header("Authorization", format!("Bearer {}", keys.author)).call())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use needle_core::share::SharedPart;

    /// Against a Worker run by `wrangler dev`: `NEEDLE_WORKER=http://localhost:8787 cargo test
    /// -p needle-desktop -- --ignored talks_to`.
    #[test]
    #[ignore = "needs a Worker at NEEDLE_WORKER"]
    fn talks_to_the_worker() {
        assert!(std::env::var("NEEDLE_WORKER").is_ok(), "set NEEDLE_WORKER");
        let record = ShareRecord::new(random().unwrap(), "Beta readers", "tidewater", Covers::Book, now());
        let keys = keys(&record).unwrap();
        let shared = Shared {
            v: share::FORMAT,
            project: "Tidewater".into(),
            title: "Tidewater".into(),
            parts: vec![SharedPart { chapter: None, scene: "night-market".into(), title: "The night market".into(), markdown: "The lanterns were lit before the bells.\n".into() }],
            updated: now(),
            version: "v1".into(),
            moved: Default::default(),
        };
        upload(&keys, &shared).unwrap();

        // A reader reads it, and comments with the token the link gives them.
        let mut got = answer(agent().get(format!("{}/s/{}", worker(), keys.link.id)).call()).unwrap();
        let sealed = got.body_mut().read_to_vec().unwrap();
        let opened: Shared = serde_json::from_slice(&share::open(&keys.link.key, &share::text_label(&keys.link.id), &sealed).unwrap()).unwrap();
        assert_eq!(opened, shared);
        let prose = anchor::prose(&shared.parts[0].markdown);
        let on = On { scene: "night-market".into(), anchor: Anchor::at(&prose, 18..38).unwrap() };
        assert_eq!(on.anchor.quote, "lit before the bells");
        let comment = Comment { v: 1, name: "Ines".into(), text: "Why before?".into(), on: Some(on.clone()), version: "v1".into(), reply_to: None };
        let body = share::seal(&keys.link.key, random().unwrap(), &share::comment_label(&keys.link.id), &serde_json::to_vec(&comment).unwrap());
        answer(agent().post(format!("{}/s/{}/comments", worker(), keys.link.id)).header("X-Comment-Token", keys.link.comment_token()).send(&body[..])).unwrap();

        let listed = comments(&keys).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].comment.name.as_str(), listed[0].author, listed[0].resolved), ("Ines", false, false));
        assert_eq!(listed[0].comment.on, Some(on));

        // Placed in the text as it is now, it's found by its words.
        let now = HashMap::from([("night-market".to_owned(), anchor::prose("A new line.\n\nThe lanterns were lit before the bells.\n"))]);
        let vault_dir = tempfile::TempDir::new().unwrap();
        let vault = Vault::create(vault_dir.path()).unwrap();
        let placed = place(None, &vault, "tidewater", &listed[0].comment, &now).unwrap();
        assert_eq!(placed.anchor.find(&now["night-market"]).map(|r| r.start), Some(30));

        answer(agent().delete(format!("{}/s/{}", worker(), keys.link.id)).header("Authorization", format!("Bearer {}", keys.author)).call()).unwrap();
        assert_eq!(answer(agent().get(format!("{}/s/{}", worker(), keys.link.id)).call()).err().as_deref(), Some("no such share"));
    }
}

//! Writing on the phone (DESIGN §9; the canvas's Phone writing page). A scene or note opens to
//! read and **Write** turns the page into the editor (Enter 2), with a bar of formats above the
//! keyboard that turns into notes to link while one is typed (Format 1 and 1b). A quiet line in
//! the top bar says where the words are (Save 1), and a strip under it says when they couldn't
//! go to GitHub (Save 3). A note's title and other names can be changed too.
//!
//! What's written goes into the snapshot as it's typed and is kept on the phone (`save.rs`);
//! it's sent to GitHub after a pause, at least every couple of minutes while typing goes on,
//! and straight away on Done, on leaving, or when the phone puts the page away.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use leptos::{ev, html, prelude::*, task::spawn_local};
use needle_core::merge::{self, CONFLICTS};
use needle_core::names::Owner;
use needle_core::scene::SceneFile;
use needle_core::settings::TypographySettings;
use needle_vault::store::{Change, Snapshot, Store};
use needle_vault::{Project, Vault};
use wasm_bindgen::{JsCast, prelude::Closure};

use crate::app::{self, CHEVRON_LEFT, Ctx, SIZES};
use crate::editor::{Editor, LinkBridge};
use crate::github::{self, Creds, WriteError};
use crate::save::{self, Outcome};

/// How long after the last change a send waits.
const IDLE_MS: f64 = 20_000.0;
/// The longest changes wait while typing goes on.
const MAX_MS: f64 = 120_000.0;
/// How long after failing to reach GitHub it tries again.
const RETRY_MS: f64 = 30_000.0;

/// Where what's been written is.
#[derive(Clone, Debug, PartialEq)]
pub enum Saving {
    /// Nothing written since the vault was read.
    Clean,
    /// Kept on the phone, to go to GitHub soon.
    Kept,
    Sending,
    /// On GitHub, since this time (ms since the epoch).
    Sent(f64),
    /// Kept on the phone: GitHub couldn't be reached.
    Offline,
    /// Kept on the phone: the token can only read.
    ReadOnly,
    /// Kept on the phone: GitHub refused, saying this.
    Failed(String),
}

/// What's open to write in.
#[derive(Clone, Debug, PartialEq)]
pub enum Doc {
    Scene(String),
    Note(Owner, String),
}

impl Doc {
    fn path(&self, vault: &Vault, project: &Project) -> Result<PathBuf, String> {
        match self {
            Self::Scene(slug) => project.scene_path(slug),
            Self::Note(owner, path) => vault.notes_of(owner).and_then(|n| n.file_path(path)),
        }
        .map_err(|e| e.to_string())
    }

    fn save(&self, vault: &Vault, project: &Project, markdown: &str) -> Result<bool, String> {
        match self {
            Self::Scene(slug) => project.save_body(slug, markdown),
            Self::Note(owner, path) => vault.notes_of(owner).and_then(|n| n.save_body(path, markdown)),
        }
        .map_err(|e| e.to_string())
    }
}

// --- Sending ---------------------------------------------------------------------------------

fn drafts_key(creds: &Creds) -> String {
    format!("needle-phone-drafts:{}", creds.repo.trim())
}

fn now() -> f64 {
    js_sys::Date::now()
}

fn stamp() -> String {
    merge::stamp((now() / 1000.0) as i64)
}

impl Ctx {
    /// The changes kept on this phone for the vault it's connected to.
    pub(crate) fn kept_drafts(&self) -> Vec<save::Draft> {
        let Some(creds) = self.creds.get_untracked() else { return Vec::new() };
        app::remembered(&drafts_key(&creds)).and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default()
    }

    /// Keeps the snapshot's unsent changes in the phone's storage.
    fn persist(&self) {
        let (Some(creds), Some(snapshot)) = (self.creds.get_untracked(), self.snapshot.get_value()) else { return };
        let drafts = save::drafts(&snapshot);
        match drafts.is_empty() {
            true => app::forget(&drafts_key(&creds)),
            false => app::remember(&drafts_key(&creds), &serde_json::to_string(&drafts).unwrap_or_default()),
        }
    }

    pub(crate) fn forget_drafts(&self) {
        if let Some(creds) = self.creds.get_untracked() {
            app::forget(&drafts_key(&creds));
        }
    }

    /// Puts the changes kept on the phone back into a snapshot just read from GitHub, and
    /// sends them if there are any.
    pub(crate) fn restore(&self, snapshot: &Snapshot) {
        let drafts = self.kept_drafts();
        if drafts.is_empty() {
            return;
        }
        let outcomes = save::restore(snapshot, &drafts, &stamp());
        self.report(&outcomes.iter().map(|(_, o)| *o).collect::<Vec<_>>());
        self.persist();
        self.saving.set(Saving::Kept);
        self.schedule(0.0);
    }

    /// Says when a change was merged with one made elsewhere, and has the screens read again.
    fn report(&self, outcomes: &[Outcome]) {
        if outcomes.iter().any(|o| *o != Outcome::Same) {
            self.touched();
        }
        if outcomes.contains(&Outcome::Clashed) {
            self.notice.set(Some(
                "Changed on another device too, in the same place. Both versions are kept: settle it on the desktop.".to_owned(),
            ));
        } else if outcomes.contains(&Outcome::Merged) {
            self.say("Took in changes made on another device".to_owned());
        }
    }

    /// After a change went into the snapshot: keeps it, and sends it after a pause.
    pub(crate) fn changed(&self) {
        self.persist();
        if !matches!(self.saving.get_untracked(), Saving::Sending) {
            self.saving.set(Saving::Kept);
        }
        let since = self.kept_since.get_value().unwrap_or_else(now);
        self.kept_since.set_value(Some(since));
        let waited = now() - since;
        self.schedule(if waited >= MAX_MS { 0.0 } else { IDLE_MS.min(MAX_MS - waited) });
    }

    fn schedule(&self, after_ms: f64) {
        if let Some(timer) = self.timer.get_value() {
            timer.clear();
        }
        let ctx = *self;
        let timer = set_timeout_with_handle(move || ctx.send(), Duration::from_millis(after_ms as u64)).ok();
        self.timer.set_value(timer);
    }

    /// Takes in what's being typed, then sends everything now.
    pub(crate) fn send_now(&self) {
        self.flush_editor();
        self.send();
    }

    fn flush_editor(&self) {
        self.editor.with_value(|e| {
            if let Some(e) = e {
                e.flush();
            }
        });
    }

    fn pending(&self) -> usize {
        self.snapshot.get_value().map(|s| s.changes().len()).unwrap_or(0)
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.pending() > 0
    }

    fn send(&self) {
        if let Some(timer) = self.timer.get_value() {
            timer.clear();
            self.timer.set_value(None);
        }
        if self.sending.get_value() {
            self.again.set_value(true);
            return;
        }
        let (Some(creds), Some(snapshot)) = (self.creds.get_untracked(), self.snapshot.get_value()) else { return };
        if snapshot.changes().is_empty() {
            return;
        }
        self.sending.set_value(true);
        self.saving.set(Saving::Sending);
        let ctx = *self;
        spawn_local(async move {
            let result = ctx.send_once(&creds, &snapshot).await;
            ctx.sending.set_value(false);
            ctx.persist();
            let left = !snapshot.changes().is_empty();
            match result {
                Ok(()) if left => ctx.saving.set(Saving::Kept),
                Ok(()) => {
                    ctx.kept_since.set_value(None);
                    ctx.saving.set(Saving::Sent(now()));
                }
                Err(WriteError::Offline) => ctx.saving.set(Saving::Offline),
                Err(WriteError::ReadOnly) => ctx.saving.set(Saving::ReadOnly),
                Err(WriteError::Moved) => ctx.saving.set(Saving::Failed("GitHub kept changing while this was sent.".to_owned())),
                Err(WriteError::Other(e)) => ctx.saving.set(Saving::Failed(e)),
            }
            if ctx.again.try_update_value(std::mem::take).unwrap_or(false) {
                ctx.send();
            } else if left {
                ctx.schedule(match ctx.saving.get_untracked() {
                    Saving::Kept => IDLE_MS,
                    // A token that can only read won't change by itself.
                    Saving::ReadOnly => return,
                    _ => RETRY_MS,
                });
            }
        });
    }

    /// Brings the changes up to date with GitHub and commits them, trying again if the branch
    /// moves on in between.
    async fn send_once(&self, creds: &Creds, snapshot: &Snapshot) -> Result<(), WriteError> {
        for _ in 0..3 {
            let paths: Vec<String> = snapshot
                .changes()
                .into_iter()
                .filter(|(_, c)| matches!(c, Change::Write(_)))
                .map(|(p, _)| p.to_string_lossy().replace('\\', "/"))
                .collect();
            let current = github::current(creds, &paths).await?;
            // Anything typed while GitHub answered, so a merge takes it in too.
            self.flush_editor();
            let stamp = stamp();
            let open = self.editing_path.get_value();
            let mut outcomes = Vec::new();
            let mut open_changed = false;
            for (path, found) in &current.files {
                let outcome = save::reconcile(snapshot, Path::new(path), found.as_ref().map(|(s, t)| (s.as_str(), t.as_str())), &stamp);
                if outcome != Outcome::Same && open.as_deref() == Some(Path::new(path)) {
                    open_changed = true;
                }
                outcomes.push(outcome);
            }
            if open_changed && let Some(path) = &open {
                let body = snapshot.read_to_string(path).map(|t| SceneFile::parse(&t).markdown()).unwrap_or_default();
                self.editor.with_value(|e| {
                    if let Some(e) = e {
                        e.replace_markdown(&body);
                    }
                });
            }
            self.report(&outcomes);
            self.persist();

            // What was looked at goes, and the copies a clash keeps.
            let sent: Vec<(PathBuf, Vec<u8>)> = snapshot
                .changes()
                .into_iter()
                .filter_map(|(p, c)| match c {
                    Change::Write(bytes) => Some((p, bytes)),
                    Change::Remove => None,
                })
                .filter(|(p, _)| p.starts_with(CONFLICTS) || paths.iter().any(|q| Path::new(q) == p))
                .collect();
            if sent.is_empty() {
                return Ok(());
            }
            let message = save::message(snapshot, &sent.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>());
            let writes: Vec<(String, Vec<u8>)> = sent.iter().map(|(p, b)| (p.to_string_lossy().replace('\\', "/"), b.clone())).collect();
            match github::commit(creds, &current.head, &message, &writes, &[]).await {
                Ok(shas) => {
                    for ((path, bytes), (_, sha)) in sent.into_iter().zip(shas) {
                        snapshot.sent(&path, Some(&sha), &Change::Write(bytes));
                    }
                    return Ok(());
                }
                Err(WriteError::Moved) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(WriteError::Moved)
    }
}

// --- The screen ------------------------------------------------------------------------------

/// What the writing screen shows above the text.
enum Head {
    Scene { label: String, title: String, status: String },
    Note { kind: String, title: String, aliases: Vec<String> },
}

struct Start {
    path: PathBuf,
    body: String,
    words: usize,
    typography: TypographySettings,
    links: LinkBridge,
    head: Head,
}

fn start(vault: &Vault, project: &Project, doc: &Doc) -> Result<Start, String> {
    let path = doc.path(vault, project)?;
    let file = SceneFile::parse(&vault.store().read_to_string(&path).map_err(|e| e.to_string())?);
    let body = file.markdown();
    let head = match doc {
        Doc::Scene(slug) => {
            let reading = crate::model::reading(vault, project, slug)?;
            let label = match &reading.chapter {
                Some(chapter) => format!("Piece {} · {chapter}", reading.piece),
                None => format!("Piece {}", reading.piece),
            };
            Head::Scene { label, title: reading.scene.title, status: reading.scene.status }
        }
        Doc::Note(owner, path) => {
            let card = crate::model::note(vault, project, owner, path)?;
            Head::Note { kind: card.kind, title: card.title, aliases: card.aliases }
        }
    };
    let names = vault.names(project).map_err(|e| e.to_string())?;
    Ok(Start {
        path,
        words: needle_core::words::count_markdown_words(&body),
        body,
        typography: vault.settings().map(|s| s.typography).unwrap_or_default(),
        links: LinkBridge::new(names, project.config.kind),
        head,
    })
}

/// The bar's buttons: what each runs, its label, and what it shows.
const BUTTONS: [(&str, &str, &str); 7] = [
    ("bold", "Bold", r#"<span class="g bold">B</span>"#),
    ("italic", "Italic", r#"<span class="g italic">I</span>"#),
    ("heading", "Heading", r#"<span class="g title">H</span>"#),
    ("quote", "Quote", r#"<span class="g title quote">“</span>"#),
    ("list", "List", LIST),
    ("link", "Link to a note", r#"<span class="g brackets">[[ ]]</span>"#),
    ("sceneBreak", "Scene break", STITCHES),
];

/// The writing screen. `scroll` is where the page was scrolled to when Write was tapped;
/// `back` names where Back goes. `on_done` leaves writing, `on_back` goes back as well.
#[component]
pub fn Writer(doc: Doc, scroll: i32, back: &'static str, on_done: impl Fn() + Copy + 'static, on_back: impl Fn() + Copy + 'static) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    // Read once, untracked, so typing never redraws the page around the editor.
    let Some(Ok(start)) = ctx.with_untracked(|v, p| start(v, p, &doc)) else {
        return view! {
            <header class="topbar">
                <button type="button" class="back" on:click=move |_| on_back()><span inner_html=CHEVRON_LEFT></span>{back}</button>
            </header>
            <main class="scroll"><p class="problem padded">"This can’t be opened to write in."</p></main>
        }
        .into_any();
    };
    ctx.editing_path.set_value(Some(start.path.clone()));
    on_cleanup(move || ctx.editing_path.set_value(None));

    let words = RwSignal::new(start.words);
    let formats = RwSignal::new(Vec::<String>::new());
    let suggestions = RwSignal::new(None::<Vec<(String, String)>>);
    let on_change = {
        let doc = doc.clone();
        move |markdown: String, count: u32| {
            words.try_set(count as usize);
            match ctx.with_untracked(|v, p| doc.save(v, p, &markdown)) {
                Some(Ok(_)) => ctx.changed(),
                Some(Err(e)) => ctx.say(format!("Couldn’t keep that: {e}")),
                None => {}
            }
        }
    };

    // The keyboard takes the bottom of the screen; the page sits above it, bar and all.
    let frame = RwSignal::new(String::new());
    let viewport = web_sys::window().and_then(|w| w.visual_viewport());
    if let Some(vv) = viewport {
        let fit = {
            let vv = vv.clone();
            move || {
                frame.try_set(format!("top: {}px; height: {}px", vv.offset_top(), vv.height()));
            }
        };
        fit();
        let listener = Closure::<dyn Fn()>::new(fit);
        for event in ["resize", "scroll"] {
            let _ = vv.add_event_listener_with_callback(event, listener.as_ref().unchecked_ref());
        }
        let kept = StoredValue::new_local(Some((vv, listener)));
        on_cleanup(move || {
            if let Some((vv, listener)) = kept.try_update_value(Option::take).flatten() {
                for event in ["resize", "scroll"] {
                    let _ = vv.remove_event_listener_with_callback(event, listener.as_ref().unchecked_ref());
                }
            }
        });
    }

    // Back where the reading was.
    let page = NodeRef::<html::Main>::new();
    Effect::new(move |_| {
        if let Some(page) = page.get() {
            page.set_scroll_top(scroll);
        }
    });

    let size = move || format!("font-size: {}px", SIZES[ctx.size.get()]);
    let done = move |_| {
        ctx.flush_editor();
        on_done();
        ctx.send_now();
    };
    let go_back = move |_| {
        ctx.flush_editor();
        on_back();
        ctx.send_now();
    };
    let head = head_view(ctx, doc.clone(), start.head, words);
    let links = Rc::new(start.links);

    view! {
        <div class="writing" style=move || frame.get()>
            <header class="topbar">
                <button type="button" class="back" on:click=go_back><span inner_html=CHEVRON_LEFT></span>{back}</button>
                <span class="grow"></span>
                <SaveLine />
                <button type="button" class="icon" aria-label="Text size" on:click=move |_| app::step_size(ctx)>
                    <span class="title aa">"Aa"</span>
                </button>
                <button type="button" class="done" on:click=done>"Done"</button>
            </header>
            <Strip />
            <main class="scroll page writer" node_ref=page>
                {head}
                <div class="prose" style=size>
                    <Editor
                        handle=ctx.editor
                        markdown=start.body
                        typography=start.typography
                        links
                        on_change
                        on_formats=move |on| { formats.try_set(on); }
                        on_suggestions=move |list| { suggestions.try_set(list); }
                    />
                </div>
            </main>
            <Bar formats suggestions />
        </div>
    }
    .into_any()
}

fn head_view(ctx: Ctx, doc: Doc, head: Head, words: RwSignal<usize>) -> impl IntoView {
    match head {
        Head::Scene { label, title, status } => view! {
            <div class="sc label">{label}</div>
            <h1 class="title heading">{title}</h1>
            <div class="meta">
                <span class="pill sc">{status}</span>
                <span class="words">{move || format!("{} words", app::count(words.get()))}</span>
            </div>
        }
        .into_any(),
        Head::Note { kind, title, aliases } => {
            let Doc::Note(owner, path) = doc else { return ().into_any() };
            view! { <NoteHead ctx kind title aliases owner path /> }.into_any()
        }
    }
}

/// A note's kind, its title to change, and its other names to add to or take from.
#[component]
fn NoteHead(ctx: Ctx, kind: String, title: String, aliases: Vec<String>, owner: Owner, path: String) -> impl IntoView {
    let title_now = StoredValue::new(title.clone());
    let aliases = RwSignal::new(aliases);
    let adding = RwSignal::new(false);
    let (owner, path) = (StoredValue::new(owner), StoredValue::new(path));
    let rename = move |ev: ev::Event| {
        let title = event_target_value(&ev).trim().to_owned();
        if title.is_empty() || title == title_now.get_value() {
            return;
        }
        match ctx.vault.with_untracked(|v| v.as_ref().map(|v| v.rename_note(&owner.get_value(), &path.get_value(), &title))) {
            Some(Ok(_)) => {
                title_now.set_value(title);
                ctx.changed();
            }
            Some(Err(e)) => ctx.say(format!("Couldn’t rename it: {e}")),
            None => {}
        }
    };
    let save_aliases = move |list: Vec<String>| {
        let saved = ctx.vault.with_untracked(|v| {
            let v = v.as_ref()?;
            Some(v.notes_of(&owner.get_value()).and_then(|n| n.update_header(&path.get_value(), |h| h.set_list("aliases", &list))))
        });
        match saved {
            Some(Ok(_)) => {
                aliases.set(list);
                ctx.changed();
            }
            Some(Err(e)) => ctx.say(format!("Couldn’t change its names: {e}")),
            None => {}
        }
    };
    let add = move |ev: ev::Event| {
        let name = event_target_value(&ev).trim().to_owned();
        adding.set(false);
        if name.is_empty() || aliases.with_untracked(|a| a.iter().any(|n| n.eq_ignore_ascii_case(&name))) {
            return;
        }
        let mut list = aliases.get_untracked();
        list.push(name);
        save_aliases(list);
    };
    let input = NodeRef::<html::Input>::new();
    Effect::new(move |_| {
        if let Some(el) = input.get() {
            let _ = el.focus();
        }
    });
    view! {
        <div class="sc kind">{kind}</div>
        <label class="hidden-label" for="note-title">"Title"</label>
        <input id="note-title" class="titlefield title" value=title autocapitalize="words" on:change=rename />
        <div class="aliases">
            <span class="hint">"Also called"</span>
            {move || aliases.get().into_iter().map(|name| {
                let gone = name.clone();
                view! {
                    <span class="alias">
                        {name.clone()}
                        <button
                            type="button"
                            class="unalias"
                            aria-label=format!("Stop calling it {name}")
                            on:click=move |_| save_aliases(aliases.get_untracked().into_iter().filter(|n| *n != gone).collect())
                        >"×"</button>
                    </span>
                }
            }).collect_view()}
            <Show
                when=move || adding.get()
                fallback=move || view! { <button type="button" class="alias add" on:click=move |_| adding.set(true)>"+ name"</button> }
            >
                <input
                    node_ref=input
                    class="alias new"
                    aria-label="Another name"
                    autocapitalize="words"
                    on:change=add
                    on:blur=move |_| { adding.try_set(false); }
                />
            </Show>
        </div>
    }
}

/// Where the words are: "Saved on this phone", "On GitHub · just now", "Not sent".
#[component]
fn SaveLine() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let now = app::ticker();
    view! {
        {move || {
            let (text, warn) = match ctx.saving.get() {
                Saving::Clean => (String::new(), false),
                Saving::Kept => ("Saved on this phone".to_owned(), false),
                Saving::Sending => ("Sending…".to_owned(), false),
                Saving::Sent(at) => (format!("On GitHub · {}", app::ago(now.get() - at)), false),
                Saving::Offline | Saving::ReadOnly | Saving::Failed(_) => ("Not sent".to_owned(), true),
            };
            view! { <span class="saved" class:warn=warn role="status">{text}</span> }
        }}
    }
}

/// Why what's written isn't on GitHub (Save 3).
#[component]
fn Strip() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let why = move || match ctx.saving.get() {
        Saving::Offline => Some("Not on GitHub yet: no connection. It’s kept on this phone and goes up when you’re back online.".to_owned()),
        Saving::ReadOnly => Some(
            "GitHub won’t take changes with this token: it can only read. It’s kept on this phone; in Settings, use a token that can write.".to_owned(),
        ),
        Saving::Failed(e) => Some(format!("Not on GitHub yet. {e} It’s kept on this phone and will be tried again.")),
        _ => None,
    };
    view! {
        {move || why().map(|text| view! { <div class="strip" role="status"><span inner_html=WARN></span><span>{text}</span></div> })}
    }
}

/// A change merged with one made on another device in the same place, said until it's closed.
#[component]
pub fn Notice() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    view! {
        {move || ctx.notice.get().map(|text| view! {
            <div class="notice" role="status">
                <span inner_html=WARN></span>
                <span class="grow">{text}</span>
                <button type="button" class="unalias" aria-label="Close" on:click=move |_| ctx.notice.set(None)>"×"</button>
            </div>
        })}
    }
}

/// The formats, or while a link is typed the notes it could be (Format 1 and 1b).
#[component]
fn Bar(formats: RwSignal<Vec<String>>, suggestions: RwSignal<Option<Vec<(String, String)>>>) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    // On mousedown, so the text keeps the cursor and the keyboard stays up.
    let keep_focus = |ev: ev::MouseEvent| ev.prevent_default();
    let run = move |name: &'static str| {
        ctx.editor.with_value(|e| {
            if let Some(e) = e {
                e.format(name);
            }
        });
    };
    let pick = move |i: usize| {
        ctx.editor.with_value(|e| {
            if let Some(e) = e {
                e.pick_suggestion(i as u32);
            }
        });
    };
    let hide = move |_| {
        ctx.editor.with_value(|e| {
            if let Some(e) = e {
                e.blur();
            }
        });
    };
    view! {
        <div class="fmt" role="toolbar" aria-label="Formatting">
            {move || match suggestions.get() {
                Some(list) => view! {
                    <div class="chips" role="listbox" aria-label="Notes to link">
                        {list.into_iter().enumerate().map(|(i, (label, detail))| {
                            let loose = detail == "no note yet";
                            view! {
                                <button type="button" class="chip" class:first=i == 0 class:loose=loose role="option" on:mousedown=keep_focus on:click=move |_| pick(i)>
                                    <span>{label}</span>
                                    {(!detail.is_empty()).then(|| view! { <span class="detail">{detail}</span> })}
                                </button>
                            }
                        }).collect_view()}
                    </div>
                }.into_any(),
                None => view! {
                    {BUTTONS.into_iter().map(|(name, label, glyph)| view! {
                        <button
                            type="button"
                            class="wb"
                            class:on=move || formats.with(|f| f.iter().any(|n| n == name))
                            aria-label=label
                            aria-pressed=move || formats.with(|f| f.iter().any(|n| n == name)).to_string()
                            on:mousedown=keep_focus
                            on:click=move |_| run(name)
                            inner_html=glyph
                        ></button>
                    }).collect_view()}
                    <span class="grow"></span>
                    <span class="sep"></span>
                    <button type="button" class="wb muted" aria-label="Hide keyboard" on:mousedown=keep_focus on:click=hide inner_html=HIDE></button>
                }.into_any(),
            }}
        </div>
    }
}

/// The Write button, for a scene or note being read.
pub fn write_button(on: impl Fn() + 'static) -> impl IntoView {
    view! {
        <button type="button" class="write" on:click=move |_| on()>
            <span inner_html=PENCIL></span>
            "Write"
        </button>
    }
}

const LIST: &str = r#"<svg width="21" height="21" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" aria-hidden="true"><path d="M9 6h11M9 12h11M9 18h11"/><circle cx="4.5" cy="6" r="1"/><circle cx="4.5" cy="12" r="1"/><circle cx="4.5" cy="18" r="1"/></svg>"#;
const STITCHES: &str = r#"<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" aria-hidden="true"><path d="M3 13.5l3-3M10.5 13.5l3-3M18 13.5l3-3"/></svg>"#;
const HIDE: &str = r#"<svg width="21" height="21" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6"/></svg>"#;
const PENCIL: &str = r#"<svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20l1-4.5L16 4.5a2.1 2.1 0 0 1 3 3L8 18.5z"/><path d="M14 6.5l3 3"/></svg>"#;
const WARN: &str = r#"<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 4l9 16H3z"/><path d="M12 10v4.5M12 17.3v.2"/></svg>"#;

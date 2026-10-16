//! The phone's screens (DESIGN §9; Nav 1, Read 1, Setup and Link 1 on the canvas's Phone page):
//! a tab bar for the outline, notes and search; a plain page for reading a scene; a sheet for a
//! linked note. Reading only, for now: nothing here writes to the vault.

use leptos::{ev, prelude::*, task::spawn_local};
use needle_core::names::Owner;
use needle_vault::{Project, Vault};
use wasm_bindgen::JsCast;

use crate::github::{self, Creds};
use crate::load;
use crate::model::{self, Found, Row};
use crate::search;

// --- What the phone remembers -----------------------------------------------------------------

const REPO: &str = "needle-phone-repo";
const TOKEN: &str = "needle-phone-token";
const PROJECT: &str = "needle-phone-project";
const SIZE: &str = "needle-phone-size";
/// Reading sizes, in px, that Aa steps through.
const SIZES: [u32; 4] = [16, 18, 20, 22];

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn remembered(key: &str) -> Option<String> {
    storage()?.get_item(key).ok()?.filter(|v| !v.is_empty())
}

fn remember(key: &str, value: &str) {
    if let Some(s) = storage() {
        let _ = s.set_item(key, value);
    }
}

fn forget(key: &str) {
    if let Some(s) = storage() {
        let _ = s.remove_item(key);
    }
}

fn saved_creds() -> Option<Creds> {
    Some(Creds { repo: remembered(REPO)?, token: remembered(TOKEN)? })
}

// --- State -----------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Screen {
    Outline,
    Notes,
    Search,
    Scene(String),
    Note(Owner, String),
}

impl Screen {
    fn is_tab(&self) -> bool {
        matches!(self, Self::Outline | Self::Notes | Self::Search)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Load {
    /// Files in hand, of how many.
    Loading(usize, usize),
    Failed(String),
    Ready,
}

#[derive(Clone, Copy)]
struct Ctx {
    creds: RwSignal<Option<Creds>>,
    vault: RwSignal<Option<Vault>>,
    load: RwSignal<Load>,
    /// When the vault was last read from GitHub, in ms since the epoch.
    read_at: RwSignal<f64>,
    refreshing: RwSignal<bool>,
    project: RwSignal<Option<String>>,
    screen: RwSignal<Screen>,
    /// Where Back goes, most recent last.
    trail: StoredValue<Vec<Screen>>,
    sheet: RwSignal<Option<(Owner, String)>>,
    picker: RwSignal<bool>,
    settings: RwSignal<bool>,
    toast: RwSignal<Option<String>>,
    size: RwSignal<usize>,
    query: RwSignal<String>,
}

impl Ctx {
    fn go(&self, to: Screen) {
        let from = self.screen.get_untracked();
        self.trail.update_value(|t| t.push(from));
        self.sheet.set(None);
        self.screen.set(to);
    }

    fn back(&self) {
        let to = self.trail.try_update_value(|t| t.pop()).flatten().unwrap_or(Screen::Outline);
        self.screen.set(to);
    }

    fn tab(&self, to: Screen) {
        self.trail.set_value(Vec::new());
        self.screen.set(to);
    }

    /// Runs `f` on the vault and the open project, tracking both.
    fn with<T>(&self, f: impl FnOnce(&Vault, &Project) -> T) -> Option<T> {
        let slug = self.project.get()?;
        self.vault.with(|v| {
            let v = v.as_ref()?;
            let project = v.project(&slug).ok()?;
            Some(f(v, &project))
        })
    }

    fn say(&self, text: String) {
        let toast = self.toast;
        toast.set(Some(text.clone()));
        set_timeout(
            move || {
                if toast.try_get_untracked().flatten().as_deref() == Some(text.as_str()) {
                    toast.try_set(None);
                }
            },
            std::time::Duration::from_secs(3),
        );
    }

    /// Opens the note a link names, as a sheet over what's open.
    fn follow(&self, name: &str) {
        match self.with(|v, p| model::resolve(v, p, name)).flatten() {
            Some(note) => self.sheet.set(Some(note)),
            None => self.say(format!("No note called “{name}” yet")),
        }
    }

    /// Reads the vault from GitHub. The first time the loading screen shows; after that what's
    /// there stays until the new copy is in.
    fn read(&self) {
        let Some(creds) = self.creds.get_untracked() else { return };
        let ctx = *self;
        let first = self.vault.with_untracked(Option::is_none);
        if first {
            ctx.load.set(Load::Loading(0, 0));
        } else {
            ctx.refreshing.set(true);
        }
        spawn_local(async move {
            let opened = load::open(&creds, |done, total| {
                if first {
                    ctx.load.try_set(Load::Loading(done, total));
                }
            })
            .await;
            ctx.refreshing.try_set(false);
            match opened {
                Ok(vault) => {
                    let projects: Vec<String> = vault.projects().unwrap_or_default().into_iter().map(|p| p.slug).collect();
                    let keep = ctx.project.get_untracked().filter(|p| projects.contains(p));
                    let project = keep.or_else(|| remembered(PROJECT).filter(|p| projects.contains(p))).or_else(|| projects.first().cloned());
                    ctx.project.set(project);
                    ctx.vault.set(Some(vault));
                    ctx.read_at.set(js_sys::Date::now());
                    ctx.load.set(Load::Ready);
                }
                Err(e) if first => ctx.load.set(Load::Failed(e)),
                Err(e) => ctx.say(e),
            }
        });
    }
}

// --- The app ---------------------------------------------------------------------------------

#[component]
pub fn App() -> impl IntoView {
    let ctx = Ctx {
        creds: RwSignal::new(saved_creds()),
        vault: RwSignal::new(None),
        load: RwSignal::new(Load::Loading(0, 0)),
        read_at: RwSignal::new(0.0),
        refreshing: RwSignal::new(false),
        project: RwSignal::new(None),
        screen: RwSignal::new(Screen::Outline),
        trail: StoredValue::new(Vec::new()),
        sheet: RwSignal::new(None),
        picker: RwSignal::new(false),
        settings: RwSignal::new(false),
        toast: RwSignal::new(None),
        size: RwSignal::new(remembered(SIZE).and_then(|s| s.parse().ok()).filter(|&i: &usize| i < SIZES.len()).unwrap_or(1)),
        query: RwSignal::new(String::new()),
    };
    provide_context(ctx);
    if ctx.creds.get_untracked().is_some() {
        ctx.read();
    }

    view! {
        {move || match (ctx.creds.get().is_some(), ctx.load.get()) {
            (false, _) => view! { <Setup /> }.into_any(),
            (true, Load::Ready) => view! { <Main /> }.into_any(),
            (true, Load::Loading(done, total)) => view! { <Loading done total /> }.into_any(),
            (true, Load::Failed(error)) => view! { <Failed error /> }.into_any(),
        }}
        {move || ctx.toast.get().map(|text| view! { <div class="toast" role="status">{text}</div> })}
    }
}

#[component]
fn Setup() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let repo = RwSignal::new(remembered(REPO).unwrap_or_default());
    let token = RwSignal::new(String::new());
    let problem = RwSignal::new(None::<&'static str>);
    let connect = move |ev: ev::SubmitEvent| {
        ev.prevent_default();
        let (r, t) = (repo.get_untracked().trim().to_owned(), token.get_untracked().trim().to_owned());
        if !r.contains('/') {
            problem.set(Some("The repository looks like owner/name."));
            return;
        }
        if t.is_empty() {
            problem.set(Some("Paste a token to read the repository with."));
            return;
        }
        remember(REPO, &r);
        remember(TOKEN, &t);
        ctx.creds.set(Some(Creds { repo: r, token: t }));
        ctx.read();
    };
    view! {
        <main class="setup">
            <span class="spool" inner_html=SPOOL></span>
            <h1 class="title">"Needle and Thread"</h1>
            <p class="lead">"Read your vault on this phone, straight from its GitHub repository."</p>
            <form on:submit=connect>
                <label class="sc" for="repo">"Vault repository"</label>
                <input
                    id="repo"
                    autocapitalize="off"
                    autocomplete="off"
                    spellcheck="false"
                    placeholder="you/your-vault"
                    prop:value=move || repo.get()
                    on:input=move |ev| repo.set(event_target_value(&ev))
                />
                <label class="sc" for="token">"Token"</label>
                <input
                    id="token"
                    type="password"
                    autocomplete="off"
                    placeholder="github_pat_…"
                    prop:value=move || token.get()
                    on:input=move |ev| token.set(event_target_value(&ev))
                />
                <p class="hint">
                    "A fine-grained token that can only read this one repository’s contents. If you lose the phone, revoke it on GitHub. "
                    <a href="https://github.com/settings/personal-access-tokens/new" target="_blank" rel="noopener">"Make one"</a>
                    " — Repository access: only this repository; Permissions: Contents, read-only."
                </p>
                {move || problem.get().map(|p| view! { <p class="problem">{p}</p> })}
                <button type="submit" class="primary">"Connect"</button>
            </form>
            <p class="hint centred">"It’s kept on this phone only, and sent nowhere but GitHub."</p>
        </main>
    }
}

#[component]
fn Loading(done: usize, total: usize) -> impl IntoView {
    view! {
        <main class="setup">
            <span class="spool" inner_html=SPOOL></span>
            <p class="lead">"Reading your vault from GitHub…"</p>
            <p class="hint">{if total == 0 { "Listing its files".to_owned() } else { format!("{done} of {total} files") }}</p>
            <div class="bar"><i style=move || format!("width: {}%", (done * 100).checked_div(total).unwrap_or(0))></i></div>
        </main>
    }
}

#[component]
fn Failed(error: String) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    view! {
        <main class="setup">
            <span class="spool" inner_html=SPOOL></span>
            <p class="lead">"Couldn’t read the vault"</p>
            <p class="problem">{error}</p>
            <button type="button" class="primary" on:click=move |_| ctx.read()>"Try again"</button>
            <button
                type="button"
                class="secondary"
                on:click=move |_| {
                    forget(TOKEN);
                    ctx.creds.set(None);
                }
            >
                "Change the repository or token"
            </button>
        </main>
    }
}

#[component]
fn Main() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    view! {
        <div class="screen">
            {move || match ctx.screen.get() {
                Screen::Outline => view! { <TabTop /> <OutlineList /> }.into_any(),
                Screen::Notes => view! { <TabTop /> <NotesList /> }.into_any(),
                Screen::Search => view! { <TabTop /> <SearchPage /> }.into_any(),
                Screen::Scene(slug) => view! { <Reader slug /> }.into_any(),
                Screen::Note(owner, path) => view! { <NotePage owner path /> }.into_any(),
            }}
            <Show when=move || ctx.screen.with(Screen::is_tab)>
                <Tabs />
            </Show>
        </div>
        {move || ctx.sheet.get().map(|(owner, path)| view! { <NoteSheet owner path /> })}
        <Show when=move || ctx.picker.get()>
            <ProjectPicker />
        </Show>
        <Show when=move || ctx.settings.get()>
            <SettingsSheet />
        </Show>
    }
}

#[component]
fn TabTop() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let title = move || ctx.with(|_, p| p.config.title.clone()).unwrap_or_else(|| "No projects yet".to_owned());
    let several = move || ctx.vault.with(|v| v.as_ref().and_then(|v| v.projects().ok()).is_some_and(|p| p.len() > 1));
    view! {
        <header class="topbar">
            <button type="button" class="project" on:click=move |_| ctx.picker.set(true)>
                <span class="title">{title}</span>
                <Show when=several>
                    <span class="chevron" inner_html=CHEVRON_DOWN></span>
                </Show>
            </button>
            <button type="button" class="icon" aria-label="Settings" on:click=move |_| ctx.settings.set(true) inner_html=GEAR></button>
        </header>
    }
}

#[component]
fn Tabs() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let tab = move |screen: Screen, name: &'static str, icon: &'static str| {
        let (is, current) = (screen.clone(), screen.clone());
        view! {
            <button
                type="button"
                class="tab"
                class:on=move || ctx.screen.get() == is
                aria-current=move || (ctx.screen.get() == current).then_some("page")
                on:click={
                    let to = screen.clone();
                    move |_| ctx.tab(to.clone())
                }
            >
                <span inner_html=icon></span>
                <span>{name}</span>
            </button>
        }
    };
    view! {
        <nav class="tabs" aria-label="Sections">
            {tab(Screen::Outline, "Outline", LIST)}
            {tab(Screen::Notes, "Notes", NOTE)}
            {tab(Screen::Search, "Search", SEARCH)}
        </nav>
    }
}

/// "From GitHub · 3 minutes ago", with a button to read it again.
#[component]
fn Freshness() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    // Ticks once a minute so "minutes ago" keeps up.
    let now = RwSignal::new(js_sys::Date::now());
    let handle = set_interval_with_handle(move || now.set(js_sys::Date::now()), std::time::Duration::from_secs(60)).ok();
    on_cleanup(move || {
        if let Some(h) = handle {
            h.clear();
        }
    });
    let ago = move || {
        let minutes = ((now.get() - ctx.read_at.get()) / 60_000.0).max(0.0) as u64;
        match minutes {
            0 => "just now".to_owned(),
            1 => "a minute ago".to_owned(),
            m if m < 60 => format!("{m} minutes ago"),
            m if m < 120 => "an hour ago".to_owned(),
            m => format!("{} hours ago", m / 60),
        }
    };
    view! {
        <button type="button" class="fresh" on:click=move |_| ctx.read() disabled=move || ctx.refreshing.get()>
            <span class:spin=move || ctx.refreshing.get() inner_html=REFRESH></span>
            {move || if ctx.refreshing.get() { "Reading from GitHub…".to_owned() } else { format!("From GitHub · {}", ago()) }}
        </button>
    }
}

#[component]
fn OutlineList() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let rows = move || ctx.with(model::outline).unwrap_or(Ok(Vec::new()));
    view! {
        <main class="scroll">
            <Freshness />
            {move || match rows() {
                Err(e) => view! { <p class="problem padded">{e}</p> }.into_any(),
                Ok(rows) if rows.is_empty() => view! { <p class="hint padded">"No scenes in this project yet."</p> }.into_any(),
                Ok(rows) => rows.into_iter().map(|row| match row {
                    Row::Part { title, words } => view! {
                        <div class="part sc"><span>{title}</span><span class="words">{count(words)}</span></div>
                    }.into_any(),
                    Row::Chapter { number, title, words } => view! {
                        <div class="chapter"><span class="n">{number}</span><span class="t title">{title}</span><span class="words">{count(words)}</span></div>
                    }.into_any(),
                    Row::Unplaced { words } => view! {
                        <div class="part sc"><span>"Not in a chapter"</span><span class="words">{count(words)}</span></div>
                    }.into_any(),
                    Row::Scene(scene) => {
                        let slug = scene.slug.clone();
                        view! {
                            <button type="button" class="row scene" on:click=move |_| ctx.go(Screen::Scene(slug.clone()))>
                                <span class="status" title=scene.status.clone() inner_html=status_mark(scene.progress)></span>
                                <span class="t">{scene.title}</span>
                                <span class="words">{count(scene.words)}</span>
                            </button>
                        }.into_any()
                    }
                }).collect_view().into_any(),
            }}
        </main>
    }
}

#[component]
fn Reader(slug: String) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let reading = {
        let slug = slug.clone();
        move || ctx.with(|v, p| model::reading(v, p, &slug))
    };
    let size = move || format!("font-size: {}px", SIZES[ctx.size.get()]);
    let step_size = move |_| {
        let next = (ctx.size.get_untracked() + 1) % SIZES.len();
        ctx.size.set(next);
        remember(SIZE, &next.to_string());
    };
    view! {
        <header class="topbar">
            <button type="button" class="back" on:click=move |_| ctx.back()>
                <span inner_html=CHEVRON_LEFT></span>
                "Outline"
            </button>
            <span class="grow"></span>
            <button type="button" class="icon" aria-label="Text size" on:click=step_size>
                <span class="title aa">"Aa"</span>
            </button>
        </header>
        {move || match reading() {
            None => view! { <main class="scroll"><p class="hint padded">"This scene isn’t in the vault any more."</p></main> }.into_any(),
            Some(Err(e)) => view! { <main class="scroll"><p class="problem padded">{e}</p></main> }.into_any(),
            Some(Ok(r)) => {
                let piece = match &r.chapter {
                    Some(chapter) => format!("Piece {} · {chapter}", r.piece),
                    None => format!("Piece {}", r.piece),
                };
                let position = format!("{} of {}", r.piece, r.of);
                view! {
                    <main class="scroll page">
                        <div class="sc label">{piece}</div>
                        <h1 class="title heading">{r.scene.title.clone()}</h1>
                        <div class="meta">
                            <span class="pill sc">{r.scene.status.clone()}</span>
                            <span class="words">{format!("{} words", count(r.scene.words))}</span>
                        </div>
                        {(!r.summary.is_empty()).then(|| view! { <p class="summary">{r.summary.clone()}</p> })}
                        <div class="prose" style=size on:click=move |ev: ev::MouseEvent| follow_link(ctx, ev) inner_html=r.html.clone()></div>
                    </main>
                    <nav class="steps" aria-label="Scenes">
                        {neighbour(ctx, r.previous.clone(), true)}
                        <span class="words">{position}</span>
                        {neighbour(ctx, r.next.clone(), false)}
                    </nav>
                }.into_any()
            }
        }}
    }
}

/// The previous or next scene's button, or a gap the same size.
fn neighbour(ctx: Ctx, scene: Option<(String, String)>, before: bool) -> impl IntoView {
    match scene {
        // Stepping along replaces the scene rather than stacking it, so Back still goes to
        // wherever the reading started.
        Some((slug, title)) => view! {
            <button type="button" class="back step" class:next=!before on:click=move |_| ctx.screen.set(Screen::Scene(slug.clone()))>
                {before.then(|| view! { <span inner_html=CHEVRON_LEFT></span> })}
                <span class="step-title">{title}</span>
                {(!before).then(|| view! { <span inner_html=CHEVRON_RIGHT></span> })}
            </button>
        }
        .into_any(),
        None => view! { <span class="step"></span> }.into_any(),
    }
}

/// A tap on a name linked in the text opens its note.
fn follow_link(ctx: Ctx, ev: ev::MouseEvent) {
    let Some(target) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
    let Ok(Some(link)) = target.closest(".wl") else { return };
    if let Some(name) = link.get_attribute("data-target") {
        ctx.follow(&name);
    }
}

#[component]
fn NoteSheet(owner: Owner, path: String) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let card = {
        let (owner, path) = (owner.clone(), path.clone());
        move || ctx.with(|v, p| model::note(v, p, &owner, &path))
    };
    view! {
        <div class="scrim" on:click=move |_| ctx.sheet.set(None)></div>
        <section class="sheet" role="dialog" aria-modal="true">
            <button type="button" class="grip" aria-label="Close" on:click=move |_| ctx.sheet.set(None)></button>
            {move || match card() {
                Some(Ok(card)) => {
                    let (owner, path) = (card.owner.clone(), card.path.clone());
                    view! {
                        <div class="sc kind">{card.kind.clone()}</div>
                        <h2 class="title heading">{card.title.clone()}</h2>
                        {(!card.aliases.is_empty()).then(|| view! { <div class="hint">{format!("Also called {}", card.aliases.join(", "))}</div> })}
                        {(!card.gist.is_empty()).then(|| view! { <p class="gist">{card.gist.clone()}</p> })}
                        {scene_list(ctx, card.scenes.clone())}
                        <button type="button" class="secondary" on:click=move |_| ctx.go(Screen::Note(owner.clone(), path.clone()))>
                            "Open the whole note"
                        </button>
                    }.into_any()
                }
                Some(Err(e)) => view! { <p class="problem">{e}</p> }.into_any(),
                None => view! { <p class="hint">"This note isn’t in the vault any more."</p> }.into_any(),
            }}
        </section>
    }
}

/// "In these scenes", for a note.
fn scene_list(ctx: Ctx, scenes: Vec<(model::Scene, String)>) -> impl IntoView {
    (!scenes.is_empty()).then(|| {
        view! {
            <div class="sc label">"In these scenes"</div>
            {scenes.into_iter().map(|(scene, how)| {
                let slug = scene.slug.clone();
                view! {
                    <button type="button" class="row scene flush" on:click=move |_| ctx.go(Screen::Scene(slug.clone()))>
                        <span class="status" title=scene.status.clone() inner_html=status_mark(scene.progress)></span>
                        <span class="t">{scene.title}</span>
                        <span class="words">{how}</span>
                    </button>
                }
            }).collect_view()}
        }
    })
}

#[component]
fn NotePage(owner: Owner, path: String) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let card = move || ctx.with(|v, p| model::note(v, p, &owner, &path));
    let size = move || format!("font-size: {}px", SIZES[ctx.size.get()]);
    view! {
        <header class="topbar">
            <button type="button" class="back" on:click=move |_| ctx.back()>
                <span inner_html=CHEVRON_LEFT></span>
                "Back"
            </button>
        </header>
        <main class="scroll page">
            {move || match card() {
                Some(Ok(card)) => view! {
                    <div class="sc kind">{card.kind.clone()}</div>
                    <h1 class="title heading">{card.title.clone()}</h1>
                    {(!card.aliases.is_empty()).then(|| view! { <div class="hint">{format!("Also called {}", card.aliases.join(", "))}</div> })}
                    <div class="prose" style=size on:click=move |ev: ev::MouseEvent| follow_link(ctx, ev) inner_html=card.html.clone()></div>
                    {scene_list(ctx, card.scenes.clone())}
                }.into_any(),
                Some(Err(e)) => view! { <p class="problem">{e}</p> }.into_any(),
                None => view! { <p class="hint">"This note isn’t in the vault any more."</p> }.into_any(),
            }}
        </main>
    }
}

#[component]
fn NotesList() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let groups = move || ctx.with(model::notes).unwrap_or_default();
    view! {
        <main class="scroll">
            {move || {
                let groups = groups();
                if groups.is_empty() {
                    return view! { <p class="hint padded">"No notes in this project yet."</p> }.into_any();
                }
                groups.into_iter().map(|(kind, notes)| view! {
                    <div class="part sc"><span>{kind}</span><span class="words">{notes.len()}</span></div>
                    {notes.into_iter().map(|n| {
                        let (owner, path) = (n.owner.clone(), n.path.clone());
                        view! {
                            <button type="button" class="row" on:click=move |_| ctx.go(Screen::Note(owner.clone(), path.clone()))>
                                <span class="t">{n.title}</span>
                                {n.from.map(|w| view! { <span class="words">{w}</span> })}
                            </button>
                        }
                    }).collect_view()}
                }).collect_view().into_any()
            }}
        </main>
    }
}

#[component]
fn SearchPage() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    // Built once per vault and project, not per keystroke.
    let docs = Memo::new(move |_| ctx.with(model::documents).unwrap_or_default().into_iter().map(|d| (d.item, d.title, d.text)).collect::<Vec<_>>());
    let hits = move || {
        let query = ctx.query.get();
        docs.with(|docs| {
            let docs: Vec<search::Doc<Found>> = docs.iter().map(|(item, title, text)| search::Doc { item: item.clone(), title: title.clone(), text: text.clone() }).collect();
            search::search(&docs, &query)
        })
    };
    let input = NodeRef::<leptos::html::Input>::new();
    Effect::new(move |_| {
        if let Some(el) = input.get() {
            let _ = el.focus();
        }
    });
    view! {
        <main class="scroll">
            <div class="searchbox">
                <span inner_html=SEARCH></span>
                <input
                    node_ref=input
                    type="search"
                    placeholder="Search scenes and notes"
                    aria-label="Search scenes and notes"
                    prop:value=move || ctx.query.get()
                    on:input=move |ev| ctx.query.set(event_target_value(&ev))
                />
            </div>
            {move || {
                let hits = hits();
                if ctx.query.with(|q| q.trim().is_empty()) {
                    return view! { <p class="hint padded">"Every word has to be there; titles come first."</p> }.into_any();
                }
                if hits.is_empty() {
                    return view! { <p class="hint padded">"Nothing found."</p> }.into_any();
                }
                hits.into_iter().map(|hit| {
                    let item = hit.item.clone();
                    let kind = match &hit.item { Found::Scene(_) => "Scene", Found::Note(..) => "Note" };
                    view! {
                        <button type="button" class="row hit" on:click=move |_| match item.clone() {
                            Found::Scene(slug) => ctx.go(Screen::Scene(slug)),
                            Found::Note(owner, path) => ctx.go(Screen::Note(owner, path)),
                        }>
                            <span class="hit-head"><span class="t">{hit.title}</span><span class="words">{kind}</span></span>
                            {(!hit.matched.is_empty()).then(|| view! {
                                <span class="snippet">{hit.before}<mark>{hit.matched}</mark>{hit.after}</span>
                            })}
                        </button>
                    }
                }).collect_view().into_any()
            }}
        </main>
    }
}

#[component]
fn ProjectPicker() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let projects = move || ctx.vault.with(|v| v.as_ref().and_then(|v| v.projects().ok()).unwrap_or_default());
    view! {
        <div class="scrim" on:click=move |_| ctx.picker.set(false)></div>
        <section class="sheet" role="dialog" aria-modal="true" aria-label="Projects">
            <button type="button" class="grip" aria-label="Close" on:click=move |_| ctx.picker.set(false)></button>
            <div class="sc label">"Projects"</div>
            {move || projects().into_iter().map(|p| {
                let slug = p.slug.clone();
                let on = ctx.project.get().as_deref() == Some(p.slug.as_str());
                view! {
                    <button type="button" class="row" class:current=on on:click=move |_| {
                        remember(PROJECT, &slug);
                        ctx.project.set(Some(slug.clone()));
                        ctx.picker.set(false);
                        ctx.tab(Screen::Outline);
                    }>
                        <span class="t title">{p.config.title.clone()}</span>
                    </button>
                }
            }).collect_view()}
        </section>
    }
}

#[component]
fn SettingsSheet() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let repo = move || ctx.creds.get().map(|c| c.repo).unwrap_or_default();
    let disconnect = move |_| {
        forget(TOKEN);
        forget(PROJECT);
        ctx.settings.set(false);
        ctx.vault.set(None);
        ctx.creds.set(None);
        ctx.tab(Screen::Outline);
        spawn_local(github::clear_cache());
    };
    view! {
        <div class="scrim" on:click=move |_| ctx.settings.set(false)></div>
        <section class="sheet" role="dialog" aria-modal="true" aria-label="Settings">
            <button type="button" class="grip" aria-label="Close" on:click=move |_| ctx.settings.set(false)></button>
            <div class="sc label">"Reading from"</div>
            <p class="repo">{repo}</p>
            <p class="hint">"Read only: nothing on this phone changes the vault."</p>
            <button
                type="button"
                class="secondary"
                on:click=move |_| {
                    ctx.settings.set(false);
                    ctx.read();
                }
            >
                "Read it again now"
            </button>
            <button type="button" class="secondary danger" on:click=disconnect>"Disconnect this phone"</button>
            <p class="hint">"Forgets the token and everything read from the vault, on this phone only."</p>
        </section>
    }
}

// --- Small pieces ----------------------------------------------------------------------------

/// "1,412".
fn count(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A scene's status as a ring filling up, like the desktop's.
fn status_mark(progress: Option<f64>) -> String {
    let ring = |extra: &str| format!(r#"<circle cx="8" cy="8" r="5.5" fill="none" stroke="currentColor" stroke-width="1.3"{extra}/>"#);
    let inner = match progress {
        None => ring(r#" stroke-dasharray="2 1.6""#),
        Some(p) if p <= 0.0 => ring(""),
        Some(p) if p >= 1.0 => format!(r#"{}<circle cx="8" cy="8" r="5.5" fill="var(--done)"/>"#, ring("")),
        Some(p) => {
            let angle = p * std::f64::consts::TAU;
            let (x, y) = (8.0 + 5.5 * angle.sin(), 8.0 - 5.5 * angle.cos());
            let large = if p > 0.5 { 1 } else { 0 };
            format!(r#"{}<path d="M8 8V2.5A5.5 5.5 0 {large} 1 {x:.2} {y:.2}z" fill="var(--draft)"/>"#, ring(""))
        }
    };
    format!(r#"<svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">{inner}</svg>"#)
}

const LIST: &str = r#"<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" aria-hidden="true"><path d="M8 6h12M8 12h12M8 18h12"/><circle cx="4" cy="6" r="1"/><circle cx="4" cy="12" r="1"/><circle cx="4" cy="18" r="1"/></svg>"#;
const NOTE: &str = r#"<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 3h9l4 4v14H6z"/><path d="M14 3v5h5M9 13h7M9 17h5"/></svg>"#;
const SEARCH: &str = r#"<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="6.5"/><path d="M16 16l4.5 4.5"/></svg>"#;
const GEAR: &str = r#"<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" aria-hidden="true"><path d="M21 4h-7"/><path d="M10 4H3"/><path d="M21 12h-9"/><path d="M8 12H3"/><path d="M21 20h-5"/><path d="M12 20H3"/><path d="M14 2v4"/><path d="M8 10v4"/><path d="M16 18v4"/></svg>"#;
const CHEVRON_LEFT: &str = r#"<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 5l-7 7 7 7"/></svg>"#;
const CHEVRON_RIGHT: &str = r#"<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 5l7 7-7 7"/></svg>"#;
const CHEVRON_DOWN: &str = r#"<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6"/></svg>"#;
const REFRESH: &str = r#"<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7"/><path d="M20 4v7h-7"/></svg>"#;
const SPOOL: &str = r#"<svg width="44" height="44" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4.5" y="2.5" width="15" height="3" rx="1.5"/><rect x="4.5" y="18.5" width="15" height="3" rx="1.5"/><path d="M7 5.5v13"/><path d="M17 5.5v13"/><path d="m7 9.5 10-2"/><path d="m7 13 10-2"/><path d="m7 16.5 10-2"/></svg>"#;

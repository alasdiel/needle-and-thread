//! The reader page (DESIGN §10; Reader 1, 1b and 1c on the canvas's Sharing page): the shared
//! text as a plain page, one scene at a time; commented words marked, their thread opening in a
//! sheet when tapped; select words to comment on them. Opened with the author key, the sheets
//! also offer replying as author, resolving and deleting, and the link can be revoked.

use std::ops::Range;
use std::time::Duration;

use leptos::{ev, prelude::*, task::spawn_local};
use needle_core::anchor::{Anchor, prose};
use needle_core::share::{Comment, FORMAT, Link, MAX_COMMENT_CHARS, On, Shared};
use wasm_bindgen::{JsCast, prelude::Closure};

use crate::model::{self, Opened, Thread};
use crate::render;
use crate::worker;

const NAME: &str = "needle-reader-name";
const SIZE: &str = "needle-reader-size";
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

/// Words picked to comment on.
#[derive(Clone, Debug, PartialEq)]
struct Pick {
    scene: String,
    range: Range<usize>,
    quote: String,
    /// Where to show the Comment button, in px from the window's top left.
    top: f64,
    left: f64,
}

#[derive(Clone, Debug, PartialEq)]
enum Sheet {
    Thread(u64),
    New(Pick),
    All,
    Contents,
}

#[derive(Clone, Copy)]
struct Ctx {
    link: StoredValue<Link>,
    author: StoredValue<Option<String>>,
    shared: RwSignal<Option<Result<Shared, String>>>,
    comments: RwSignal<Vec<Opened>>,
    /// Which part (scene) is open.
    part: RwSignal<usize>,
    sheet: RwSignal<Option<Sheet>>,
    pick: RwSignal<Option<Pick>>,
    size: RwSignal<usize>,
    toast: RwSignal<Option<String>>,
    busy: RwSignal<bool>,
}

impl Ctx {
    fn say(&self, text: impl Into<String>) {
        self.toast.set(Some(text.into()));
        let toast = self.toast;
        set_timeout(
            move || {
                toast.try_set(None);
            },
            Duration::from_secs(4),
        );
    }

    fn load_text(&self) {
        let ctx = *self;
        spawn_local(async move {
            let link = ctx.link.get_value();
            let got = worker::text(&link).await;
            // A failed refresh keeps what's on screen; only the first load shows the failure.
            if got.is_ok() || ctx.shared.with_untracked(|s| s.as_ref().is_none_or(|s| s.is_err())) {
                ctx.shared.set(Some(got));
            }
        });
    }

    fn load_comments(&self) {
        let ctx = *self;
        spawn_local(async move {
            if let Ok(comments) = worker::comments(&ctx.link.get_value()).await {
                ctx.comments.set(comments);
            }
        });
    }

    fn threads(&self) -> Vec<Thread> {
        let comments = self.comments.get();
        self.shared.with(|s| match s {
            Some(Ok(shared)) => model::threads(shared, &comments),
            _ => Vec::new(),
        })
    }

    /// Opens a thread, going to its scene first.
    fn open_thread(&self, thread: &Thread) {
        if let Some(on) = &thread.at
            && let Some(Ok(shared)) = self.shared.get_untracked()
            && let Some(i) = shared.parts.iter().position(|p| p.scene == on.scene)
        {
            self.part.set(i);
        }
        self.sheet.set(Some(Sheet::Thread(thread.head.id)));
    }

    /// Runs a call to the Worker, then reloads the comments, or says what went wrong.
    fn act(&self, call: impl std::future::Future<Output = Result<(), String>> + 'static, done: &'static str) {
        let ctx = *self;
        ctx.busy.set(true);
        spawn_local(async move {
            let result = call.await;
            ctx.busy.set(false);
            match result {
                Ok(()) => {
                    if !done.is_empty() {
                        ctx.say(done);
                    }
                    ctx.load_comments();
                }
                Err(e) => ctx.say(e),
            }
        });
    }
}

#[component]
pub fn App() -> impl IntoView {
    let hash = window().location().hash().unwrap_or_default();
    // A new link pasted into the same tab reads afresh.
    let reload = Closure::<dyn Fn()>::new(|| {
        let _ = window().location().reload();
    });
    window().set_onhashchange(Some(reload.as_ref().unchecked_ref()));
    reload.forget();

    let Some((link, author)) = Link::parse_author(&hash) else {
        return view! {
            <main class="setup">
                <span class="spool" inner_html=SPOOL></span>
                <h1 class="title">"Nothing to read here"</h1>
                <p class="lead">"This page opens text shared from Needle and Thread. The link you were sent ends in a long code after #; check it was copied whole."</p>
            </main>
        }
        .into_any();
    };
    let size = remembered(SIZE).and_then(|s| s.parse().ok()).filter(|&s: &usize| s < SIZES.len()).unwrap_or(1);
    let ctx = Ctx {
        link: StoredValue::new(link),
        author: StoredValue::new(author),
        shared: RwSignal::new(None),
        comments: RwSignal::new(Vec::new()),
        part: RwSignal::new(0),
        sheet: RwSignal::new(None),
        pick: RwSignal::new(None),
        size: RwSignal::new(size),
        toast: RwSignal::new(None),
        busy: RwSignal::new(false),
    };
    provide_context(ctx);
    ctx.load_text();
    ctx.load_comments();
    let _ = set_interval_with_handle(move || ctx.load_comments(), Duration::from_secs(60));
    let _ = set_interval_with_handle(move || ctx.load_text(), Duration::from_secs(300));
    watch_selection(ctx);
    let _ = window_event_listener(ev::keydown, move |ev| {
        if ev.key() == "Escape" {
            ctx.sheet.set(None);
        }
    });

    view! {
        <div class="screen">
            {move || match ctx.shared.get() {
                None => view! {
                    <main class="setup">
                        <span class="spool spin" inner_html=SPOOL></span>
                        <p class="lead">"Opening…"</p>
                    </main>
                }.into_any(),
                Some(Err(e)) => view! {
                    <main class="setup">
                        <span class="spool" inner_html=SPOOL></span>
                        <h1 class="title">"Can’t open this"</h1>
                        <p class="lead">{e}</p>
                        <button type="button" class="primary" on:click=move |_| ctx.load_text()>"Try again"</button>
                    </main>
                }.into_any(),
                Some(Ok(_)) => view! { <Page /> }.into_any(),
            }}
        </div>
        {move || ctx.pick.get().filter(|_| ctx.sheet.get().is_none()).map(|pick| {
            let style = format!("top: {}px; left: {}px", pick.top, pick.left);
            view! {
                <button type="button" class="pick" style=style
                    // Keeps the selection when clicked with a mouse; a tap may still clear it,
                    // which watch_selection allows for.
                    on:mousedown=move |ev: ev::MouseEvent| ev.prevent_default()
                    on:click=move |_| {
                        clear_selection();
                        ctx.sheet.set(Some(Sheet::New(pick.clone())));
                    }>
                    <span inner_html=TALK></span>"Comment"
                </button>
            }
        })}
        {move || ctx.sheet.get().map(|sheet| view! { <SheetView sheet /> })}
        {move || ctx.toast.get().map(|t| view! { <div class="toast" role="status">{t}</div> })}
    }
    .into_any()
}

#[component]
fn Page() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let shared = move || ctx.shared.get().and_then(Result::ok);
    let part = move || shared().and_then(|s| s.parts.get(ctx.part.get().min(s.parts.len().saturating_sub(1))).cloned());
    let open_count = move || ctx.threads().iter().filter(|t| !t.head.resolved).count();
    let html = move || {
        let Some(part) = part() else { return String::new() };
        let threads = ctx.threads();
        render::html(&part.markdown, &model::marks(&part.markdown, &part.scene, &threads))
    };
    let size = move || format!("font-size: {}px", SIZES[ctx.size.get()]);
    let step_size = move |_| {
        let next = (ctx.size.get_untracked() + 1) % SIZES.len();
        ctx.size.set(next);
        remember(SIZE, &next.to_string());
    };
    let open_mark = move |ev: ev::MouseEvent| {
        let Some(target) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        // Not while picking words: a tap that ends a selection isn't a tap on a mark.
        if ctx.pick.get_untracked().is_some() {
            return;
        }
        if let Ok(Some(mark)) = target.closest("mark.cm")
            && let Some(id) = mark.get_attribute("data-c").and_then(|c| c.parse().ok())
        {
            ctx.sheet.set(Some(Sheet::Thread(id)));
        }
    };
    let count = move || shared().map_or(0, |s| s.parts.len());
    let step = move |by: isize| {
        let next = ctx.part.get_untracked() as isize + by;
        if next >= 0 && (next as usize) < count() {
            ctx.part.set(next as usize);
            ctx.pick.set(None);
            window().scroll_to_with_x_and_y(0.0, 0.0);
            if let Some(main) = document().query_selector("main.page").ok().flatten() {
                main.set_scroll_top(0);
            }
        }
    };
    let neighbour = move |by: isize| {
        let i = ctx.part.get() as isize + by;
        shared().and_then(|s| (i >= 0).then(|| s.parts.get(i as usize).map(|p| p.title.clone())).flatten())
    };
    view! {
        <header class="topbar">
            <button type="button" class="project" on:click=move |_| ctx.sheet.set(Some(Sheet::Contents))>
                <span class="grow">
                    <span class="sc label">{move || shared().map(|s| if s.title == s.project { s.project } else { format!("{} · {}", s.project, s.title) })}</span>
                    <span class="title shared-title">"Shared with you"</span>
                </span>
            </button>
            {ctx.author.get_value().is_some().then(|| view! { <span class="pill sc author-pill">"author"</span> })}
            <button type="button" class="icon count" aria-label=move || format!("Comments, {}", open_count()) on:click=move |_| ctx.sheet.set(Some(Sheet::All))>
                <span inner_html=TALK></span>
                <span>{open_count}</span>
            </button>
            <button type="button" class="icon" aria-label="Text size" on:click=step_size>
                <span class="title aa">"Aa"</span>
            </button>
        </header>
        <main class="scroll page">
            {move || part().map(|p| view! {
                {p.chapter.clone().map(|c| view! { <div class="sc label">{c}</div> })}
                <h1 class="title heading">{p.title.clone()}</h1>
            })}
            <div class="prose" style=size on:click=open_mark inner_html=html></div>
            {move || (count() == 1 || ctx.part.get() + 1 == count()).then(|| view! { <p class="hint end">"Select any words to comment on them."</p> })}
        </main>
        {move || (count() > 1).then(|| view! {
            <nav class="steps" aria-label="Scenes">
                {move || match neighbour(-1) {
                    Some(title) => view! { <button type="button" class="back step" on:click=move |_| step(-1)><span inner_html=CHEVRON_LEFT></span><span class="step-title">{title}</span></button> }.into_any(),
                    None => view! { <span class="step"></span> }.into_any(),
                }}
                <span class="words">{move || format!("{} of {}", ctx.part.get() + 1, count())}</span>
                {move || match neighbour(1) {
                    Some(title) => view! { <button type="button" class="back step next" on:click=move |_| step(1)><span class="step-title">{title}</span><span inner_html=CHEVRON_RIGHT></span></button> }.into_any(),
                    None => view! { <span class="step"></span> }.into_any(),
                }}
            </nav>
        })}
    }
}

// --- Picking words ---------------------------------------------------------------------------

/// Follows the selection: words picked in the text offer the Comment button.
fn watch_selection(ctx: Ctx) {
    let changed = Closure::<dyn Fn()>::new(move || {
        if ctx.sheet.get_untracked().is_some() {
            return;
        }
        match picked(ctx) {
            Some(pick) => ctx.pick.set(Some(pick)),
            // Tapping the Comment button clears the selection in some browsers before the tap
            // lands, so the button stays a moment longer.
            None => set_timeout(
                move || {
                    if ctx.sheet.get_untracked().is_none() && picked(ctx).is_none() {
                        ctx.pick.set(None);
                    }
                },
                Duration::from_millis(400),
            ),
        }
    });
    let _ = document().add_event_listener_with_callback("selectionchange", changed.as_ref().unchecked_ref());
    changed.forget();
}

fn picked(ctx: Ctx) -> Option<Pick> {
    let selection = window().get_selection().ok()??;
    if selection.is_collapsed() || selection.range_count() == 0 {
        return None;
    }
    let range = selection.get_range_at(0).ok()?;
    let prose_el = document().query_selector(".prose").ok()??;
    let inside = |node: &web_sys::Node| prose_el.contains(Some(node));
    if !inside(&range.start_container().ok()?) || !inside(&range.end_container().ok()?) {
        return None;
    }
    let start = offset(&range.start_container().ok()?, range.start_offset().ok()?, true)?;
    let end = offset(&range.end_container().ok()?, range.end_offset().ok()?, false)?;
    let part = ctx.shared.with_untracked(|s| s.as_ref()?.as_ref().ok()?.parts.get(ctx.part.get_untracked()).cloned())?;
    let anchor = Anchor::at(&prose(&part.markdown), start.min(end)..start.max(end))?;
    let rect = range.get_bounding_client_rect();
    let width = window().inner_width().ok()?.as_f64().unwrap_or(390.0);
    let height = window().inner_height().ok()?.as_f64().unwrap_or(800.0);
    // Under the words (the phone's own menu takes the space above), kept on screen.
    let top = (rect.bottom() + 12.0).min(height - 140.0).max(70.0);
    let left = (rect.left() + rect.width() / 2.0 - 60.0).clamp(12.0, width - 140.0);
    Some(Pick { scene: part.scene, range: start.min(end)..start.max(end), quote: anchor.quote, top, left })
}

/// Where a point in the page's text is in the scene's prose, in characters. A point between
/// elements is moved into the text after it (`forward`) or before it.
fn offset(node: &web_sys::Node, at: u32, forward: bool) -> Option<usize> {
    if let Some(text) = node.dyn_ref::<web_sys::Text>() {
        let span = text.parent_element()?.closest("[data-o]").ok()??;
        let base: usize = span.get_attribute("data-o")?.parse().ok()?;
        let data = js_sys::JsString::from(text.data());
        let before: String = data.slice(0, at.min(data.length())).into();
        return Some(base + before.chars().count());
    }
    let children = node.child_nodes();
    let (next, edge) = if forward && at < children.length() {
        (children.get(at)?, true)
    } else if at > 0 {
        (children.get(at - 1)?, false)
    } else {
        (node.clone(), true)
    };
    let text = text_at(&next, edge)?;
    let length = text.length();
    offset(&text.into(), if edge { 0 } else { length }, forward)
}

/// The first (or last) text inside `node`.
fn text_at(node: &web_sys::Node, first: bool) -> Option<web_sys::Text> {
    if let Some(text) = node.dyn_ref::<web_sys::Text>() {
        return Some(text.clone());
    }
    let children = node.child_nodes();
    let order: Vec<u32> = if first { (0..children.length()).collect() } else { (0..children.length()).rev().collect() };
    order.into_iter().find_map(|i| text_at(&children.get(i)?, first))
}

fn clear_selection() {
    if let Ok(Some(selection)) = window().get_selection() {
        let _ = selection.remove_all_ranges();
    }
}

// --- Sheets ----------------------------------------------------------------------------------

#[component]
fn SheetView(sheet: Sheet) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let close = move |_| ctx.sheet.set(None);
    let label = match &sheet {
        Sheet::Thread(_) => "Comments on these words",
        Sheet::New(_) => "New comment",
        Sheet::All => "Comments",
        Sheet::Contents => "Contents",
    };
    let body = match sheet {
        Sheet::Thread(id) => view! { <ThreadSheet id /> }.into_any(),
        Sheet::New(pick) => view! { <NewSheet pick /> }.into_any(),
        Sheet::All => view! { <AllSheet /> }.into_any(),
        Sheet::Contents => view! { <ContentsSheet /> }.into_any(),
    };
    view! {
        <div class="scrim" on:click=close></div>
        <section class="sheet" role="dialog" aria-modal="true" aria-label=label>
            <button type="button" class="grip" aria-label="Close" on:click=close></button>
            {body}
        </section>
    }
}

fn when(seconds: f64) -> String {
    let date = js_sys::Date::new(&(seconds * 1000.0).into());
    let options = js_sys::Object::new();
    for (k, v) in [("month", "short"), ("day", "numeric"), ("hour", "2-digit"), ("minute", "2-digit")] {
        let _ = js_sys::Reflect::set(&options, &k.into(), &v.into());
    }
    date.to_locale_string("default", &options).into()
}

/// One comment in a thread, with Delete for the author.
fn comment_view(ctx: Ctx, c: Opened) -> impl IntoView {
    let author_key = ctx.author.get_value();
    let id = c.id;
    view! {
        <div class="said-by">
            <b>{c.who()}</b>
            {c.author.then(|| view! { <span class="pill sc small">"author"</span> })}
            <span class="words">{when(c.created)}</span>
            {author_key.map(|key| view! {
                <button type="button" class="link danger" on:click=move |_| {
                    let (link, key) = (ctx.link.get_value(), key.clone());
                    ctx.act(async move { worker::delete(&link, &key, id).await }, "Deleted.");
                }>"Delete"</button>
            })}
        </div>
        <p class="said">{c.comment.text.clone()}</p>
    }
}

#[component]
fn ThreadSheet(id: u64) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let thread = move || ctx.threads().into_iter().find(|t| t.head.id == id);
    let text = RwSignal::new(String::new());
    let name = RwSignal::new(remembered(NAME).unwrap_or_default());
    let is_author = ctx.author.get_value().is_some();
    let reply = move |_| {
        let words = text.get_untracked().trim().to_owned();
        if words.is_empty() {
            return;
        }
        let who = name.get_untracked().trim().to_owned();
        if !is_author && who.is_empty() {
            ctx.say("Add your name, so the author knows who said it.");
            return;
        }
        if !is_author {
            remember(NAME, &who);
        }
        let comment = Comment { v: FORMAT, name: if is_author { String::new() } else { who }, text: words, on: None, version: String::new(), reply_to: Some(id) };
        let (link, key) = (ctx.link.get_value(), ctx.author.get_value());
        text.set(String::new());
        ctx.act(async move { worker::post(&link, key.as_deref(), &comment).await }, "");
    };
    view! {
        {move || match thread() {
            None => view! { <p class="hint">"This comment was deleted."</p> }.into_any(),
            Some(t) => {
                let resolved = t.head.resolved;
                view! {
                    <p class="quote">"“"<span>{t.quote().to_owned()}</span>"”"
                        {t.at.is_none().then(|| view! { <span class="pill sc small">"text removed"</span> })}
                        {resolved.then(|| view! { <span class="pill sc small">"resolved"</span> })}
                    </p>
                    {comment_view(ctx, t.head.clone())}
                    <div class="replies">
                        {t.replies.into_iter().map(|r| comment_view(ctx, r)).collect_view()}
                    </div>
                    {ctx.author.get_value().map(|key| view! {
                        <div class="row-actions">
                            <button type="button" class="secondary" on:click=move |_| {
                                let (link, key) = (ctx.link.get_value(), key.clone());
                                ctx.act(async move { worker::resolve(&link, &key, id, !resolved).await }, if resolved { "Reopened." } else { "Resolved." });
                            }>{if resolved { "Reopen" } else { "Resolve" }}</button>
                        </div>
                    })}
                }.into_any()
            }
        }}
        {(!is_author && remembered(NAME).is_none()).then(|| view! {
            <label class="field-label sc" for="reply-name">"Your name"</label>
            <input id="reply-name" class="field" type="text" autocomplete="name" prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) />
        })}
        <label class="field-label sc" for="reply">{if is_author { "Reply as author" } else { "Reply" }}</label>
        <textarea id="reply" class="field" rows="3" maxlength=MAX_COMMENT_CHARS.to_string() prop:value=move || text.get() on:input=move |ev| text.set(event_target_value(&ev))></textarea>
        <button type="button" class="primary" disabled=move || ctx.busy.get() || text.get().trim().is_empty() on:click=reply>"Reply"</button>
    }
}

#[component]
fn NewSheet(pick: Pick) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let text = RwSignal::new(String::new());
    let known = remembered(NAME);
    let name = RwSignal::new(known.clone().unwrap_or_default());
    let asking = RwSignal::new(known.is_none() && ctx.author.get_value().is_none());
    let is_author = ctx.author.get_value().is_some();
    let quote = pick.quote.clone();
    let send = move |_| {
        let words = text.get_untracked().trim().to_owned();
        let who = name.get_untracked().trim().to_owned();
        if words.is_empty() {
            return;
        }
        if !is_author && who.is_empty() {
            ctx.say("Add your name, so the author knows who said it.");
            return;
        }
        if !is_author {
            remember(NAME, &who);
        }
        let Some(Ok(shared)) = ctx.shared.get_untracked() else { return };
        let Some(part) = shared.parts.iter().find(|p| p.scene == pick.scene) else { return };
        let Some(anchor) = Anchor::at(&prose(&part.markdown), pick.range.clone()) else { return };
        let comment = Comment {
            v: FORMAT,
            name: if is_author { String::new() } else { who },
            text: words,
            on: Some(On { scene: pick.scene.clone(), anchor }),
            version: shared.version.clone(),
            reply_to: None,
        };
        let (link, key) = (ctx.link.get_value(), ctx.author.get_value());
        ctx.sheet.set(None);
        ctx.pick.set(None);
        ctx.act(async move { worker::post(&link, key.as_deref(), &comment).await }, "Comment added.");
    };
    view! {
        <div class="sc label">"Commenting on"</div>
        <p class="quote">"“"<span>{quote}</span>"”"</p>
        {move || if asking.get() {
            view! {
                <label class="field-label sc" for="new-name">"Your name"</label>
                <input id="new-name" class="field" type="text" autocomplete="name" prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) />
                <p class="hint small">"Asked once; this device remembers it."</p>
            }.into_any()
        } else if is_author {
            view! { <p class="hint small">"As the author."</p> }.into_any()
        } else {
            view! {
                <p class="hint small">"As " {move || name.get()} " · "
                    <button type="button" class="link" on:click=move |_| asking.set(true)>"Change"</button>
                </p>
            }.into_any()
        }}
        <label class="field-label sc" for="new-text">"Comment"</label>
        <textarea id="new-text" class="field" rows="4" maxlength=MAX_COMMENT_CHARS.to_string() prop:value=move || text.get() on:input=move |ev| text.set(event_target_value(&ev))></textarea>
        <div class="row-actions">
            <button type="button" class="secondary" on:click=move |_| ctx.sheet.set(None)>"Cancel"</button>
            <button type="button" class="primary" disabled=move || ctx.busy.get() || text.get().trim().is_empty() on:click=send>"Comment"</button>
        </div>
    }
}

#[component]
fn AllSheet() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let title_of = move |scene: &str| {
        ctx.shared.with(|s| s.as_ref().and_then(|s| s.as_ref().ok()).and_then(|s| s.parts.iter().find(|p| p.scene == scene).map(|p| p.title.clone())))
    };
    let row = move |t: Thread| {
        let place = t.at.as_ref().and_then(|on| title_of(&on.scene)).unwrap_or_else(|| "text removed".to_owned());
        let first = t.head.comment.text.lines().next().unwrap_or_default().to_owned();
        let replies = t.replies.len();
        let thread = t.clone();
        view! {
            <button type="button" class="row thread" on:click=move |_| ctx.open_thread(&thread)>
                <span class="sc label">{place}</span>
                <span class="quote">"“"<span>{t.quote().to_owned()}</span>"”"</span>
                <span><b>{t.head.who()}</b>": " {first}</span>
                {(replies > 0).then(|| view! { <span class="words">{if replies == 1 { "1 reply".to_owned() } else { format!("{replies} replies") }}</span> })}
            </button>
        }
    };
    let revoke = move |_| {
        let Some(key) = ctx.author.get_value() else { return };
        if !window().confirm_with_message("Revoke this link? The text and every comment are deleted, and the link stops working. This can’t be undone.").unwrap_or(false) {
            return;
        }
        let link = ctx.link.get_value();
        spawn_local(async move {
            match worker::revoke(&link, &key).await {
                Ok(()) => ctx.shared.set(Some(Err("You revoked this link. The text and its comments are gone.".into()))),
                Err(e) => ctx.say(e),
            }
            ctx.sheet.set(None);
        });
    };
    view! {
        <h2 class="title sheet-title">"Comments"</h2>
        {move || {
            let (open, resolved): (Vec<Thread>, Vec<Thread>) = ctx.threads().into_iter().partition(|t| !t.head.resolved);
            view! {
                {open.is_empty().then(|| view! { <p class="hint">"No comments yet. Select any words to comment on them."</p> })}
                {open.into_iter().map(row).collect_view()}
                {(!resolved.is_empty()).then(|| view! { <div class="sc label group">"Resolved"</div> })}
                {resolved.into_iter().map(row).collect_view()}
            }
        }}
        {ctx.author.get_value().is_some().then(|| view! {
            <button type="button" class="secondary danger" on:click=revoke>"Revoke this link"</button>
        })}
    }
}

#[component]
fn ContentsSheet() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let parts = move || ctx.shared.with(|s| s.as_ref().and_then(|s| s.as_ref().ok()).map(|s| s.parts.clone()).unwrap_or_default());
    view! {
        <h2 class="title sheet-title">"Contents"</h2>
        {move || parts().into_iter().enumerate().map(|(i, p)| view! {
            {p.chapter.clone().map(|c| view! { <div class="sc label group">{c}</div> })}
            <button type="button" class="row" class:current=move || ctx.part.get() == i on:click=move |_| {
                ctx.part.set(i);
                ctx.sheet.set(None);
            }>{p.title.clone()}</button>
        }).collect_view()}
    }
}

const TALK: &str = r#"<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1.1-4.6A8 8 0 1 1 21 12z"/></svg>"#;
const CHEVRON_LEFT: &str = r#"<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 5l-7 7 7 7"/></svg>"#;
const CHEVRON_RIGHT: &str = r#"<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 5l7 7-7 7"/></svg>"#;
const SPOOL: &str = r#"<svg width="44" height="44" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4.5" y="2.5" width="15" height="3" rx="1.5"/><rect x="4.5" y="18.5" width="15" height="3" rx="1.5"/><path d="M7 5.5v13"/><path d="M17 5.5v13"/><path d="m7 9.5 10-2"/><path d="m7 13 10-2"/><path d="m7 16.5 10-2"/></svg>"#;

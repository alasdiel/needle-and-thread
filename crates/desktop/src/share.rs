//! Sharing in the window (DESIGN §10): the Shared panel (Share 2 on the canvas's Sharing page),
//! with every comment readers left and every link handed out; and a comment's card, which opens
//! beside its words when they're clicked (Comments 1, only the clicked one showing).

use leptos::prelude::*;
use wasm_bindgen::JsValue;

use crate::history::{clock_of, day_label};
use crate::icons::{Glyph, Icon};
use crate::tauri::{CommentView, Covers, ShareView};

/// Which half of the Shared panel shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SharedTab {
    Comments,
    Links,
}

/// What a new link could share, from what's open: the scene, its chapter, its part, the book.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub covers: Covers,
    pub label: String,
}

/// A comment and its replies, oldest first.
#[derive(Clone, Debug, PartialEq)]
pub struct Thread {
    pub head: CommentView,
    pub replies: Vec<CommentView>,
}

impl Thread {
    /// The newest comment in it, which marks it seen up to there.
    pub fn last_id(&self) -> u64 {
        self.replies.iter().map(|r| r.id).chain([self.head.id]).max().unwrap_or(self.head.id)
    }

    pub fn fresh(&self) -> bool {
        self.head.fresh || self.replies.iter().any(|r| r.fresh)
    }
}

/// Comments as threads: each that's on some words, with its replies.
pub fn threads(comments: &[CommentView]) -> Vec<Thread> {
    let mut threads: Vec<Thread> = comments
        .iter()
        .filter(|c| c.reply_to.is_none())
        .map(|head| {
            let mut replies: Vec<CommentView> = comments.iter().filter(|c| c.share == head.share && c.reply_to == Some(head.id)).cloned().collect();
            replies.sort_by(|a, b| a.created.total_cmp(&b.created));
            Thread { head: head.clone(), replies }
        })
        .collect();
    threads.sort_by(|a, b| a.head.created.total_cmp(&b.head.created));
    threads
}

/// The marks the editor draws for a scene: its open threads whose words are still there.
pub fn marks(comments: &[CommentView], scene: &str) -> JsValue {
    let marks = js_sys::Array::new();
    for c in comments.iter().filter(|c| c.reply_to.is_none() && !c.resolved && c.scene.as_deref() == Some(scene)) {
        let Some(anchor) = &c.anchor else { continue };
        let mark = js_sys::Object::new();
        crate::editor::set(&mark, "id", &(c.id as f64).into());
        crate::editor::set(&mark, "quote", &anchor.quote.as_str().into());
        crate::editor::set(&mark, "n", &(anchor.n as f64).into());
        marks.push(&mark);
    }
    marks.into()
}

/// Calls `f` now and again every 150 ms until it says it's done, `times` at most: for something
/// that needs a scene that's still opening.
pub fn retry(times: u32, f: impl Fn() -> bool + Copy + 'static) {
    if f() || times == 0 {
        return;
    }
    set_timeout(move || retry(times - 1, f), std::time::Duration::from_millis(150));
}

fn when(seconds: f64) -> String {
    let date = js_sys::Date::new(&JsValue::from_f64(seconds * 1000.0));
    format!("{} {}", day_label(&date), clock_of(&date))
}

fn who(c: &CommentView) -> String {
    if c.author || c.name.trim().is_empty() { "You".to_owned() } else { c.name.trim().to_owned() }
}

fn copy(text: &str) {
    if let Some(window) = web_sys::window() {
        let promise = window.navigator().clipboard().write_text(text);
        // A refused clipboard leaves the link showing to copy by hand.
        leptos::task::spawn_local(async move {
            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
        });
    }
}

fn confirm(question: &str) -> bool {
    web_sys::window().and_then(|w| w.confirm_with_message(question).ok()).unwrap_or(false)
}

#[component]
pub fn SharedPanel(
    tab: RwSignal<SharedTab>,
    #[prop(into)] shares: Signal<Vec<ShareView>>,
    #[prop(into)] comments: Signal<Vec<CommentView>>,
    #[prop(into)] choices: Signal<Vec<Choice>>,
    /// The open scene's file name.
    #[prop(into)]
    scene: Signal<Option<String>>,
    /// A scene's title now, if it's still in the project.
    scene_title: impl Fn(&str) -> Option<String> + Copy + Send + Sync + 'static,
    /// Couldn't load the comments, or the last thing asked failed.
    #[prop(into)]
    problem: Signal<Option<String>>,
    #[prop(into)] busy: Signal<bool>,
    /// The link just made, shown with its address to copy.
    #[prop(into)]
    made: Signal<Option<String>>,
    on_make: impl Fn(Covers, String) + Copy + Send + Sync + 'static,
    on_revoke: impl Fn(String) + Copy + Send + Sync + 'static,
    on_author: impl Fn(String) + Copy + Send + Sync + 'static,
    on_open: impl Fn(Thread) + Copy + Send + Sync + 'static,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let open_count = move || threads(&comments.get()).iter().filter(|t| !t.head.resolved).count();
    view! {
        <aside class="side-panel shared-panel" aria-label="Shared">
            <div class="panel-head">
                <h2>"Shared"</h2>
                <button class="icon-button" title="Close" aria-label="Close Shared" on:click=move |_| on_close()>
                    <Icon glyph=Glyph::Close />
                </button>
            </div>
            <div class="segmented two shared-tabs" role="tablist">
                <button role="tab" class:active=move || tab.get() == SharedTab::Comments aria-selected=move || (tab.get() == SharedTab::Comments).to_string() on:click=move |_| tab.set(SharedTab::Comments)>
                    {move || format!("Comments · {}", open_count())}
                </button>
                <button role="tab" class:active=move || tab.get() == SharedTab::Links aria-selected=move || (tab.get() == SharedTab::Links).to_string() on:click=move |_| tab.set(SharedTab::Links)>
                    {move || format!("Links · {}", shares.with(Vec::len))}
                </button>
            </div>
            {move || problem.get().map(|p| view! { <p class="shared-problem" role="alert">{p}</p> })}
            {move || match tab.get() {
                SharedTab::Comments => view! { <CommentList comments scene scene_title on_open /> }.into_any(),
                SharedTab::Links => view! { <LinkList shares choices busy made on_make on_revoke on_author /> }.into_any(),
            }}
        </aside>
    }
}

#[component]
fn CommentList(
    #[prop(into)] comments: Signal<Vec<CommentView>>,
    #[prop(into)] scene: Signal<Option<String>>,
    scene_title: impl Fn(&str) -> Option<String> + Copy + Send + Sync + 'static,
    on_open: impl Fn(Thread) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let row = move |t: Thread| {
        let first = t.head.text.lines().next().unwrap_or_default().to_owned();
        let replies = t.replies.len();
        let fresh = t.fresh();
        let removed = t.head.anchor.is_none();
        let head = t.head.clone();
        view! {
            <button class="shared-row" on:click=move |_| on_open(t.clone())>
                <span class="shared-quote">"“"<span>{head.quote.clone()}</span>"”"</span>
                <span class="shared-said">
                    {fresh.then(|| view! { <span class="note-dot" title="New"></span> })}
                    <b>{who(&head)}</b>
                    " · " {first}
                </span>
                <span class="shared-meta">
                    {head.share_name.clone()}
                    {(replies > 0).then(|| if replies == 1 { " · 1 reply".to_owned() } else { format!(" · {replies} replies") })}
                    {removed.then_some(" · text removed")}
                </span>
            </button>
        }
    };
    view! {
        {move || {
            let all = threads(&comments.get());
            if all.is_empty() {
                return view! { <p class="hint shared-empty">"No comments yet. Readers select words on the page you share to comment on them."</p> }.into_any();
            }
            let here = scene.get();
            let (resolved, open): (Vec<Thread>, Vec<Thread>) = all.into_iter().partition(|t| t.head.resolved);
            let (in_scene, rest): (Vec<Thread>, Vec<Thread>) = open.into_iter().partition(|t| t.head.anchor.is_some() && t.head.scene.is_some() && t.head.scene == here);
            // Other scenes, each with its title, in the order their first comment came.
            let mut groups: Vec<(String, Vec<Thread>)> = Vec::new();
            for t in rest {
                let title = match (&t.head.anchor, &t.head.scene) {
                    (Some(_), Some(s)) => scene_title(s).unwrap_or_else(|| "A scene no longer here".to_owned()),
                    _ => "Text removed".to_owned(),
                };
                match groups.iter_mut().find(|(g, _)| *g == title) {
                    Some((_, list)) => list.push(t),
                    None => groups.push((title, vec![t])),
                }
            }
            view! {
                {(!in_scene.is_empty()).then(|| view! { <h3 class="shared-group">"In this scene"</h3> })}
                {in_scene.into_iter().map(row).collect_view()}
                {groups.into_iter().map(|(title, list)| view! {
                    <h3 class="shared-group">{title}</h3>
                    {list.into_iter().map(row).collect_view()}
                }).collect_view()}
                {(!resolved.is_empty()).then(|| view! { <h3 class="shared-group">"Resolved"</h3> })}
                {resolved.into_iter().map(row).collect_view()}
            }.into_any()
        }}
    }
}

#[component]
fn LinkList(
    #[prop(into)] shares: Signal<Vec<ShareView>>,
    #[prop(into)] choices: Signal<Vec<Choice>>,
    #[prop(into)] busy: Signal<bool>,
    #[prop(into)] made: Signal<Option<String>>,
    on_make: impl Fn(Covers, String) + Copy + Send + Sync + 'static,
    on_revoke: impl Fn(String) + Copy + Send + Sync + 'static,
    on_author: impl Fn(String) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let picked = RwSignal::new(0usize);
    let name = RwSignal::new(String::new());
    let copied = RwSignal::new(None::<String>);
    let make = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let Some(choice) = choices.with_untracked(|c| c.get(picked.get_untracked()).cloned()) else { return };
        on_make(choice.covers, name.get_untracked());
        name.set(String::new());
    };
    let copy_link = move |id: String, link: String| {
        copy(&link);
        copied.set(Some(id));
    };
    let link = move |share: ShareView| {
        let (id, link) = (share.id.clone(), share.link.clone());
        let (author_id, revoke_id, copy_id) = (id.clone(), id.clone(), id.clone());
        let status = match (&share.error, share.sent) {
            (Some(e), _) => format!("Not sent: {e}"),
            (None, Some(at)) => format!("Sent {}", when(at as f64)),
            (None, None) => "Not sent yet this session".to_owned(),
        };
        let counts = match (share.comments, share.fresh) {
            (0, _) => String::new(),
            (n, 0) => format!(" · {n} comment{}", if n == 1 { "" } else { "s" }),
            (n, f) => format!(" · {n} comment{}, {f} new", if n == 1 { "" } else { "s" }),
        };
        let just_made = move || made.get().as_deref() == Some(id.as_str());
        // `view!` may take the Show's copy first, so each gets its own.
        let made_class = just_made.clone();
        let link_text = link.clone();
        view! {
            <div class="shared-link" class:made=made_class>
                <div class="shared-link-name"><b>{share.name.clone()}</b><span class="shared-meta">" · " {share.title.clone()}</span></div>
                <div class="shared-meta" class:error=share.error.is_some()>{status}{counts}</div>
                <Show when=just_made>
                    <div class="shared-address"><code>{link_text.clone()}</code></div>
                </Show>
                <div class="shared-actions">
                    <button class="quiet" on:click=move |_| copy_link(copy_id.clone(), link.clone())>
                        {let id = share.id.clone(); move || if copied.get().as_deref() == Some(id.as_str()) { "Copied" } else { "Copy link" }}
                    </button>
                    <button class="quiet" on:click=move |_| on_author(author_id.clone())>"Open as author"</button>
                    <span class="grow"></span>
                    <button class="quiet danger" on:click=move |_| {
                        if confirm("Revoke this link? Readers lose the text and every comment on it is deleted. This can't be undone.") {
                            on_revoke(revoke_id.clone());
                        }
                    }>"Revoke"</button>
                </div>
            </div>
        }
    };
    view! {
        <form class="shared-new" on:submit=make>
            <label for="share-what">"What readers see"</label>
            <select id="share-what" on:change=move |ev| picked.set(event_target_value(&ev).parse().unwrap_or(0))>
                {move || choices.get().into_iter().enumerate().map(|(i, c)| view! { <option value=i.to_string() selected=move || picked.get() == i>{c.label}</option> }).collect_view()}
            </select>
            <label for="share-name">"A name, for you"</label>
            <input id="share-name" placeholder="Beta readers" prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) />
            <div class="shared-new-foot">
                <span class="hint">"Readers see each snapshot, never what you’re typing."</span>
                <button type="submit" class="primary" disabled=move || busy.get() || name.get().trim().is_empty()>
                    {move || if busy.get() { "Making…" } else { "Make a link" }}
                </button>
            </div>
        </form>
        {move || shares.get().into_iter().map(link).collect_view()}
    }
}

/// A comment's card, beside its words: the thread, Reply, Resolve, and Delete for each comment.
#[component]
pub fn CommentCard(
    #[prop(into)] thread: Signal<Option<Thread>>,
    /// Where the words are, from the top of the piece.
    top: f64,
    #[prop(into)] busy: Signal<bool>,
    on_reply: impl Fn(String) + Copy + Send + Sync + 'static,
    on_resolve: impl Fn(bool) + Copy + Send + Sync + 'static,
    on_delete: impl Fn(u64) + Copy + Send + Sync + 'static,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let text = RwSignal::new(String::new());
    let said = move |c: CommentView| {
        let id = c.id;
        view! {
            <div class="comment-who">
                <b>{who(&c)}</b>
                {(!c.author).then(|| view! { <span class="comment-badge">{c.share_name.to_lowercase()}</span> })}
                <span class="comment-when">{when(c.created)}</span>
                <button class="comment-delete" title="Delete this comment" aria-label="Delete this comment" on:click=move |_| {
                    if confirm("Delete this comment? Readers won't see it any more.") {
                        on_delete(id);
                    }
                }>
                    <Icon glyph=Glyph::Close size=13 />
                </button>
            </div>
            <p class="comment-text">{c.text.clone()}</p>
        }
    };
    view! {
        <div class="comment-card" style=format!("top: {top}px") role="dialog" aria-label="Comment">
            {move || match thread.get() {
                None => view! { <p class="hint">"This comment was deleted."</p> }.into_any(),
                Some(t) => {
                    let resolved = t.head.resolved;
                    view! {
                        {t.head.anchor.is_none().then(|| view! { <p class="comment-removed">"The words this was on are gone: “" {t.head.quote.clone()} "”"</p> })}
                        {said(t.head.clone())}
                        {(!t.replies.is_empty()).then(|| view! {
                            <div class="comment-replies">{t.replies.into_iter().map(said).collect_view()}</div>
                        })}
                        <textarea class="comment-reply" rows="2" placeholder="Reply" aria-label="Reply"
                            prop:value=move || text.get() on:input=move |ev| text.set(event_target_value(&ev))></textarea>
                        <div class="comment-actions">
                            <button class="quiet" on:click=move |_| on_resolve(!resolved)>{if resolved { "Reopen" } else { "Resolve" }}</button>
                            <span class="grow"></span>
                            <button class="quiet" on:click=move |_| on_close()>"Close"</button>
                            <button class="primary" disabled=move || busy.get() || text.get().trim().is_empty() on:click=move |_| {
                                on_reply(text.get_untracked());
                                text.set(String::new());
                            }>"Reply"</button>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

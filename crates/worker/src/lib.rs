//! The sharing Worker (DESIGN §10). It keeps shared text in KV and comments in D1, all sealed
//! with keys it never sees, and checks who may do what by hashes of their secrets:
//!
//! | | |
//! |---|---|
//! | `PUT /s/{id}` | the author key: upload a version (the first one sets the keys) |
//! | `GET /s/{id}` | anyone with the link |
//! | `DELETE /s/{id}` | the author key: revoke, removing the text and its comments |
//! | `GET /s/{id}/comments?since=` | anyone with the link |
//! | `POST /s/{id}/comments` | the comment token, derived from the link's key; or the author key |
//! | `PATCH`/`DELETE /s/{id}/comments/{cid}` | the author key: resolve, delete |

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use needle_core::share::{fingerprint, valid_id};
use serde::{Deserialize, Serialize};
use worker::wasm_bindgen::JsValue;
use worker::*;

/// KV allows 25 MiB; a whole long manuscript is a few.
const MAX_TEXT: usize = 20 * 1024 * 1024;
const MAX_COMMENT: usize = 16 * 1024;
/// Comments one share takes in a minute, from everyone together.
const COMMENTS_PER_MINUTE: u32 = 30;

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    if req.method() == Method::Options {
        return Response::empty()?.with_status(204).with_cors(&cors());
    }
    let response = Router::new()
        .put_async("/s/:id", put_text)
        .get_async("/s/:id", get_text)
        .delete_async("/s/:id", revoke)
        .get_async("/s/:id/comments", list_comments)
        .post_async("/s/:id/comments", add_comment)
        .patch_async("/s/:id/comments/:cid", resolve_comment)
        .delete_async("/s/:id/comments/:cid", delete_comment)
        .run(req, env)
        .await?;
    response.with_cors(&cors())
}

/// Readers come from GitHub Pages, the desktop from its own webview: anywhere, since every call
/// carries its own proof and nothing rides on cookies.
fn cors() -> Cors {
    Cors::new()
        .with_origins(["*"])
        .with_methods([Method::Get, Method::Put, Method::Post, Method::Patch, Method::Delete, Method::Options])
        .with_allowed_headers(["Authorization", "Content-Type", "X-Comment-Token", "X-Commenter"])
        .with_exposed_headers(["X-Updated"])
        .with_max_age(86_400)
}

fn now() -> f64 {
    Date::now().as_millis() as f64 / 1000.0
}

/// The share's id from the path, if it's one.
fn share_id(ctx: &RouteContext<()>) -> Option<String> {
    ctx.param("id").filter(|id| valid_id(id)).cloned()
}

fn bearer(req: &Request) -> Option<String> {
    req.headers().get("Authorization").ok()??.strip_prefix("Bearer ").map(str::to_owned)
}

#[derive(Deserialize)]
struct Keys {
    author: String,
    commenter: String,
}

async fn keys(db: &D1Database, id: &str) -> Result<Option<Keys>> {
    db.prepare("SELECT author, commenter FROM shares WHERE id = ?1").bind(&[id.into()])?.first::<Keys>(None).await
}

/// Whether `req` carries this share's author key.
fn is_author(req: &Request, keys: &Keys) -> bool {
    bearer(req).is_some_and(|key| fingerprint(&key) == keys.author)
}

fn refuse(status: u16, why: &str) -> Result<Response> {
    Response::error(why, status)
}

// --- The text --------------------------------------------------------------------------------

/// A new version of the text. The first upload sets the author key and the comment token (by
/// their hashes, the second in `X-Commenter`); after that only the author key can replace it.
async fn put_text(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let Some(id) = share_id(&ctx) else { return refuse(404, "no such share") };
    let Some(author) = bearer(&req) else { return refuse(401, "the author key is missing") };
    let db = ctx.env.d1("DB")?;
    let body = req.bytes().await?;
    if body.len() > MAX_TEXT {
        return refuse(413, "too long to share");
    }
    match keys(&db, &id).await? {
        Some(keys) if fingerprint(&author) != keys.author => return refuse(403, "that isn't this share's author key"),
        Some(_) => {
            db.prepare("UPDATE shares SET updated = ?2 WHERE id = ?1").bind(&[id.as_str().into(), now().into()])?.run().await?;
        }
        None => {
            let Some(commenter) = req.headers().get("X-Commenter")?.filter(|c| c.len() == 64) else {
                return refuse(400, "a new share needs X-Commenter");
            };
            db.prepare("INSERT INTO shares (id, author, commenter, updated) VALUES (?1, ?2, ?3, ?4)")
                .bind(&[id.as_str().into(), fingerprint(&author).into(), commenter.into(), now().into()])?
                .run()
                .await?;
        }
    }
    ctx.env.kv("TEXT")?.put_bytes(&id, &body)?.execute().await?;
    Ok(Response::empty()?.with_status(204))
}

async fn get_text(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let Some(id) = share_id(&ctx) else { return refuse(404, "no such share") };
    // D1 first: a revoked share's row is gone at once, while KV can serve a deleted value for up
    // to a minute.
    if keys(&ctx.env.d1("DB")?, &id).await?.is_none() {
        return refuse(404, "no such share");
    }
    let Some(text) = ctx.env.kv("TEXT")?.get(&id).bytes().await? else { return refuse(404, "no such share") };
    let mut response = Response::from_bytes(text)?;
    response.headers_mut().set("Content-Type", "application/octet-stream")?;
    response.headers_mut().set("Cache-Control", "no-store")?;
    Ok(response)
}

/// Revokes a share: its text and every comment go.
async fn revoke(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let Some(id) = share_id(&ctx) else { return refuse(404, "no such share") };
    let db = ctx.env.d1("DB")?;
    let Some(keys) = keys(&db, &id).await? else { return refuse(404, "no such share") };
    if !is_author(&req, &keys) {
        return refuse(403, "only the author can revoke a share");
    }
    db.batch(vec![
        db.prepare("DELETE FROM comments WHERE share = ?1").bind(&[id.as_str().into()])?,
        db.prepare("DELETE FROM shares WHERE id = ?1").bind(&[id.as_str().into()])?,
    ])
    .await?;
    ctx.env.kv("TEXT")?.delete(&id).await?;
    Ok(Response::empty()?.with_status(204))
}

// --- Comments --------------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct Comment {
    id: f64,
    /// Sealed with the link's key, base64url; empty once deleted.
    body: String,
    author: f64,
    resolved: f64,
    deleted: f64,
    created: f64,
    updated: f64,
}

/// What a reader sees of a comment: the Worker's bookkeeping plus the sealed body.
#[derive(Serialize)]
struct CommentView {
    id: u64,
    body: String,
    author: bool,
    resolved: bool,
    deleted: bool,
    created: f64,
    updated: f64,
}

impl From<Comment> for CommentView {
    fn from(c: Comment) -> Self {
        Self {
            id: c.id as u64,
            body: c.body,
            author: c.author != 0.0,
            resolved: c.resolved != 0.0,
            deleted: c.deleted != 0.0,
            created: c.created,
            updated: c.updated,
        }
    }
}

/// Comments changed since `since` (seconds), oldest first; all of them without it.
async fn list_comments(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let Some(id) = share_id(&ctx) else { return refuse(404, "no such share") };
    let db = ctx.env.d1("DB")?;
    if keys(&db, &id).await?.is_none() {
        return refuse(404, "no such share");
    }
    let since = req.url()?.query_pairs().find(|(k, _)| k == "since").and_then(|(_, v)| v.parse::<f64>().ok()).unwrap_or(-1.0);
    let rows = db
        .prepare("SELECT id, body, author, resolved, deleted, created, updated FROM comments WHERE share = ?1 AND updated > ?2 ORDER BY id")
        .bind(&[id.as_str().into(), since.into()])?
        .all()
        .await?
        .results::<Comment>()?;
    let mut response = Response::from_json(&rows.into_iter().map(CommentView::from).collect::<Vec<_>>())?;
    response.headers_mut().set("Cache-Control", "no-store")?;
    Ok(response)
}

/// A new comment, sealed by whoever wrote it. Anyone with the link can, by showing the comment
/// token; the author key marks it as the author's.
async fn add_comment(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let Some(id) = share_id(&ctx) else { return refuse(404, "no such share") };
    let db = ctx.env.d1("DB")?;
    let Some(keys) = keys(&db, &id).await? else { return refuse(404, "no such share") };
    let author = is_author(&req, &keys);
    let token = req.headers().get("X-Comment-Token")?;
    if !author && token.is_none_or(|t| fingerprint(&t) != keys.commenter) {
        return refuse(403, "commenting needs the link");
    }
    let body = req.bytes().await?;
    if body.is_empty() || body.len() > MAX_COMMENT {
        return refuse(413, "a comment has to be shorter than that");
    }
    #[derive(Deserialize)]
    struct Count {
        n: f64,
    }
    let recent = db
        .prepare("SELECT COUNT(*) AS n FROM comments WHERE share = ?1 AND created > ?2")
        .bind(&[id.as_str().into(), (now() - 60.0).into()])?
        .first::<Count>(None)
        .await?
        .map_or(0.0, |c| c.n);
    if !author && recent >= f64::from(COMMENTS_PER_MINUTE) {
        return refuse(429, "too many comments at once; try again in a minute");
    }
    let at = now();
    #[derive(Deserialize)]
    struct Made {
        id: f64,
    }
    let made = db
        .prepare("INSERT INTO comments (share, body, author, created, updated) VALUES (?1, ?2, ?3, ?4, ?4) RETURNING id")
        .bind(&[id.as_str().into(), B64.encode(&body).into(), JsValue::from(if author { 1 } else { 0 }), at.into()])?
        .first::<Made>(None)
        .await?;
    let Some(made) = made else { return refuse(500, "the comment wasn't kept") };
    Ok(Response::from_json(&serde_json::json!({ "id": made.id as u64, "created": at }))?.with_status(201))
}

#[derive(Deserialize)]
struct Resolution {
    resolved: bool,
}

async fn resolve_comment(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let (Some(id), Some(cid)) = (share_id(&ctx), ctx.param("cid").and_then(|c| c.parse::<u32>().ok())) else {
        return refuse(404, "no such comment");
    };
    let db = ctx.env.d1("DB")?;
    let Some(keys) = keys(&db, &id).await? else { return refuse(404, "no such share") };
    if !is_author(&req, &keys) {
        return refuse(403, "only the author can resolve a comment");
    }
    let Resolution { resolved } = req.json().await?;
    db.prepare("UPDATE comments SET resolved = ?3, updated = ?4 WHERE share = ?1 AND id = ?2 AND deleted = 0")
        .bind(&[id.as_str().into(), cid.into(), JsValue::from(if resolved { 1 } else { 0 }), now().into()])?
        .run()
        .await?;
    Ok(Response::empty()?.with_status(204))
}

async fn delete_comment(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let (Some(id), Some(cid)) = (share_id(&ctx), ctx.param("cid").and_then(|c| c.parse::<u32>().ok())) else {
        return refuse(404, "no such comment");
    };
    let db = ctx.env.d1("DB")?;
    let Some(keys) = keys(&db, &id).await? else { return refuse(404, "no such share") };
    if !is_author(&req, &keys) {
        return refuse(403, "only the author can delete a comment");
    }
    db.prepare("UPDATE comments SET deleted = 1, body = '', updated = ?3 WHERE share = ?1 AND id = ?2")
        .bind(&[id.as_str().into(), cid.into(), now().into()])?
        .run()
        .await?;
    Ok(Response::empty()?.with_status(204))
}

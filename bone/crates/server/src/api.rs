use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query as QueryParams, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{Stream, StreamExt};
use mail_parser::{MessageParser, MimeHeaders};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use crate::actions::{ANSWERED, Command, Op, SEEN};
use crate::clean::{self, IMAGE_SCHEME};
use crate::compose::{self, Outgoing, Threading};
use crate::config::AccountConfig;
use crate::db::Db;
use crate::hub::Hub;
use crate::images::Images;
use crate::push::{Pusher, Subscription};
use crate::query::Query;
use crate::store::Blobs;
use crate::views::{Cursor, MessageMeta};

const PAGE: u32 = 50;
const MAX_PAGE: u32 = 200;
const ACTION_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct App {
    pub db: Db,
    pub blobs: Arc<Blobs>,
    pub hub: Arc<Hub>,
    pub tokens: Arc<Vec<[u8; 32]>>,
    pub accounts: Arc<HashMap<String, mpsc::Sender<Command>>>,
    pub outbound: Arc<HashMap<String, Outbound>>,
    pub pusher: Arc<Pusher>,
    pub images: Arc<Images>,
}

#[derive(Clone)]
pub struct Outbound {
    pub config: AccountConfig,
    pub login: String,
}

pub fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

pub fn router(app: App) -> Router {
    Router::new()
        .route("/api/folders", get(folders))
        .route("/api/messages", get(list))
        .route("/api/conversations", get(conversations))
        .route("/api/addresses", get(addresses))
        .route("/api/addresses/new", get(new_address))
        .route("/api/messages/{id}", get(message))
        .route("/api/messages/{id}/raw", get(raw))
        .route("/api/messages/{id}/attachments/{n}", get(attachment))
        .route("/api/messages/{id}/cid/{cid}", get(inline_image))
        .route("/api/threads/{thread}", get(thread))
        .route("/api/events", get(events))
        .route("/api/actions", post(act))
        .route("/api/messages/{id}/reply", get(reply_draft))
        .route("/api/send", post(send))
        .route("/api/push/key", get(push_key))
        .route("/api/img/{id}", get(image))
        .route("/api/push", post(push_subscribe).delete(push_unsubscribe))
        .with_state(app)
}

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::warn!("{e:#}");
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "something went wrong".into(),
        )
    }
}

type ApiResult<T> = Result<T, ApiError>;
type Params = QueryParams<HashMap<String, String>>;

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "not found".into())
}

fn authed(app: &App, headers: &HeaderMap, params: &HashMap<String, String>) -> ApiResult<()> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| params.get("token").map(String::as_str));
    match token {
        Some(t) if app.tokens.contains(&hash_token(t)) => Ok(()),
        _ => Err(ApiError(StatusCode::UNAUTHORIZED, "unknown device".into())),
    }
}

async fn folders(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    Ok(Json(app.db.folder_views().await?))
}

async fn list(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    let query = Query::parse(params.get("q").map_or("", String::as_str));
    let before = match params.get("before") {
        Some(c) => Some(parse_cursor(c).ok_or_else(|| {
            ApiError(StatusCode::BAD_REQUEST, "before must be <date>:<id>".into())
        })?),
        None => None,
    };
    let limit = params
        .get("limit")
        .and_then(|l| l.parse().ok())
        .unwrap_or(PAGE)
        .clamp(1, MAX_PAGE);
    let messages = app
        .db
        .list(
            params.get("folder").map(String::as_str),
            &query,
            before,
            limit,
        )
        .await?;
    let next = (messages.len() == limit as usize)
        .then(|| messages.last().map(|m| format!("{}:{}", m.date, m.id)))
        .flatten();
    Ok(Json(json!({ "messages": messages, "next": next })))
}

async fn conversations(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    let query = Query::parse(params.get("q").map_or("", String::as_str));
    let before = match params.get("before") {
        Some(c) => Some(parse_cursor(c).ok_or_else(|| {
            ApiError(StatusCode::BAD_REQUEST, "before must be <date>:<id>".into())
        })?),
        None => None,
    };
    let limit = params
        .get("limit")
        .and_then(|l| l.parse().ok())
        .unwrap_or(PAGE)
        .clamp(1, MAX_PAGE);
    let conversations = app
        .db
        .conversations(
            params.get("folder").map(String::as_str),
            &query,
            before,
            limit,
        )
        .await?;
    let next = (conversations.len() == limit as usize)
        .then(|| {
            conversations
                .last()
                .map(|c| format!("{}:{}", c.latest.date, c.latest.id))
        })
        .flatten();
    Ok(Json(
        json!({ "conversations": conversations, "next": next }),
    ))
}

fn catch_all_domains(app: &App) -> Vec<String> {
    app.outbound
        .values()
        .filter(|o| o.config.catch_all)
        .filter_map(|o| compose::domain(&o.login).map(str::to_ascii_lowercase))
        .collect()
}

async fn addresses(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    let groups = app
        .db
        .sendable_addresses(&logins(&app), &catch_all_domains(&app))
        .await?;
    let suggested = match params.get("to").filter(|t| !t.trim().is_empty()) {
        Some(to) => {
            let own = app.db.own(&logins(&app)).await?;
            app.db.suggested_from(to, &own).await?
        }
        None => None,
    };
    Ok(Json(json!({ "groups": groups, "suggested": suggested })))
}

async fn new_address(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    let Some(domain) = catch_all_domains(&app).into_iter().next() else {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "none of your mailboxes takes a catch-all".into(),
        ));
    };
    let own = app.db.own(&logins(&app)).await?;
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos() as u64);
    let address = compose::new_address(
        params.get("service").map_or("", String::as_str),
        &domain,
        &own.addresses,
        seed,
    )
    .ok_or_else(|| {
        ApiError(
            StatusCode::BAD_REQUEST,
            "type what the address is for".into(),
        )
    })?;
    Ok(Json(json!({ "address": address })))
}

fn parse_cursor(s: &str) -> Option<Cursor> {
    let (date, id) = s.split_once(':')?;
    Some(Cursor {
        date: date.parse().ok()?,
        id: id.parse().ok()?,
    })
}

async fn thread(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
    Path(thread): Path<i64>,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    let messages = app.db.thread(thread).await?;
    if messages.is_empty() {
        return Err(not_found());
    }
    Ok(Json(messages))
}

#[derive(Serialize)]
struct MessageBody {
    #[serde(flatten)]
    meta: MessageMeta,
    html: Option<String>,
    text: Option<String>,
    remote_images: usize,
}

async fn message(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
    Path(id): Path<i64>,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    let meta = app.db.message(id).await?.ok_or_else(not_found)?;
    let raw = app.blobs.get(&meta.blob)?;
    let (html, text, remote) = tokio::task::spawn_blocking(move || {
        let Some(msg) = MessageParser::default().parse(&raw) else {
            return (None, None, Vec::new());
        };
        let text = msg.body_text(0).map(|t| t.into_owned());
        match msg.body_html(0) {
            Some(html) => {
                let cleaned = clean::clean_html(&html);
                let html = link_inline_images(&cleaned.html, id);
                (Some(html), text, cleaned.images)
            }
            None => (None, text, Vec::new()),
        }
    })
    .await
    .map_err(anyhow::Error::from)?;
    let remote_images = remote.len();
    let html = match html {
        Some(mut html) => {
            let ids = app.images.register(&remote).await?;
            for (n, img) in ids.iter().enumerate().rev() {
                html = html.replace(
                    &format!("\"{IMAGE_SCHEME}:{n}\""),
                    &format!("\"{}\"", app.images.path_for(img)),
                );
            }
            Some(html)
        }
        None => None,
    };
    Ok(Json(MessageBody {
        meta,
        html,
        text,
        remote_images,
    }))
}

fn link_inline_images(html: &str, id: i64) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find("src=\"cid:") {
        out.push_str(&rest[..start]);
        let after = &rest[start + "src=\"cid:".len()..];
        let end = after.find('"').unwrap_or(after.len());
        let cid = after[..end].replace("&amp;", "&");
        let encoded: String = url::form_urlencoded::byte_serialize(cid.as_bytes()).collect();
        out.push_str(&format!("src=\"/api/messages/{id}/cid/{encoded}\""));
        rest = &after[(end + 1).min(after.len())..];
    }
    out.push_str(rest);
    out
}

async fn raw(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
    Path(id): Path<i64>,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    let meta = app.db.message(id).await?.ok_or_else(not_found)?;
    let raw = app.blobs.get(&meta.blob)?;
    Ok(download(raw, "message/rfc822", &format!("{id}.eml"), false))
}

async fn attachment(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
    Path((id, n)): Path<(i64, usize)>,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    let meta = app.db.message(id).await?.ok_or_else(not_found)?;
    let raw = app.blobs.get(&meta.blob)?;
    let msg = MessageParser::default().parse(&raw).ok_or_else(not_found)?;
    let part = msg
        .attachments()
        .filter(|p| p.attachment_name().is_some())
        .nth(n)
        .ok_or_else(not_found)?;
    Ok(download(
        part.contents().to_vec(),
        &content_type(part),
        part.attachment_name().unwrap_or("attachment"),
        false,
    ))
}

async fn inline_image(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
    Path((id, cid)): Path<(i64, String)>,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    let meta = app.db.message(id).await?.ok_or_else(not_found)?;
    let raw = app.blobs.get(&meta.blob)?;
    let msg = MessageParser::default().parse(&raw).ok_or_else(not_found)?;
    let part = msg
        .parts
        .iter()
        .find(|p| {
            p.content_id()
                .is_some_and(|c| c.trim_start_matches('<').trim_end_matches('>') == cid)
        })
        .ok_or_else(not_found)?;
    let mime = content_type(part);
    // svg can carry script, so it's only ever offered as a download
    if !mime.starts_with("image/") || mime.contains("svg") {
        return Err(not_found());
    }
    Ok(download(part.contents().to_vec(), &mime, &cid, true))
}

fn content_type(part: &mail_parser::MessagePart<'_>) -> String {
    part.content_type()
        .map(|c| match c.subtype() {
            Some(sub) => format!("{}/{}", c.ctype(), sub),
            None => c.ctype().to_string(),
        })
        .unwrap_or_else(|| "application/octet-stream".into())
        .to_ascii_lowercase()
}

fn download(body: Vec<u8>, mime: &str, name: &str, inline: bool) -> Response {
    let encoded: String = url::form_urlencoded::byte_serialize(name.as_bytes()).collect();
    let disposition = format!(
        "{}; filename*=UTF-8''{}",
        if inline { "inline" } else { "attachment" },
        encoded.replace('+', "%20")
    );
    let mut res = body.into_response();
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    if let Ok(v) = HeaderValue::from_str(&disposition) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    res
}

#[derive(Deserialize)]
struct ActionRequest {
    ids: Vec<i64>,
    op: String,
    folder: Option<String>,
}

async fn act(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
    Json(req): Json<ActionRequest>,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    let op = Op::parse(&req.op, req.folder.as_deref())
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    if req.ids.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "no messages given".into(),
        ));
    }
    if let Op::Move(target) = &op {
        let known = crate::views::is_role(target)
            || app
                .db
                .folder_views()
                .await?
                .iter()
                .any(|f| &f.key == target);
        if !known {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                format!("there is no folder called {target}"),
            ));
        }
    }
    let located = app.db.locate(&req.ids).await?;
    if located.len() != req.ids.len() {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "some of those messages no longer exist, refresh and try again".into(),
        ));
    }
    let mut groups: BTreeMap<(String, i64), Vec<u32>> = BTreeMap::new();
    for l in located {
        groups
            .entry((l.account, l.folder_id))
            .or_default()
            .push(l.uid);
    }
    let mut replies = Vec::new();
    // archiving means done, so the message is marked read on the way out
    let ops = match &op {
        Op::Move(target) if target == "archive" => vec![
            Op::Flags {
                add: vec![SEEN],
                remove: vec![],
            },
            op.clone(),
        ],
        _ => vec![op.clone()],
    };
    for ((account, folder_id), uids) in groups {
        let tx = app
            .accounts
            .get(&account)
            .ok_or_else(|| anyhow::anyhow!("no sync task for account {account}"))?;
        for op in &ops {
            let (reply, rx) = oneshot::channel();
            tx.send(Command {
                folder_id,
                uids: uids.clone(),
                op: op.clone(),
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("sync task for {account} has stopped"))?;
            replies.push(rx);
        }
    }
    for rx in replies {
        match tokio::time::timeout(ACTION_TIMEOUT, rx).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(msg))) => return Err(ApiError(StatusCode::BAD_GATEWAY, msg)),
            _ => {
                return Err(ApiError(
                    StatusCode::GATEWAY_TIMEOUT,
                    "the mail server didn't answer in time".into(),
                ));
            }
        }
    }
    Ok(Json(json!({ "done": req.ids.len() })))
}

async fn command(
    app: &App,
    account: &str,
    folder_id: i64,
    uids: Vec<u32>,
    op: Op,
) -> Result<(), String> {
    let Some(tx) = app.accounts.get(account) else {
        return Err(format!("no sync task for account {account}"));
    };
    let (reply, rx) = oneshot::channel();
    tx.send(Command {
        folder_id,
        uids,
        op,
        reply,
    })
    .await
    .map_err(|_| format!("sync task for {account} has stopped"))?;
    match tokio::time::timeout(ACTION_TIMEOUT, rx).await {
        Ok(Ok(result)) => result,
        _ => Err("the mail server didn't answer in time".into()),
    }
}

fn logins(app: &App) -> Vec<String> {
    app.outbound.values().map(|o| o.login.clone()).collect()
}

async fn reply_draft(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
    Path(id): Path<i64>,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    let original = app.db.message(id).await?.ok_or_else(not_found)?;
    let own = app.db.own(&logins(&app)).await?;
    let fallback = app
        .outbound
        .get(&original.account)
        .map(|o| o.login.clone())
        .unwrap_or_default();
    let all = params.get("all").is_some_and(|v| v == "1" || v == "true");
    Ok(Json(compose::reply(&original, &own, &fallback, all)))
}

async fn send(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
    Json(out): Json<Outgoing>,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    let bad = |m: String| ApiError(StatusCode::BAD_REQUEST, m);
    let from_addr = out.from.trim().to_ascii_lowercase();
    let own = app.db.own(&logins(&app)).await?;
    if !own.contains(&from_addr) {
        return Err(bad(format!("{from_addr} isn't one of your addresses")));
    }
    let from_domain = compose::domain(&from_addr)
        .ok_or_else(|| bad(format!("{from_addr} is not a valid address")))?;
    let account = match app.db.sender_account(from_domain).await? {
        Some(a) => a,
        None => app
            .outbound
            .iter()
            .find(|(_, o)| compose::domain(&o.login) == Some(from_domain))
            .map(|(name, _)| name.clone())
            .ok_or_else(|| bad(format!("none of your mailboxes can send as {from_addr}")))?,
    };
    let outbound = app
        .outbound
        .get(&account)
        .ok_or_else(|| anyhow::anyhow!("no config for account {account}"))?;

    let original = match out.reply_to {
        Some(id) => Some(app.db.message(id).await?.ok_or_else(not_found)?),
        None => None,
    };
    let threading = original.as_ref().map(|o| Threading {
        message_id: o.message_id.clone(),
        parents: o.parents.clone(),
    });
    let from = crate::parse::Mailbox {
        name: app.db.display_name(&from_addr).await?,
        addr: from_addr.clone(),
    };
    let message_id = compose::new_message_id(&from_addr);
    let message = compose::build(&out, &from, &message_id, threading.as_ref(), false)
        .map_err(|e| bad(format!("{e:#}")))?;
    let copy = compose::build(&out, &from, &message_id, threading.as_ref(), true)
        .map_err(|e| bad(format!("{e:#}")))?;

    compose::submit(&outbound.config, &message)
        .await
        .map_err(|e| {
            ApiError(
                StatusCode::BAD_GATEWAY,
                format!("Migadu didn't accept it: {e:#}"),
            )
        })?;
    tracing::info!(account, "sent a message");

    let sent_folder = app
        .db
        .folders(&account)
        .await?
        .into_iter()
        .find(|f| f.role.as_deref() == Some("sent"));
    let saved = match sent_folder {
        Some(f) => {
            command(
                &app,
                &account,
                f.id,
                Vec::new(),
                Op::Append(copy.formatted()),
            )
            .await
        }
        None => Err("this mailbox has no Sent folder".into()),
    };
    if let Err(e) = &saved {
        tracing::warn!(account, "sent but not saved to Sent: {e}");
    }
    if let Some(o) = &original
        && let Some(l) = app.db.locate(&[o.row.id]).await?.into_iter().next()
    {
        let answered = Op::Flags {
            add: vec![ANSWERED],
            remove: vec![],
        };
        if let Err(e) = command(&app, &l.account, l.folder_id, vec![l.uid], answered).await {
            tracing::warn!("couldn't mark the original as answered: {e}");
        }
    }
    Ok(Json(json!({
        "sent": true,
        "from": from_addr,
        "saved": saved.is_ok(),
        "error": saved.err(),
    })))
}

async fn events(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    authed(&app, &headers, &params)?;
    let changes = BroadcastStream::new(app.hub.subscribe()).map(|change| {
        Ok(match change {
            Ok(change) => Event::default()
                .event("changed")
                .json_data(change)
                .unwrap_or_default(),
            Err(BroadcastStreamRecvError::Lagged(_)) => Event::default().event("resync").data(""),
        })
    });
    let notices = BroadcastStream::new(app.hub.notices()).filter_map(|notice| async move {
        notice.ok().map(|n| {
            Ok(Event::default()
                .event("new")
                .json_data(n)
                .unwrap_or_default())
        })
    });
    let stream = futures_util::stream::select(changes, notices);
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn image(
    State(app): State<App>,
    QueryParams(params): Params,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let sig = params.get("sig").map_or("", String::as_str);
    if !app.images.verify(&id, sig) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "bad image signature".into(),
        ));
    }
    let image = app.images.get(&id).await?.ok_or_else(not_found)?;
    let mut res = image.bytes.into_response();
    let h = res.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&image.content_type) {
        h.insert(header::CONTENT_TYPE, v);
    }
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=31536000, immutable"),
    );
    Ok(res)
}

async fn push_key(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    Ok(Json(json!({ "key": app.pusher.public_key() })))
}

async fn push_subscribe(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
    Json(sub): Json<Subscription>,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    let parsed = url::Url::parse(&sub.endpoint)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "endpoint is not a URL".into()))?;
    if !matches!(parsed.scheme(), "https" | "http") || sub.p256dh.is_empty() || sub.auth.is_empty()
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "a subscription needs an http(s) endpoint, p256dh and auth".into(),
        ));
    }
    app.db.add_subscription(&sub, crate::notify::now()).await?;
    tracing::info!(
        host = parsed.host_str().unwrap_or(""),
        "push subscription added"
    );
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct Unsubscribe {
    endpoint: String,
}

async fn push_unsubscribe(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
    Json(req): Json<Unsubscribe>,
) -> ApiResult<impl IntoResponse> {
    authed(&app, &headers, &params)?;
    let removed = app.db.remove_subscription(&req.endpoint).await?;
    Ok(Json(json!({ "removed": removed })))
}

pub fn with_web(router: Router, dir: &std::path::Path) -> Router {
    let index = dir.join("index.html");
    router
        .fallback_service(
            tower_http::services::ServeDir::new(dir)
                .fallback(tower_http::services::ServeFile::new(index)),
        )
        .layer(axum::middleware::from_fn(web_cache))
}

// files in the nix store are all dated 1970, so anything revalidated by date would never look changed
async fn web_cache(mut request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let path = request.uri().path();
    if path.starts_with("/api/") {
        return next.run(request).await;
    }
    let hashed = path.starts_with("/assets/");
    if !hashed {
        request.headers_mut().remove(header::IF_MODIFIED_SINCE);
        request.headers_mut().remove(header::IF_NONE_MATCH);
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    if hashed {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    } else {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        headers.remove(header::LAST_MODIFIED);
    }
    response
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::db::NewMessage;
    use crate::parse;

    const TOKEN: &str = "test-token";

    fn temp() -> PathBuf {
        crate::testutil::temp_dir("api")
    }

    struct Fixture {
        app: App,
        dir: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    async fn add(
        app: &App,
        account: &str,
        path: &str,
        role: Option<&str>,
        uid: u32,
        flags: &str,
        raw: &str,
    ) {
        app.db.upsert_folder(account, path, role).await.unwrap();
        let folder = app
            .db
            .folders(account)
            .await
            .unwrap()
            .into_iter()
            .find(|f| f.path == path)
            .unwrap();
        app.db
            .insert_messages(
                folder.id,
                &[NewMessage {
                    uid,
                    modseq: 1,
                    flags: flags.into(),
                    internal_date: 0,
                    size: raw.len() as u32,
                }],
            )
            .await
            .unwrap();
        let raw = raw.replace('\n', "\r\n");
        let blob = app.blobs.put(raw.as_bytes()).unwrap();
        app.db.set_blob(folder.id, uid, &blob).await.unwrap();
        let own = ["example.com".to_string()].into();
        let p = parse::parse(raw.as_bytes(), &blob, &own);
        app.db
            .store_parsed(parse::VERSION, &[(blob, p)])
            .await
            .unwrap();
    }

    async fn fixture() -> Fixture {
        let dir = temp();
        let db = Db::memory().await.unwrap();
        let app = App {
            db: db.clone(),
            blobs: Arc::new(Blobs::open(&dir).unwrap()),
            hub: Arc::new(Hub::new()),
            tokens: Arc::new(vec![hash_token(TOKEN)]),
            accounts: Arc::new(HashMap::new()),
            outbound: Arc::new(HashMap::new()),
            pusher: Arc::new(Pusher::open(&dir).unwrap()),
            images: Arc::new(Images::open(&dir, &dir, db.clone()).unwrap()),
        };
        add(&app, "main", "INBOX", Some("inbox"), 1, "\\Seen",
            "From: Shop <news@shop.example.net>\nTo: me@example.com\nMessage-ID: <1@x>\nDate: Tue, 20 Oct 2026 10:00:00 +0000\nSubject: Your invoice\n\nThanks for the order").await;
        add(&app, "catch", "INBOX", Some("inbox"), 1, "",
            "From: a@example.net\nX-Envelope-To: shop.fox@example.com\nTo: shop.fox@example.com\nMessage-ID: <2@x>\nDate: Wed, 21 Oct 2026 10:00:00 +0000\nSubject: Hello\n\nhi there").await;
        add(&app, "main", "Sent", Some("sent"), 1, "\\Seen",
            "From: me@example.com\nTo: a@example.net\nMessage-ID: <3@x>\nIn-Reply-To: <2@x>\nReferences: <2@x>\nDate: Thu, 22 Oct 2026 10:00:00 +0000\nSubject: Re: Hello\n\nhello back").await;
        add(&app, "catch", "Receipts", None, 1, "\\Seen \\Flagged", concat!(
            "From: Shop <news@shop.example.net>\nTo: shop.fox@example.com\nMessage-ID: <4@x>\nDate: Fri, 23 Oct 2026 10:00:00 +0000\nSubject: Receipt\nMIME-Version: 1.0\n",
            "Content-Type: multipart/related; boundary=b\n\n",
            "--b\nContent-Type: text/html\n\n<p onclick=\"x()\">Paid</p><img src=\"cid:logo@shop\"><img src=\"https://t.example.net/open.gif\">\n",
            "--b\nContent-Type: image/png\nContent-ID: <logo@shop>\nContent-Transfer-Encoding: base64\n\niVBORw0KGgo=\n",
            "--b\nContent-Type: application/pdf\nContent-Disposition: attachment; filename=\"receipt 1.pdf\"\nContent-Transfer-Encoding: base64\n\naGVsbG8=\n",
            "--b--\n")).await;
        Fixture { app, dir }
    }

    async fn get(f: &Fixture, uri: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
        let res = router(f.app.clone())
            .oneshot(
                Request::get(uri)
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let body = res.into_body().collect().await.unwrap().to_bytes().to_vec();
        (status, headers, body)
    }

    async fn json(f: &Fixture, uri: &str) -> serde_json::Value {
        let (status, _, body) = get(f, uri).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{uri}: {}",
            String::from_utf8_lossy(&body)
        );
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn requires_a_known_token() {
        let f = fixture().await;
        let res = router(f.app.clone())
            .oneshot(Request::get("/api/folders").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let res = router(f.app.clone())
            .oneshot(
                Request::get("/api/folders?token=wrong")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let res = router(f.app.clone())
            .oneshot(
                Request::get(format!("/api/folders?token={TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn folders_merge_across_mailboxes() {
        let f = fixture().await;
        let v = json(&f, "/api/folders").await;
        assert_eq!(
            v,
            json!([
                { "key": "inbox", "name": "Inbox", "total": 2, "unread": 1 },
                { "key": "sent", "name": "Sent", "total": 1, "unread": 0 },
                { "key": "Receipts", "name": "Receipts", "total": 1, "unread": 0 },
            ])
        );
    }

    #[tokio::test]
    async fn lists_newest_first_with_a_cursor() {
        let f = fixture().await;
        let v = json(&f, "/api/messages?folder=inbox&limit=1").await;
        assert_eq!(v["messages"][0]["subject"], "Hello");
        assert_eq!(v["messages"][0]["recipient"], "shop.fox@example.com");
        assert_eq!(v["messages"][0]["unread"], true);
        let next = v["next"].as_str().unwrap().to_string();
        let v = json(
            &f,
            &format!("/api/messages?folder=inbox&limit=1&before={next}"),
        )
        .await;
        assert_eq!(v["messages"][0]["subject"], "Your invoice");
        let v = json(
            &f,
            &format!(
                "/api/messages?folder=inbox&limit=1&before={}",
                v["next"].as_str().unwrap()
            ),
        )
        .await;
        assert_eq!(v["messages"], json!([]));
        assert_eq!(v["next"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn searches_across_all_folders() {
        let f = fixture().await;
        let subjects = |v: serde_json::Value| -> Vec<String> {
            v["messages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["subject"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(
            subjects(json(&f, "/api/messages?q=from:shop").await),
            vec!["Receipt", "Your invoice"]
        );
        assert_eq!(
            subjects(json(&f, "/api/messages?q=from:shop%20in:inbox").await),
            vec!["Your invoice"]
        );
        assert_eq!(
            subjects(json(&f, "/api/messages?q=is:unread").await),
            vec!["Hello"]
        );
        assert_eq!(
            subjects(json(&f, "/api/messages?q=has:attachment%20is:flagged").await),
            vec!["Receipt"]
        );
        assert_eq!(
            subjects(json(&f, "/api/messages?q=hello%20before:2026-10-22").await),
            vec!["Hello"]
        );
        let (status, _, _) = get(&f, "/api/messages?before=nope").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn conversations_group_threads() {
        let f = fixture().await;
        let all = json(&f, "/api/conversations").await;
        let summary: Vec<(String, i64)> = all["conversations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c["subject"].as_str().unwrap().to_string(),
                    c["messages"].as_i64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("Receipt".to_string(), 1),
                ("Re: Hello".to_string(), 2),
                ("Your invoice".to_string(), 1)
            ]
        );
        let hello = &all["conversations"][1];
        assert_eq!(hello["unread"], true);
        let inbox = json(&f, "/api/conversations?folder=inbox").await;
        assert_eq!(inbox["conversations"].as_array().unwrap().len(), 2);
        assert_eq!(inbox["conversations"][0]["messages"], 1);
    }

    #[tokio::test]
    async fn addresses_and_suggestions() {
        let mut f = fixture().await;
        let v = json(&f, "/api/addresses?to=a@example.net").await;
        assert_eq!(
            v["groups"],
            json!([{ "domain": "example.com", "catch_all": false, "addresses": [
                { "address": "shop.fox@example.com", "count": 1, "mailbox": false }
            ] }])
        );
        assert_eq!(v["suggested"], "me@example.com");
        let (status, _, _) = get(&f, "/api/addresses/new?service=bandcamp").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let config = AccountConfig {
            name: "catch".into(),
            imap: String::new(),
            smtp: String::new(),
            login_file: "/dev/null".into(),
            password_file: "/dev/null".into(),
            catch_all: true,
        };
        f.app.outbound = Arc::new(HashMap::from([(
            "catch".to_string(),
            Outbound {
                config,
                login: "catch@example.com".into(),
            },
        )]));
        let v = json(&f, "/api/addresses").await;
        assert_eq!(v["groups"][0]["catch_all"], true);
        assert_eq!(v["groups"][0]["addresses"][1]["mailbox"], true);
        let v = json(&f, "/api/addresses/new?service=Band%20Camp").await;
        let address = v["address"].as_str().unwrap();
        assert!(address.starts_with("band-camp."), "{address}");
        assert!(address.ends_with("@example.com"), "{address}");
    }

    #[tokio::test]
    async fn threads_span_mailboxes() {
        let f = fixture().await;
        let v = json(&f, "/api/messages?q=subject:hello").await;
        let thread = v["messages"][0]["thread"].as_i64().unwrap();
        let t = json(&f, &format!("/api/threads/{thread}")).await;
        let folders: Vec<&str> = t
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["folder"].as_str().unwrap())
            .collect();
        assert_eq!(folders, vec!["inbox", "sent"]);
        let (status, _, _) = get(&f, "/api/threads/999").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn message_has_clean_html_and_working_parts() {
        let f = fixture().await;
        let v = json(&f, "/api/messages?q=receipt").await;
        let id = v["messages"][0]["id"].as_i64().unwrap();
        let m = json(&f, &format!("/api/messages/{id}")).await;
        let img = f
            .app
            .images
            .path_for(&Images::id("https://t.example.net/open.gif"));
        assert_eq!(
            m["html"],
            format!(
                "<p>Paid</p><img src=\"/api/messages/{id}/cid/logo%40shop\"><img src=\"{img}\">"
            )
        );
        assert!(!m["html"].as_str().unwrap().contains(IMAGE_SCHEME));
        let (status, _, _) = get(&f, &format!("{img}x")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(m["remote_images"], 1);
        assert_eq!(m["attachment_list"][0]["name"], "receipt 1.pdf");
        assert_eq!(m["folder"], "Receipts");

        let (status, headers, body) = get(&f, &format!("/api/messages/{id}/cid/logo%40shop")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "image/png");
        assert_eq!(body, b"\x89PNG\r\n\x1a\n");

        let (status, headers, body) = get(&f, &format!("/api/messages/{id}/attachments/0")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers[header::CONTENT_DISPOSITION],
            "attachment; filename*=UTF-8''receipt%201.pdf"
        );
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(body, b"hello");
        let (status, _, _) = get(&f, &format!("/api/messages/{id}/attachments/1")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, headers, body) = get(&f, &format!("/api/messages/{id}/raw")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "message/rfc822");
        assert!(body.starts_with(b"From: Shop"));
    }

    async fn post(app: App, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
        let res = router(app)
            .oneshot(
                Request::post("/api/actions")
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = res.status();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&body).unwrap_or_default())
    }

    #[tokio::test]
    async fn actions_are_grouped_per_mailbox_folder() {
        let mut f = fixture().await;
        let (main_tx, main_rx) = mpsc::channel::<Command>(8);
        let (catch_tx, catch_rx) = mpsc::channel::<Command>(8);
        f.app.accounts = Arc::new(HashMap::from([
            ("main".to_string(), main_tx),
            ("catch".to_string(), catch_tx),
        ]));
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        for (name, mut rx) in [("main", main_rx), ("catch", catch_rx)] {
            let seen = seen.clone();
            tokio::spawn(async move {
                while let Some(cmd) = rx.recv().await {
                    seen.lock()
                        .unwrap()
                        .push((name, cmd.uids.len(), cmd.op.clone()));
                    let reply = if name == "catch" && cmd.op == Op::Delete {
                        Err("Migadu said no".to_string())
                    } else {
                        Ok(())
                    };
                    let _ = cmd.reply.send(reply);
                }
            });
        }
        let ids: Vec<i64> = json(&f, "/api/messages").await["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_i64().unwrap())
            .collect();
        assert_eq!(ids.len(), 4);

        let (status, body) = post(f.app.clone(), json!({ "ids": ids, "op": "read" })).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["done"], 4);
        let mut calls = seen.lock().unwrap().clone();
        calls.sort_by_key(|c| (c.0, c.1));
        assert_eq!(
            calls.iter().map(|c| (c.0, c.1)).collect::<Vec<_>>(),
            vec![("catch", 1), ("catch", 1), ("main", 1), ("main", 1)]
        );

        seen.lock().unwrap().clear();
        let (status, _) = post(f.app.clone(), json!({ "ids": [ids[0]], "op": "archive" })).await;
        assert_eq!(status, StatusCode::OK);
        let ops: Vec<Op> = seen.lock().unwrap().iter().map(|c| c.2.clone()).collect();
        assert_eq!(
            ops,
            vec![
                Op::Flags {
                    add: vec![SEEN],
                    remove: vec![]
                },
                Op::Move("archive".into())
            ]
        );

        let (status, body) = post(f.app.clone(), json!({ "ids": ids, "op": "delete" })).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"], "Migadu said no");

        let (status, _) = post(f.app.clone(), json!({ "ids": [999], "op": "read" })).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = post(f.app.clone(), json!({ "ids": ids, "op": "move" })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = post(f.app.clone(), json!({ "ids": [], "op": "read" })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, body) = post(
            f.app.clone(),
            json!({ "ids": ids, "op": "move", "folder": "flagged" }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "there is no folder called flagged");
        seen.lock().unwrap().clear();
        let (status, _) = post(
            f.app.clone(),
            json!({ "ids": [ids[0]], "op": "move", "folder": "Receipts" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn inline_images_point_at_the_api() {
        assert_eq!(
            link_inline_images(r#"<img src="cid:a&amp;b"> <img src="cid:c">"#, 7),
            r#"<img src="/api/messages/7/cid/a%26b"> <img src="/api/messages/7/cid/c">"#
        );
    }
    #[tokio::test]
    async fn web_files_are_never_cached_but_hashed_assets_are() {
        let dir = temp();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), "<!doctype html>").unwrap();
        std::fs::write(dir.join("assets/app.js"), "x").unwrap();
        std::fs::write(dir.join("favicon.svg"), "<svg/>").unwrap();
        let web = with_web(Router::new(), &dir);
        for uri in ["/", "/inbox/4", "/favicon.svg"] {
            let res = web
                .clone()
                .oneshot(
                    Request::get(uri)
                        .header(header::IF_MODIFIED_SINCE, "Thu, 01 Jan 1970 00:00:01 GMT")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "{uri}");
            assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");
            assert!(res.headers().get(header::LAST_MODIFIED).is_none());
        }
        let res = web
            .oneshot(Request::get("/assets/app.js").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(
            res.headers()[header::CACHE_CONTROL]
                .to_str()
                .unwrap()
                .contains("immutable")
        );
    }
}

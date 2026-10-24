use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Query as QueryParams, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::{Stream, StreamExt};
use mail_parser::{MessageParser, MimeHeaders};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use crate::clean;
use crate::db::Db;
use crate::hub::Hub;
use crate::query::Query;
use crate::store::Blobs;
use crate::views::{Cursor, MessageMeta};

const PAGE: u32 = 50;
const MAX_PAGE: u32 = 200;

#[derive(Clone)]
pub struct App {
    pub db: Db,
    pub blobs: Arc<Blobs>,
    pub hub: Arc<Hub>,
    pub tokens: Arc<Vec<[u8; 32]>>,
}

pub fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

pub fn router(app: App) -> Router {
    Router::new()
        .route("/api/folders", get(folders))
        .route("/api/messages", get(list))
        .route("/api/messages/{id}", get(message))
        .route("/api/messages/{id}/raw", get(raw))
        .route("/api/messages/{id}/attachments/{n}", get(attachment))
        .route("/api/messages/{id}/cid/{cid}", get(inline_image))
        .route("/api/threads/{thread}", get(thread))
        .route("/api/events", get(events))
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
    let (html, text, remote_images) = tokio::task::spawn_blocking(move || {
        let Some(msg) = MessageParser::default().parse(&raw) else {
            return (None, None, 0);
        };
        let text = msg.body_text(0).map(|t| t.into_owned());
        match msg.body_html(0) {
            Some(html) => {
                let cleaned = clean::clean_html(&html);
                let html = link_inline_images(&cleaned.html, id);
                (Some(html), text, cleaned.images.len())
            }
            None => (None, text, 0),
        }
    })
    .await
    .map_err(anyhow::Error::from)?;
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

async fn events(
    State(app): State<App>,
    headers: HeaderMap,
    QueryParams(params): Params,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    authed(&app, &headers, &params)?;
    let stream = BroadcastStream::new(app.hub.subscribe()).map(|change| {
        Ok(match change {
            Ok(change) => Event::default()
                .event("changed")
                .json_data(change)
                .unwrap_or_default(),
            Err(BroadcastStreamRecvError::Lagged(_)) => Event::default().event("resync").data(""),
        })
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::clean::IMAGE_SCHEME;
    use crate::db::NewMessage;
    use crate::parse;

    const TOKEN: &str = "test-token";

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bone-api-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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
        let app = App {
            db: Db::memory().await.unwrap(),
            blobs: Arc::new(Blobs::open(&dir).unwrap()),
            hub: Arc::new(Hub::new()),
            tokens: Arc::new(vec![hash_token(TOKEN)]),
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
        assert_eq!(
            m["html"],
            format!(
                "<p>Paid</p><img src=\"/api/messages/{id}/cid/logo%40shop\"><img src=\"{IMAGE_SCHEME}:0\">"
            )
        );
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

    #[test]
    fn inline_images_point_at_the_api() {
        assert_eq!(
            link_inline_images(r#"<img src="cid:a&amp;b"> <img src="cid:c">"#, 7),
            r#"<img src="/api/messages/7/cid/a%26b"> <img src="/api/messages/7/cid/c">"#
        );
    }
}

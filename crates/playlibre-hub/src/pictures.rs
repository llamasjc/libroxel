//! Pictures of avatars: a head and shoulders and a whole body, drawn again
//! whenever the look they were drawn from changes.

use std::sync::Arc;

use axum::extract::{Extension, Path as UrlPath, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use playlibre_avatar::{Shot, look_of, render};
use rusqlite::{Connection, OptionalExtension, params};

use crate::limits::{Caller, Limit, Limits, too_many};
use crate::{Db, owner};

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS avatar_pictures (
            player_id   TEXT PRIMARY KEY,
            look        TEXT NOT NULL,
            head        BLOB NOT NULL,
            body        BLOB NOT NULL
        );",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))
}

pub(crate) const HEAD: (u32, u32) = (256, 256);
pub(crate) const BODY: (u32, u32) = (300, 400);

fn draw(look: &serde_json::Value, name: &str, shot: Shot) -> Vec<u8> {
    let (w, h) = if shot == Shot::Head { HEAD } else { BODY };
    render(&look_of(look, name), shot, w, h).png()
}

fn png(bytes: Vec<u8>, tag: &str, headers: &HeaderMap, cache: &'static str) -> Response {
    let etag = format!("\"{tag}\"");
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return StatusCode::NOT_MODIFIED.into_response();
    }
    let mut response = bytes.into_response();
    let h = response.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    if let Ok(v) = HeaderValue::from_str(&etag) {
        h.insert(header::ETAG, v);
    }
    response
}

fn tag(look: &str) -> String {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in look.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
    }
    format!("{h:016x}")
}

type Drawn = (String, String, String, Option<String>, Option<Vec<u8>>, Option<Vec<u8>>);

pub(crate) async fn picture(
    State(db): State<Db>,
    UrlPath((id, file)): UrlPath<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let Some(shot) = file.strip_suffix(".png").and_then(Shot::named) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let number: Option<i64> = id.parse().ok();
    let found: Option<Drawn> = {
        let db = db.lock().unwrap();
        db.query_row(
            &format!(
                "SELECT a.player_id, a.name, a.look, p.look, p.head, p.body FROM accounts a
                 LEFT JOIN avatar_pictures p ON p.player_id = a.player_id
                 WHERE (a.player_id = ?1 OR a.number = ?2) AND NOT {}",
                owner::banned_sql("a.player_id")
            ),
            params![id, number],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()
        .ok()
        .flatten()
    };
    let Some((player_id, name, look, drawn, head, body)) = found else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let cache = "public, no-cache";
    if drawn.as_deref() == Some(look.as_str())
        && let (Some(head), Some(body)) = (head, body)
    {
        return png(if shot == Shot::Head { head } else { body }, &tag(&look), &headers, cache);
    }
    let json: serde_json::Value = serde_json::from_str(&look).unwrap_or(serde_json::Value::Null);
    let (j, n) = (json.clone(), name.clone());
    let Ok((head, body)) =
        tokio::task::spawn_blocking(move || (draw(&j, &n, Shot::Head), draw(&j, &n, Shot::Body))).await
    else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let _ = db.lock().unwrap().execute(
        "INSERT INTO avatar_pictures (player_id, look, head, body) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(player_id) DO UPDATE SET look = ?2, head = ?3, body = ?4",
        params![player_id, look, head, body],
    );
    png(if shot == Shot::Head { head } else { body }, &tag(&look), &headers, cache)
}

#[derive(serde::Deserialize)]
pub(crate) struct Preview {
    look: String,
    #[serde(default)]
    name: String,
}

pub(crate) async fn preview(
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    UrlPath(file): UrlPath<String>,
    Query(q): Query<Preview>,
    headers: HeaderMap,
) -> Response {
    let Some(shot) = file.strip_suffix(".png").and_then(Shot::named) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if q.look.len() > 8 * 1024 || q.name.len() > 64 {
        return (StatusCode::BAD_REQUEST, "that look is too big").into_response();
    }
    if !limits.take(Limit::Picture, &caller.0) {
        return too_many("pictures").into_response();
    }
    let json: serde_json::Value = serde_json::from_str(&q.look).unwrap_or(serde_json::Value::Null);
    let key = tag(&format!("{}\n{}", q.name, q.look));
    let Ok(bytes) = tokio::task::spawn_blocking(move || draw(&json, &q.name, shot)).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    png(bytes, &key, &headers, "public, max-age=86400")
}

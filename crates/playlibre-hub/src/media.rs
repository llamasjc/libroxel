//! A game's own pictures: an icon and a few screenshots, put up by its
//! creator and shown on its page, in lists and in the app.
//!
//! Every picture is decoded and drawn again at a set size before it is
//! kept, so what the Hub serves is always a plain image of a known size,
//! with nothing hidden inside it.

use std::io::Cursor;

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use image::{DynamicImage, ImageFormat, imageops::FilterType};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::social::{Failure, fresh, internal};
use crate::{Db, hidden_sql, manage, verify};

/// Slot 0 is the icon; 1 to this are screenshots.
pub const MAX_SCREENSHOTS: u32 = 6;
/// The most an uploaded picture can be, before it is drawn again.
pub const MAX_UPLOAD: usize = 6 * 1024 * 1024;
/// Icons are square, this many pixels on a side.
pub const ICON_SIZE: u32 = 512;
/// Screenshots fit inside this.
pub const SHOT_SIZE: (u32, u32) = (1280, 720);
/// Pictures bigger than this on a side aren't taken at all.
const MAX_SIDE: u32 = 8192;

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS game_media (
            game_id     TEXT NOT NULL,
            slot        INTEGER NOT NULL,
            bytes       BLOB NOT NULL,
            mime        TEXT NOT NULL,
            version     INTEGER NOT NULL,
            PRIMARY KEY (game_id, slot)
        );",
    )
    .map_err(|e| format!("could not set up game pictures: {e}"))
}

/// What a creator sends to `POST /games/{id}/media`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaChange {
    pub player_id: String,
    /// Milliseconds since 1970.
    pub time: i64,
    /// 0 for the icon, 1 and up for screenshots.
    pub slot: u32,
    /// The picture (PNG or JPEG) in base64; `None` takes it down.
    #[serde(default)]
    pub image: Option<String>,
    /// [`media_message`] signed with the creator's identity.
    pub signature: String,
}

/// What a creator signs: the picture by its SHA-256, or "remove".
pub fn media_message(player_id: &str, game_id: &str, time: i64, slot: u32, image: Option<&[u8]>) -> Vec<u8> {
    let what =
        image.map_or_else(|| "remove".to_owned(), |b| data_encoding::HEXLOWER.encode(&Sha256::digest(b)));
    format!("playlibre media\n{player_id}\n{game_id}\n{time}\n{slot}\n{what}").into_bytes()
}

/// Which pictures a game has, for [`crate::GameInfo`]: slot and version
/// (part of the address, so a new picture isn't hidden by an old cached one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaRef {
    pub slot: u32,
    pub version: i64,
}

/// Reads `slot:version,slot:version` as SQLite's `group_concat` writes it.
pub(crate) fn parse_refs(text: Option<String>) -> Vec<MediaRef> {
    let mut refs: Vec<MediaRef> = text
        .unwrap_or_default()
        .split(',')
        .filter_map(|pair| {
            let (slot, version) = pair.split_once(':')?;
            Some(MediaRef { slot: slot.parse().ok()?, version: version.parse().ok()? })
        })
        .collect();
    refs.sort_by_key(|r| r.slot);
    refs
}

/// Draws an uploaded picture again at its slot's size: (bytes, mime type).
pub fn redraw(bytes: &[u8], slot: u32) -> Result<(Vec<u8>, &'static str), String> {
    let format = image::guess_format(bytes).map_err(|_| "that isn't a picture".to_owned())?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return Err("pictures must be PNG or JPEG".into());
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let picture = reader.decode().map_err(|e| format!("could not read that picture: {e}"))?;
    let mut out = Cursor::new(Vec::new());
    if slot == 0 {
        let side = picture.width().min(picture.height());
        let square =
            picture.crop_imm((picture.width() - side) / 2, (picture.height() - side) / 2, side, side);
        let icon = square.resize_exact(ICON_SIZE, ICON_SIZE, FilterType::Lanczos3);
        DynamicImage::ImageRgba8(icon.to_rgba8())
            .write_to(&mut out, ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        Ok((out.into_inner(), "image/png"))
    } else {
        let (w, h) = SHOT_SIZE;
        let shot = if picture.width() > w || picture.height() > h {
            picture.resize(w, h, FilterType::Lanczos3)
        } else {
            picture
        };
        DynamicImage::ImageRgb8(shot.to_rgb8())
            .write_to(&mut out, ImageFormat::Jpeg)
            .map_err(|e| e.to_string())?;
        Ok((out.into_inner(), "image/jpeg"))
    }
}

/// Takes every picture of a game down.
pub(crate) fn forget(db: &Connection, game_id: &str) {
    let _ = db.execute("DELETE FROM game_media WHERE game_id = ?1", params![game_id]);
}

fn bad(why: impl Into<String>) -> Failure {
    (StatusCode::BAD_REQUEST, why.into())
}

pub(crate) async fn change_media(
    State(db): State<Db>,
    UrlPath(game_id): UrlPath<String>,
    Json(c): Json<MediaChange>,
) -> Result<Json<Vec<MediaRef>>, Failure> {
    if c.slot > MAX_SCREENSHOTS {
        return Err(bad(format!("a game has an icon and up to {MAX_SCREENSHOTS} screenshots")));
    }
    let bytes = match &c.image {
        Some(text) => {
            let b =
                data_encoding::BASE64.decode(text.as_bytes()).map_err(|_| bad("that picture is garbled"))?;
            if b.len() > MAX_UPLOAD {
                return Err(bad(format!("pictures can be at most {} MB", MAX_UPLOAD / 1024 / 1024)));
            }
            Some(b)
        }
        None => None,
    };
    if !verify(
        &c.player_id,
        &media_message(&c.player_id, &game_id, c.time, c.slot, bytes.as_deref()),
        &c.signature,
    ) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match".into()));
    }
    {
        let db = db.lock().unwrap();
        manage::owned(&db, &game_id, &c.player_id)?;
        fresh(&db, &c.player_id, &format!("media {game_id}"), c.time)?;
    }
    let drawn = match bytes {
        Some(b) => {
            let slot = c.slot;
            Some(
                tokio::task::spawn_blocking(move || redraw(&b, slot))
                    .await
                    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "could not draw that picture".into()))?
                    .map_err(bad)?,
            )
        }
        None => None,
    };
    let db = db.lock().unwrap();
    match drawn {
        Some((bytes, mime)) => db.execute(
            "INSERT INTO game_media (game_id, slot, bytes, mime, version) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(game_id, slot) DO UPDATE SET bytes = ?3, mime = ?4, version = ?5",
            params![game_id, c.slot, bytes, mime, c.time],
        ),
        None => {
            db.execute("DELETE FROM game_media WHERE game_id = ?1 AND slot = ?2", params![game_id, c.slot])
        }
    }
    .map_err(internal)?;
    Ok(Json(refs(&db, &game_id)))
}

/// The pictures a game has, icon first.
pub fn refs(db: &Connection, game_id: &str) -> Vec<MediaRef> {
    parse_refs(
        db.query_row(
            "SELECT group_concat(slot || ':' || version) FROM game_media WHERE game_id = ?1",
            params![game_id],
            |r| r.get(0),
        )
        .ok()
        .flatten(),
    )
}

pub(crate) async fn get_media(
    State(db): State<Db>,
    UrlPath((game_id, file)): UrlPath<(String, String)>,
    headers: HeaderMap,
) -> Response {
    // `<slot>` or `<slot>-<version>` (with any extension).
    let name = file.split('.').next().unwrap_or("");
    let Some(slot) = name.split('-').next().and_then(|s| s.parse::<u32>().ok()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let found: Option<(Vec<u8>, String, i64)> = {
        let db = db.lock().unwrap();
        db.query_row(
            &format!(
                "SELECT m.bytes, m.mime, m.version FROM game_media m JOIN games g ON g.game_id = m.game_id
                 WHERE m.game_id = ?1 AND m.slot = ?2 AND NOT {}",
                hidden_sql("game", "g.game_id")
            ),
            params![game_id, slot],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .ok()
        .flatten()
    };
    let Some((bytes, mime, version)) = found else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let etag = format!("\"{version}\"");
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return StatusCode::NOT_MODIFIED.into_response();
    }
    let mut response = bytes.into_response();
    let h = response.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&mime) {
        h.insert(header::CONTENT_TYPE, v);
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=300"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if let Ok(v) = HeaderValue::from_str(&etag) {
        h.insert(header::ETAG, v);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(w, h).write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn icons_are_squared_and_screenshots_fit() {
        let (icon, mime) = redraw(&png(900, 300), 0).unwrap();
        assert_eq!(mime, "image/png");
        let icon = image::load_from_memory(&icon).unwrap();
        assert_eq!((icon.width(), icon.height()), (ICON_SIZE, ICON_SIZE));
        let (shot, mime) = redraw(&png(2560, 1080), 1).unwrap();
        assert_eq!(mime, "image/jpeg");
        let shot = image::load_from_memory(&shot).unwrap();
        assert!(shot.width() <= SHOT_SIZE.0 && shot.height() <= SHOT_SIZE.1);
        assert!(redraw(b"not a picture", 0).is_err());
    }

    #[test]
    fn refs_read_back_in_slot_order() {
        assert_eq!(
            parse_refs(Some("2:30,0:10,1:20".into())),
            vec![
                MediaRef { slot: 0, version: 10 },
                MediaRef { slot: 1, version: 20 },
                MediaRef { slot: 2, version: 30 }
            ]
        );
        assert!(parse_refs(None).is_empty());
    }
}

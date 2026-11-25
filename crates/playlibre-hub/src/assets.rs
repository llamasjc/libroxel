use std::path::Path;
use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Extension, Path as UrlPath, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::limits::{Caller, Limit, Limits, too_many};
use crate::{Db, HubClient, now};

pub const MAX_ASSET: usize = 16 * 1024 * 1024;

pub const ASSET_PREFIX: &str = "asset://";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OfficialGame {
    pub name: String,
    pub about: String,
    #[serde(default)]
    pub symbol: String,
    pub hash: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StarterPlace {
    pub name: String,
    pub about: String,
    #[serde(default)]
    pub symbol: String,
    pub place: String,
}

pub fn asset_hash(bytes: &[u8]) -> String {
    data_encoding::HEXLOWER.encode(&Sha256::digest(bytes))
}

pub fn valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS assets (
            hash        TEXT PRIMARY KEY,
            data        BLOB NOT NULL,
            created     INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS official (
            name        TEXT PRIMARY KEY,
            about       TEXT NOT NULL,
            symbol      TEXT NOT NULL DEFAULT '',
            place       TEXT NOT NULL,
            hash        TEXT NOT NULL,
            position    INTEGER NOT NULL,
            updated     INTEGER NOT NULL
        );",
    )
    .map_err(|e| format!("could not set up the Hub's asset tables: {e}"))
}

pub(crate) async fn put_asset(
    State(db): State<Db>,
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    UrlPath(hash): UrlPath<String>,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
    if !valid_hash(&hash) || asset_hash(&body) != hash {
        return Err((StatusCode::BAD_REQUEST, "the file does not match its hash".into()));
    }
    if body.len() > MAX_ASSET {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "that file is too big".into()));
    }
    let db = db.lock().unwrap();
    if has(&db, &hash) {
        return Ok(StatusCode::NO_CONTENT);
    }
    if !limits.take(Limit::Asset, &caller.0) {
        return Err(too_many("files uploaded"));
    }
    db.execute(
        "INSERT OR IGNORE INTO assets (hash, data, created) VALUES (?1, ?2, ?3)",
        params![hash, body.as_ref(), now()],
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::CREATED)
}

fn has(db: &Connection, hash: &str) -> bool {
    db.query_row("SELECT 1 FROM assets WHERE hash = ?1", params![hash], |_| Ok(()))
        .optional()
        .ok()
        .flatten()
        .is_some()
}

pub(crate) async fn get_asset(
    State(db): State<Db>,
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    UrlPath(hash): UrlPath<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if !valid_hash(&hash) {
        return Err((StatusCode::NOT_FOUND, String::new()));
    }
    if !caller.local() && !limits.take_logged(Limit::AssetGet, &caller.0) {
        return Err(too_many("files downloaded"));
    }
    let db = db.lock().unwrap();
    let data: Vec<u8> = db
        .query_row("SELECT data FROM assets WHERE hash = ?1", params![hash], |r| r.get(0))
        .optional()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, String::new()))?
        .ok_or((StatusCode::NOT_FOUND, String::new()))?;
    Ok((
        [
            (header::CACHE_CONTROL, "private, max-age=31536000, immutable"),
            (header::CONTENT_TYPE, "application/octet-stream"),
        ],
        data,
    ))
}

pub(crate) async fn missing_assets(
    State(db): State<Db>,
    Json(hashes): Json<Vec<String>>,
) -> Json<Vec<String>> {
    let db = db.lock().unwrap();
    Json(hashes.into_iter().take(4096).filter(|h| valid_hash(h) && !has(&db, h)).collect())
}

pub(crate) async fn list_official(State(db): State<Db>) -> Json<Vec<OfficialGame>> {
    let db = db.lock().unwrap();
    Json(official(&db))
}

pub fn official(db: &Connection) -> Vec<OfficialGame> {
    let Ok(mut statement) =
        db.prepare("SELECT name, about, hash, LENGTH(place), symbol FROM official ORDER BY position, name")
    else {
        return Vec::new();
    };
    statement
        .query_map([], |r| {
            Ok(OfficialGame {
                name: r.get(0)?,
                about: r.get(1)?,
                hash: r.get(2)?,
                size: r.get::<_, i64>(3)? as u64,
                symbol: r.get(4)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

pub(crate) async fn official_place(
    State(db): State<Db>,
    UrlPath(name): UrlPath<String>,
) -> Result<String, StatusCode> {
    let db = db.lock().unwrap();
    db.query_row("SELECT place FROM official WHERE name = ?1", params![name], |r| r.get(0))
        .optional()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)
}

pub fn set_official(db: &Connection, games: &[StarterPlace]) -> Result<(), String> {
    let fail = |e: rusqlite::Error| format!("could not keep the official games: {e}");
    let tx = db.unchecked_transaction().map_err(fail)?;
    tx.execute("DELETE FROM official", []).map_err(fail)?;
    tx.execute("DELETE FROM prints WHERE game_id LIKE 'starter:%'", []).map_err(fail)?;
    for (position, game) in games.iter().enumerate() {
        tx.execute(
            "INSERT OR REPLACE INTO official (name, about, symbol, place, hash, position, updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                game.name,
                game.about,
                game.symbol,
                game.place,
                asset_hash(game.place.as_bytes()),
                position as i64,
                now()
            ],
        )
        .map_err(fail)?;
        crate::copies::keep(
            &tx,
            &format!("starter:{}", game.name),
            "starter",
            &crate::copies::prints(&game.place),
        )
        .map_err(fail)?;
    }
    tx.commit().map_err(fail)
}

pub fn official_from(program: &Path) -> Result<Vec<StarterPlace>, String> {
    let output = std::process::Command::new(program)
        .arg("starters")
        .output()
        .map_err(|e| format!("could not run {}: {e}", program.display()))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
}

impl HubClient {
    pub fn asset(&self, hash: &str) -> Result<Vec<u8>, String> {
        if !valid_hash(hash) {
            return Err("not an asset".into());
        }
        let bytes = self
            .slow
            .get(format!("{}/assets/{hash}", self.base))
            .call()
            .and_then(|mut r| r.body_mut().with_config().limit(MAX_ASSET as u64 + 1).read_to_vec())
            .map_err(|e| format!("could not download the file: {e}"))?;
        if asset_hash(&bytes) != hash {
            return Err("the file came back wrong".into());
        }
        Ok(bytes)
    }

    pub fn put_asset(&self, bytes: &[u8]) -> Result<String, String> {
        let hash = asset_hash(bytes);
        self.slow
            .post(format!("{}/assets/{hash}", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .content_type("application/octet-stream")
            .send(bytes)
            .map_err(|e| format!("could not reach the Hub: {e}"))
            .and_then(|mut r| {
                if r.status().is_success() {
                    Ok(hash)
                } else {
                    let why = r.body_mut().read_to_string().unwrap_or_default();
                    Err(format!("the Hub did not take a file: {why}"))
                }
            })
    }

    pub fn missing_assets(&self, hashes: &[String]) -> Result<Vec<String>, String> {
        self.agent
            .post(format!("{}/assets/missing", self.base))
            .send_json(hashes)
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    pub fn official(&self) -> Result<Vec<OfficialGame>, String> {
        let mut response = self
            .agent
            .get(format!("{}/official", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(|e| format!("could not reach the Hub: {e}"))?;
        if response.status().as_u16() == 404 {
            return Ok(Vec::new());
        }
        response.body_mut().read_json().map_err(|e| e.to_string())
    }

    pub fn official_place(&self, name: &str) -> Result<String, String> {
        self.slow
            .get(format!("{}/official/{}", self.base, encode(name)))
            .call()
            .and_then(|mut r| r.body_mut().with_config().limit(crate::MAX_PLACE as u64 + 1).read_to_string())
            .map_err(|e| format!("could not download {name}: {e}"))
    }
}

pub(crate) fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

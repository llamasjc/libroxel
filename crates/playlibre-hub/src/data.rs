//! Saved data for games that run on servers: what `DataStoreService` keeps,
//! held by the Hub so it outlasts any one server.
//!
//! Each server of a game keeps its own copy while it runs and trades
//! changes with the Hub every few seconds: it sends the keys its scripts
//! wrote (or removed) since the last trade and gets back every key another
//! server changed since then. Every change gets the next number of its game,
//! so a server only ever asks for "everything after number n".
//!
//! Only a server the Hub trusts with the game may trade: one of the Hub's
//! own servers of that game, a server run by one of the Hub's owners, or
//! the game's creator (hosting it from their own computer). Anyone else
//! hosting a game keeps its data on their own computer, as before.

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::social::{Failure, fresh, internal};
use crate::{Db, fleet, owner, verify};

/// A saved value can be at most this long once encoded (as in the public
/// API docs).
pub const MAX_VALUE: usize = 4 * 1024 * 1024;
/// A store or key name can be at most this long.
pub const MAX_NAME: usize = 200;
/// One trade carries at most this much (servers split bigger ones).
pub const MAX_TRADE: usize = 6 * 1024 * 1024;
/// All of a game's saved data together can be at most this big.
pub const MAX_GAME_DATA: i64 = 512 * 1024 * 1024;
/// A trade gets back at most this many changes; the server asks again for the rest.
pub const MAX_CHANGES_BACK: usize = 2000;

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS game_data (
            game_id     TEXT NOT NULL,
            store       TEXT NOT NULL,
            key         TEXT NOT NULL,
            value       TEXT,
            size        INTEGER NOT NULL,
            seq         INTEGER NOT NULL,
            PRIMARY KEY (game_id, store, key)
        );
        CREATE INDEX IF NOT EXISTS game_data_seq ON game_data(game_id, seq);",
    )
    .map_err(|e| format!("could not set up saved game data: {e}"))
}

/// One key a server wrote: `value` is `None` when it was removed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataChange {
    pub store: String,
    pub key: String,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
}

/// What a server sends to `POST /games/{id}/data`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataTrade {
    /// The server's id (its identity's player id).
    pub server_id: String,
    /// Milliseconds since 1970; each trade's must be later than the last.
    pub time: i64,
    /// The number of the last change it has (0 at first).
    pub since: i64,
    #[serde(default)]
    pub changes: Vec<DataChange>,
    /// [`data_message`] signed with the server's identity.
    pub signature: String,
}

/// What the Hub answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DataTraded {
    /// The number of the last change in `changes` (or `since` if none):
    /// send it as the next trade's `since`.
    pub seq: i64,
    /// Keys changed after `since`, oldest first.
    pub changes: Vec<DataChange>,
    /// More changes are waiting: trade again soon.
    #[serde(default)]
    pub more: bool,
}

/// A fingerprint of `changes`, so the signature covers them without
/// signing megabytes.
pub fn changes_hash(changes: &[DataChange]) -> String {
    let text = serde_json::to_string(changes).unwrap_or_default();
    data_encoding::HEXLOWER.encode(&Sha256::digest(text.as_bytes()))
}

pub fn data_message(
    server_id: &str,
    game_id: &str,
    time: i64,
    since: i64,
    changes: &[DataChange],
) -> Vec<u8> {
    format!("playlibre data\n{server_id}\n{game_id}\n{time}\n{since}\n{}", changes_hash(changes)).into_bytes()
}

/// Whether `server_id` may keep `game_id`'s saved data.
pub(crate) fn trusted(db: &Connection, server_id: &str, game_id: &str) -> bool {
    if fleet::runs(db, server_id, game_id) || owner::is_owner(db, server_id) {
        return true;
    }
    if game_id.starts_with("starter:") {
        return false;
    }
    if crate::groups::may_edit(db, game_id, server_id) {
        return true;
    }
    db.query_row(
        "SELECT 1 FROM games WHERE game_id = ?1 AND author_id = ?2",
        params![game_id, server_id],
        |_| Ok(()),
    )
    .optional()
    .ok()
    .flatten()
    .is_some()
}

fn bad(why: impl Into<String>) -> Failure {
    (StatusCode::BAD_REQUEST, why.into())
}

/// Puts `changes` in and gives back what changed after `since`.
pub fn trade(
    db: &Connection,
    game_id: &str,
    since: i64,
    changes: &[DataChange],
) -> Result<DataTraded, Failure> {
    let mut size = 0usize;
    for c in changes {
        if c.store.is_empty() || c.store.len() > MAX_NAME || c.key.is_empty() || c.key.len() > MAX_NAME {
            return Err(bad(format!("store and key names are 1 to {MAX_NAME} bytes")));
        }
        let len = c.value.as_ref().map_or(0, |v| serde_json::to_string(v).map_or(0, |s| s.len()));
        if len > MAX_VALUE {
            return Err(bad(format!("a saved value can be at most {MAX_VALUE} bytes")));
        }
        size += len + c.store.len() + c.key.len();
    }
    if size > MAX_TRADE {
        return Err(bad("that is too much at once"));
    }
    let used: i64 = db
        .query_row("SELECT COALESCE(SUM(size), 0) FROM game_data WHERE game_id = ?1", params![game_id], |r| {
            r.get(0)
        })
        .map_err(internal)?;
    if used + size as i64 > MAX_GAME_DATA {
        return Err((StatusCode::INSUFFICIENT_STORAGE, "this game has saved as much data as it can".into()));
    }
    let mut seq: i64 = db
        .query_row("SELECT COALESCE(MAX(seq), 0) FROM game_data WHERE game_id = ?1", params![game_id], |r| {
            r.get(0)
        })
        .map_err(internal)?;
    let first_new = seq + 1;
    for c in changes {
        seq += 1;
        let value = c.value.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default());
        let len = value.as_ref().map_or(0, String::len);
        db.execute(
            "INSERT INTO game_data (game_id, store, key, value, size, seq) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(game_id, store, key) DO UPDATE SET value = ?4, size = ?5, seq = ?6",
            params![game_id, c.store, c.key, value, len as i64, seq],
        )
        .map_err(internal)?;
    }
    // What other servers changed, leaving out what this trade just put in
    // (the server has it already).
    let mut statement = db
        .prepare(
            "SELECT store, key, value, seq FROM game_data
             WHERE game_id = ?1 AND seq > ?2 AND seq < ?3 ORDER BY seq LIMIT ?4",
        )
        .map_err(internal)?;
    let rows: Vec<(String, String, Option<String>, i64)> = statement
        .query_map(params![game_id, since, first_new, MAX_CHANGES_BACK as i64 + 1], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .map_err(internal)?
        .filter_map(Result::ok)
        .collect();
    let more = rows.len() > MAX_CHANGES_BACK;
    let rows = &rows[..rows.len().min(MAX_CHANGES_BACK)];
    let back: Vec<DataChange> = rows
        .iter()
        .map(|(store, key, value, _)| DataChange {
            store: store.clone(),
            key: key.clone(),
            value: value.as_deref().and_then(|v| serde_json::from_str(v).ok()),
        })
        .collect();
    // Caught up: the server has everything up to the newest number. Not
    // caught up: only up to the last change handed back.
    let reached = if more { rows.last().map_or(since, |r| r.3) } else { seq.max(since) };
    Ok(DataTraded { seq: reached, changes: back, more })
}

pub(crate) async fn trade_data(
    State(db): State<Db>,
    UrlPath(game_id): UrlPath<String>,
    Json(t): Json<DataTrade>,
) -> Result<Json<DataTraded>, Failure> {
    if !verify(&t.server_id, &data_message(&t.server_id, &game_id, t.time, t.since, &t.changes), &t.signature)
    {
        return Err((StatusCode::FORBIDDEN, "the signature does not match".into()));
    }
    let db = db.lock().unwrap();
    if !trusted(&db, &t.server_id, &game_id) {
        return Err((StatusCode::FORBIDDEN, "only the game's own servers keep its saved data".into()));
    }
    fresh(&db, &t.server_id, &format!("data {game_id}"), t.time)?;
    trade(&db, &game_id, t.since, &t.changes).map(Json)
}

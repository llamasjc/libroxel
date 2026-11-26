//! Games with more than one place, and servers of their own.
//!
//! A game's start place is the place it was published with; its creator
//! can add more (a lobby and its levels, say), each with a number of its
//! own, `game.PlaceId`. `TeleportService` sends players from one place to
//! another, and to reserved servers that only the players sent there join.
//!
//! A private server is a server of a game that its owner and their friends
//! play on, away from everyone else: Play on it finds (or starts) that
//! server and only lets them in.

use axum::Json;
use axum::extract::{Extension, Path as UrlPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::limits::{Caller, Limit, Limits, too_many};
use crate::social::{Failure, fresh, internal};
use crate::{Db, MAX_PLACE, fleet, hidden_sql, manage, now, verify};

/// The most places a game can have, its start place included.
pub const MAX_PLACES: usize = 20;
/// The most private servers a player can have.
pub const MAX_PRIVATE_SERVERS: i64 = 10;
/// A place's or private server's name is at most this long.
const MAX_NAME: usize = 50;

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS places (
            place_id    INTEGER PRIMARY KEY,
            game_id     TEXT NOT NULL,
            name        TEXT NOT NULL,
            place       TEXT,
            version     INTEGER NOT NULL,
            updated     INTEGER NOT NULL,
            start       INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS places_game ON places(game_id);
        CREATE TABLE IF NOT EXISTS private_servers (
            code        TEXT PRIMARY KEY,
            game_id     TEXT NOT NULL,
            owner_id    TEXT NOT NULL,
            name        TEXT NOT NULL,
            created     INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS private_servers_owner ON private_servers(owner_id);
        -- Every game has its start place: the place it was published with.
        INSERT INTO places (game_id, name, place, version, updated, start)
            SELECT g.game_id, 'Start', NULL, 1, g.created, 1 FROM games g
            WHERE NOT EXISTS (SELECT 1 FROM places p WHERE p.game_id = g.game_id AND p.start = 1);
        CREATE TRIGGER IF NOT EXISTS games_start_place AFTER INSERT ON games
            WHEN NOT EXISTS (SELECT 1 FROM places p WHERE p.game_id = NEW.game_id AND p.start = 1)
        BEGIN
            INSERT INTO places (game_id, name, place, version, updated, start)
                VALUES (NEW.game_id, 'Start', NULL, 1, NEW.created, 1);
        END;",
    )
    .map_err(|e| format!("could not set up places: {e}"))
}

/// One of a game's places.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceInfo {
    /// `game.PlaceId` in its scripts.
    pub place_id: i64,
    pub game_id: String,
    pub name: String,
    /// The place the game starts in (the one it was published with).
    pub start: bool,
    pub version: u32,
    pub size: u64,
}

fn place_row(r: &rusqlite::Row) -> rusqlite::Result<PlaceInfo> {
    Ok(PlaceInfo {
        place_id: r.get(0)?,
        game_id: r.get(1)?,
        name: r.get(2)?,
        start: r.get(3)?,
        version: r.get::<_, i64>(4)? as u32,
        size: r.get::<_, i64>(5)? as u64,
    })
}

const PLACE_COLUMNS: &str =
    "p.place_id, p.game_id, p.name, p.start, CASE WHEN p.start = 1 THEN g.version ELSE p.version END,
    LENGTH(COALESCE(p.place, g.place))";

pub fn places(db: &Connection, game_id: &str) -> Vec<PlaceInfo> {
    db.prepare(&format!(
        "SELECT {PLACE_COLUMNS} FROM places p JOIN games g ON g.game_id = p.game_id
         WHERE p.game_id = ?1 ORDER BY p.start DESC, p.place_id"
    ))
    .and_then(|mut s| {
        s.query_map(params![game_id], place_row).map(|rows| rows.filter_map(Result::ok).collect())
    })
    .unwrap_or_default()
}

pub fn place(db: &Connection, place_id: i64) -> Option<PlaceInfo> {
    db.query_row(
        &format!(
            "SELECT {PLACE_COLUMNS} FROM places p JOIN games g ON g.game_id = p.game_id WHERE p.place_id = ?1"
        ),
        params![place_id],
        place_row,
    )
    .optional()
    .ok()
    .flatten()
}

/// The start place's number of a game.
pub fn start_place(db: &Connection, game_id: &str) -> Option<i64> {
    db.query_row("SELECT place_id FROM places WHERE game_id = ?1 AND start = 1", params![game_id], |r| {
        r.get(0)
    })
    .optional()
    .ok()
    .flatten()
}

/// What a creator sends to `POST /games/{id}/places`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaceChange {
    pub player_id: String,
    pub time: i64,
    /// The place to change; 0 adds a new one.
    #[serde(default)]
    pub place_id: i64,
    #[serde(default)]
    pub name: String,
    /// The saved place; `None` removes the place (or, for the start place,
    /// only renames it).
    #[serde(default)]
    pub place: Option<String>,
    pub signature: String,
}

pub fn place_message(player_id: &str, game_id: &str, c: &PlaceChange) -> Vec<u8> {
    let what = c.place.as_ref().map_or_else(
        || "remove".to_owned(),
        |p| data_encoding::HEXLOWER.encode(&Sha256::digest(p.as_bytes())),
    );
    format!("playlibre place\n{player_id}\n{game_id}\n{}\n{}\n{}\n{what}", c.time, c.place_id, c.name)
        .into_bytes()
}

fn bad(why: impl Into<String>) -> Failure {
    (StatusCode::BAD_REQUEST, why.into())
}

pub(crate) async fn change_place(
    State(db): State<Db>,
    UrlPath(game_id): UrlPath<String>,
    Json(c): Json<PlaceChange>,
) -> Result<Json<Vec<PlaceInfo>>, Failure> {
    if !verify(&c.player_id, &place_message(&c.player_id, &game_id, &c), &c.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match".into()));
    }
    let name = c.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(bad(format!("a place needs a name of up to {MAX_NAME} characters")));
    }
    if crate::filter::clean(name).as_deref() != Some(name) {
        return Err(bad("pick a friendlier name"));
    }
    if c.place.as_ref().is_some_and(|p| p.len() > MAX_PLACE) {
        return Err(bad("that place is too big to publish here"));
    }
    let db = db.lock().unwrap();
    manage::owned(&db, &game_id, &c.player_id)?;
    fresh(&db, &c.player_id, &format!("place {game_id}"), c.time)?;
    let now = now();
    if c.place_id == 0 {
        let Some(text) = &c.place else { return Err(bad("a new place needs its saved place")) };
        if places(&db, &game_id).len() >= MAX_PLACES {
            return Err(bad(format!("a game can have up to {MAX_PLACES} places")));
        }
        db.execute(
            "INSERT INTO places (game_id, name, place, version, updated, start) VALUES (?1, ?2, ?3, 1, ?4, 0)",
            params![game_id, name, text, now],
        )
        .map_err(internal)?;
    } else {
        let found = place(&db, c.place_id).filter(|p| p.game_id == game_id);
        let Some(found) = found else {
            return Err((StatusCode::NOT_FOUND, "that game has no such place".into()));
        };
        match (&c.place, found.start) {
            (_, true) => {
                // The start place changes by publishing the game.
                db.execute("UPDATE places SET name = ?2 WHERE place_id = ?1", params![c.place_id, name])
            }
            (Some(text), false) => db.execute(
                "UPDATE places SET name = ?2, place = ?3, version = version + 1, updated = ?4 WHERE place_id = ?1",
                params![c.place_id, name, text, now],
            ),
            (None, false) => db.execute("DELETE FROM places WHERE place_id = ?1", params![c.place_id]),
        }
        .map_err(internal)?;
    }
    Ok(Json(places(&db, &game_id)))
}

pub(crate) async fn list_places(
    State(db): State<Db>,
    UrlPath(game_id): UrlPath<String>,
) -> Result<Json<Vec<PlaceInfo>>, Failure> {
    let db = db.lock().unwrap();
    let listed = db
        .query_row(
            &format!("SELECT 1 FROM games WHERE game_id = ?1 AND NOT {}", hidden_sql("game", "game_id")),
            params![game_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(internal)?;
    if listed.is_none() {
        return Err((StatusCode::NOT_FOUND, "there is no published game with that id".into()));
    }
    Ok(Json(places(&db, &game_id)))
}

pub(crate) async fn place_info(
    State(db): State<Db>,
    UrlPath(place_id): UrlPath<i64>,
) -> Result<Json<PlaceInfo>, Failure> {
    place(&db.lock().unwrap(), place_id)
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "there is no place with that number".into()))
}

/// What a download of a place other than the start place is signed for
/// (see [`crate::DownloadProof`]).
pub fn place_download_id(place_id: i64) -> String {
    format!("place:{place_id}")
}

/// A place's saved place, for the game's creator and the Hub's servers.
pub(crate) async fn download_place(
    State(db): State<Db>,
    State(limits): State<std::sync::Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    UrlPath(place_id): UrlPath<i64>,
    headers: HeaderMap,
) -> Result<String, Failure> {
    let db = db.lock().unwrap();
    let found: Option<(String, String, Option<String>)> = db
        .query_row(
            &format!(
                "SELECT p.game_id, COALESCE(p.place, g.place), g.author_id FROM places p
                 JOIN games g ON g.game_id = p.game_id
                 WHERE p.place_id = ?1 AND g.private = 0 AND NOT {}",
                hidden_sql("game", "g.game_id")
            ),
            params![place_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(internal)?;
    let Some((game_id, text, author_id)) = found else {
        return Err((StatusCode::NOT_FOUND, "there is no place with that number".into()));
    };
    let trusted = crate::download_signer(&headers, &place_download_id(place_id)).is_some_and(|signer| {
        author_id.as_deref() == Some(signer.as_str()) || fleet::runs(&db, &signer, &game_id)
    });
    if !trusted {
        if !limits.take_logged(Limit::Download, &caller.0) {
            return Err(too_many("games downloaded"));
        }
        return Err((
            StatusCode::FORBIDDEN,
            "only its creator and the Hub's servers get a game's places".into(),
        ));
    }
    Ok(text)
}

// ---------------------------------------------------------------------------
// Private servers
// ---------------------------------------------------------------------------

/// A private server: its owner and their friends play on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivateServer {
    /// What Play sends to join it (see [`crate::PlayRequest::access_code`]).
    pub code: String,
    pub game_id: String,
    pub game: String,
    pub owner_id: String,
    pub owner: String,
    pub name: String,
    /// Players on it now.
    #[serde(default)]
    pub players: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrivateAction {
    Create,
    Remove,
}

/// What a player sends to `POST /private-servers`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivateServerChange {
    pub player_id: String,
    pub action: PrivateAction,
    /// The game, to make one.
    #[serde(default)]
    pub game_id: String,
    #[serde(default)]
    pub name: String,
    /// The server, to remove it.
    #[serde(default)]
    pub code: String,
    pub time: i64,
    pub signature: String,
}

pub fn private_server_message(c: &PrivateServerChange) -> Vec<u8> {
    let action = match c.action {
        PrivateAction::Create => "create",
        PrivateAction::Remove => "remove",
    };
    format!(
        "playlibre private server\n{}\n{action}\n{}\n{}\n{}\n{}",
        c.player_id, c.game_id, c.name, c.code, c.time
    )
    .into_bytes()
}

pub fn private_servers_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("playlibre private servers\n{player_id}\n{time}").into_bytes()
}

fn new_code() -> String {
    let mut bytes = [0u8; 15];
    getrandom::fill(&mut bytes).expect("the system has a random number source");
    data_encoding::BASE32_NOPAD.encode(&bytes).to_lowercase()
}

/// Whether two players are friends.
fn friends(db: &Connection, a: &str, b: &str) -> bool {
    db.query_row(
        "SELECT 1 FROM friends WHERE accepted = 1 AND ((from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1))",
        params![a, b],
        |_| Ok(()),
    )
    .optional()
    .ok()
    .flatten()
    .is_some()
}

/// The owner of a private server, if `code` names one.
pub(crate) fn private_owner(db: &Connection, code: &str) -> Option<(String, String)> {
    db.query_row("SELECT game_id, owner_id FROM private_servers WHERE code = ?1", params![code], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })
    .optional()
    .ok()
    .flatten()
}

/// Whether `player_id` (who proved who they are, or `None`) may play on
/// the server `code`: anyone with a reserved server's code, but only the
/// owner and their friends on a private server.
pub(crate) fn may_join(db: &Connection, code: &str, player_id: Option<&str>) -> bool {
    match private_owner(db, code) {
        None => true,
        Some((_, owner)) => {
            player_id.is_some_and(|me| me == owner || friends(db, me, &owner))
                && !crate::owner::is_banned(db, &owner)
        }
    }
}

fn private_list(db: &Connection, player_id: &str) -> Vec<PrivateServer> {
    db.prepare(&format!(
        "SELECT ps.code, ps.game_id, g.title, ps.owner_id, COALESCE(a.name, 'Someone'), ps.name,
             (SELECT COALESCE(SUM(s.players), 0) FROM fleet f JOIN servers s ON s.server_id = f.server_id
              WHERE f.game_id LIKE ps.game_id || '%@' || ps.code)
         FROM private_servers ps JOIN games g ON g.game_id = ps.game_id
         LEFT JOIN accounts a ON a.player_id = ps.owner_id
         WHERE (ps.owner_id = ?1 OR EXISTS (SELECT 1 FROM friends f WHERE f.accepted = 1
                AND ((f.from_id = ?1 AND f.to_id = ps.owner_id) OR (f.to_id = ?1 AND f.from_id = ps.owner_id))))
           AND NOT {}
         ORDER BY ps.owner_id = ?1 DESC, ps.created DESC LIMIT 100",
        hidden_sql("game", "g.game_id")
    ))
    .and_then(|mut s| {
        s.query_map(params![player_id], |r| {
            Ok(PrivateServer {
                code: r.get(0)?,
                game_id: r.get(1)?,
                game: r.get(2)?,
                owner_id: r.get(3)?,
                owner: r.get(4)?,
                name: r.get(5)?,
                players: r.get::<_, i64>(6)? as u32,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
    })
    .unwrap_or_default()
}

pub(crate) async fn change_private_server(
    State(db): State<Db>,
    Json(c): Json<PrivateServerChange>,
) -> Result<Json<Vec<PrivateServer>>, Failure> {
    if !verify(&c.player_id, &private_server_message(&c), &c.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match".into()));
    }
    let db = db.lock().unwrap();
    fresh(&db, &c.player_id, "private server", c.time)?;
    match c.action {
        PrivateAction::Create => {
            let name = c.name.trim();
            if name.is_empty() || name.chars().count() > MAX_NAME {
                return Err(bad(format!("a private server needs a name of up to {MAX_NAME} characters")));
            }
            if crate::filter::clean(name).as_deref() != Some(name) {
                return Err(bad("pick a friendlier name"));
            }
            let playable = db
                .query_row(
                    &format!(
                        "SELECT 1 FROM games WHERE game_id = ?1 AND private = 0 AND NOT {}",
                        hidden_sql("game", "game_id")
                    ),
                    params![c.game_id],
                    |_| Ok(()),
                )
                .optional()
                .map_err(internal)?;
            if playable.is_none() {
                return Err((StatusCode::NOT_FOUND, "there is no published game with that id".into()));
            }
            let signed_up = db
                .query_row("SELECT 1 FROM accounts WHERE player_id = ?1", params![c.player_id], |_| Ok(()))
                .optional()
                .map_err(internal)?;
            if signed_up.is_none() {
                return Err((StatusCode::FORBIDDEN, "make an account to have private servers".into()));
            }
            let count: i64 = db
                .query_row(
                    "SELECT COUNT(*) FROM private_servers WHERE owner_id = ?1",
                    params![c.player_id],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            if count >= MAX_PRIVATE_SERVERS {
                return Err(bad(format!("you can have up to {MAX_PRIVATE_SERVERS} private servers")));
            }
            db.execute(
                "INSERT INTO private_servers (code, game_id, owner_id, name, created) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![new_code(), c.game_id, c.player_id, name, now()],
            )
            .map_err(internal)?;
        }
        PrivateAction::Remove => {
            let removed = db
                .execute(
                    "DELETE FROM private_servers WHERE code = ?1 AND owner_id = ?2",
                    params![c.code, c.player_id],
                )
                .map_err(internal)?;
            if removed == 0 {
                return Err((StatusCode::NOT_FOUND, "you have no private server like that".into()));
            }
        }
    }
    Ok(Json(private_list(&db, &c.player_id)))
}

#[derive(Debug, Deserialize)]
pub(crate) struct SignedQuery {
    time: i64,
    signature: String,
}

pub(crate) async fn list_private_servers(
    State(db): State<Db>,
    UrlPath(player_id): UrlPath<String>,
    Query(q): Query<SignedQuery>,
) -> Result<Json<Vec<PrivateServer>>, Failure> {
    if !verify(&player_id, &private_servers_message(&player_id, q.time), &q.signature) {
        return Err((StatusCode::FORBIDDEN, "only you can see your private servers".into()));
    }
    if (q.time - crate::social::now_ms()).abs() > 10 * 60 * 1000 {
        return Err(bad("this computer's clock is too far off"));
    }
    Ok(Json(private_list(&db.lock().unwrap(), &player_id)))
}

/// Forgets a removed game's extra places and private servers.
pub(crate) fn forget(db: &Connection, game_id: &str) {
    let _ = db.execute("DELETE FROM places WHERE game_id = ?1", params![game_id]);
    let _ = db.execute("DELETE FROM private_servers WHERE game_id = ?1", params![game_id]);
}

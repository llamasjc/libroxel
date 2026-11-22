//! Friends and presence: who is friends with whom, who is online, and which
//! server each of them is in.
//!
//! Everything a player changes is signed with their identity key, so no one
//! can send requests, accept them or say where someone is on their behalf.
//! A player's friends list (with where their friends are) is only given to
//! that player, again checked by a signature.

use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{Db, EXPIRES_AFTER_SECS, Person, SERVER_COLUMNS, ServerInfo, clip, now, server_row, verify};

/// How far a signed time may be from the Hub's clock, in milliseconds.
const CLOCK_SLACK_MS: i64 = 24 * 60 * 60 * 1000;

/// How old a signed request to read a friends list may be, in milliseconds.
const LIST_SLACK_MS: i64 = 10 * 60 * 1000;

/// The most friends (and requests) one player keeps.
pub const MAX_FRIENDS: i64 = 200;

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// Sending, accepting, declining or ending a friendship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FriendAction {
    /// Sends a request, or accepts the one they sent.
    Add,
    /// Unfriends, declines their request or takes back yours.
    Remove,
}

impl FriendAction {
    fn word(self) -> &'static str {
        match self {
            FriendAction::Add => "add",
            FriendAction::Remove => "remove",
        }
    }
}

/// A signed change to a friendship between `player_id` and `other_id`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FriendChange {
    pub player_id: String,
    pub other_id: String,
    pub action: FriendAction,
    /// When it was signed, in milliseconds since 1970. Each change a player
    /// makes must be newer than their last one.
    pub time: i64,
    /// [`friend_message`] signed with the player's identity key.
    pub signature: String,
}

/// What a player signs to change a friendship.
pub fn friend_message(player_id: &str, other_id: &str, action: FriendAction, time: i64) -> Vec<u8> {
    format!("libroxel friend\n{player_id}\n{other_id}\n{}\n{time}", action.word()).into_bytes()
}

/// Where two players stand after a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FriendState {
    /// A request is waiting for the other player.
    Requested,
    Friends,
    /// Neither friends nor asked.
    None,
}

/// What a player signs to read their own friends list.
pub fn friends_list_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel friends\n{player_id}\n{time}").into_bytes()
}

/// Telling the Hub the player is online, and what they are playing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PresenceUpdate {
    pub player_id: String,
    /// `false` signs off at once rather than waiting to be missed.
    pub online: bool,
    /// The game they are in, if any, by title.
    pub game: Option<String>,
    /// That game's id (see [`crate::Heartbeat::game_id`]).
    pub game_id: Option<String>,
    /// The listed server they are in, so friends can join them.
    pub server_id: Option<String>,
    /// Milliseconds since 1970; each update must be newer than the last.
    pub time: i64,
    /// [`presence_message`] signed with the player's identity key.
    pub signature: String,
}

/// What a player signs to say where they are.
pub fn presence_message(update: &PresenceUpdate) -> Vec<u8> {
    let or_none = |s: &Option<String>| s.clone().unwrap_or_default();
    format!(
        "libroxel presence\n{}\n{}\n{}\n{}\n{}\n{}",
        update.player_id,
        update.online,
        or_none(&update.game),
        or_none(&update.game_id),
        or_none(&update.server_id),
        update.time
    )
    .into_bytes()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebPresence {
    pub player_id: String,
    pub time: i64,
    pub signature: String,
}

pub fn web_presence_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel web\n{player_id}\n{time}").into_bytes()
}

/// One friend, as their friends see them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Friend {
    pub player_id: String,
    /// Their account number (see [`crate::Profile::number`]).
    #[serde(default)]
    pub number: u64,
    pub name: String,
    /// How their avatar looks, as the app saves it.
    pub look: serde_json::Value,
    pub online: bool,
    /// The game they are in, if they are in one.
    pub game: Option<String>,
    pub game_id: Option<String>,
    /// The server they are in, while it is listed: join it with its code.
    pub server: Option<ServerInfo>,
    /// Seconds since they were last online, if they ever were.
    pub last_online_secs_ago: Option<i64>,
}

/// A player's friends and their waiting requests.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Friends {
    /// Friends in a game first, then online, then by name.
    pub friends: Vec<Friend>,
    /// People asking to be friends.
    pub incoming: Vec<Person>,
    /// People this player asked.
    pub outgoing: Vec<Person>,
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS friends (
            from_id     TEXT NOT NULL,
            to_id       TEXT NOT NULL,
            accepted    INTEGER NOT NULL,
            since       INTEGER NOT NULL,
            PRIMARY KEY (from_id, to_id)
        );
        CREATE INDEX IF NOT EXISTS friends_to ON friends(to_id);
        CREATE TABLE IF NOT EXISTS presence (
            player_id   TEXT PRIMARY KEY,
            online      INTEGER NOT NULL,
            game        TEXT,
            game_id     TEXT,
            server_id   TEXT,
            last_seen   INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS web_presence (
            player_id   TEXT PRIMARY KEY,
            last_seen   INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS signed (
            player_id   TEXT NOT NULL,
            kind        TEXT NOT NULL,
            time        INTEGER NOT NULL,
            PRIMARY KEY (player_id, kind)
        );",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))
}

type Failure = (StatusCode, String);

fn internal(e: rusqlite::Error) -> Failure {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Checks a signed time is near now and newer than the player's last one
/// of this `kind`, and remembers it.
pub(crate) fn fresh(db: &Connection, player_id: &str, kind: &str, time: i64) -> Result<(), Failure> {
    if (time - now_ms()).abs() > CLOCK_SLACK_MS {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    let last: Option<i64> = db
        .query_row(
            "SELECT time FROM signed WHERE player_id = ?1 AND kind = ?2",
            params![player_id, kind],
            |r| r.get(0),
        )
        .optional()
        .map_err(internal)?;
    if last.is_some_and(|last| time <= last) {
        return Err((StatusCode::CONFLICT, "a newer change is already here".into()));
    }
    db.execute(
        "INSERT INTO signed (player_id, kind, time) VALUES (?1, ?2, ?3)
         ON CONFLICT(player_id, kind) DO UPDATE SET time = ?3",
        params![player_id, kind, time],
    )
    .map_err(internal)?;
    Ok(())
}

fn account_name(db: &Connection, player_id: &str) -> Result<Option<String>, Failure> {
    db.query_row("SELECT name FROM accounts WHERE player_id = ?1", params![player_id], |r| r.get(0))
        .optional()
        .map_err(internal)
}

/// Where `a` and `b` stand: (a asked b, b asked a, friends).
fn standing(db: &Connection, a: &str, b: &str) -> Result<(bool, bool, bool), Failure> {
    let row = |from: &str, to: &str| -> Result<Option<bool>, Failure> {
        db.query_row(
            "SELECT accepted FROM friends WHERE from_id = ?1 AND to_id = ?2",
            params![from, to],
            |r| r.get::<_, i64>(0).map(|a| a != 0),
        )
        .optional()
        .map_err(internal)
    };
    let (ab, ba) = (row(a, b)?, row(b, a)?);
    let friends = ab == Some(true) || ba == Some(true);
    Ok((ab == Some(false), ba == Some(false), friends))
}

pub(crate) async fn change_friend(
    State(db): State<Db>,
    Json(change): Json<FriendChange>,
) -> Result<Json<FriendState>, Failure> {
    let message = friend_message(&change.player_id, &change.other_id, change.action, change.time);
    if !verify(&change.player_id, &message, &change.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    if change.player_id == change.other_id {
        return Err((StatusCode::BAD_REQUEST, "you can't be friends with yourself".into()));
    }
    let db = db.lock().unwrap();
    if crate::owner::is_banned(&db, &change.player_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from Libroxel".into()));
    }
    let (me, them) = (change.player_id.as_str(), change.other_id.as_str());
    match change.action {
        FriendAction::Add => {
            if account_name(&db, me)?.is_none() {
                return Err((StatusCode::BAD_REQUEST, "claim a name on the Avatar tab first".into()));
            }
            if account_name(&db, them)?.is_none() {
                return Err((StatusCode::NOT_FOUND, "there is nobody with that id".into()));
            }
            fresh(&db, me, "friends", change.time)?;
            let (_, they_asked, friends) = standing(&db, me, them)?;
            if friends {
                return Ok(Json(FriendState::Friends));
            }
            if they_asked {
                db.execute(
                    "UPDATE friends SET accepted = 1, since = ?3 WHERE from_id = ?1 AND to_id = ?2",
                    params![them, me, now()],
                )
                .map_err(internal)?;
                return Ok(Json(FriendState::Friends));
            }
            let count: i64 = db
                .query_row(
                    "SELECT COUNT(*) FROM friends WHERE from_id = ?1 OR to_id = ?1",
                    params![me],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            if count >= MAX_FRIENDS {
                return Err((StatusCode::BAD_REQUEST, format!("you can have up to {MAX_FRIENDS} friends")));
            }
            db.execute(
                "INSERT OR IGNORE INTO friends (from_id, to_id, accepted, since) VALUES (?1, ?2, 0, ?3)",
                params![me, them, now()],
            )
            .map_err(internal)?;
            Ok(Json(FriendState::Requested))
        }
        FriendAction::Remove => {
            fresh(&db, me, "friends", change.time)?;
            db.execute(
                "DELETE FROM friends WHERE (from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1)",
                params![me, them],
            )
            .map_err(internal)?;
            Ok(Json(FriendState::None))
        }
    }
}

pub(crate) async fn update_presence(
    State(db): State<Db>,
    Json(update): Json<PresenceUpdate>,
) -> Result<StatusCode, Failure> {
    if !verify(&update.player_id, &presence_message(&update), &update.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    let db = db.lock().unwrap();
    if crate::owner::is_banned(&db, &update.player_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from Libroxel".into()));
    }
    fresh(&db, &update.player_id, "presence", update.time)?;
    let text = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(clip);
    db.execute(
        "INSERT INTO presence (player_id, online, game, game_id, server_id, last_seen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(player_id) DO UPDATE SET online = ?2, game = ?3, game_id = ?4, server_id = ?5, last_seen = ?6",
        params![
            update.player_id,
            update.online,
            text(&update.game),
            text(&update.game_id),
            text(&update.server_id),
            now()
        ],
    )
    .map_err(internal)?;
    if let Some(game_id) = text(&update.game_id).filter(|_| update.online) {
        let _ = crate::analytics::record(&db, &game_id, &update.player_id, now());
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn update_web_presence(
    State(db): State<Db>,
    Json(update): Json<WebPresence>,
) -> Result<StatusCode, Failure> {
    if !verify(&update.player_id, &web_presence_message(&update.player_id, update.time), &update.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    let db = db.lock().unwrap();
    if crate::owner::is_banned(&db, &update.player_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from Libroxel".into()));
    }
    fresh(&db, &update.player_id, "web", update.time)?;
    db.execute(
        "INSERT INTO web_presence (player_id, last_seen) VALUES (?1, ?2)
         ON CONFLICT(player_id) DO UPDATE SET last_seen = ?2",
        params![update.player_id, now()],
    )
    .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListSigned {
    time: i64,
    signature: String,
}

pub(crate) async fn list_friends(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Query(signed): Query<ListSigned>,
) -> Result<Json<Friends>, Failure> {
    if !verify(&id, &friends_list_message(&id, signed.time), &signed.signature) {
        return Err((StatusCode::FORBIDDEN, "only you can see your friends list".into()));
    }
    if (signed.time - now_ms()).abs() > LIST_SLACK_MS {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    let db = db.lock().unwrap();
    let now = now();
    let alive = now - EXPIRES_AFTER_SECS;
    let people = |sql: &str| -> Result<Vec<Person>, Failure> {
        let mut statement = db.prepare(sql).map_err(internal)?;
        statement
            .query_map(params![id], |r| {
                Ok(Person {
                    player_id: r.get(0)?,
                    name: r.get(1)?,
                    about: r.get(2)?,
                    number: r.get::<_, i64>(3)? as u64,
                })
            })
            .map(|rows| rows.filter_map(Result::ok).collect())
            .map_err(internal)
    };
    let incoming = people(
        "SELECT a.player_id, a.name, a.about, a.number FROM friends f JOIN accounts a ON a.player_id = f.from_id
         WHERE f.to_id = ?1 AND f.accepted = 0 ORDER BY f.since DESC",
    )?;
    let outgoing = people(
        "SELECT a.player_id, a.name, a.about, a.number FROM friends f JOIN accounts a ON a.player_id = f.to_id
         WHERE f.from_id = ?1 AND f.accepted = 0 ORDER BY f.since DESC",
    )?;
    let mut statement = db
        .prepare(
            "SELECT a.player_id, a.name, a.look, p.online, p.game, p.game_id, p.server_id, p.last_seen, a.number, w.last_seen
             FROM friends f
             JOIN accounts a ON a.player_id = CASE WHEN f.from_id = ?1 THEN f.to_id ELSE f.from_id END
             LEFT JOIN presence p ON p.player_id = a.player_id
             LEFT JOIN web_presence w ON w.player_id = a.player_id
             WHERE (f.from_id = ?1 OR f.to_id = ?1) AND f.accepted = 1",
        )
        .map_err(internal)?;
    type Row = (
        String,
        String,
        String,
        Option<bool>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        i64,
        Option<i64>,
    );
    let rows: Vec<Row> = statement
        .query_map(params![id], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
            ))
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .map_err(internal)?;
    let mut friends = Vec::new();
    for (player_id, name, look, online, game, game_id, server_id, app_seen, number, web_seen) in rows {
        let in_app = online == Some(true) && app_seen.is_some_and(|t| t >= alive);
        let on_web = web_seen.is_some_and(|t| t >= alive);
        let last_seen = match (app_seen, web_seen) {
            (Some(a), Some(w)) => Some(a.max(w)),
            (a, w) => a.or(w),
        };
        let online = in_app || on_web;
        let server = match (&server_id, in_app) {
            (Some(server_id), true) => db
                .query_row(
                    &format!("SELECT {SERVER_COLUMNS} FROM servers WHERE server_id = ?1 AND last_seen >= ?2"),
                    params![server_id, alive],
                    |r| server_row(r, now),
                )
                .optional()
                .map_err(internal)?,
            _ => None,
        };
        friends.push(Friend {
            player_id,
            number: number as u64,
            name,
            look: serde_json::from_str(&look).unwrap_or(serde_json::Value::Null),
            online,
            game: game.filter(|_| in_app),
            game_id: game_id.filter(|_| in_app),
            server,
            last_online_secs_ago: last_seen.map(|t| now - t),
        });
    }
    friends.sort_by(|a, b| {
        let rank = |f: &Friend| (!f.online as u8) * 2 + (f.game.is_none() as u8);
        rank(a).cmp(&rank(b)).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(Json(Friends { friends, incoming, outgoing }))
}

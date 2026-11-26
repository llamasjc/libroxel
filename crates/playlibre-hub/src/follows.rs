//! Following games and creators, and the notifications a player gets:
//! a friend request, a message, a group invite, a badge, a pass sold, a
//! game they follow getting an update, or a creator they follow putting
//! out a new game.
//!
//! Notifications are kept for a while (the newest [`KEEP`] of each player)
//! and only the player can read them, checked by a signature.

use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::social::{Failure, fresh, internal, now_ms};
use crate::{Db, now, verify};

/// Notifications kept for each player.
pub const KEEP: i64 = 200;
/// The most games and creators a player can follow.
pub const MAX_FOLLOWS: i64 = 1000;
/// A signed read is good for this long.
const READ_SLACK_MS: i64 = 10 * 60 * 1000;

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS follows (
            player_id   TEXT NOT NULL,
            kind        TEXT NOT NULL,
            target      TEXT NOT NULL,
            time        INTEGER NOT NULL,
            PRIMARY KEY (player_id, kind, target)
        );
        CREATE INDEX IF NOT EXISTS follows_target ON follows(kind, target);
        CREATE TABLE IF NOT EXISTS notifications (
            notification_id INTEGER PRIMARY KEY,
            player_id   TEXT NOT NULL,
            kind        TEXT NOT NULL,
            text        TEXT NOT NULL,
            link        TEXT NOT NULL,
            about       TEXT NOT NULL DEFAULT '',
            time        INTEGER NOT NULL,
            seen        INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS notifications_player ON notifications(player_id, notification_id);",
    )
    .map_err(|e| format!("could not set up follows and notifications: {e}"))
}

/// What can be followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FollowKind {
    Game,
    Creator,
}

impl FollowKind {
    fn word(self) -> &'static str {
        match self {
            FollowKind::Game => "game",
            FollowKind::Creator => "creator",
        }
    }
}

/// What a player sends to `POST /follows`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowChange {
    pub player_id: String,
    pub kind: FollowKind,
    /// A game's id, or a creator's player id.
    pub target: String,
    /// `false` stops following.
    pub follow: bool,
    pub time: i64,
    pub signature: String,
}

pub fn follow_message(c: &FollowChange) -> Vec<u8> {
    format!(
        "playlibre follow\n{}\n{}\n{}\n{}\n{}",
        c.player_id,
        c.kind.word(),
        c.target,
        u8::from(c.follow),
        c.time
    )
    .into_bytes()
}

/// What a player follows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Following {
    /// Game ids.
    pub games: Vec<String>,
    /// Creators' player ids.
    pub creators: Vec<String>,
}

/// One notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    pub notification_id: i64,
    /// `friend`, `message`, `group`, `badge`, `sale`, `update` or `new_game`.
    pub kind: String,
    pub text: String,
    /// Where it leads on the website, such as `/games/12/sky-tower`.
    pub link: String,
    pub secs_ago: i64,
    pub seen: bool,
}

/// A player's notifications, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Notifications {
    pub unseen: u32,
    pub items: Vec<Notification>,
}

pub fn notifications_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("playlibre notifications\n{player_id}\n{time}").into_bytes()
}

/// Marks every notification up to `up_to` seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationsSeen {
    pub player_id: String,
    pub up_to: i64,
    pub time: i64,
    pub signature: String,
}

pub fn notifications_seen_message(player_id: &str, up_to: i64, time: i64) -> Vec<u8> {
    format!("playlibre notifications seen\n{player_id}\n{up_to}\n{time}").into_bytes()
}

/// Tells `player_id` something. `about` lets a newer one of the same kind
/// and subject replace an unseen older one (so ten messages from one
/// friend are one notification).
pub(crate) fn notify(db: &Connection, player_id: &str, kind: &str, about: &str, text: &str, link: &str) {
    if !about.is_empty() {
        let _ = db.execute(
            "DELETE FROM notifications WHERE player_id = ?1 AND kind = ?2 AND about = ?3 AND seen = 0",
            params![player_id, kind, about],
        );
    }
    let _ = db.execute(
        "INSERT INTO notifications (player_id, kind, text, link, about, time) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![player_id, kind, crate::clip(text), link, about, now()],
    );
    let _ = db.execute(
        "DELETE FROM notifications WHERE player_id = ?1 AND notification_id IN (
             SELECT notification_id FROM notifications WHERE player_id = ?1
             ORDER BY notification_id DESC LIMIT -1 OFFSET ?2)",
        params![player_id, KEEP],
    );
}

/// An account's name, for notification text.
pub(crate) fn name_of(db: &Connection, player_id: &str) -> String {
    db.query_row("SELECT name FROM accounts WHERE player_id = ?1", params![player_id], |r| r.get(0))
        .optional()
        .ok()
        .flatten()
        .unwrap_or_else(|| "Someone".to_owned())
}

/// Where a game's page is on the website.
pub(crate) fn game_link(db: &Connection, game_id: &str, title: &str) -> String {
    let number: Option<i64> = db
        .query_row("SELECT number FROM game_numbers WHERE game_id = ?1", params![game_id], |r| r.get(0))
        .optional()
        .ok()
        .flatten();
    match number {
        Some(n) => format!("/games/{n}/{}", crate::game_slug(title)),
        None => format!("/game/{game_id}"),
    }
}

/// Tells everyone following a game that it was updated, and everyone
/// following its creator about a new game.
pub(crate) fn game_published(
    db: &Connection,
    game_id: &str,
    title: &str,
    author_id: Option<&str>,
    new: bool,
) {
    let private: bool = db
        .query_row("SELECT private FROM games WHERE game_id = ?1", params![game_id], |r| r.get(0))
        .unwrap_or(true);
    if private {
        return;
    }
    let link = game_link(db, game_id, title);
    let (kind, target, text) = match (new, author_id) {
        (true, Some(author)) => {
            ("new_game", author.to_owned(), format!("{} put out a new game: {title}", name_of(db, author)))
        }
        (false, _) => ("update", game_id.to_owned(), format!("{title} was updated")),
        (true, None) => return,
    };
    let follow_kind = if new { "creator" } else { "game" };
    let followers: Vec<String> = db
        .prepare("SELECT player_id FROM follows WHERE kind = ?1 AND target = ?2 LIMIT 100000")
        .and_then(|mut s| {
            s.query_map(params![follow_kind, target], |r| r.get(0))
                .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default();
    for player in followers {
        notify(db, &player, kind, game_id, &text, &link);
    }
}

pub fn following(db: &Connection, player_id: &str) -> Following {
    let list = |kind: &str| -> Vec<String> {
        db.prepare("SELECT target FROM follows WHERE player_id = ?1 AND kind = ?2 ORDER BY time DESC")
            .and_then(|mut s| {
                s.query_map(params![player_id, kind], |r| r.get(0))
                    .map(|rows| rows.filter_map(Result::ok).collect())
            })
            .unwrap_or_default()
    };
    Following { games: list("game"), creators: list("creator") }
}

/// How many players follow a game or a creator.
pub fn followers(db: &Connection, kind: FollowKind, target: &str) -> u32 {
    db.query_row(
        "SELECT COUNT(*) FROM follows WHERE kind = ?1 AND target = ?2",
        params![kind.word(), target],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

fn signed_read(player_id: &str, message: &[u8], signature: &str, time: i64) -> Result<(), Failure> {
    if !verify(player_id, message, signature) {
        return Err((StatusCode::FORBIDDEN, "only you can see that".into()));
    }
    if (time - now_ms()).abs() > READ_SLACK_MS {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    Ok(())
}

pub(crate) async fn change_follow(
    State(db): State<Db>,
    Json(c): Json<FollowChange>,
) -> Result<Json<Following>, Failure> {
    if !verify(&c.player_id, &follow_message(&c), &c.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match".into()));
    }
    let db = db.lock().unwrap();
    fresh(&db, &c.player_id, "follow", c.time)?;
    if c.follow {
        let exists = match c.kind {
            FollowKind::Game => {
                db.query_row("SELECT 1 FROM games WHERE game_id = ?1", params![c.target], |_| Ok(()))
            }
            FollowKind::Creator => {
                db.query_row("SELECT 1 FROM accounts WHERE player_id = ?1", params![c.target], |_| Ok(()))
            }
        }
        .optional()
        .map_err(internal)?
        .is_some();
        if !exists {
            return Err((StatusCode::NOT_FOUND, "there is nothing like that to follow".into()));
        }
        if c.kind == FollowKind::Creator && c.target == c.player_id {
            return Err((StatusCode::BAD_REQUEST, "you can't follow yourself".into()));
        }
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM follows WHERE player_id = ?1", params![c.player_id], |r| {
                r.get(0)
            })
            .map_err(internal)?;
        if count >= MAX_FOLLOWS {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("you can follow up to {MAX_FOLLOWS} games and creators"),
            ));
        }
        db.execute(
            "INSERT OR IGNORE INTO follows (player_id, kind, target, time) VALUES (?1, ?2, ?3, ?4)",
            params![c.player_id, c.kind.word(), c.target, now()],
        )
        .map_err(internal)?;
    } else {
        db.execute(
            "DELETE FROM follows WHERE player_id = ?1 AND kind = ?2 AND target = ?3",
            params![c.player_id, c.kind.word(), c.target],
        )
        .map_err(internal)?;
    }
    Ok(Json(following(&db, &c.player_id)))
}

#[derive(Debug, Deserialize)]
pub(crate) struct SignedQuery {
    time: i64,
    signature: String,
}

pub fn follows_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("playlibre follows\n{player_id}\n{time}").into_bytes()
}

pub(crate) async fn get_following(
    State(db): State<Db>,
    UrlPath(player_id): UrlPath<String>,
    Query(q): Query<SignedQuery>,
) -> Result<Json<Following>, Failure> {
    signed_read(&player_id, &follows_message(&player_id, q.time), &q.signature, q.time)?;
    Ok(Json(following(&db.lock().unwrap(), &player_id)))
}

/// How many follow a game or creator: `GET /followers/{kind}/{target}`.
pub(crate) async fn get_followers(
    State(db): State<Db>,
    UrlPath((kind, target)): UrlPath<(String, String)>,
) -> Result<Json<u32>, Failure> {
    let kind = match kind.as_str() {
        "game" => FollowKind::Game,
        "creator" => FollowKind::Creator,
        _ => return Err((StatusCode::NOT_FOUND, "games and creators can be followed".into())),
    };
    Ok(Json(followers(&db.lock().unwrap(), kind, &target)))
}

pub fn notifications(db: &Connection, player_id: &str) -> Notifications {
    let now = now();
    let items: Vec<Notification> = db
        .prepare(
            "SELECT notification_id, kind, text, link, time, seen FROM notifications WHERE player_id = ?1
             ORDER BY notification_id DESC LIMIT 50",
        )
        .and_then(|mut s| {
            s.query_map(params![player_id], |r| {
                Ok(Notification {
                    notification_id: r.get(0)?,
                    kind: r.get(1)?,
                    text: r.get(2)?,
                    link: r.get(3)?,
                    secs_ago: now - r.get::<_, i64>(4)?,
                    seen: r.get(5)?,
                })
            })
            .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default();
    let unseen = db
        .query_row(
            "SELECT COUNT(*) FROM notifications WHERE player_id = ?1 AND seen = 0",
            params![player_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    Notifications { unseen, items }
}

pub(crate) async fn get_notifications(
    State(db): State<Db>,
    UrlPath(player_id): UrlPath<String>,
    Query(q): Query<SignedQuery>,
) -> Result<Json<Notifications>, Failure> {
    signed_read(&player_id, &notifications_message(&player_id, q.time), &q.signature, q.time)?;
    Ok(Json(notifications(&db.lock().unwrap(), &player_id)))
}

pub(crate) async fn notifications_seen(
    State(db): State<Db>,
    Json(s): Json<NotificationsSeen>,
) -> Result<Json<Notifications>, Failure> {
    if !verify(&s.player_id, &notifications_seen_message(&s.player_id, s.up_to, s.time), &s.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match".into()));
    }
    let db = db.lock().unwrap();
    fresh(&db, &s.player_id, "notifications", s.time)?;
    db.execute(
        "UPDATE notifications SET seen = 1 WHERE player_id = ?1 AND notification_id <= ?2",
        params![s.player_id, s.up_to],
    )
    .map_err(internal)?;
    Ok(Json(notifications(&db, &s.player_id)))
}

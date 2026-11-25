//! Likes, dislikes, favorites and comments on games.
//!
//! A game is a published game's id or `starter:<name>` for an official one.
//! Everything is signed by a player with an account.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{Db, hidden_sql, now, verify};
use crate::limits::{Caller, Limit, Limits, too_many};
use crate::social::now_ms;

/// The longest comment, in characters.
pub const MAX_COMMENT: usize = 300;

const CLOCK_SLACK_MS: i64 = 24 * 60 * 60 * 1000;

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS game_votes (
            game_id     TEXT NOT NULL,
            player_id   TEXT NOT NULL,
            vote        INTEGER NOT NULL,
            time        INTEGER NOT NULL,
            PRIMARY KEY (game_id, player_id)
        );
        CREATE TABLE IF NOT EXISTS game_favorites (
            game_id     TEXT NOT NULL,
            player_id   TEXT NOT NULL,
            time        INTEGER NOT NULL,
            PRIMARY KEY (game_id, player_id)
        );
        CREATE INDEX IF NOT EXISTS game_favorites_player ON game_favorites(player_id);
        CREATE TABLE IF NOT EXISTS game_comments (
            comment_id  INTEGER PRIMARY KEY,
            game_id     TEXT NOT NULL,
            player_id   TEXT NOT NULL,
            text        TEXT NOT NULL,
            time        INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS game_comments_game ON game_comments(game_id, comment_id);
        CREATE TABLE IF NOT EXISTS rating_clock (
            player_id   TEXT PRIMARY KEY,
            time        INTEGER NOT NULL
        );",
    )
    .map_err(|e| format!("could not set up ratings: {e}"))
}

/// A player's like or dislike.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vote {
    Like,
    Dislike,
    #[default]
    None,
}

impl Vote {
    fn word(self) -> &'static str {
        match self {
            Vote::Like => "like",
            Vote::Dislike => "dislike",
            Vote::None => "none",
        }
    }

    fn of(n: i64) -> Vote {
        match n {
            1 => Vote::Like,
            -1 => Vote::Dislike,
            _ => Vote::None,
        }
    }
}

/// A game's likes and favorites, and what one player gave it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ratings {
    pub likes: u32,
    pub dislikes: u32,
    pub favorites: u32,
    #[serde(default)]
    pub vote: Vote,
    #[serde(default)]
    pub favorite: bool,
}

/// A player setting their vote and favorite on a game.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rate {
    pub player_id: String,
    pub vote: Vote,
    pub favorite: bool,
    /// Milliseconds since 1970, newer than the player's last.
    pub time: i64,
    /// [`rate_message`] signed with the player's identity key.
    pub signature: String,
}

pub fn rate_message(player_id: &str, game_id: &str, vote: Vote, favorite: bool, time: i64) -> Vec<u8> {
    format!("libroxel rate\n{player_id}\n{game_id}\n{}\n{favorite}\n{time}", vote.word()).into_bytes()
}

/// A comment as everyone sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comment {
    pub comment_id: i64,
    pub player_id: String,
    pub number: u64,
    pub name: String,
    pub text: String,
    pub secs_ago: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewComment {
    pub player_id: String,
    pub text: String,
    pub time: i64,
    /// [`comment_message`] signed with the player's identity key.
    pub signature: String,
}

pub fn comment_message(player_id: &str, game_id: &str, text: &str, time: i64) -> Vec<u8> {
    format!("libroxel comment\n{player_id}\n{game_id}\n{text}\n{time}").into_bytes()
}

/// Taking a comment down: its writer, or the game's author.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoveComment {
    pub player_id: String,
    pub time: i64,
    /// [`remove_comment_message`] signed with the player's identity key.
    pub signature: String,
}

pub fn remove_comment_message(player_id: &str, comment_id: i64, time: i64) -> Vec<u8> {
    format!("libroxel uncomment\n{player_id}\n{comment_id}\n{time}").into_bytes()
}

#[derive(Debug, Deserialize)]
pub(crate) struct Who {
    player_id: Option<String>,
}

type Failed = (StatusCode, String);

fn internal(e: rusqlite::Error) -> Failed {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn game_exists(db: &Connection, game_id: &str) -> bool {
    let sql = match game_id.strip_prefix("starter:") {
        Some(_) => "SELECT 1 FROM official WHERE 'starter:' || name = ?1",
        None => "SELECT 1 FROM games WHERE game_id = ?1",
    };
    db.query_row(sql, params![game_id], |_| Ok(())).optional().ok().flatten().is_some()
}

/// Checks a signed change: a real account, not banned, with a newer time.
fn check(db: &Connection, player_id: &str, message: &[u8], signature: &str, time: i64) -> Result<(), Failed> {
    if !verify(player_id, message, signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    if (time - now_ms()).abs() > CLOCK_SLACK_MS {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    let has_account = db
        .query_row("SELECT 1 FROM accounts WHERE player_id = ?1", params![player_id], |_| Ok(()))
        .optional()
        .map_err(internal)?
        .is_some();
    if !has_account {
        return Err((StatusCode::FORBIDDEN, "you need an account for that".into()));
    }
    if crate::owner::is_banned(db, player_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from PlayLibre".into()));
    }
    let last: Option<i64> = db
        .query_row("SELECT time FROM rating_clock WHERE player_id = ?1", params![player_id], |r| r.get(0))
        .optional()
        .map_err(internal)?;
    if last.is_some_and(|last| time <= last) {
        return Err((StatusCode::CONFLICT, "that was already sent".into()));
    }
    db.execute(
        "INSERT INTO rating_clock (player_id, time) VALUES (?1, ?2)
         ON CONFLICT(player_id) DO UPDATE SET time = ?2",
        params![player_id, time],
    )
    .map_err(internal)?;
    Ok(())
}

/// SQL for a game's like, dislike and favorite counts, `column` being its id.
pub(crate) fn counts_sql(column: &str) -> String {
    format!(
        "(SELECT COUNT(*) FROM game_votes v WHERE v.game_id = {column} AND v.vote = 1),
         (SELECT COUNT(*) FROM game_votes v WHERE v.game_id = {column} AND v.vote = -1),
         (SELECT COUNT(*) FROM game_favorites f WHERE f.game_id = {column})"
    )
}

fn ratings(db: &Connection, game_id: &str, player_id: Option<&str>) -> rusqlite::Result<Ratings> {
    let (likes, dislikes, favorites): (i64, i64, i64) =
        db.query_row(&format!("SELECT {}", counts_sql("?1")), params![game_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    let mut out = Ratings {
        likes: likes as u32,
        dislikes: dislikes as u32,
        favorites: favorites as u32,
        ..Ratings::default()
    };
    if let Some(player_id) = player_id {
        out.vote = db
            .query_row(
                "SELECT vote FROM game_votes WHERE game_id = ?1 AND player_id = ?2",
                params![game_id, player_id],
                |r| r.get(0),
            )
            .optional()?
            .map_or(Vote::None, Vote::of);
        out.favorite = db
            .query_row(
                "SELECT 1 FROM game_favorites WHERE game_id = ?1 AND player_id = ?2",
                params![game_id, player_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
    }
    Ok(out)
}

pub(crate) async fn get_ratings(
    State(db): State<Db>,
    UrlPath(game_id): UrlPath<String>,
    Query(who): Query<Who>,
) -> Result<Json<Ratings>, Failed> {
    let db = db.lock().unwrap();
    if !game_exists(&db, &game_id) {
        return Err((StatusCode::NOT_FOUND, "there is no game with that id".into()));
    }
    ratings(&db, &game_id, who.player_id.as_deref()).map(Json).map_err(internal)
}

pub(crate) async fn rate(
    State(db): State<Db>,
    UrlPath(game_id): UrlPath<String>,
    Json(rate): Json<Rate>,
) -> Result<Json<Ratings>, Failed> {
    let db = db.lock().unwrap();
    if !game_exists(&db, &game_id) {
        return Err((StatusCode::NOT_FOUND, "there is no game with that id".into()));
    }
    let message = rate_message(&rate.player_id, &game_id, rate.vote, rate.favorite, rate.time);
    check(&db, &rate.player_id, &message, &rate.signature, rate.time)?;
    let now = now();
    match rate.vote {
        Vote::None => db.execute(
            "DELETE FROM game_votes WHERE game_id = ?1 AND player_id = ?2",
            params![game_id, rate.player_id],
        ),
        vote => db.execute(
            "INSERT INTO game_votes (game_id, player_id, vote, time) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(game_id, player_id) DO UPDATE SET vote = ?3, time = ?4 WHERE vote != ?3",
            params![game_id, rate.player_id, if vote == Vote::Like { 1 } else { -1 }, now],
        ),
    }
    .map_err(internal)?;
    if rate.favorite {
        db.execute(
            "INSERT OR IGNORE INTO game_favorites (game_id, player_id, time) VALUES (?1, ?2, ?3)",
            params![game_id, rate.player_id, now],
        )
    } else {
        db.execute(
            "DELETE FROM game_favorites WHERE game_id = ?1 AND player_id = ?2",
            params![game_id, rate.player_id],
        )
    }
    .map_err(internal)?;
    ratings(&db, &game_id, Some(&rate.player_id)).map(Json).map_err(internal)
}

fn comment_row(r: &rusqlite::Row, now: i64) -> rusqlite::Result<Comment> {
    Ok(Comment {
        comment_id: r.get(0)?,
        player_id: r.get(1)?,
        number: r.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64,
        name: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
        text: r.get(4)?,
        secs_ago: now - r.get::<_, i64>(5)?,
    })
}

const COMMENT_COLUMNS: &str = "c.comment_id, c.player_id, a.number, a.name, c.text, c.time";

pub(crate) async fn list_comments(
    State(db): State<Db>,
    UrlPath(game_id): UrlPath<String>,
) -> Json<Vec<Comment>> {
    let db = db.lock().unwrap();
    let now = now();
    let Ok(mut statement) = db.prepare(&format!(
        "SELECT {COMMENT_COLUMNS} FROM game_comments c LEFT JOIN accounts a ON a.player_id = c.player_id
         WHERE c.game_id = ?1 AND NOT {hidden} AND NOT {banned}
         ORDER BY c.comment_id DESC LIMIT 100",
        hidden = hidden_sql("comment", "c.comment_id"),
        banned = crate::owner::banned_sql("c.player_id"),
    )) else {
        return Json(Vec::new());
    };
    Json(
        statement
            .query_map(params![game_id], |r| comment_row(r, now))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default(),
    )
}

pub(crate) async fn add_comment(
    State(db): State<Db>,
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    UrlPath(game_id): UrlPath<String>,
    Json(new): Json<NewComment>,
) -> Result<Json<Comment>, Failed> {
    if !limits.take(Limit::Comment, &caller.0) {
        return Err(too_many("comments"));
    }
    let text = new.text.trim();
    if text.is_empty() || text.chars().count() > MAX_COMMENT {
        return Err((StatusCode::BAD_REQUEST, format!("a comment is 1 to {MAX_COMMENT} characters")));
    }
    let db = db.lock().unwrap();
    if !game_exists(&db, &game_id) {
        return Err((StatusCode::NOT_FOUND, "there is no game with that id".into()));
    }
    check(
        &db,
        &new.player_id,
        &comment_message(&new.player_id, &game_id, &new.text, new.time),
        &new.signature,
        new.time,
    )?;
    crate::moderate::not_muted(&db, &new.player_id)?;
    let Some(text) = crate::filter::clean(text) else {
        return Err((StatusCode::BAD_REQUEST, "that comment isn't allowed".into()));
    };
    let now = now();
    db.execute(
        "INSERT INTO game_comments (game_id, player_id, text, time) VALUES (?1, ?2, ?3, ?4)",
        params![game_id, new.player_id, text, now],
    )
    .map_err(internal)?;
    let id = db.last_insert_rowid();
    db.query_row(
        &format!(
            "SELECT {COMMENT_COLUMNS} FROM game_comments c LEFT JOIN accounts a ON a.player_id = c.player_id
             WHERE c.comment_id = ?1"
        ),
        params![id],
        |r| comment_row(r, now),
    )
    .map(Json)
    .map_err(internal)
}

pub(crate) async fn remove_comment(
    State(db): State<Db>,
    UrlPath(comment_id): UrlPath<i64>,
    Json(remove): Json<RemoveComment>,
) -> Result<StatusCode, Failed> {
    let db = db.lock().unwrap();
    let found: Option<(String, Option<String>)> = db
        .query_row(
            "SELECT c.player_id, g.author_id FROM game_comments c LEFT JOIN games g ON g.game_id = c.game_id
             WHERE c.comment_id = ?1",
            params![comment_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(internal)?;
    let Some((writer, author)) = found else {
        return Err((StatusCode::NOT_FOUND, "there is no comment with that id".into()));
    };
    if remove.player_id != writer && author.as_deref() != Some(remove.player_id.as_str()) {
        return Err((StatusCode::FORBIDDEN, "only its writer or the game's author can take it down".into()));
    }
    let message = remove_comment_message(&remove.player_id, comment_id, remove.time);
    check(&db, &remove.player_id, &message, &remove.signature, remove.time)?;
    db.execute("DELETE FROM game_comments WHERE comment_id = ?1", params![comment_id]).map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn favorites(
    State(db): State<Db>,
    UrlPath(player_id): UrlPath<String>,
) -> Json<Vec<String>> {
    let db = db.lock().unwrap();
    let Ok(mut statement) =
        db.prepare("SELECT game_id FROM game_favorites WHERE player_id = ?1 ORDER BY time DESC LIMIT 200")
    else {
        return Json(Vec::new());
    };
    Json(
        statement
            .query_map(params![player_id], |r| r.get(0))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default(),
    )
}

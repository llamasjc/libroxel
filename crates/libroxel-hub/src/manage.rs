use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::social::now_ms;
use crate::{
    Db, GAME_COLUMNS, GAMES, GameInfo, analytics, clip, clip_to, creator_message, game_row, now, verify,
};

pub const KEEP_VERSIONS: i64 = 10;

pub const MOST_PLAYERS: u32 = 100;

const SIGNED_SLACK_MS: i64 = 10 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Version {
    pub version: u32,
    pub title: String,
    pub size: u64,
    pub published_secs_ago: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Change {
    Settings { title: String, description: String, private: bool, max_players: u32 },
    Restore { version: u32 },
    Remove,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManageGame {
    pub player_id: String,
    pub time: i64,
    pub signature: String,
    pub change: Change,
}

pub fn manage_message(player_id: &str, game_id: &str, time: i64, change: &Change) -> Vec<u8> {
    let what = match change {
        Change::Settings { title, description, private, max_players } => {
            format!("settings\n{}\n{max_players}\n{title}\n{description}", u8::from(*private))
        }
        Change::Restore { version } => format!("restore\n{version}"),
        Change::Remove => "remove".into(),
    };
    format!("libroxel manage\n{player_id}\n{game_id}\n{time}\n{what}").into_bytes()
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    let _ = db.execute("ALTER TABLE games ADD COLUMN private INTEGER NOT NULL DEFAULT 0", []);
    let _ = db.execute("ALTER TABLE games ADD COLUMN max_players INTEGER NOT NULL DEFAULT 0", []);
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS game_versions (
            game_id     TEXT NOT NULL,
            version     INTEGER NOT NULL,
            title       TEXT NOT NULL,
            place       TEXT NOT NULL,
            published   INTEGER NOT NULL,
            PRIMARY KEY (game_id, version)
        );
        INSERT INTO game_versions (game_id, version, title, place, published)
            SELECT game_id, version, title, place, updated FROM games g
            WHERE NOT EXISTS (SELECT 1 FROM game_versions v WHERE v.game_id = g.game_id);",
    )
    .map_err(|e| format!("could not set up game versions: {e}"))
}

pub(crate) fn keep_version(
    db: &Connection,
    game_id: &str,
    version: i64,
    title: &str,
    place: &str,
    time: i64,
) -> rusqlite::Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO game_versions (game_id, version, title, place, published) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![game_id, version, title, place, time],
    )?;
    db.execute(
        "DELETE FROM game_versions WHERE game_id = ?1 AND version <= ?2",
        params![game_id, version - KEEP_VERSIONS],
    )?;
    Ok(())
}

pub(crate) fn forget(db: &Connection, game_id: &str) {
    let _ = db.execute("DELETE FROM game_versions WHERE game_id = ?1", params![game_id]);
    analytics::forget_game(db, game_id);
}

pub(crate) fn most_players(db: &Connection, game_id: Option<&str>, asked: u32) -> u32 {
    let set: u32 = game_id
        .and_then(|id| {
            db.query_row("SELECT max_players FROM games WHERE game_id = ?1", params![id], |r| r.get(0))
                .optional()
                .ok()
                .flatten()
        })
        .unwrap_or(0);
    match (set, asked) {
        (0, n) => n,
        (s, 0) => s,
        (s, n) => s.min(n),
    }
}

type Failed = (StatusCode, String);

fn internal(e: rusqlite::Error) -> Failed {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn fresh(time: i64) -> Result<(), Failed> {
    if (time - now_ms()).abs() > SIGNED_SLACK_MS {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    Ok(())
}

pub(crate) fn owned(db: &Connection, game_id: &str, player_id: &str) -> Result<(), Failed> {
    let author: Option<Option<String>> = db
        .query_row("SELECT author_id FROM games WHERE game_id = ?1", params![game_id], |r| r.get(0))
        .optional()
        .map_err(internal)?;
    match author {
        None => Err((StatusCode::NOT_FOUND, "there is no published game with that id".into())),
        Some(Some(a)) if a == player_id => Ok(()),
        Some(_) => Err((StatusCode::FORBIDDEN, "that isn't your game".into())),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct Signed {
    player_id: Option<String>,
    time: i64,
    signature: String,
}

pub(crate) async fn my_games(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Query(query): Query<Signed>,
) -> Result<Json<Vec<GameInfo>>, Failed> {
    if !verify(&id, &creator_message(&id, query.time), &query.signature) {
        return Err((StatusCode::FORBIDDEN, "only you can see your games".into()));
    }
    fresh(query.time)?;
    let db = db.lock().unwrap();
    let now = now();
    let mut statement = db
        .prepare(&format!(
            "SELECT {GAME_COLUMNS} FROM {GAMES} WHERE g.author_id = ?1 ORDER BY g.updated DESC"
        ))
        .map_err(internal)?;
    let games = statement
        .query_map(params![id], |r| game_row(r, now))
        .map_err(internal)?
        .filter_map(Result::ok)
        .collect();
    Ok(Json(games))
}

pub(crate) async fn versions(
    State(db): State<Db>,
    UrlPath(game_id): UrlPath<String>,
    Query(query): Query<Signed>,
) -> Result<Json<Vec<Version>>, Failed> {
    let player_id = query.player_id.unwrap_or_default();
    if !verify(&player_id, &creator_message(&player_id, query.time), &query.signature) {
        return Err((StatusCode::FORBIDDEN, "only its creator can see a game's versions".into()));
    }
    fresh(query.time)?;
    let db = db.lock().unwrap();
    owned(&db, &game_id, &player_id)?;
    let now = now();
    let mut statement = db
        .prepare(
            "SELECT version, title, LENGTH(place), published FROM game_versions WHERE game_id = ?1
             ORDER BY version DESC",
        )
        .map_err(internal)?;
    let list = statement
        .query_map(params![game_id], |r| {
            Ok(Version {
                version: r.get::<_, i64>(0)? as u32,
                title: r.get(1)?,
                size: r.get::<_, i64>(2)? as u64,
                published_secs_ago: now - r.get::<_, i64>(3)?,
            })
        })
        .map_err(internal)?
        .filter_map(Result::ok)
        .collect();
    Ok(Json(list))
}

pub(crate) async fn manage(
    State(db): State<Db>,
    UrlPath(game_id): UrlPath<String>,
    Json(ask): Json<ManageGame>,
) -> Result<StatusCode, Failed> {
    if !verify(
        &ask.player_id,
        &manage_message(&ask.player_id, &game_id, ask.time, &ask.change),
        &ask.signature,
    ) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match".into()));
    }
    fresh(ask.time)?;
    let db = db.lock().unwrap();
    owned(&db, &game_id, &ask.player_id)?;
    match ask.change {
        Change::Settings { title, description, private, max_players } => {
            if title.trim().is_empty() {
                return Err((StatusCode::BAD_REQUEST, "a published game needs a title".into()));
            }
            db.execute(
                "UPDATE games SET title = ?2, description = ?3, private = ?4, max_players = ?5 WHERE game_id = ?1",
                params![
                    game_id,
                    clip(&title),
                    clip_to(&description, crate::MAX_DESCRIPTION),
                    private,
                    max_players.min(MOST_PLAYERS)
                ],
            )
            .map_err(internal)?;
        }
        Change::Restore { version } => {
            let old: Option<String> = db
                .query_row(
                    "SELECT place FROM game_versions WHERE game_id = ?1 AND version = ?2",
                    params![game_id, version],
                    |r| r.get(0),
                )
                .optional()
                .map_err(internal)?;
            let Some(place) = old else {
                return Err((StatusCode::NOT_FOUND, "that version isn't kept".into()));
            };
            let (title, current): (String, i64) = db
                .query_row("SELECT title, version FROM games WHERE game_id = ?1", params![game_id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .map_err(internal)?;
            let now = now();
            db.execute(
                "UPDATE games SET place = ?2, version = ?3, updated = ?4 WHERE game_id = ?1",
                params![game_id, place, current + 1, now],
            )
            .map_err(internal)?;
            keep_version(&db, &game_id, current + 1, &title, &place, now).map_err(internal)?;
            let owner: String = db
                .query_row(
                    "SELECT COALESCE(author_id, game_id) FROM games WHERE game_id = ?1",
                    params![game_id],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            crate::copies::keep(&db, &game_id, &owner, &crate::copies::prints(&place)).map_err(internal)?;
        }
        Change::Remove => {
            db.execute("DELETE FROM games WHERE game_id = ?1", params![game_id]).map_err(internal)?;
            crate::copies::forget(&db, &game_id);
            forget(&db, &game_id);
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::social::now_ms;
use crate::{Db, EXPIRES_AFTER_SECS, now, verify};

pub const KEEP_DAYS: i64 = 90;

const DAY: i64 = 86_400;

const SESSION_GAP_SECS: i64 = 90;

const SIGNED_SLACK_MS: i64 = 10 * 60 * 1000;

pub fn creator_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel creator\n{player_id}\n{time}").into_bytes()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DayStats {
    pub day: i64,
    pub visits: u32,
    pub players: u32,
    pub new_players: u32,
    pub playtime_secs: u64,
    pub likes: u32,
    pub favorites: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GameStats {
    pub game_id: String,
    pub title: String,
    pub visits: u32,
    pub players: u32,
    pub average_playtime_secs: u64,
    pub day1_retention: Option<f64>,
    pub day7_retention: Option<f64>,
    pub likes: u32,
    pub dislikes: u32,
    pub favorites: u32,
    pub playing: u32,
    pub peak: u32,
    pub days: Vec<DayStats>,
    pub concurrent_every_secs: i64,
    pub concurrent: Vec<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Creator {
    pub from: i64,
    pub to: i64,
    pub games: Vec<GameStats>,
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS analytics_secret (
            id          INTEGER PRIMARY KEY CHECK (id = 0),
            secret      BLOB NOT NULL
        );
        CREATE TABLE IF NOT EXISTS play_sessions (
            game_id     TEXT NOT NULL,
            player      TEXT NOT NULL,
            started     INTEGER NOT NULL,
            last_seen   INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS play_sessions_game ON play_sessions(game_id, last_seen);
        CREATE INDEX IF NOT EXISTS play_sessions_player ON play_sessions(game_id, player, started);
        CREATE TABLE IF NOT EXISTS play_days (
            game_id     TEXT NOT NULL,
            player      TEXT NOT NULL,
            day         INTEGER NOT NULL,
            PRIMARY KEY (game_id, player, day)
        );
        CREATE INDEX IF NOT EXISTS play_days_day ON play_days(day);
        CREATE TABLE IF NOT EXISTS game_players (
            game_id     TEXT NOT NULL,
            player      TEXT NOT NULL,
            first_day   INTEGER NOT NULL,
            last_day    INTEGER NOT NULL,
            PRIMARY KEY (game_id, player)
        );
        CREATE INDEX IF NOT EXISTS game_players_last ON game_players(last_day);",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))?;
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| format!("could not make the analytics secret: {e}"))?;
    db.execute(
        "INSERT OR IGNORE INTO analytics_secret (id, secret) VALUES (0, ?1)",
        params![secret.to_vec()],
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))?;
    Ok(())
}

pub(crate) fn player_code(db: &Connection, game_id: &str, player_id: &str) -> rusqlite::Result<String> {
    let secret: Vec<u8> =
        db.query_row("SELECT secret FROM analytics_secret WHERE id = 0", [], |r| r.get(0))?;
    let mut hash = Sha256::new();
    hash.update(&secret);
    hash.update(game_id.as_bytes());
    hash.update([0]);
    hash.update(player_id.as_bytes());
    Ok(data_encoding::HEXLOWER.encode(&hash.finalize()[..12]))
}

pub fn record(db: &Connection, game_id: &str, player_id: &str, now: i64) -> rusqlite::Result<()> {
    let published: Option<i64> =
        db.query_row("SELECT 1 FROM games WHERE game_id = ?1", params![game_id], |r| r.get(0)).optional()?;
    if published.is_none() {
        return Ok(());
    }
    let player = player_code(db, game_id, player_id)?;
    let last: Option<(i64, i64)> = db
        .query_row(
            "SELECT rowid, last_seen FROM play_sessions WHERE game_id = ?1 AND player = ?2
             ORDER BY started DESC LIMIT 1",
            params![game_id, player],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match last {
        Some((row, seen)) if seen >= now - SESSION_GAP_SECS && seen <= now => {
            db.execute("UPDATE play_sessions SET last_seen = ?2 WHERE rowid = ?1", params![row, now])?;
        }
        _ => {
            db.execute(
                "INSERT INTO play_sessions (game_id, player, started, last_seen) VALUES (?1, ?2, ?3, ?3)",
                params![game_id, player, now],
            )?;
            prune(db, now)?;
        }
    }
    let day = now.div_euclid(DAY);
    db.execute(
        "INSERT OR IGNORE INTO play_days (game_id, player, day) VALUES (?1, ?2, ?3)",
        params![game_id, player, day],
    )?;
    db.execute(
        "INSERT INTO game_players (game_id, player, first_day, last_day) VALUES (?1, ?2, ?3, ?3)
         ON CONFLICT(game_id, player) DO UPDATE SET last_day = MAX(last_day, ?3)",
        params![game_id, player, day],
    )?;
    Ok(())
}

fn prune(db: &Connection, now: i64) -> rusqlite::Result<()> {
    let cut = now - KEEP_DAYS * DAY;
    db.execute("DELETE FROM play_sessions WHERE last_seen < ?1", params![cut])?;
    db.execute("DELETE FROM play_days WHERE day < ?1", params![cut.div_euclid(DAY)])?;
    db.execute("DELETE FROM game_players WHERE last_day < ?1", params![cut.div_euclid(DAY)])?;
    Ok(())
}

pub(crate) fn forget_game(db: &Connection, game_id: &str) {
    for table in ["play_sessions", "play_days", "game_players"] {
        let _ = db.execute(&format!("DELETE FROM {table} WHERE game_id = ?1"), params![game_id]);
    }
}

fn has_table(db: &Connection, name: &str) -> bool {
    db.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", params![name], |_| Ok(()))
        .is_ok()
}

fn count(db: &Connection, sql: &str, args: impl rusqlite::Params) -> u32 {
    db.query_row(sql, args, |r| r.get::<_, i64>(0)).unwrap_or(0) as u32
}

fn retention(db: &Connection, game_id: &str, days: i64, today: i64, after: i64) -> Option<f64> {
    let last = today - after;
    let from_day = last - days + 1;
    let (cohort, kept): (i64, i64) = db
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(EXISTS (SELECT 1 FROM play_days d WHERE d.game_id = p.game_id
                 AND d.player = p.player AND d.day = p.first_day + ?4)), 0)
             FROM game_players p WHERE p.game_id = ?1 AND p.first_day BETWEEN ?2 AND ?3",
            params![game_id, from_day, last, after],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()?;
    (cohort > 0).then(|| kept as f64 / cohort as f64)
}

pub fn creator_stats(db: &Connection, author_id: &str, days: i64, now: i64) -> rusqlite::Result<Creator> {
    let days = days.clamp(1, KEEP_DAYS);
    let today = now.div_euclid(DAY);
    let from_day = today - days + 1;
    let from = from_day * DAY;
    let votes = has_table(db, "game_votes");
    let favorites = has_table(db, "game_favorites");
    let every = ((days * DAY) / 168).max(900);
    let buckets = ((now - from) / every + 1) as usize;
    let mut games: Vec<(String, String)> = {
        let mut statement =
            db.prepare("SELECT game_id, title FROM games WHERE author_id = ?1 ORDER BY created DESC")?;
        statement.query_map(params![author_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    let mut out = Vec::new();
    for (game_id, title) in games.drain(..) {
        let mut stats = GameStats { game_id: game_id.clone(), title, ..GameStats::default() };
        let mut by_day: Vec<DayStats> =
            (from_day..=today).map(|day| DayStats { day, ..DayStats::default() }).collect();
        let mut concurrent = vec![0u32; buckets];
        let mut total_time = 0u64;
        {
            let mut statement = db.prepare(
                "SELECT started, last_seen FROM play_sessions WHERE game_id = ?1 AND last_seen >= ?2
                 ORDER BY started LIMIT 200000",
            )?;
            let mut rows = statement.query(params![game_id, from])?;
            while let Some(row) = rows.next()? {
                let (started, seen): (i64, i64) = (row.get(0)?, row.get(1)?);
                let length = (seen - started).max(0) as u64;
                if started >= from {
                    stats.visits += 1;
                    total_time += length;
                    if let Some(d) = by_day.get_mut((started.div_euclid(DAY) - from_day) as usize) {
                        d.visits += 1;
                        d.playtime_secs += length;
                    }
                }
                let first = ((started.max(from) - from) / every) as usize;
                let last = ((seen.min(now) - from) / every) as usize;
                for slot in concurrent.iter_mut().take(last + 1).skip(first) {
                    *slot += 1;
                }
            }
        }
        stats.average_playtime_secs = if stats.visits > 0 { total_time / stats.visits as u64 } else { 0 };
        stats.players = count(
            db,
            "SELECT COUNT(DISTINCT player) FROM play_days WHERE game_id = ?1 AND day >= ?2",
            params![game_id, from_day],
        );
        for d in &mut by_day {
            d.players = count(
                db,
                "SELECT COUNT(*) FROM play_days WHERE game_id = ?1 AND day = ?2",
                params![game_id, d.day],
            );
            d.new_players = count(
                db,
                "SELECT COUNT(*) FROM game_players WHERE game_id = ?1 AND first_day = ?2",
                params![game_id, d.day],
            );
            if d.visits > 0 {
                d.playtime_secs /= d.visits as u64;
            }
        }
        stats.day1_retention = retention(db, &game_id, days, today, 1);
        stats.day7_retention = retention(db, &game_id, days, today, 7);
        if votes {
            stats.likes = count(
                db,
                "SELECT COUNT(*) FROM game_votes WHERE game_id = ?1 AND vote > 0",
                params![game_id],
            );
            stats.dislikes = count(
                db,
                "SELECT COUNT(*) FROM game_votes WHERE game_id = ?1 AND vote < 0",
                params![game_id],
            );
            for d in &mut by_day {
                d.likes = count(
                    db,
                    "SELECT COUNT(*) FROM game_votes WHERE game_id = ?1 AND vote > 0 AND time >= ?2 AND time < ?3",
                    params![game_id, d.day * DAY, (d.day + 1) * DAY],
                );
            }
        }
        if favorites {
            stats.favorites =
                count(db, "SELECT COUNT(*) FROM game_favorites WHERE game_id = ?1", params![game_id]);
            for d in &mut by_day {
                d.favorites = count(
                    db,
                    "SELECT COUNT(*) FROM game_favorites WHERE game_id = ?1 AND time >= ?2 AND time < ?3",
                    params![game_id, d.day * DAY, (d.day + 1) * DAY],
                );
            }
        }
        stats.playing = count(
            db,
            "SELECT COUNT(*) FROM play_sessions WHERE game_id = ?1 AND last_seen >= ?2",
            params![game_id, now - EXPIRES_AFTER_SECS],
        );
        stats.peak = concurrent.iter().copied().max().unwrap_or(0);
        stats.days = by_day;
        stats.concurrent_every_secs = every;
        stats.concurrent = concurrent;
        out.push(stats);
    }
    Ok(Creator { from, to: now, games: out })
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreatorQuery {
    time: i64,
    signature: String,
    days: Option<i64>,
}

pub(crate) async fn creator(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Query(query): Query<CreatorQuery>,
) -> Result<Json<Creator>, (StatusCode, String)> {
    if !verify(&id, &creator_message(&id, query.time), &query.signature) {
        return Err((StatusCode::FORBIDDEN, "only you can see your games' numbers".into()));
    }
    if (query.time - now_ms()).abs() > SIGNED_SLACK_MS {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    let db = db.lock().unwrap();
    creator_stats(&db, &id, query.days.unwrap_or(7), now())
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

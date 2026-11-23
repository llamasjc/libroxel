use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::social::now_ms;
use crate::{Db, now, owner, verify};

pub const KEEP_DAYS: i64 = 90;

const DAY: i64 = 86_400;

pub const PING_EVERY_SECS: u64 = 300;

const SESSION_GAP_SECS: i64 = 660;

const SIGNED_SLACK_MS: i64 = 10 * 60 * 1000;

const MAX_CRASH: usize = 16 * 1024;

const MAX_LABEL: usize = 40;

pub fn admin_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel admin\n{player_id}\n{time}").into_bytes()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Ping {
    pub install: String,
    pub version: String,
    pub platform: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Crash {
    pub install: String,
    pub version: String,
    pub platform: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UsageDay {
    pub day: i64,
    pub users: u32,
    pub new_users: u32,
    pub sessions: u32,
    pub average_session_secs: u64,
    pub crashes: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Share {
    pub name: String,
    pub users: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CrashGroup {
    pub title: String,
    pub count: u32,
    pub users: u32,
    pub last: i64,
    pub versions: Vec<String>,
    pub platforms: Vec<String>,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    pub from: i64,
    pub to: i64,
    pub users: u32,
    pub online: u32,
    pub sessions: u32,
    pub crashes: u32,
    pub days: Vec<UsageDay>,
    pub versions: Vec<Share>,
    pub platforms: Vec<Share>,
    pub top_crashes: Vec<CrashGroup>,
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS app_sessions (
            install     TEXT NOT NULL,
            version     TEXT NOT NULL,
            platform    TEXT NOT NULL,
            started     INTEGER NOT NULL,
            last_seen   INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS app_sessions_install ON app_sessions(install, started);
        CREATE INDEX IF NOT EXISTS app_sessions_seen ON app_sessions(last_seen);
        CREATE TABLE IF NOT EXISTS app_days (
            install     TEXT NOT NULL,
            day         INTEGER NOT NULL,
            version     TEXT NOT NULL,
            platform    TEXT NOT NULL,
            PRIMARY KEY (install, day)
        );
        CREATE INDEX IF NOT EXISTS app_days_day ON app_days(day);
        CREATE TABLE IF NOT EXISTS app_installs (
            install     TEXT PRIMARY KEY,
            first_day   INTEGER NOT NULL,
            last_day    INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS crashes (
            install     TEXT NOT NULL,
            version     TEXT NOT NULL,
            platform    TEXT NOT NULL,
            title       TEXT NOT NULL,
            text        TEXT NOT NULL,
            time        INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS crashes_time ON crashes(time);",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))
}

fn install_code(db: &Connection, install: &str) -> rusqlite::Result<String> {
    let secret: Vec<u8> =
        db.query_row("SELECT secret FROM analytics_secret WHERE id = 0", [], |r| r.get(0))?;
    let mut hash = Sha256::new();
    hash.update(&secret);
    hash.update(b"app\0");
    hash.update(install.as_bytes());
    Ok(data_encoding::HEXLOWER.encode(&hash.finalize()[..12]))
}

fn label(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .take(MAX_LABEL)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn valid_install(install: &str) -> bool {
    (16..=64).contains(&install.len()) && install.chars().all(|c| c.is_ascii_alphanumeric())
}

pub fn record_ping(db: &Connection, ping: &Ping, now: i64) -> rusqlite::Result<()> {
    let install = install_code(db, &ping.install)?;
    let (version, platform) = (label(&ping.version), label(&ping.platform));
    let last: Option<(i64, i64)> = db
        .query_row(
            "SELECT rowid, last_seen FROM app_sessions WHERE install = ?1 ORDER BY started DESC LIMIT 1",
            params![install],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    match last {
        Some((row, seen)) if seen >= now - SESSION_GAP_SECS && seen <= now => {
            db.execute("UPDATE app_sessions SET last_seen = ?2 WHERE rowid = ?1", params![row, now])?;
        }
        _ => {
            db.execute(
                "INSERT INTO app_sessions (install, version, platform, started, last_seen)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![install, version, platform, now],
            )?;
            prune(db, now)?;
        }
    }
    let day = now.div_euclid(DAY);
    db.execute(
        "INSERT INTO app_days (install, day, version, platform) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(install, day) DO UPDATE SET version = ?3, platform = ?4",
        params![install, day, version, platform],
    )?;
    db.execute(
        "INSERT INTO app_installs (install, first_day, last_day) VALUES (?1, ?2, ?2)
         ON CONFLICT(install) DO UPDATE SET last_day = MAX(last_day, ?2)",
        params![install, day],
    )?;
    Ok(())
}

pub fn record_crash(db: &Connection, crash: &Crash, now: i64) -> rusqlite::Result<()> {
    let install = install_code(db, &crash.install)?;
    let text = crate::scrub::scrub(crash.text.trim());
    let mut end = text.len().min(MAX_CRASH);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let text = &text[..end];
    let title: String = text.lines().next().unwrap_or_default().chars().take(200).collect();
    db.execute(
        "INSERT INTO crashes (install, version, platform, title, text, time) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![install, label(&crash.version), label(&crash.platform), title, text, now],
    )?;
    Ok(())
}

fn prune(db: &Connection, now: i64) -> rusqlite::Result<()> {
    let cut = now - KEEP_DAYS * DAY;
    let cut_day = cut.div_euclid(DAY);
    db.execute("DELETE FROM app_sessions WHERE last_seen < ?1", params![cut])?;
    db.execute("DELETE FROM app_days WHERE day < ?1", params![cut_day])?;
    db.execute("DELETE FROM app_installs WHERE last_day < ?1", params![cut_day])?;
    db.execute("DELETE FROM crashes WHERE time < ?1", params![cut])?;
    Ok(())
}

fn count(db: &Connection, sql: &str, args: impl rusqlite::Params) -> u32 {
    db.query_row(sql, args, |r| r.get::<_, i64>(0)).unwrap_or(0) as u32
}

fn shares(db: &Connection, column: &str, from_day: i64) -> rusqlite::Result<Vec<Share>> {
    let mut statement = db.prepare(&format!(
        "SELECT {column}, COUNT(DISTINCT install) AS n FROM app_days WHERE day >= ?1
         GROUP BY {column} ORDER BY n DESC, {column} LIMIT 20"
    ))?;
    statement
        .query_map(params![from_day], |r| Ok(Share { name: r.get(0)?, users: r.get::<_, i64>(1)? as u32 }))?
        .collect()
}

fn distinct(db: &Connection, column: &str, title: &str, from: i64) -> Vec<String> {
    let Ok(mut statement) = db.prepare(&format!(
        "SELECT {column} FROM crashes WHERE title = ?1 AND time >= ?2 GROUP BY {column} ORDER BY MAX(time) DESC LIMIT 6"
    )) else {
        return Vec::new();
    };
    statement
        .query_map(params![title, from], |r| r.get(0))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

pub fn usage(db: &Connection, days: i64, now: i64) -> rusqlite::Result<Usage> {
    let days = days.clamp(1, KEEP_DAYS);
    let today = now.div_euclid(DAY);
    let from_day = today - days + 1;
    let from = from_day * DAY;
    let mut by_day: Vec<UsageDay> =
        (from_day..=today).map(|day| UsageDay { day, ..UsageDay::default() }).collect();
    let mut sessions = 0;
    {
        let mut statement =
            db.prepare("SELECT started, last_seen FROM app_sessions WHERE started >= ?1 LIMIT 500000")?;
        let mut rows = statement.query(params![from])?;
        while let Some(row) = rows.next()? {
            let (started, seen): (i64, i64) = (row.get(0)?, row.get(1)?);
            sessions += 1;
            if let Some(d) = by_day.get_mut((started.div_euclid(DAY) - from_day) as usize) {
                d.sessions += 1;
                d.average_session_secs += (seen - started).max(0) as u64;
            }
        }
    }
    for d in &mut by_day {
        if d.sessions > 0 {
            d.average_session_secs /= d.sessions as u64;
        }
        d.users = count(db, "SELECT COUNT(*) FROM app_days WHERE day = ?1", params![d.day]);
        d.new_users = count(db, "SELECT COUNT(*) FROM app_installs WHERE first_day = ?1", params![d.day]);
        d.crashes = count(
            db,
            "SELECT COUNT(*) FROM crashes WHERE time >= ?1 AND time < ?2",
            params![d.day * DAY, (d.day + 1) * DAY],
        );
    }
    let top_crashes = {
        let mut statement = db.prepare(
            "SELECT title, COUNT(*) AS n, COUNT(DISTINCT install), MAX(time),
                 (SELECT text FROM crashes c WHERE c.title = crashes.title ORDER BY time DESC LIMIT 1)
             FROM crashes WHERE time >= ?1 GROUP BY title ORDER BY n DESC, MAX(time) DESC LIMIT 20",
        )?;
        let rows: Vec<CrashGroup> = statement
            .query_map(params![from], |r| {
                Ok(CrashGroup {
                    title: r.get(0)?,
                    count: r.get::<_, i64>(1)? as u32,
                    users: r.get::<_, i64>(2)? as u32,
                    last: r.get(3)?,
                    text: r.get(4)?,
                    ..CrashGroup::default()
                })
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|mut g| {
                g.versions = distinct(db, "version", &g.title, from);
                g.platforms = distinct(db, "platform", &g.title, from);
                g
            })
            .collect()
    };
    Ok(Usage {
        from,
        to: now,
        users: count(db, "SELECT COUNT(DISTINCT install) FROM app_days WHERE day >= ?1", params![from_day]),
        online: count(
            db,
            "SELECT COUNT(DISTINCT install) FROM app_sessions WHERE last_seen >= ?1",
            params![now - SESSION_GAP_SECS],
        ),
        sessions,
        crashes: count(db, "SELECT COUNT(*) FROM crashes WHERE time >= ?1", params![from]),
        days: by_day,
        versions: shares(db, "version", from_day)?,
        platforms: shares(db, "platform", from_day)?,
        top_crashes,
    })
}

pub(crate) async fn ping(
    State(db): State<Db>,
    Json(ping): Json<Ping>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !valid_install(&ping.install) {
        return Err((StatusCode::BAD_REQUEST, "bad install".into()));
    }
    let db = db.lock().unwrap();
    record_ping(&db, &ping, now()).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn crash(
    State(db): State<Db>,
    Json(crash): Json<Crash>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !valid_install(&crash.install) || crash.text.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "bad crash".into()));
    }
    let db = db.lock().unwrap();
    record_crash(&db, &crash, now()).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminQuery {
    time: i64,
    signature: String,
    days: Option<i64>,
}

pub(crate) async fn admin(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Query(query): Query<AdminQuery>,
) -> Result<Json<Usage>, (StatusCode, String)> {
    if !verify(&id, &admin_message(&id, query.time), &query.signature) {
        return Err((StatusCode::FORBIDDEN, "not signed".into()));
    }
    if (query.time - now_ms()).abs() > SIGNED_SLACK_MS {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    let db = db.lock().unwrap();
    if !owner::is_owner(&db, &id) {
        return Err((StatusCode::FORBIDDEN, "only the Hub's owners can see this".into()));
    }
    usage(&db, query.days.unwrap_or(30), now())
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

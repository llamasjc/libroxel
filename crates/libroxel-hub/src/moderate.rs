use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{Db, HIDE_AFTER_REPORTS, now, owner, verify};

type Failure = (StatusCode, String);

fn internal(e: impl std::fmt::Display) -> Failure {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn bad(why: &str) -> Failure {
    (StatusCode::BAD_REQUEST, why.to_owned())
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS muted (
            player_id   TEXT PRIMARY KEY,
            reason      TEXT NOT NULL,
            since       INTEGER NOT NULL,
            until       INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS warnings (
            warning_id  INTEGER PRIMARY KEY,
            player_id   TEXT NOT NULL,
            reason      TEXT NOT NULL,
            time        INTEGER NOT NULL,
            seen        INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS warnings_player ON warnings(player_id);
        CREATE TABLE IF NOT EXISTS mod_log (
            log_id       INTEGER PRIMARY KEY,
            moderator_id TEXT NOT NULL,
            action       TEXT NOT NULL,
            kind         TEXT NOT NULL,
            target       TEXT NOT NULL,
            player_id    TEXT NOT NULL,
            reason       TEXT NOT NULL,
            text         TEXT NOT NULL,
            until        INTEGER,
            time         INTEGER NOT NULL,
            signed       INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS mod_log_moderator ON mod_log(moderator_id, signed);",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModAction {
    Dismiss,
    Remove,
    Warn,
    Mute,
    Unmute,
    Ban,
    Unban,
}

impl ModAction {
    pub fn name(self) -> &'static str {
        match self {
            ModAction::Dismiss => "dismiss",
            ModAction::Remove => "remove",
            ModAction::Warn => "warn",
            ModAction::Mute => "mute",
            ModAction::Unmute => "unmute",
            ModAction::Ban => "ban",
            ModAction::Unban => "unban",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModChange {
    pub moderator_id: String,
    pub action: ModAction,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub player_id: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub hours: u32,
    pub time: i64,
    pub signature: String,
}

pub fn moderate_message(c: &ModChange) -> Vec<u8> {
    format!(
        "libroxel moderate\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        c.moderator_id,
        c.action.name(),
        c.kind,
        c.target,
        c.player_id,
        c.reason,
        c.hours,
        c.time
    )
    .into_bytes()
}

pub fn moderation_message(moderator_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel moderation\n{moderator_id}\n{time}").into_bytes()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReportReason {
    pub reporter_id: String,
    pub reporter: Option<String>,
    pub reason: String,
    pub secs_ago: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Queued {
    pub kind: String,
    pub target: String,
    pub text: Option<String>,
    pub author_id: Option<String>,
    pub author: Option<String>,
    pub place: Option<String>,
    pub reports: Vec<ReportReason>,
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Penalty {
    pub player_id: String,
    pub name: Option<String>,
    pub reason: String,
    pub since_secs_ago: i64,
    pub left_secs: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogEntry {
    pub moderator_id: String,
    pub moderator: Option<String>,
    pub action: String,
    pub kind: String,
    pub target: String,
    pub player_id: String,
    pub player: Option<String>,
    pub reason: String,
    pub text: String,
    pub hours: Option<i64>,
    pub secs_ago: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Dashboard {
    pub reports: Vec<Queued>,
    pub banned: Vec<Penalty>,
    pub muted: Vec<Penalty>,
    pub log: Vec<LogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Warning {
    pub warning_id: i64,
    pub reason: String,
    pub secs_ago: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WarningsSeen {
    pub player_id: String,
    pub time: i64,
    pub signature: String,
}

pub fn warnings_seen_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel warnings seen\n{player_id}\n{time}").into_bytes()
}

pub(crate) fn muted_for(db: &Connection, player_id: &str) -> Option<i64> {
    let until: i64 = db
        .query_row("SELECT until FROM muted WHERE player_id = ?1", params![player_id], |r| r.get(0))
        .optional()
        .ok()
        .flatten()?;
    let left = until - now();
    (left > 0).then_some(left)
}

pub(crate) fn not_muted(db: &Connection, player_id: &str) -> Result<(), Failure> {
    match muted_for(db, player_id) {
        Some(left) => Err((StatusCode::FORBIDDEN, format!("you are muted for {}", span(left)))),
        None => Ok(()),
    }
}

fn span(secs: i64) -> String {
    let (n, unit) = if secs < 3600 {
        ((secs + 59) / 60, "minute")
    } else if secs < 2 * 86_400 {
        ((secs + 3599) / 3600, "hour")
    } else {
        ((secs + 86_399) / 86_400, "day")
    };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

pub(crate) fn unseen_warnings(db: &Connection, player_id: &str) -> Vec<Warning> {
    let now = now();
    db.prepare(
        "SELECT warning_id, reason, time FROM warnings WHERE player_id = ?1 AND seen = 0 ORDER BY time",
    )
    .and_then(|mut s| {
        s.query_map(params![player_id], |r| {
            Ok(Warning { warning_id: r.get(0)?, reason: r.get(1)?, secs_ago: now - r.get::<_, i64>(2)? })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
    })
    .unwrap_or_default()
}

fn content(db: &Connection, kind: &str, target: &str) -> Option<(String, Option<String>, Option<String>)> {
    let sql = match kind {
        "game" => "SELECT title || CASE WHEN description = '' THEN '' ELSE ': ' || description END, author_id, game_id
                   FROM games WHERE game_id = ?1",
        "item" => "SELECT name, creator_id, NULL FROM items WHERE item_id = ?1",
        "comment" => "SELECT text, player_id, game_id FROM game_comments WHERE comment_id = ?1",
        "message" => "SELECT text, from_id, NULL FROM messages WHERE message_id = ?1",
        "group" => "SELECT name || CASE WHEN about = '' THEN '' ELSE ': ' || about END,
                    (SELECT player_id FROM group_members m WHERE m.group_id = g.group_id AND m.role = 2), group_id
                    FROM groups g WHERE group_id = ?1",
        "post" => "SELECT text, player_id, group_id FROM group_posts WHERE post_id = ?1",
        _ => return None,
    };
    db.query_row(sql, params![target], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional().ok().flatten()
}

fn name_of(db: &Connection, player_id: &str) -> Option<String> {
    db.query_row("SELECT name FROM accounts WHERE player_id = ?1", params![player_id], |r| r.get(0))
        .optional()
        .ok()
        .flatten()
}

pub fn dashboard(db: &Connection) -> Dashboard {
    let now = now();
    let targets: Vec<(String, String, bool)> = db
        .prepare(
            "SELECT r.kind, r.target,
                 EXISTS (SELECT 1 FROM cleared c WHERE c.kind = r.kind AND c.target = r.target)
             FROM reports r GROUP BY r.kind, r.target ORDER BY COUNT(*) DESC, MAX(r.time) DESC LIMIT 200",
        )
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default();
    let reports = targets
        .into_iter()
        .map(|(kind, target, cleared)| {
            let reasons: Vec<ReportReason> = db
                .prepare(
                    "SELECT r.reporter_id, a.name, r.reason, r.time FROM reports r
                     LEFT JOIN accounts a ON a.player_id = r.reporter_id
                     WHERE r.kind = ?1 AND r.target = ?2 ORDER BY r.time DESC",
                )
                .and_then(|mut s| {
                    s.query_map(params![kind, target], |r| {
                        Ok(ReportReason {
                            reporter_id: r.get(0)?,
                            reporter: r.get(1)?,
                            reason: r.get(2)?,
                            secs_ago: now - r.get::<_, i64>(3)?,
                        })
                    })
                    .map(|rows| rows.filter_map(Result::ok).collect())
                })
                .unwrap_or_default();
            let found = content(db, &kind, &target);
            let author_id = found.as_ref().and_then(|f| f.1.clone());
            Queued {
                hidden: reasons.len() as i64 >= HIDE_AFTER_REPORTS && !cleared,
                author: author_id.as_deref().and_then(|id| name_of(db, id)),
                text: found.as_ref().map(|f| f.0.clone()),
                place: found.and_then(|f| f.2),
                author_id,
                kind,
                target,
                reports: reasons,
            }
        })
        .collect();
    let penalties = |sql: &str| -> Vec<Penalty> {
        db.prepare(sql)
            .and_then(|mut s| {
                s.query_map(params![now], |r| {
                    Ok(Penalty {
                        player_id: r.get(0)?,
                        name: r.get(1)?,
                        reason: r.get(2)?,
                        since_secs_ago: now - r.get::<_, i64>(3)?,
                        left_secs: r.get::<_, Option<i64>>(4)?.map(|until| until - now),
                    })
                })
                .map(|rows| rows.filter_map(Result::ok).collect())
            })
            .unwrap_or_default()
    };
    let banned = penalties(
        "SELECT b.player_id, a.name, b.reason, b.since, b.until FROM banned b
         LEFT JOIN accounts a ON a.player_id = b.player_id
         WHERE b.until IS NULL OR b.until > ?1 ORDER BY b.since DESC LIMIT 500",
    );
    let muted = penalties(
        "SELECT m.player_id, a.name, m.reason, m.since, m.until FROM muted m
         LEFT JOIN accounts a ON a.player_id = m.player_id
         WHERE m.until > ?1 ORDER BY m.since DESC LIMIT 500",
    );
    let log = db
        .prepare(
            "SELECT l.moderator_id, mo.name, l.action, l.kind, l.target, l.player_id, p.name, l.reason, l.text,
                 l.until, l.time
             FROM mod_log l LEFT JOIN accounts mo ON mo.player_id = l.moderator_id
             LEFT JOIN accounts p ON p.player_id = l.player_id
             ORDER BY l.log_id DESC LIMIT 200",
        )
        .and_then(|mut s| {
            s.query_map([], |r| {
                let time: i64 = r.get(10)?;
                Ok(LogEntry {
                    moderator_id: r.get(0)?,
                    moderator: r.get(1)?,
                    action: r.get(2)?,
                    kind: r.get(3)?,
                    target: r.get(4)?,
                    player_id: r.get(5)?,
                    player: r.get(6)?,
                    reason: r.get(7)?,
                    text: r.get(8)?,
                    hours: r.get::<_, Option<i64>>(9)?.map(|until| (until - time + 1800) / 3600),
                    secs_ago: now - time,
                })
            })
            .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default();
    Dashboard { reports, banned, muted, log }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn log(
    db: &Connection,
    moderator_id: &str,
    action: ModAction,
    kind: &str,
    target: &str,
    player_id: &str,
    reason: &str,
    text: &str,
    until: Option<i64>,
    signed: i64,
) {
    let _ = db.execute(
        "INSERT INTO mod_log (moderator_id, action, kind, target, player_id, reason, text, until, time, signed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![moderator_id, action.name(), kind, target, player_id, reason, text, until, now(), signed],
    );
}

pub fn apply(db: &Connection, moderator_id: &str, c: &ModChange) -> Result<(), String> {
    let kind = c.kind.trim();
    let target = c.target.trim();
    let reason = c.reason.trim();
    if reason.chars().count() > 300 {
        return Err("a reason is up to 300 letters".into());
    }
    let found = if kind.is_empty() { None } else { content(db, kind, target) };
    let text = found.as_ref().map(|f| f.0.chars().take(300).collect::<String>()).unwrap_or_default();
    let resolve = || -> Result<(), String> {
        db.execute("DELETE FROM reports WHERE kind = ?1 AND target = ?2", params![kind, target])
            .map_err(|e| e.to_string())?;
        Ok(())
    };
    let who = || -> Result<String, String> {
        let given = c.player_id.trim();
        if !given.is_empty() {
            return owner::resolve(db, given);
        }
        found.as_ref().and_then(|f| f.1.clone()).ok_or_else(|| "pick a player".to_owned())
    };
    let now = now();
    let player;
    let mut until = None;
    match c.action {
        ModAction::Dismiss | ModAction::Remove => {
            let open: i64 = db
                .query_row(
                    "SELECT COUNT(*) FROM reports WHERE kind = ?1 AND target = ?2",
                    params![kind, target],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if open == 0 && c.action == ModAction::Dismiss {
                return Err("there are no reports on that".into());
            }
            player = found.as_ref().and_then(|f| f.1.clone()).unwrap_or_default();
            if c.action == ModAction::Remove {
                crate::take_down(db, kind, target)?;
            } else {
                crate::clear_reports(db, kind, target)?;
            }
            resolve()?;
        }
        ModAction::Warn => {
            if reason.is_empty() {
                return Err("say why".into());
            }
            player = who()?;
            db.execute(
                "INSERT INTO warnings (player_id, reason, time) VALUES (?1, ?2, ?3)",
                params![player, reason, now],
            )
            .map_err(|e| e.to_string())?;
        }
        ModAction::Mute => {
            if c.hours == 0 || c.hours > 24 * 365 {
                return Err("a mute is 1 hour to a year".into());
            }
            player = who()?;
            if owner::is_owner(db, &player) {
                return Err("that's an owner".into());
            }
            let end = now + i64::from(c.hours) * 3600;
            until = Some(end);
            db.execute(
                "INSERT INTO muted (player_id, reason, since, until) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(player_id) DO UPDATE SET reason = ?2, since = ?3, until = ?4",
                params![player, reason, now, end],
            )
            .map_err(|e| e.to_string())?;
        }
        ModAction::Unmute => {
            player = who()?;
            if db
                .execute("DELETE FROM muted WHERE player_id = ?1", params![player])
                .map_err(|e| e.to_string())?
                == 0
            {
                return Err("they aren't muted".into());
            }
        }
        ModAction::Ban => {
            if c.hours > 24 * 365 * 10 {
                return Err("a ban is up to 10 years, or for good".into());
            }
            player = who()?;
            until = (c.hours > 0).then(|| now + i64::from(c.hours) * 3600);
            let reason = if reason.is_empty() { "no reason given" } else { reason };
            owner::ban_until(db, &player, reason, until)?;
        }
        ModAction::Unban => {
            player = who()?;
            owner::unban(db, &player)?;
        }
    }
    log(db, moderator_id, c.action, kind, target, &player, reason, &text, until, c.time);
    Ok(())
}

#[derive(Deserialize)]
pub(crate) struct Signed {
    time: i64,
    signature: String,
}

fn moderator(db: &Connection, id: &str, signed: bool) -> Result<(), Failure> {
    if !signed {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    if !owner::is_owner(db, id) || owner::is_banned(db, id) {
        return Err((StatusCode::FORBIDDEN, "only the Hub's owners can moderate".into()));
    }
    Ok(())
}

fn fresh(time: i64) -> Result<(), Failure> {
    if (time - crate::social::now_ms()).abs() > 10 * 60 * 1000 {
        return Err(bad("this computer's clock is too far off"));
    }
    Ok(())
}

pub(crate) async fn get_dashboard(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<Signed>,
) -> Result<Json<Dashboard>, Failure> {
    let db = db.lock().unwrap();
    moderator(&db, &id, verify(&id, &moderation_message(&id, q.time), &q.signature))?;
    fresh(q.time)?;
    Ok(Json(dashboard(&db)))
}

pub(crate) async fn moderate(State(db): State<Db>, Json(c): Json<ModChange>) -> Result<StatusCode, Failure> {
    let db = db.lock().unwrap();
    moderator(&db, &c.moderator_id, verify(&c.moderator_id, &moderate_message(&c), &c.signature))?;
    fresh(c.time)?;
    let last: Option<i64> = db
        .query_row("SELECT MAX(signed) FROM mod_log WHERE moderator_id = ?1", params![c.moderator_id], |r| {
            r.get(0)
        })
        .map_err(internal)?;
    if last.is_some_and(|last| c.time <= last) {
        return Err((StatusCode::CONFLICT, "that was already sent".into()));
    }
    apply(&db, &c.moderator_id, &c).map_err(|e| bad(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn warnings_seen(
    State(db): State<Db>,
    Json(s): Json<WarningsSeen>,
) -> Result<StatusCode, Failure> {
    if !verify(&s.player_id, &warnings_seen_message(&s.player_id, s.time), &s.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    fresh(s.time)?;
    let db = db.lock().unwrap();
    db.execute("UPDATE warnings SET seen = 1 WHERE player_id = ?1", params![s.player_id])
        .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

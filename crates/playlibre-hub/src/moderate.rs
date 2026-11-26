use axum::Json;
use axum::extract::{Extension, Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::limits::{Caller, Limit, Limits, too_many};
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
        CREATE INDEX IF NOT EXISTS mod_log_moderator ON mod_log(moderator_id, signed);
        CREATE TABLE IF NOT EXISTS appeals (
            appeal_id   INTEGER PRIMARY KEY,
            player_id   TEXT NOT NULL,
            kind        TEXT NOT NULL,
            warning_id  INTEGER,
            since       INTEGER NOT NULL,
            penalty     TEXT NOT NULL,
            text        TEXT NOT NULL,
            time        INTEGER NOT NULL,
            state       TEXT NOT NULL DEFAULT 'open',
            reply       TEXT NOT NULL DEFAULT '',
            decided     INTEGER,
            UNIQUE (player_id, kind, since)
        );
        CREATE INDEX IF NOT EXISTS appeals_state ON appeals(state, time);",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))?;
    let _ = db.execute("ALTER TABLE reports ADD COLUMN evidence TEXT NOT NULL DEFAULT ''", []);
    let _ = db.execute("ALTER TABLE reports ADD COLUMN chat TEXT NOT NULL DEFAULT '[]'", []);
    Ok(())
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
    Accept,
    Deny,
    /// Lets a player add hats to the catalog.
    HatMaker,
    /// Takes that away.
    NoHatMaker,
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
            ModAction::Accept => "accept",
            ModAction::Deny => "deny",
            ModAction::HatMaker => "hat_maker",
            ModAction::NoHatMaker => "no_hat_maker",
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
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub chat: Vec<ChatLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatLine {
    pub from_id: String,
    pub from: Option<String>,
    pub text: String,
    pub secs_ago: i64,
    #[serde(default)]
    pub reported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct StoredLine {
    id: String,
    from: String,
    text: String,
    time: i64,
}

const CHAT_CONTEXT: i64 = 20;

/// The chat a report is about, kept with it in case it's deleted: for a
/// message, the conversation around it; for a player, the reporter's latest
/// messages with them.
pub(crate) fn chat_around(db: &Connection, kind: &str, target: &str, reporter_id: &str) -> String {
    let (other, sent) = match kind {
        "message" => match db
            .query_row("SELECT from_id, sent FROM messages WHERE message_id = ?1", params![target], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })
            .optional()
            .ok()
            .flatten()
        {
            Some((from, sent)) => (from, Some(sent)),
            None => return "[]".into(),
        },
        "player" => (target.to_owned(), None),
        _ => return "[]".into(),
    };
    let pick = |sql: &str, at: i64| -> Vec<StoredLine> {
        db.prepare(sql)
            .and_then(|mut s| {
                s.query_map(params![reporter_id, other, at, CHAT_CONTEXT], |r| {
                    Ok(StoredLine { id: r.get(0)?, from: r.get(1)?, text: r.get(2)?, time: r.get(3)? })
                })
                .map(|rows| rows.filter_map(Result::ok).collect())
            })
            .unwrap_or_default()
    };
    const BETWEEN: &str = "((from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1))";
    let mut lines = pick(
        &format!(
            "SELECT message_id, from_id, unseal(text), sent FROM messages WHERE {BETWEEN} AND sent <= ?3
             ORDER BY sent DESC, rowid DESC LIMIT ?4"
        ),
        sent.unwrap_or(i64::MAX),
    );
    lines.reverse();
    if let Some(sent) = sent {
        lines.extend(pick(
            &format!(
                "SELECT message_id, from_id, unseal(text), sent FROM messages WHERE {BETWEEN} AND sent > ?3
                 ORDER BY sent, rowid LIMIT ?4 / 4"
            ),
            sent,
        ));
    }
    serde_json::to_string(&lines).unwrap_or_else(|_| "[]".into())
}

fn chat_lines(db: &Connection, stored: &str, reported: &str) -> Vec<ChatLine> {
    let now = now();
    serde_json::from_str::<Vec<StoredLine>>(stored)
        .unwrap_or_default()
        .into_iter()
        .map(|l| ChatLine {
            from: name_of(db, &l.from),
            reported: l.id == reported,
            from_id: l.from,
            text: l.text,
            secs_ago: now - l.time,
        })
        .collect()
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
    #[serde(default)]
    pub appeals: Vec<OpenAppeal>,
    /// Players given Hat-Maker by hand (owners are ones anyway).
    #[serde(default)]
    pub hat_makers: Vec<HatMaker>,
}

/// A player who may add hats to the catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HatMaker {
    pub player_id: String,
    pub name: Option<String>,
}

/// An appeal waiting for a moderator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAppeal {
    pub appeal_id: i64,
    pub player_id: String,
    pub name: Option<String>,
    /// "ban", "mute" or "warning".
    pub kind: String,
    /// Why they were banned, muted or warned.
    pub penalty: String,
    pub text: String,
    pub secs_ago: i64,
    /// How long the ban or mute has left (None: for good, or a warning).
    pub left_secs: Option<i64>,
    /// Warnings, mutes and bans they've had before.
    pub history: Vec<LogEntry>,
}

/// An appeal, as the player who made it sees it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Appeal {
    pub appeal_id: i64,
    pub kind: String,
    pub warning_id: Option<i64>,
    pub penalty: String,
    pub text: String,
    /// "open", "accepted" or "denied".
    pub state: String,
    pub reply: String,
    pub secs_ago: i64,
}

/// Where an account stands with the Hub's moderators, as only its owner sees it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Standing {
    pub banned: Option<Penalty>,
    pub muted: Option<Penalty>,
    /// Warnings from the last 90 days, newest first.
    pub warnings: Vec<Warning>,
    pub appeals: Vec<Appeal>,
}

/// Asking to lift a ban, mute or warning.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppealAsk {
    pub player_id: String,
    /// "ban", "mute" or "warning".
    pub kind: String,
    #[serde(default)]
    pub warning_id: i64,
    pub text: String,
    pub time: i64,
    /// [`appeal_message`] signed with the player's identity key.
    pub signature: String,
}

pub fn appeal_message(player_id: &str, kind: &str, warning_id: i64, text: &str, time: i64) -> Vec<u8> {
    format!("libroxel appeal\n{player_id}\n{kind}\n{warning_id}\n{text}\n{time}").into_bytes()
}

pub const MAX_APPEAL: usize = 1000;
const WARNINGS_FOR: i64 = 90 * 86_400;

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
        "market" => "SELECT name || CASE WHEN about = '' THEN '' ELSE ': ' || about END, creator_id, NULL
                     FROM market WHERE asset_id = ?1",
        "comment" => "SELECT text, player_id, game_id FROM game_comments WHERE comment_id = ?1",
        "message" => "SELECT unseal(text), from_id, NULL FROM messages WHERE message_id = ?1",
        "group" => "SELECT name || CASE WHEN about = '' THEN '' ELSE ': ' || about END,
                    (SELECT player_id FROM group_members m WHERE m.group_id = g.group_id AND m.role = 2), group_id
                    FROM groups g WHERE group_id = ?1",
        "post" => "SELECT text, player_id, group_id FROM group_posts WHERE post_id = ?1",
        "player" => "SELECT name, player_id, NULL FROM accounts WHERE player_id = ?1",
        "appeal" => "SELECT text, player_id, NULL FROM appeals WHERE appeal_id = ?1",
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
                    "SELECT r.reporter_id, a.name, r.reason, r.time, r.evidence, r.chat FROM reports r
                     LEFT JOIN accounts a ON a.player_id = r.reporter_id
                     WHERE r.kind = ?1 AND r.target = ?2 ORDER BY r.time DESC",
                )
                .and_then(|mut s| {
                    s.query_map(params![kind, target], |r| {
                        Ok((
                            ReportReason {
                                reporter_id: r.get(0)?,
                                reporter: r.get(1)?,
                                reason: r.get(2)?,
                                secs_ago: now - r.get::<_, i64>(3)?,
                                evidence: r.get(4)?,
                                chat: Vec::new(),
                            },
                            r.get::<_, String>(5)?,
                        ))
                    })
                    .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
                })
                .unwrap_or_default()
                .into_iter()
                .map(|(mut reason, chat)| {
                    reason.chat = chat_lines(db, &chat, &target);
                    reason
                })
                .collect();
            let found = content(db, &kind, &target);
            let author_id = found.as_ref().and_then(|f| f.1.clone());
            Queued {
                hidden: kind != "player" && reasons.len() as i64 >= HIDE_AFTER_REPORTS && !cleared,
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
    let log = log_entries(db, "1", "", 200);
    let appeals = db
        .prepare(
            "SELECT p.appeal_id, p.player_id, a.name, p.kind, p.penalty, p.text, p.time,
                 CASE p.kind WHEN 'ban' THEN (SELECT b.until FROM banned b WHERE b.player_id = p.player_id)
                             WHEN 'mute' THEN (SELECT m.until FROM muted m WHERE m.player_id = p.player_id) END
             FROM appeals p LEFT JOIN accounts a ON a.player_id = p.player_id
             WHERE p.state = 'open' ORDER BY p.time LIMIT 200",
        )
        .and_then(|mut s| {
            s.query_map([], |r| {
                Ok(OpenAppeal {
                    appeal_id: r.get(0)?,
                    player_id: r.get(1)?,
                    name: r.get(2)?,
                    kind: r.get(3)?,
                    penalty: r.get(4)?,
                    text: r.get(5)?,
                    secs_ago: now - r.get::<_, i64>(6)?,
                    left_secs: r.get::<_, Option<i64>>(7)?.map(|until| until - now),
                    history: Vec::new(),
                })
            })
            .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
        })
        .unwrap_or_default()
        .into_iter()
        .map(|mut a| {
            a.history = log_entries(db, "l.player_id = ?1 AND l.action IN ('warn', 'mute', 'ban')", &a.player_id, 20);
            a
        })
        .collect();
    let hat_makers =
        owner::hat_makers(db).into_iter().map(|(player_id, name)| HatMaker { player_id, name }).collect();
    Dashboard { reports, banned, muted, log, appeals, hat_makers }
}

fn log_entries(db: &Connection, filter: &str, player_id: &str, limit: i64) -> Vec<LogEntry> {
    let now = now();
    db.prepare(&format!(
        "SELECT l.moderator_id, mo.name, l.action, l.kind, l.target, l.player_id, p.name, l.reason, l.text,
             l.until, l.time
         FROM mod_log l LEFT JOIN accounts mo ON mo.player_id = l.moderator_id
         LEFT JOIN accounts p ON p.player_id = l.player_id
         WHERE {filter} AND ?1 IS NOT NULL ORDER BY l.log_id DESC LIMIT ?2"
    ))
    .and_then(|mut s| {
        s.query_map(params![player_id, limit], |r| {
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
    .unwrap_or_default()
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
        ModAction::HatMaker | ModAction::NoHatMaker => {
            player = owner::set_hat_maker(db, &who()?, c.action == ModAction::HatMaker)?;
        }
        ModAction::Accept | ModAction::Deny => {
            if kind != "appeal" {
                return Err("pick an appeal".into());
            }
            let (appeal_kind, warning_id, state): (String, Option<i64>, String) = db
                .query_row(
                    "SELECT kind, warning_id, state FROM appeals WHERE appeal_id = ?1",
                    params![target],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(|e| e.to_string())?
                .ok_or("there is no such appeal")?;
            if state != "open" {
                return Err("that appeal was already answered".into());
            }
            player = found.as_ref().and_then(|f| f.1.clone()).unwrap_or_default();
            if c.action == ModAction::Accept {
                match appeal_kind.as_str() {
                    "ban" => {
                        let _ = owner::unban(db, &player);
                    }
                    "mute" => {
                        db.execute("DELETE FROM muted WHERE player_id = ?1", params![player])
                            .map_err(|e| e.to_string())?;
                    }
                    _ => {
                        db.execute(
                            "DELETE FROM warnings WHERE warning_id = ?1 AND player_id = ?2",
                            params![warning_id, player],
                        )
                        .map_err(|e| e.to_string())?;
                    }
                }
            }
            db.execute(
                "UPDATE appeals SET state = ?2, reply = ?3, decided = ?4 WHERE appeal_id = ?1",
                params![
                    target,
                    if c.action == ModAction::Accept { "accepted" } else { "denied" },
                    reason,
                    now
                ],
            )
            .map_err(|e| e.to_string())?;
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

pub(crate) fn standing(db: &Connection, player_id: &str) -> Standing {
    let now = now();
    let penalty = |sql: &str| -> Option<Penalty> {
        db.query_row(sql, params![player_id, now], |r| {
            Ok(Penalty {
                player_id: player_id.to_owned(),
                name: None,
                reason: r.get(0)?,
                since_secs_ago: now - r.get::<_, i64>(1)?,
                left_secs: r.get::<_, Option<i64>>(2)?.map(|until| until - now),
            })
        })
        .optional()
        .ok()
        .flatten()
    };
    let warnings = db
        .prepare(
            "SELECT warning_id, reason, time FROM warnings WHERE player_id = ?1 AND time > ?2 ORDER BY time DESC",
        )
        .and_then(|mut s| {
            s.query_map(params![player_id, now - WARNINGS_FOR], |r| {
                Ok(Warning { warning_id: r.get(0)?, reason: r.get(1)?, secs_ago: now - r.get::<_, i64>(2)? })
            })
            .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default();
    let appeals = db
        .prepare(
            "SELECT appeal_id, kind, warning_id, penalty, text, state, reply, time FROM appeals
             WHERE player_id = ?1 ORDER BY time DESC LIMIT 50",
        )
        .and_then(|mut s| {
            s.query_map(params![player_id], |r| {
                Ok(Appeal {
                    appeal_id: r.get(0)?,
                    kind: r.get(1)?,
                    warning_id: r.get(2)?,
                    penalty: r.get(3)?,
                    text: r.get(4)?,
                    state: r.get(5)?,
                    reply: r.get(6)?,
                    secs_ago: now - r.get::<_, i64>(7)?,
                })
            })
            .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default();
    Standing {
        banned: penalty(
            "SELECT reason, since, until FROM banned WHERE player_id = ?1 AND (until IS NULL OR until > ?2)",
        ),
        muted: penalty("SELECT reason, since, until FROM muted WHERE player_id = ?1 AND until > ?2"),
        warnings,
        appeals,
    }
}

pub fn file_appeal(db: &Connection, ask: &AppealAsk) -> Result<i64, String> {
    let text = ask.text.trim();
    if text.is_empty() {
        return Err("say why it should be lifted".into());
    }
    if text.chars().count() > MAX_APPEAL {
        return Err(format!("an appeal is up to {MAX_APPEAL} letters"));
    }
    let now = now();
    let (since, penalty, warning_id): (i64, String, Option<i64>) = match ask.kind.as_str() {
        "ban" => db
            .query_row(
                "SELECT since, reason FROM banned WHERE player_id = ?1 AND (until IS NULL OR until > ?2)",
                params![ask.player_id, now],
                |r| Ok((r.get(0)?, r.get(1)?, None)),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .ok_or("you aren't banned")?,
        "mute" => db
            .query_row(
                "SELECT since, reason FROM muted WHERE player_id = ?1 AND until > ?2",
                params![ask.player_id, now],
                |r| Ok((r.get(0)?, r.get(1)?, None)),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .ok_or("you aren't muted")?,
        "warning" => db
            .query_row(
                "SELECT time, reason, warning_id FROM warnings WHERE warning_id = ?1 AND player_id = ?2 AND time > ?3",
                params![ask.warning_id, ask.player_id, now - WARNINGS_FOR],
                |r| Ok((r.get(0)?, r.get(1)?, Some(r.get(2)?))),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .ok_or("there is no such warning")?,
        _ => return Err("only bans, mutes and warnings can be appealed".into()),
    };
    let done = db
        .execute(
            "INSERT OR IGNORE INTO appeals (player_id, kind, warning_id, since, penalty, text, time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![ask.player_id, ask.kind, warning_id, since, penalty, text, now],
        )
        .map_err(|e| e.to_string())?;
    if done == 0 {
        return Err("you already appealed that".into());
    }
    Ok(db.last_insert_rowid())
}

pub(crate) async fn appeal(
    State(db): State<Db>,
    State(limits): State<std::sync::Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    Json(ask): Json<AppealAsk>,
) -> Result<StatusCode, Failure> {
    if !limits.take(Limit::Report, &caller.0) {
        return Err(too_many("appeals"));
    }
    let message = appeal_message(&ask.player_id, &ask.kind, ask.warning_id, &ask.text, ask.time);
    if !verify(&ask.player_id, &message, &ask.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    fresh(ask.time)?;
    let db = db.lock().unwrap();
    file_appeal(&db, &ask).map_err(|e| bad(&e))?;
    Ok(StatusCode::CREATED)
}

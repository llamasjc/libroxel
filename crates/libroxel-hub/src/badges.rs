use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{Db, now, verify};
use crate::social::{Failure, fresh, internal};

pub const MAX_BADGES: i64 = 30;
pub const BADGE_CREDITS: u32 = 100;
pub const MAX_BADGE_NAME: usize = 40;
pub const MAX_BADGE_ABOUT: usize = 200;
const PLAYING_SLACK_SECS: i64 = 120;
const EARLY_UNTIL: i64 = 1_798_761_600;

pub const BADGE_ICONS: &[&str] = &[
    "star",
    "trophy",
    "medal",
    "crown",
    "lightning",
    "fire",
    "sword",
    "shield",
    "diamond",
    "flag",
    "target",
    "skull",
    "ghost",
    "rocket",
    "heart",
    "key",
    "clock",
    "smiley",
    "flower",
    "planet",
    "coin",
    "footprints",
    "hammer",
    "butterfly",
    "confetti",
    "compass",
    "horse",
];

pub const BADGE_COLORS: &[&str] =
    &["red", "orange", "yellow", "green", "teal", "blue", "navy", "purple", "pink", "brown", "grey", "black"];

pub struct PlatformBadge {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub icon: &'static str,
    pub color: &'static str,
    pub game: Option<&'static str>,
    pub needs: i64,
}

pub const PLATFORM_BADGES: &[PlatformBadge] = &[
    PlatformBadge {
        id: "first-steps",
        name: "First Steps",
        description: "Played a first game",
        icon: "footprints",
        color: "green",
        game: None,
        needs: 1,
    },
    PlatformBadge {
        id: "creator",
        name: "Creator",
        description: "Published a game",
        icon: "hammer",
        color: "orange",
        game: None,
        needs: 1,
    },
    PlatformBadge {
        id: "social-butterfly",
        name: "Social Butterfly",
        description: "Made 5 friends",
        icon: "butterfly",
        color: "pink",
        game: None,
        needs: 5,
    },
    PlatformBadge {
        id: "party-starter",
        name: "Party Starter",
        description: "Led a party",
        icon: "confetti",
        color: "purple",
        game: None,
        needs: 1,
    },
    PlatformBadge {
        id: "explorer",
        name: "Explorer",
        description: "Played 10 different games",
        icon: "compass",
        color: "teal",
        game: None,
        needs: 10,
    },
    PlatformBadge {
        id: "early-adopter",
        name: "Early Adopter",
        description: "Joined in 2026",
        icon: "rocket",
        color: "blue",
        game: None,
        needs: 1,
    },
    PlatformBadge {
        id: "supporter",
        name: "Supporter",
        description: "Donated to Libroxel",
        icon: "heart",
        color: "red",
        game: None,
        needs: 1,
    },
    PlatformBadge {
        id: "chess-master",
        name: "Chess Master",
        description: "Won 10 games of chess",
        icon: "horse",
        color: "navy",
        game: Some("starter:Chess"),
        needs: 1,
    },
];

pub fn platform_badge(id: &str) -> Option<&'static PlatformBadge> {
    PLATFORM_BADGES.iter().find(|b| b.id == id)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Badge {
    pub badge_id: String,
    #[serde(default)]
    pub game_id: Option<String>,
    #[serde(default)]
    pub game_title: Option<String>,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub color: String,
    #[serde(default)]
    pub earned_secs_ago: Option<i64>,
    #[serde(default)]
    pub earned_by: u64,
}

impl Badge {
    fn platform(b: &PlatformBadge) -> Self {
        Badge {
            badge_id: b.id.into(),
            game_id: b.game.map(Into::into),
            game_title: b.game.and_then(|g| g.strip_prefix("starter:")).map(Into::into),
            name: b.name.into(),
            description: b.description.into(),
            icon: b.icon.into(),
            color: b.color.into(),
            earned_secs_ago: None,
            earned_by: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BadgeAction {
    Create,
    Update,
    Remove,
}

impl BadgeAction {
    fn word(self) -> &'static str {
        match self {
            BadgeAction::Create => "create",
            BadgeAction::Update => "update",
            BadgeAction::Remove => "remove",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BadgeChange {
    pub player_id: String,
    pub action: BadgeAction,
    #[serde(default)]
    pub badge_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub color: String,
    pub time: i64,
    pub signature: String,
}

pub fn badge_message(game_id: &str, c: &BadgeChange) -> Vec<u8> {
    format!(
        "libroxel badge\n{}\n{game_id}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        c.player_id,
        c.action.word(),
        c.badge_id,
        c.name,
        c.description,
        c.icon,
        c.color,
        c.time
    )
    .into_bytes()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BadgeAward {
    pub player_id: String,
    pub game_id: String,
    pub badge_id: String,
    pub time: i64,
    pub signature: String,
}

pub fn award_message(player_id: &str, game_id: &str, badge_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel award\n{player_id}\n{game_id}\n{badge_id}\n{time}").into_bytes()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Awarded {
    pub badge: Badge,
    pub new: bool,
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS badges (
            badge_id    TEXT PRIMARY KEY,
            game_id     TEXT NOT NULL,
            name        TEXT NOT NULL,
            description TEXT NOT NULL,
            icon        TEXT NOT NULL,
            color       TEXT NOT NULL,
            created     INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS badges_game ON badges(game_id);
        CREATE TABLE IF NOT EXISTS earned (
            player_id   TEXT NOT NULL,
            badge_id    TEXT NOT NULL,
            time        INTEGER NOT NULL,
            PRIMARY KEY (player_id, badge_id)
        );
        CREATE INDEX IF NOT EXISTS earned_badge ON earned(badge_id);",
    )
    .map_err(|e| format!("could not set up badges: {e}"))
}

fn new_id() -> String {
    let mut bytes = [0u8; 9];
    getrandom::fill(&mut bytes).expect("the system has a random number source");
    data_encoding::BASE32_NOPAD.encode(&bytes).to_lowercase()
}

pub(crate) fn give(db: &Connection, player_id: &str, badge_id: &str) -> bool {
    let new = db
        .execute(
            "INSERT OR IGNORE INTO earned (player_id, badge_id, time) VALUES (?1, ?2, ?3)",
            params![player_id, badge_id, now()],
        )
        .is_ok_and(|n| n > 0);
    if platform_badge(badge_id).is_some() {
        let _ = crate::wallet::credit(db, player_id, BADGE_CREDITS, "badge", badge_id);
    }
    new
}

pub(crate) fn take(db: &Connection, player_id: &str, badge_id: &str) -> bool {
    db.execute("DELETE FROM earned WHERE player_id = ?1 AND badge_id = ?2", params![player_id, badge_id])
        .is_ok_and(|n| n > 0)
}

fn count(db: &Connection, sql: &str, player_id: &str) -> i64 {
    db.query_row(sql, params![player_id], |r| r.get(0)).unwrap_or(0)
}

pub(crate) fn refresh(db: &Connection, player_id: &str) {
    let early = format!("SELECT COUNT(*) FROM accounts WHERE player_id = ?1 AND created < {EARLY_UNTIL}");
    let checks = [
        ("first-steps", "SELECT COUNT(*) FROM played WHERE player_id = ?1"),
        ("explorer", "SELECT COUNT(*) FROM played WHERE player_id = ?1"),
        ("creator", "SELECT COUNT(*) FROM games WHERE author_id = ?1"),
        (
            "social-butterfly",
            "SELECT COUNT(*) FROM friends WHERE accepted = 1 AND (from_id = ?1 OR to_id = ?1)",
        ),
        ("early-adopter", early.as_str()),
    ];
    for (id, sql) in checks {
        let needs = platform_badge(id).map_or(1, |b| b.needs);
        if count(db, sql, player_id) >= needs {
            give(db, player_id, id);
        }
    }
}

const BADGE_COLUMNS: &str = "b.badge_id, b.game_id, g.title, b.name, b.description, b.icon, b.color";

fn badge_row(r: &rusqlite::Row) -> rusqlite::Result<Badge> {
    Ok(Badge {
        badge_id: r.get(0)?,
        game_id: r.get(1)?,
        game_title: r.get(2)?,
        name: r.get(3)?,
        description: r.get(4)?,
        icon: r.get(5)?,
        color: r.get(6)?,
        earned_secs_ago: None,
        earned_by: 0,
    })
}

fn earned_by(db: &Connection, badge_id: &str) -> u64 {
    db.query_row("SELECT COUNT(*) FROM earned WHERE badge_id = ?1", params![badge_id], |r| r.get::<_, i64>(0))
        .unwrap_or(0) as u64
}

fn find(db: &Connection, badge_id: &str) -> Result<Option<Badge>, Failure> {
    if let Some(b) = platform_badge(badge_id) {
        return Ok(Some(Badge::platform(b)));
    }
    db.query_row(
        &format!(
            "SELECT {BADGE_COLUMNS} FROM badges b LEFT JOIN games g ON g.game_id = b.game_id WHERE b.badge_id = ?1"
        ),
        params![badge_id],
        badge_row,
    )
    .optional()
    .map_err(internal)
}

pub fn earned(db: &Connection, player_id: &str) -> Vec<Badge> {
    let at = now();
    let rows: Vec<(String, i64)> = db
        .prepare("SELECT badge_id, time FROM earned WHERE player_id = ?1 ORDER BY time DESC, badge_id")
        .and_then(|mut s| {
            s.query_map(params![player_id], |r| Ok((r.get(0)?, r.get(1)?)))
                .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default();
    rows.into_iter()
        .filter_map(|(id, time)| {
            let mut badge = find(db, &id).ok().flatten()?;
            badge.earned_secs_ago = Some((at - time).max(0));
            Some(badge)
        })
        .collect()
}

pub fn of_game(db: &Connection, game_id: &str) -> Vec<Badge> {
    let mut list: Vec<Badge> =
        PLATFORM_BADGES.iter().filter(|b| b.game == Some(game_id)).map(Badge::platform).collect();
    list.extend(
        db.prepare(&format!(
            "SELECT {BADGE_COLUMNS} FROM badges b LEFT JOIN games g ON g.game_id = b.game_id
             WHERE b.game_id = ?1 ORDER BY b.created, b.badge_id"
        ))
        .and_then(|mut s| {
            s.query_map(params![game_id], badge_row)
                .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
        })
        .unwrap_or_default(),
    );
    for badge in &mut list {
        badge.earned_by = earned_by(db, &badge.badge_id);
    }
    list
}

pub(crate) async fn player_badges(State(db): State<Db>, UrlPath(id): UrlPath<String>) -> Json<Vec<Badge>> {
    let db = db.lock().unwrap();
    let player_id: Option<String> = db
        .query_row(
            "SELECT player_id FROM accounts WHERE player_id = ?1 OR name = ?1 OR number = ?2",
            params![id, id.parse::<i64>().ok()],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten();
    let Some(player_id) = player_id.filter(|p| !crate::owner::is_banned(&db, p)) else {
        return Json(Vec::new());
    };
    refresh(&db, &player_id);
    Json(earned(&db, &player_id))
}

pub(crate) async fn game_badges(State(db): State<Db>, UrlPath(id): UrlPath<String>) -> Json<Vec<Badge>> {
    let db = db.lock().unwrap();
    Json(of_game(&db, &id))
}

fn bad(why: &str) -> Failure {
    (StatusCode::BAD_REQUEST, why.to_owned())
}

fn words(text: &str, most: usize) -> Result<String, Failure> {
    let text: String = text.trim().chars().take(most).collect();
    crate::filter::clean(&text).ok_or_else(|| bad("pick friendlier words"))
}

pub(crate) async fn change_badge(
    State(db): State<Db>,
    UrlPath(game_id): UrlPath<String>,
    Json(change): Json<BadgeChange>,
) -> Result<Json<Vec<Badge>>, Failure> {
    if !verify(&change.player_id, &badge_message(&game_id, &change), &change.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    let db = db.lock().unwrap();
    if crate::owner::is_banned(&db, &change.player_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from Libroxel".into()));
    }
    crate::manage::owned(&db, &game_id, &change.player_id)?;
    fresh(&db, &change.player_id, "badge", change.time)?;
    let ours = |id: &str| -> Result<bool, Failure> {
        db.query_row(
            "SELECT 1 FROM badges WHERE badge_id = ?1 AND game_id = ?2",
            params![id, game_id],
            |_| Ok(()),
        )
        .optional()
        .map(|f| f.is_some())
        .map_err(internal)
    };
    if change.action != BadgeAction::Create && !ours(&change.badge_id)? {
        return Err((StatusCode::NOT_FOUND, "that badge isn't in this game".into()));
    }
    if change.action == BadgeAction::Remove {
        db.execute("DELETE FROM badges WHERE badge_id = ?1", params![change.badge_id]).map_err(internal)?;
        db.execute("DELETE FROM earned WHERE badge_id = ?1", params![change.badge_id]).map_err(internal)?;
        return Ok(Json(of_game(&db, &game_id)));
    }
    let name = words(&change.name, MAX_BADGE_NAME)?;
    if name.is_empty() {
        return Err(bad("a badge needs a name"));
    }
    let description = words(&change.description, MAX_BADGE_ABOUT)?;
    let icon = BADGE_ICONS.iter().find(|i| **i == change.icon).ok_or_else(|| bad("pick one of the icons"))?;
    let color =
        BADGE_COLORS.iter().find(|c| **c == change.color).ok_or_else(|| bad("pick one of the colors"))?;
    if change.action == BadgeAction::Create {
        let have: i64 = db
            .query_row("SELECT COUNT(*) FROM badges WHERE game_id = ?1", params![game_id], |r| r.get(0))
            .map_err(internal)?;
        if have >= MAX_BADGES {
            return Err(bad(&format!("a game has up to {MAX_BADGES} badges")));
        }
        db.execute(
            "INSERT INTO badges (badge_id, game_id, name, description, icon, color, created)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![new_id(), game_id, name, description, icon, color, now()],
        )
        .map_err(internal)?;
    } else {
        db.execute(
            "UPDATE badges SET name = ?2, description = ?3, icon = ?4, color = ?5 WHERE badge_id = ?1",
            params![change.badge_id, name, description, icon, color],
        )
        .map_err(internal)?;
    }
    Ok(Json(of_game(&db, &game_id)))
}

pub(crate) async fn award(
    State(db): State<Db>,
    Json(award): Json<BadgeAward>,
) -> Result<Json<Awarded>, Failure> {
    let message = award_message(&award.player_id, &award.game_id, &award.badge_id, award.time);
    if !verify(&award.player_id, &message, &award.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    let db = db.lock().unwrap();
    let me = award.player_id.as_str();
    if crate::owner::is_banned(&db, me) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from Libroxel".into()));
    }
    let badge = find(&db, &award.badge_id)?
        .filter(|b| b.game_id.as_deref() == Some(award.game_id.as_str()))
        .ok_or_else(|| (StatusCode::NOT_FOUND, "that badge isn't in this game".to_owned()))?;
    let playing: bool = db
        .query_row(
            "SELECT 1 FROM presence WHERE player_id = ?1 AND online = 1 AND game_id = ?2 AND last_seen >= ?3",
            params![me, award.game_id, now() - PLAYING_SLACK_SECS],
            |_| Ok(()),
        )
        .optional()
        .map_err(internal)?
        .is_some();
    if !playing {
        return Err((StatusCode::CONFLICT, "you aren't playing that game".into()));
    }
    fresh(&db, me, "award", award.time)?;
    let new = give(&db, me, &award.badge_id);
    Ok(Json(Awarded { badge, new }))
}

use axum::Json;
use axum::extract::{Extension, Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::limits::{Caller, Limit, Limits, too_many};
use super::social::fresh;
use super::{Db, GAME_COLUMNS, GAMES, GameInfo, ListQuery, game_row, hidden_sql, new_game_id, now};

pub const MAX_GROUPS: i64 = 30;
pub const MAX_OWNED: i64 = 5;
pub const MAX_MEMBERS: i64 = 10_000;
pub const MAX_POST: usize = 300;
const KEEP_POSTS: i64 = 200;
const POST_GAP_SECS: i64 = 5;
pub const MAX_TAG: usize = 5;
const MAX_PENDING: i64 = 500;
const VIEW_SLACK_MS: i64 = 10 * 60 * 1000;
const RESERVED: [&str; 7] = ["playlibre", "libroxel", "admin", "staff", "official", "moderator", "owner"];

pub const GROUP_COLORS: [(&str, [u8; 3]); 12] = [
    ("red", [214, 48, 49]),
    ("orange", [230, 126, 34]),
    ("yellow", [212, 172, 13]),
    ("green", [39, 174, 96]),
    ("teal", [22, 160, 133]),
    ("blue", [41, 128, 185]),
    ("navy", [36, 59, 120]),
    ("purple", [120, 80, 200]),
    ("pink", [214, 72, 150]),
    ("brown", [133, 87, 50]),
    ("grey", [110, 118, 125]),
    ("black", [30, 30, 35]),
];

pub fn group_color(name: &str) -> [u8; 3] {
    GROUP_COLORS.iter().find(|(n, _)| *n == name).map_or(GROUP_COLORS[7].1, |(_, rgb)| *rgb)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupJoin {
    #[default]
    Open,
    Request,
    Invite,
}

impl GroupJoin {
    fn word(self) -> &'static str {
        match self {
            GroupJoin::Open => "open",
            GroupJoin::Request => "request",
            GroupJoin::Invite => "invite",
        }
    }

    fn rank(self) -> i64 {
        match self {
            GroupJoin::Open => 0,
            GroupJoin::Request => 1,
            GroupJoin::Invite => 2,
        }
    }

    fn from_rank(rank: i64) -> Self {
        match rank {
            1 => GroupJoin::Request,
            2 => GroupJoin::Invite,
            _ => GroupJoin::Open,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupAction {
    Create,
    Edit,
    Join,
    Leave,
    Role,
    Kick,
    Post,
    Unpost,
    AddGame,
    RemoveGame,
    Show,
    Invite,
    Accept,
    Decline,
}

impl GroupAction {
    fn word(self) -> &'static str {
        match self {
            GroupAction::Create => "create",
            GroupAction::Edit => "edit",
            GroupAction::Join => "join",
            GroupAction::Leave => "leave",
            GroupAction::Role => "role",
            GroupAction::Kick => "kick",
            GroupAction::Post => "post",
            GroupAction::Unpost => "unpost",
            GroupAction::AddGame => "add_game",
            GroupAction::RemoveGame => "remove_game",
            GroupAction::Show => "show",
            GroupAction::Invite => "invite",
            GroupAction::Accept => "accept",
            GroupAction::Decline => "decline",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupRole {
    Member,
    Admin,
    Owner,
}

impl GroupRole {
    fn word(self) -> &'static str {
        match self {
            GroupRole::Member => "member",
            GroupRole::Admin => "admin",
            GroupRole::Owner => "owner",
        }
    }

    fn rank(self) -> i64 {
        match self {
            GroupRole::Member => 0,
            GroupRole::Admin => 1,
            GroupRole::Owner => 2,
        }
    }

    fn from_rank(rank: i64) -> Self {
        match rank {
            2 => GroupRole::Owner,
            1 => GroupRole::Admin,
            _ => GroupRole::Member,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupChange {
    pub player_id: String,
    pub action: GroupAction,
    #[serde(default)]
    pub group_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub other_id: String,
    #[serde(default)]
    pub role: Option<GroupRole>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub join: Option<GroupJoin>,
    #[serde(default)]
    pub color: String,
    pub time: i64,
    pub signature: String,
}

impl GroupChange {
    pub fn new(player_id: &str, action: GroupAction, group_id: &str, time: i64) -> Self {
        Self {
            player_id: player_id.to_owned(),
            action,
            group_id: group_id.to_owned(),
            name: String::new(),
            tag: String::new(),
            about: String::new(),
            other_id: String::new(),
            role: None,
            text: String::new(),
            join: None,
            color: String::new(),
            time,
            signature: String::new(),
        }
    }
}

pub fn group_message(c: &GroupChange) -> Vec<u8> {
    format!(
        "libroxel group\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        c.player_id,
        c.action.word(),
        c.group_id,
        c.name,
        c.tag,
        c.about,
        c.other_id,
        c.role.map_or("", GroupRole::word),
        c.text,
        c.join.map_or("", GroupJoin::word),
        c.color,
        c.time
    )
    .into_bytes()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupChanged {
    pub group_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupBadge {
    pub group_id: String,
    pub name: String,
    pub tag: String,
    #[serde(default)]
    pub color: String,
}

pub fn group_view_message(viewer: &str, group_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel group view\n{viewer}\n{group_id}\n{time}").into_bytes()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupInfo {
    pub group_id: String,
    pub name: String,
    pub tag: String,
    pub about: String,
    pub owner: String,
    pub owner_id: String,
    pub members: u32,
    pub created_secs_ago: i64,
    #[serde(default)]
    pub join: GroupJoin,
    #[serde(default)]
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupMember {
    pub player_id: String,
    #[serde(default)]
    pub number: u64,
    pub name: String,
    pub role: GroupRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupPost {
    pub post_id: i64,
    pub player_id: String,
    pub name: String,
    pub text: String,
    pub secs_ago: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupPage {
    pub group: GroupInfo,
    pub members: Vec<GroupMember>,
    pub posts: Vec<GroupPost>,
    pub games: Vec<GameInfo>,
    #[serde(default)]
    pub requests: Vec<GroupMember>,
    #[serde(default)]
    pub invites: Vec<GroupMember>,
    #[serde(default)]
    pub invited: bool,
    #[serde(default)]
    pub requested: bool,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Viewer {
    viewer: Option<String>,
    time: Option<i64>,
    signature: Option<String>,
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS groups (
            group_id    TEXT PRIMARY KEY,
            name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
            tag         TEXT NOT NULL COLLATE NOCASE,
            about       TEXT NOT NULL,
            created     INTEGER NOT NULL,
            join_mode   INTEGER NOT NULL DEFAULT 0,
            color       TEXT NOT NULL DEFAULT 'purple'
        );
        CREATE TABLE IF NOT EXISTS group_members (
            group_id    TEXT NOT NULL,
            player_id   TEXT NOT NULL,
            role        INTEGER NOT NULL,
            shown       INTEGER NOT NULL,
            joined      INTEGER NOT NULL,
            PRIMARY KEY (group_id, player_id)
        );
        CREATE INDEX IF NOT EXISTS group_members_player ON group_members(player_id);
        CREATE TABLE IF NOT EXISTS group_posts (
            post_id     INTEGER PRIMARY KEY AUTOINCREMENT,
            group_id    TEXT NOT NULL,
            player_id   TEXT NOT NULL,
            text        TEXT NOT NULL,
            time        INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS group_posts_group ON group_posts(group_id, post_id);
        CREATE TABLE IF NOT EXISTS group_games (
            game_id     TEXT PRIMARY KEY,
            group_id    TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS group_pending (
            group_id    TEXT NOT NULL,
            player_id   TEXT NOT NULL,
            invited     INTEGER NOT NULL,
            time        INTEGER NOT NULL,
            PRIMARY KEY (group_id, player_id)
        );
        CREATE INDEX IF NOT EXISTS group_pending_player ON group_pending(player_id);",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))?;
    let old: Option<String> = db
        .query_row("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'groups'", [], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    if old.is_some_and(|sql| !sql.contains("join_mode")) {
        db.execute_batch(
            "ALTER TABLE groups RENAME TO groups_old;
             CREATE TABLE groups (
                group_id    TEXT PRIMARY KEY,
                name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
                tag         TEXT NOT NULL COLLATE NOCASE,
                about       TEXT NOT NULL,
                created     INTEGER NOT NULL,
                join_mode   INTEGER NOT NULL DEFAULT 0,
                color       TEXT NOT NULL DEFAULT 'purple'
             );
             INSERT INTO groups (group_id, name, tag, about, created)
                SELECT group_id, name, tag, about, created FROM groups_old;
             DROP TABLE groups_old;",
        )
        .map_err(|e| format!("could not update the Hub's groups: {e}"))?;
    }
    db.execute("CREATE UNIQUE INDEX IF NOT EXISTS groups_tag_color ON groups(tag COLLATE NOCASE, color)", [])
        .map(|_| ())
        .map_err(|e| format!("could not set up the Hub database: {e}"))
}

type Failure = (StatusCode, String);

fn internal(e: rusqlite::Error) -> Failure {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn bad(why: &str) -> Failure {
    (StatusCode::BAD_REQUEST, why.to_owned())
}

fn clean(text: &str) -> bool {
    crate::filter::clean(text).as_deref() == Some(text)
}

pub fn valid_group_name(name: &str) -> bool {
    let n = name.chars().count();
    (3..=30).contains(&n)
        && name.trim() == name
        && !name.contains("  ")
        && name.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '\''))
        && name.chars().any(char::is_alphabetic)
}

pub fn valid_group_tag(tag: &str) -> bool {
    (2..=MAX_TAG).contains(&tag.len()) && tag.chars().all(|c| c.is_ascii_alphanumeric())
}

fn role_of(db: &Connection, group_id: &str, player_id: &str) -> Result<Option<GroupRole>, Failure> {
    db.query_row(
        "SELECT role FROM group_members WHERE group_id = ?1 AND player_id = ?2",
        params![group_id, player_id],
        |r| r.get::<_, i64>(0),
    )
    .optional()
    .map(|r| r.map(GroupRole::from_rank))
    .map_err(internal)
}

fn pending(db: &Connection, group_id: &str, player_id: &str) -> Result<Option<bool>, Failure> {
    db.query_row(
        "SELECT invited FROM group_pending WHERE group_id = ?1 AND player_id = ?2",
        params![group_id, player_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(internal)
}

fn join_mode(db: &Connection, group_id: &str) -> Result<GroupJoin, Failure> {
    db.query_row("SELECT join_mode FROM groups WHERE group_id = ?1", params![group_id], |r| r.get(0))
        .map(GroupJoin::from_rank)
        .map_err(internal)
}

fn add_member(db: &Connection, group_id: &str, player_id: &str, now: i64) -> Result<(), Failure> {
    db.execute(
        "INSERT OR IGNORE INTO group_members (group_id, player_id, role, shown, joined) VALUES (?1, ?2, 0, 0, ?3)",
        params![group_id, player_id, now],
    )
    .map_err(internal)?;
    db.execute(
        "DELETE FROM group_pending WHERE group_id = ?1 AND player_id = ?2",
        params![group_id, player_id],
    )
    .map_err(internal)?;
    show_if_none(db, group_id, player_id)
}

fn exists(db: &Connection, group_id: &str) -> Result<bool, Failure> {
    db.query_row("SELECT 1 FROM groups WHERE group_id = ?1", params![group_id], |_| Ok(()))
        .optional()
        .map(|r| r.is_some())
        .map_err(internal)
}

fn count(db: &Connection, sql: &str, value: &str) -> Result<i64, Failure> {
    db.query_row(sql, params![value], |r| r.get(0)).map_err(internal)
}

fn show_if_none(db: &Connection, group_id: &str, player_id: &str) -> Result<(), Failure> {
    db.execute(
        "UPDATE group_members SET shown = 1 WHERE group_id = ?1 AND player_id = ?2
         AND NOT EXISTS (SELECT 1 FROM group_members WHERE player_id = ?2 AND shown = 1)",
        params![group_id, player_id],
    )
    .map(|_| ())
    .map_err(internal)
}

pub(crate) fn delete_group(db: &Connection, group_id: &str) -> rusqlite::Result<usize> {
    db.execute("DELETE FROM group_members WHERE group_id = ?1", params![group_id])?;
    db.execute("DELETE FROM group_posts WHERE group_id = ?1", params![group_id])?;
    db.execute("DELETE FROM group_games WHERE group_id = ?1", params![group_id])?;
    db.execute("DELETE FROM group_pending WHERE group_id = ?1", params![group_id])?;
    db.execute("DELETE FROM groups WHERE group_id = ?1", params![group_id])
}

fn check_names(
    db: &Connection,
    c: &GroupChange,
    group_id: &str,
) -> Result<(String, String, String, String), Failure> {
    let name = c.name.trim().to_owned();
    let color =
        if c.color.trim().is_empty() { "purple".to_owned() } else { c.color.trim().to_ascii_lowercase() };
    if !GROUP_COLORS.iter().any(|(n, _)| *n == color) {
        return Err(bad("pick one of the group colours"));
    }
    let tag = c.tag.trim().to_ascii_uppercase();
    let about = c.about.trim().to_owned();
    if !valid_group_name(&name) {
        return Err(bad("a group's name is 3 to 30 letters, digits or spaces"));
    }
    if !valid_group_tag(&tag) {
        return Err(bad(&format!("a tag is 2 to {MAX_TAG} letters or digits")));
    }
    if about.chars().count() > 1000 {
        return Err(bad("that is too much to say about the group"));
    }
    if !clean(&name) || !clean(&tag) {
        return Err(bad("pick a friendlier name"));
    }
    let lower = format!("{} {}", name.to_lowercase(), tag.to_lowercase());
    if RESERVED.iter().any(|w| lower.contains(w)) {
        return Err(bad("that name or tag is kept for PlayLibre"));
    }
    let taken = |column: &str, value: &str| -> Result<bool, Failure> {
        db.query_row(
            &format!("SELECT 1 FROM groups WHERE {column} = ?1 AND group_id != ?2"),
            params![value, group_id],
            |_| Ok(()),
        )
        .optional()
        .map(|r| r.is_some())
        .map_err(internal)
    };
    if taken("name", &name)? {
        return Err((StatusCode::CONFLICT, "a group already has that name".into()));
    }
    let pair: Option<i64> = db
        .query_row(
            "SELECT 1 FROM groups WHERE tag = ?1 AND color = ?2 AND group_id != ?3",
            params![tag, color, group_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(internal)?;
    if pair.is_some() {
        return Err((
            StatusCode::CONFLICT,
            format!("a group already has {tag} in {color}; pick another tag or colour"),
        ));
    }
    let about = crate::filter::clean(&about).ok_or_else(|| bad("pick friendlier words"))?;
    Ok((name, tag, about, color))
}

pub(crate) async fn change_group(
    State(db): State<Db>,
    State(limits): State<std::sync::Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    Json(c): Json<GroupChange>,
) -> Result<Json<GroupChanged>, Failure> {
    if !crate::verify(&c.player_id, &group_message(&c), &c.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    if c.action == GroupAction::Create && !limits.take(Limit::Share, &caller.0) {
        return Err(too_many("groups made"));
    }
    let db = db.lock().unwrap();
    if crate::owner::is_banned(&db, &c.player_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from PlayLibre".into()));
    }
    let has_account: Option<i64> = db
        .query_row("SELECT 1 FROM accounts WHERE player_id = ?1", params![c.player_id], |r| r.get(0))
        .optional()
        .map_err(internal)?;
    if has_account.is_none() {
        return Err(bad("claim a name on the Avatar tab first"));
    }
    fresh(&db, &c.player_id, "group", c.time)?;
    let me = c.player_id.as_str();
    let now = now();
    let group_id = c.group_id.trim().to_owned();
    let done = |id: &str| Ok(Json(GroupChanged { group_id: id.to_owned() }));
    if matches!(c.action, GroupAction::Create | GroupAction::Edit | GroupAction::Invite) {
        crate::moderate::not_muted(&db, me)?;
    }
    if c.action == GroupAction::Create {
        let (name, tag, about, color) = check_names(&db, &c, "")?;
        if count(&db, "SELECT COUNT(*) FROM group_members WHERE player_id = ?1 AND role = 2", me)?
            >= MAX_OWNED
        {
            return Err(bad(&format!("you can own up to {MAX_OWNED} groups")));
        }
        if count(&db, "SELECT COUNT(*) FROM group_members WHERE player_id = ?1", me)? >= MAX_GROUPS {
            return Err(bad(&format!("you can be in up to {MAX_GROUPS} groups")));
        }
        let id = new_game_id();
        db.execute(
            "INSERT INTO groups (group_id, name, tag, about, created, join_mode, color)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, name, tag, about, now, c.join.unwrap_or_default().rank(), color],
        )
        .map_err(internal)?;
        db.execute(
            "INSERT INTO group_members (group_id, player_id, role, shown, joined) VALUES (?1, ?2, 2, 0, ?3)",
            params![id, me, now],
        )
        .map_err(internal)?;
        show_if_none(&db, &id, me)?;
        return done(&id);
    }
    if c.action == GroupAction::Show && group_id.is_empty() {
        db.execute("UPDATE group_members SET shown = 0 WHERE player_id = ?1", params![me])
            .map_err(internal)?;
        return done("");
    }
    if !exists(&db, &group_id)? {
        return Err((StatusCode::NOT_FOUND, "there is no group with that id".into()));
    }
    let mine = role_of(&db, &group_id, me)?;
    let need = |at_least: GroupRole| -> Result<GroupRole, Failure> {
        match mine {
            Some(role) if role >= at_least => Ok(role),
            Some(_) => Err((StatusCode::FORBIDDEN, "you can't do that in this group".into())),
            None => Err((StatusCode::FORBIDDEN, "join the group first".into())),
        }
    };
    match c.action {
        GroupAction::Create => unreachable!(),
        GroupAction::Edit => {
            need(GroupRole::Admin)?;
            let (name, tag, about, color) = check_names(&db, &c, &group_id)?;
            db.execute(
                "UPDATE groups SET name = ?2, tag = ?3, about = ?4, color = ?5,
                     join_mode = COALESCE(?6, join_mode) WHERE group_id = ?1",
                params![group_id, name, tag, about, color, c.join.map(GroupJoin::rank)],
            )
            .map_err(internal)?;
        }
        GroupAction::Join => {
            if mine.is_some() {
                return done(&group_id);
            }
            if count(&db, "SELECT COUNT(*) FROM group_members WHERE player_id = ?1", me)? >= MAX_GROUPS {
                return Err(bad(&format!("you can be in up to {MAX_GROUPS} groups")));
            }
            if count(&db, "SELECT COUNT(*) FROM group_members WHERE group_id = ?1", &group_id)? >= MAX_MEMBERS
            {
                return Err(bad("this group is full"));
            }
            let invited = pending(&db, &group_id, me)? == Some(true);
            match join_mode(&db, &group_id)? {
                GroupJoin::Open => {}
                _ if invited => {}
                GroupJoin::Request => {
                    if count(&db, "SELECT COUNT(*) FROM group_pending WHERE group_id = ?1", &group_id)?
                        >= MAX_PENDING
                    {
                        return Err(bad("this group has too many people waiting"));
                    }
                    db.execute(
                        "INSERT OR IGNORE INTO group_pending (group_id, player_id, invited, time) VALUES (?1, ?2, 0, ?3)",
                        params![group_id, me, now],
                    )
                    .map_err(internal)?;
                    return done(&group_id);
                }
                GroupJoin::Invite => return Err((StatusCode::FORBIDDEN, "this group is invite only".into())),
            }
            add_member(&db, &group_id, me, now)?;
        }
        GroupAction::Invite => {
            need(GroupRole::Admin)?;
            let them = c.other_id.trim();
            let named: Option<i64> = db
                .query_row("SELECT 1 FROM accounts WHERE player_id = ?1", params![them], |r| r.get(0))
                .optional()
                .map_err(internal)?;
            if named.is_none() || crate::owner::is_banned(&db, them) {
                return Err((StatusCode::NOT_FOUND, "there is nobody with that id".into()));
            }
            if role_of(&db, &group_id, them)?.is_some() {
                return done(&group_id);
            }
            if pending(&db, &group_id, them)? == Some(false) {
                add_member(&db, &group_id, them, now)?;
                return done(&group_id);
            }
            if count(&db, "SELECT COUNT(*) FROM group_pending WHERE group_id = ?1", &group_id)? >= MAX_PENDING
            {
                return Err(bad("this group has too many people waiting"));
            }
            db.execute(
                "INSERT OR REPLACE INTO group_pending (group_id, player_id, invited, time) VALUES (?1, ?2, 1, ?3)",
                params![group_id, them, now],
            )
            .map_err(internal)?;
        }
        GroupAction::Accept => {
            need(GroupRole::Admin)?;
            let them = c.other_id.trim();
            if pending(&db, &group_id, them)? != Some(false) {
                return Err((StatusCode::NOT_FOUND, "they haven't asked to join".into()));
            }
            if count(&db, "SELECT COUNT(*) FROM group_members WHERE player_id = ?1", them)? >= MAX_GROUPS {
                return Err(bad(&format!("they are in {MAX_GROUPS} groups already")));
            }
            add_member(&db, &group_id, them, now)?;
        }
        GroupAction::Decline => {
            need(GroupRole::Admin)?;
            db.execute(
                "DELETE FROM group_pending WHERE group_id = ?1 AND player_id = ?2",
                params![group_id, c.other_id.trim()],
            )
            .map_err(internal)?;
        }
        GroupAction::Leave => {
            if mine.is_none() {
                db.execute(
                    "DELETE FROM group_pending WHERE group_id = ?1 AND player_id = ?2",
                    params![group_id, me],
                )
                .map_err(internal)?;
                return done(&group_id);
            }
            let role = need(GroupRole::Member)?;
            let others = count(&db, "SELECT COUNT(*) FROM group_members WHERE group_id = ?1", &group_id)? - 1;
            if role == GroupRole::Owner && others > 0 {
                return Err(bad("make someone else the owner first"));
            }
            if role == GroupRole::Owner {
                delete_group(&db, &group_id).map_err(internal)?;
                return done("");
            }
            db.execute(
                "DELETE FROM group_members WHERE group_id = ?1 AND player_id = ?2",
                params![group_id, me],
            )
            .map_err(internal)?;
        }
        GroupAction::Role => {
            need(GroupRole::Owner)?;
            let them = c.other_id.as_str();
            let Some(new_role) = c.role else { return Err(bad("pick a role")) };
            if them == me {
                return Err(bad("you are the owner"));
            }
            if role_of(&db, &group_id, them)?.is_none() {
                return Err((StatusCode::NOT_FOUND, "they are not in this group".into()));
            }
            if new_role == GroupRole::Owner {
                db.execute(
                    "UPDATE group_members SET role = 1 WHERE group_id = ?1 AND player_id = ?2",
                    params![group_id, me],
                )
                .map_err(internal)?;
            }
            db.execute(
                "UPDATE group_members SET role = ?3 WHERE group_id = ?1 AND player_id = ?2",
                params![group_id, them, new_role.rank()],
            )
            .map_err(internal)?;
        }
        GroupAction::Kick => {
            let role = need(GroupRole::Admin)?;
            let them = c.other_id.as_str();
            match role_of(&db, &group_id, them)? {
                None => return Err((StatusCode::NOT_FOUND, "they are not in this group".into())),
                Some(theirs) if theirs >= role => {
                    return Err((StatusCode::FORBIDDEN, "you can't remove them".into()));
                }
                Some(_) => {}
            }
            db.execute(
                "DELETE FROM group_members WHERE group_id = ?1 AND player_id = ?2",
                params![group_id, them],
            )
            .map_err(internal)?;
        }
        GroupAction::Post => {
            need(GroupRole::Member)?;
            crate::moderate::not_muted(&db, me)?;
            let text = c.text.trim();
            if text.is_empty() {
                return Err(bad("write something first"));
            }
            let Some(text) = crate::filter::clean(text) else {
                return Err(bad("that post isn't OK to share"));
            };
            let text = text.as_str();
            if text.chars().count() > MAX_POST {
                return Err(bad(&format!("a post is up to {MAX_POST} letters")));
            }
            let last: Option<i64> = db
                .query_row("SELECT MAX(time) FROM group_posts WHERE player_id = ?1", params![me], |r| {
                    r.get(0)
                })
                .map_err(internal)?;
            if last.is_some_and(|t| now - t < POST_GAP_SECS) {
                return Err((StatusCode::TOO_MANY_REQUESTS, "wait a few seconds between posts".into()));
            }
            db.execute(
                "INSERT INTO group_posts (group_id, player_id, text, time) VALUES (?1, ?2, ?3, ?4)",
                params![group_id, me, text, now],
            )
            .map_err(internal)?;
            db.execute(
                "DELETE FROM group_posts WHERE group_id = ?1 AND post_id NOT IN
                 (SELECT post_id FROM group_posts WHERE group_id = ?1 ORDER BY post_id DESC LIMIT ?2)",
                params![group_id, KEEP_POSTS],
            )
            .map_err(internal)?;
        }
        GroupAction::Unpost => {
            let role = need(GroupRole::Member)?;
            let post_id: i64 = c.other_id.trim().parse().map_err(|_| bad("there is no such post"))?;
            let author: Option<String> = db
                .query_row(
                    "SELECT player_id FROM group_posts WHERE post_id = ?1 AND group_id = ?2",
                    params![post_id, group_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(internal)?;
            let Some(author) = author else {
                return Err((StatusCode::NOT_FOUND, "there is no such post".into()));
            };
            if author != me && role < GroupRole::Admin {
                return Err((StatusCode::FORBIDDEN, "you can't remove that post".into()));
            }
            db.execute("DELETE FROM group_posts WHERE post_id = ?1", params![post_id]).map_err(internal)?;
        }
        GroupAction::AddGame => {
            need(GroupRole::Admin)?;
            let game_id = c.other_id.trim();
            let author: Option<Option<String>> = db
                .query_row("SELECT author_id FROM games WHERE game_id = ?1", params![game_id], |r| r.get(0))
                .optional()
                .map_err(internal)?;
            match author {
                None => {
                    return Err((StatusCode::NOT_FOUND, "there is no published game with that id".into()));
                }
                Some(a) if a.as_deref() != Some(me) => {
                    return Err((StatusCode::FORBIDDEN, "only games you published can go in a group".into()));
                }
                Some(_) => {}
            }
            db.execute(
                "INSERT INTO group_games (game_id, group_id) VALUES (?1, ?2)
                 ON CONFLICT(game_id) DO UPDATE SET group_id = ?2",
                params![game_id, group_id],
            )
            .map_err(internal)?;
        }
        GroupAction::RemoveGame => {
            need(GroupRole::Admin)?;
            db.execute(
                "DELETE FROM group_games WHERE game_id = ?1 AND group_id = ?2",
                params![c.other_id.trim(), group_id],
            )
            .map_err(internal)?;
        }
        GroupAction::Show => {
            need(GroupRole::Member)?;
            db.execute(
                "UPDATE group_members SET shown = (group_id = ?2) WHERE player_id = ?1",
                params![me, group_id],
            )
            .map_err(internal)?;
        }
    }
    done(&group_id)
}

const INFO_COLUMNS: &str = "gr.group_id, gr.name, gr.tag, gr.about,
    COALESCE((SELECT a.name FROM group_members m JOIN accounts a ON a.player_id = m.player_id
              WHERE m.group_id = gr.group_id AND m.role = 2), ''),
    COALESCE((SELECT m.player_id FROM group_members m WHERE m.group_id = gr.group_id AND m.role = 2), ''),
    (SELECT COUNT(*) FROM group_members m WHERE m.group_id = gr.group_id),
    gr.created, gr.join_mode, gr.color";

fn info_row(r: &rusqlite::Row, now: i64) -> rusqlite::Result<GroupInfo> {
    Ok(GroupInfo {
        group_id: r.get(0)?,
        name: r.get(1)?,
        tag: r.get(2)?,
        about: r.get(3)?,
        owner: r.get(4)?,
        owner_id: r.get(5)?,
        members: r.get::<_, i64>(6)? as u32,
        created_secs_ago: now - r.get::<_, i64>(7)?,
        join: GroupJoin::from_rank(r.get(8)?),
        color: r.get(9)?,
    })
}

pub(crate) async fn list_groups(
    State(db): State<Db>,
    Query(query): Query<ListQuery>,
) -> Json<Vec<GroupInfo>> {
    let db = db.lock().unwrap();
    let pattern = format!("%{}%", query.q.unwrap_or_default().trim().replace(['%', '_'], ""));
    let now = now();
    let mut statement = db
        .prepare(&format!(
            "SELECT {INFO_COLUMNS} FROM groups gr
             WHERE (gr.name LIKE ?1 OR gr.tag LIKE ?1) AND NOT {hidden}
             ORDER BY 7 DESC, gr.name LIMIT 100",
            hidden = hidden_sql("group", "gr.group_id")
        ))
        .expect("the groups query is valid");
    let rows = statement
        .query_map(params![pattern], |r| info_row(r, now))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default();
    Json(rows)
}

pub(crate) async fn group_page(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Query(viewer): Query<Viewer>,
) -> Result<Json<GroupPage>, StatusCode> {
    let db = db.lock().unwrap();
    let now = now();
    let group = db
        .query_row(
            &format!(
                "SELECT {INFO_COLUMNS} FROM groups gr WHERE gr.group_id = ?1 AND NOT {}",
                hidden_sql("group", "gr.group_id")
            ),
            params![id],
            |r| info_row(r, now),
        )
        .optional()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let banned = crate::owner::banned_sql("a.player_id");
    let members = db
        .prepare(&format!(
            "SELECT a.player_id, a.number, a.name, m.role FROM group_members m
             JOIN accounts a ON a.player_id = m.player_id
             WHERE m.group_id = ?1 AND NOT {banned}
             ORDER BY m.role DESC, a.name COLLATE NOCASE LIMIT 500"
        ))
        .and_then(|mut s| {
            s.query_map(params![id], |r| {
                Ok(GroupMember {
                    player_id: r.get(0)?,
                    number: r.get::<_, i64>(1)? as u64,
                    name: r.get(2)?,
                    role: GroupRole::from_rank(r.get(3)?),
                })
            })
            .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default();
    let posts = db
        .prepare(&format!(
            "SELECT p.post_id, p.player_id, a.name, p.text, p.time FROM group_posts p
             JOIN accounts a ON a.player_id = p.player_id
             WHERE p.group_id = ?1 AND NOT {banned}
             ORDER BY p.post_id DESC LIMIT 50"
        ))
        .and_then(|mut s| {
            s.query_map(params![id], |r| {
                Ok(GroupPost {
                    post_id: r.get(0)?,
                    player_id: r.get(1)?,
                    name: r.get(2)?,
                    text: r.get(3)?,
                    secs_ago: now - r.get::<_, i64>(4)?,
                })
            })
            .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default();
    let games = db
        .prepare(&format!(
            "SELECT {GAME_COLUMNS} FROM {GAMES} WHERE gg.group_id = ?1 AND g.private = 0 AND NOT {}
             ORDER BY g.plays DESC, g.updated DESC",
            hidden_sql("game", "g.game_id")
        ))
        .and_then(|mut s| {
            s.query_map(params![id], |r| game_row(r, now)).map(|rows| rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default();
    let mut page = GroupPage {
        group,
        members,
        posts,
        games,
        requests: Vec::new(),
        invites: Vec::new(),
        invited: false,
        requested: false,
    };
    let seen = match (viewer.viewer, viewer.time, viewer.signature) {
        (Some(who), Some(time), Some(signature))
            if (time - crate::social::now_ms()).abs() <= VIEW_SLACK_MS
                && crate::verify(&who, &group_view_message(&who, &id, time), &signature) =>
        {
            Some(who)
        }
        _ => None,
    };
    if let Some(who) = seen {
        match pending(&db, &id, &who).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
            Some(true) => page.invited = true,
            Some(false) => page.requested = true,
            None => {}
        }
        let admin = role_of(&db, &id, &who).ok().flatten().is_some_and(|r| r >= GroupRole::Admin);
        if admin {
            let waiting = |invited: bool| -> Vec<GroupMember> {
                db.prepare(
                    "SELECT a.player_id, a.number, a.name FROM group_pending p
                     JOIN accounts a ON a.player_id = p.player_id
                     WHERE p.group_id = ?1 AND p.invited = ?2 ORDER BY p.time DESC",
                )
                .and_then(|mut s| {
                    s.query_map(params![id, invited], |r| {
                        Ok(GroupMember {
                            player_id: r.get(0)?,
                            number: r.get::<_, i64>(1)? as u64,
                            name: r.get(2)?,
                            role: GroupRole::Member,
                        })
                    })
                    .map(|rows| rows.filter_map(Result::ok).collect())
                })
                .unwrap_or_default()
            };
            page.requests = waiting(false);
            page.invites = waiting(true);
        }
    }
    Ok(Json(page))
}

pub fn group_invites_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel group invites\n{player_id}\n{time}").into_bytes()
}

#[derive(Debug, Deserialize)]
pub(crate) struct Signed {
    time: i64,
    signature: String,
}

pub(crate) async fn group_invites(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Query(signed): Query<Signed>,
) -> Result<Json<Vec<GroupInfo>>, Failure> {
    if !crate::verify(&id, &group_invites_message(&id, signed.time), &signed.signature) {
        return Err((StatusCode::FORBIDDEN, "only you can see your invites".into()));
    }
    if (signed.time - crate::social::now_ms()).abs() > VIEW_SLACK_MS {
        return Err(bad("this computer's clock is too far off"));
    }
    let db = db.lock().unwrap();
    let now = now();
    let rows = db
        .prepare(&format!(
            "SELECT {INFO_COLUMNS} FROM group_pending p JOIN groups gr ON gr.group_id = p.group_id
             WHERE p.player_id = ?1 AND p.invited = 1 AND NOT {}
             ORDER BY p.time DESC",
            hidden_sql("group", "gr.group_id")
        ))
        .and_then(|mut s| {
            s.query_map(params![id], |r| info_row(r, now)).map(|rows| rows.filter_map(Result::ok).collect())
        })
        .map_err(internal)?;
    Ok(Json(rows))
}

pub(crate) fn badges(db: &Connection, player_id: &str) -> (Vec<GroupBadge>, Option<GroupBadge>) {
    let rows: Vec<(GroupBadge, bool)> = db
        .prepare(&format!(
            "SELECT gr.group_id, gr.name, gr.tag, m.shown, gr.color FROM group_members m
             JOIN groups gr ON gr.group_id = m.group_id
             WHERE m.player_id = ?1 AND NOT {}
             ORDER BY m.role DESC, gr.name COLLATE NOCASE",
            hidden_sql("group", "gr.group_id")
        ))
        .and_then(|mut s| {
            s.query_map(params![player_id], |r| {
                Ok((
                    GroupBadge { group_id: r.get(0)?, name: r.get(1)?, tag: r.get(2)?, color: r.get(4)? },
                    r.get(3)?,
                ))
            })
            .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default();
    let badge = rows.iter().find(|(_, shown)| *shown).map(|(b, _)| b.clone());
    (rows.into_iter().map(|(b, _)| b).collect(), badge)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_groups_get_a_colour_and_join_mode() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE groups (
                group_id    TEXT PRIMARY KEY,
                name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
                tag         TEXT NOT NULL UNIQUE COLLATE NOCASE,
                about       TEXT NOT NULL,
                created     INTEGER NOT NULL
            );
            INSERT INTO groups VALUES ('g1', 'Builders', 'BLD', '', 1);",
        )
        .unwrap();
        create_tables(&db).unwrap();
        create_tables(&db).unwrap();
        let (join, color): (i64, String) = db
            .query_row("SELECT join_mode, color FROM groups WHERE group_id = 'g1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((join, color.as_str()), (0, "purple"));
        db.execute("INSERT INTO groups (group_id, name, tag, about, created, color) VALUES ('g2', 'B2', 'bld', '', 1, 'red')", [])
            .unwrap();
        assert!(
            db.execute("INSERT INTO groups (group_id, name, tag, about, created, color) VALUES ('g3', 'B3', 'BLD', '', 1, 'red')", [])
                .is_err()
        );
    }
}

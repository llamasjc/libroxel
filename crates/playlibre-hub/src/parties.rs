use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{Db, EXPIRES_AFTER_SECS, Person, SERVER_COLUMNS, ServerInfo, now, server_row, verify};
use crate::social::{Failure, fresh, internal, now_ms};

pub const PARTY_SIZE: i64 = 6;
const INVITE_LASTS_SECS: i64 = 10 * 60;
const VIEW_SLACK_MS: i64 = 10 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PartyAction {
    Invite,
    Accept,
    Decline,
    Leave,
    Remove,
}

impl PartyAction {
    fn word(self) -> &'static str {
        match self {
            PartyAction::Invite => "invite",
            PartyAction::Accept => "accept",
            PartyAction::Decline => "decline",
            PartyAction::Leave => "leave",
            PartyAction::Remove => "remove",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PartyChange {
    pub player_id: String,
    pub action: PartyAction,
    #[serde(default)]
    pub target: String,
    pub time: i64,
    pub signature: String,
}

pub fn party_message(player_id: &str, action: PartyAction, target: &str, time: i64) -> Vec<u8> {
    format!("libroxel party\n{player_id}\n{}\n{target}\n{time}", action.word()).into_bytes()
}

pub fn party_view_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel party view\n{player_id}\n{time}").into_bytes()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PartyMember {
    pub player_id: String,
    #[serde(default)]
    pub number: u64,
    pub name: String,
    pub look: serde_json::Value,
    pub leader: bool,
    pub online: bool,
    pub game: Option<String>,
    pub server: Option<ServerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Party {
    pub party_id: String,
    pub leader_id: String,
    pub members: Vec<PartyMember>,
    pub invited: Vec<Person>,
}

impl Party {
    pub fn leader(&self) -> Option<&PartyMember> {
        self.members.iter().find(|m| m.leader)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PartyInvite {
    pub party_id: String,
    pub from: Person,
    pub size: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PartyView {
    pub party: Option<Party>,
    pub invites: Vec<PartyInvite>,
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS parties (
            party_id    TEXT PRIMARY KEY,
            leader_id   TEXT NOT NULL,
            created     INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS party_members (
            player_id   TEXT PRIMARY KEY,
            party_id    TEXT NOT NULL,
            joined      INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS party_members_party ON party_members(party_id);
        CREATE TABLE IF NOT EXISTS party_invites (
            party_id    TEXT NOT NULL,
            player_id   TEXT NOT NULL,
            from_id     TEXT NOT NULL,
            time        INTEGER NOT NULL,
            PRIMARY KEY (party_id, player_id)
        );
        CREATE INDEX IF NOT EXISTS party_invites_player ON party_invites(player_id);",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))
}

fn new_id() -> String {
    let mut bytes = [0u8; 9];
    getrandom::fill(&mut bytes).expect("the system has a random number source");
    data_encoding::BASE32_NOPAD.encode(&bytes).to_lowercase()
}

fn party_of(db: &Connection, player_id: &str) -> Result<Option<String>, Failure> {
    db.query_row("SELECT party_id FROM party_members WHERE player_id = ?1", params![player_id], |r| r.get(0))
        .optional()
        .map_err(internal)
}

fn leader_of(db: &Connection, party_id: &str) -> Result<Option<String>, Failure> {
    db.query_row("SELECT leader_id FROM parties WHERE party_id = ?1", params![party_id], |r| r.get(0))
        .optional()
        .map_err(internal)
}

fn size_of(db: &Connection, party_id: &str) -> Result<i64, Failure> {
    db.query_row("SELECT COUNT(*) FROM party_members WHERE party_id = ?1", params![party_id], |r| r.get(0))
        .map_err(internal)
}

pub(crate) fn leave(db: &Connection, player_id: &str) -> Result<(), Failure> {
    let Some(party_id) = party_of(db, player_id)? else { return Ok(()) };
    db.execute("DELETE FROM party_members WHERE player_id = ?1", params![player_id]).map_err(internal)?;
    if leader_of(db, &party_id)?.as_deref() != Some(player_id) {
        return Ok(());
    }
    let next: Option<String> = db
        .query_row(
            "SELECT player_id FROM party_members WHERE party_id = ?1 ORDER BY joined, player_id LIMIT 1",
            params![party_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(internal)?;
    match next {
        Some(next) => {
            db.execute("UPDATE parties SET leader_id = ?2 WHERE party_id = ?1", params![party_id, next])
                .map_err(internal)?;
        }
        None => {
            db.execute("DELETE FROM parties WHERE party_id = ?1", params![party_id]).map_err(internal)?;
            db.execute("DELETE FROM party_invites WHERE party_id = ?1", params![party_id])
                .map_err(internal)?;
        }
    }
    Ok(())
}

fn bad(why: &str) -> Failure {
    (StatusCode::BAD_REQUEST, why.to_owned())
}

pub(crate) async fn change_party(
    State(db): State<Db>,
    Json(change): Json<PartyChange>,
) -> Result<Json<PartyView>, Failure> {
    let message = party_message(&change.player_id, change.action, &change.target, change.time);
    if !verify(&change.player_id, &message, &change.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    let db = db.lock().unwrap();
    if crate::owner::is_banned(&db, &change.player_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from PlayLibre".into()));
    }
    let (me, target) = (change.player_id.as_str(), change.target.as_str());
    let at = now();
    match change.action {
        PartyAction::Invite => {
            if me == target {
                return Err(bad("you're already in your party"));
            }
            if !crate::family::are_friends(&db, me, target) {
                return Err(bad("you can only invite friends"));
            }
            if crate::owner::is_banned(&db, target) {
                return Err((StatusCode::NOT_FOUND, "there is nobody with that id".into()));
            }
            if !crate::family::may_message(&db, me, target) {
                return Err((StatusCode::FORBIDDEN, "they can't get party invites".into()));
            }
            fresh(&db, me, "party", change.time)?;
            let party_id = match party_of(&db, me)? {
                Some(id) => id,
                None => {
                    let id = new_id();
                    db.execute(
                        "INSERT INTO parties (party_id, leader_id, created) VALUES (?1, ?2, ?3)",
                        params![id, me, at],
                    )
                    .map_err(internal)?;
                    db.execute(
                        "INSERT INTO party_members (player_id, party_id, joined) VALUES (?1, ?2, ?3)",
                        params![me, id, at],
                    )
                    .map_err(internal)?;
                    id
                }
            };
            if party_of(&db, target)?.as_deref() == Some(party_id.as_str()) {
                return Err(bad("they're already in your party"));
            }
            if size_of(&db, &party_id)? >= PARTY_SIZE {
                return Err(bad(&format!("a party has up to {PARTY_SIZE} people")));
            }
            db.execute(
                "INSERT INTO party_invites (party_id, player_id, from_id, time) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(party_id, player_id) DO UPDATE SET from_id = ?3, time = ?4",
                params![party_id, target, me, at],
            )
            .map_err(internal)?;
        }
        PartyAction::Accept => {
            let invited: Option<i64> = db
                .query_row(
                    "SELECT time FROM party_invites WHERE party_id = ?1 AND player_id = ?2",
                    params![target, me],
                    |r| r.get(0),
                )
                .optional()
                .map_err(internal)?;
            if invited.is_none_or(|t| at - t > INVITE_LASTS_SECS) || leader_of(&db, target)?.is_none() {
                return Err((StatusCode::NOT_FOUND, "that invite has run out".into()));
            }
            if size_of(&db, target)? >= PARTY_SIZE {
                return Err(bad("that party is full"));
            }
            fresh(&db, me, "party", change.time)?;
            if party_of(&db, me)?.as_deref() != Some(target) {
                leave(&db, me)?;
                db.execute(
                    "INSERT INTO party_members (player_id, party_id, joined) VALUES (?1, ?2, ?3)",
                    params![me, target, at],
                )
                .map_err(internal)?;
            }
            db.execute("DELETE FROM party_invites WHERE player_id = ?1", params![me]).map_err(internal)?;
            if let Some(leader) = leader_of(&db, target)? {
                crate::badges::give(&db, &leader, "party-starter");
            }
        }
        PartyAction::Decline => {
            fresh(&db, me, "party", change.time)?;
            db.execute(
                "DELETE FROM party_invites WHERE party_id = ?1 AND player_id = ?2",
                params![target, me],
            )
            .map_err(internal)?;
        }
        PartyAction::Leave => {
            fresh(&db, me, "party", change.time)?;
            leave(&db, me)?;
        }
        PartyAction::Remove => {
            let party_id = party_of(&db, me)?.ok_or_else(|| bad("you're not in a party"))?;
            if leader_of(&db, &party_id)?.as_deref() != Some(me) {
                return Err((StatusCode::FORBIDDEN, "only the party leader can do that".into()));
            }
            fresh(&db, me, "party", change.time)?;
            if party_of(&db, target)?.as_deref() == Some(party_id.as_str()) && target != me {
                leave(&db, target)?;
            }
            db.execute(
                "DELETE FROM party_invites WHERE party_id = ?1 AND player_id = ?2",
                params![party_id, target],
            )
            .map_err(internal)?;
        }
    }
    view(&db, me).map(Json)
}

#[derive(Debug, Deserialize)]
pub(crate) struct ViewSigned {
    time: i64,
    signature: String,
}

pub(crate) async fn get_party(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Query(signed): Query<ViewSigned>,
) -> Result<Json<PartyView>, Failure> {
    if !verify(&id, &party_view_message(&id, signed.time), &signed.signature) {
        return Err((StatusCode::FORBIDDEN, "only you can see your party".into()));
    }
    if (signed.time - now_ms()).abs() > VIEW_SLACK_MS {
        return Err(bad("this computer's clock is too far off"));
    }
    let db = db.lock().unwrap();
    view(&db, &id).map(Json)
}

fn view(db: &Connection, me: &str) -> Result<PartyView, Failure> {
    let at = now();
    let alive = at - EXPIRES_AFTER_SECS;
    let mut statement = db
        .prepare(
            "SELECT i.party_id, a.player_id, a.name, a.about, a.number,
                 (SELECT COUNT(*) FROM party_members m WHERE m.party_id = i.party_id)
             FROM party_invites i JOIN accounts a ON a.player_id = i.from_id
             JOIN parties p ON p.party_id = i.party_id
             WHERE i.player_id = ?1 AND i.time >= ?2 ORDER BY i.time DESC",
        )
        .map_err(internal)?;
    let invites: Vec<PartyInvite> = statement
        .query_map(params![me, at - INVITE_LASTS_SECS], |r| {
            Ok(PartyInvite {
                party_id: r.get(0)?,
                from: Person {
                    player_id: r.get(1)?,
                    name: r.get(2)?,
                    about: r.get(3)?,
                    number: r.get::<_, i64>(4)? as u64,
                },
                size: r.get::<_, i64>(5)? as u32,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .map_err(internal)?;
    let Some(party_id) = party_of(db, me)? else { return Ok(PartyView { party: None, invites }) };
    let leader_id = leader_of(db, &party_id)?.unwrap_or_default();
    let mut statement = db
        .prepare(
            "SELECT a.player_id, a.number, a.name, a.look, p.online, p.game, p.server_id, p.last_seen, w.last_seen
             FROM party_members m JOIN accounts a ON a.player_id = m.player_id
             LEFT JOIN presence p ON p.player_id = m.player_id
             LEFT JOIN web_presence w ON w.player_id = m.player_id
             WHERE m.party_id = ?1 ORDER BY m.player_id != ?2, m.joined, m.player_id",
        )
        .map_err(internal)?;
    type Row =
        (String, i64, String, String, Option<bool>, Option<String>, Option<String>, Option<i64>, Option<i64>);
    let rows: Vec<Row> = statement
        .query_map(params![party_id, leader_id], |r| {
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
            ))
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .map_err(internal)?;
    let mut members = Vec::new();
    for (player_id, number, name, look, online, game, server_id, app_seen, web_seen) in rows {
        let in_app = online == Some(true) && app_seen.is_some_and(|t| t >= alive);
        let server = match (&server_id, in_app) {
            (Some(server_id), true) => db
                .query_row(
                    &format!("SELECT {SERVER_COLUMNS} FROM servers WHERE server_id = ?1 AND last_seen >= ?2"),
                    params![server_id, alive],
                    |r| server_row(r, at),
                )
                .optional()
                .map_err(internal)?,
            _ => None,
        };
        members.push(PartyMember {
            leader: player_id == leader_id,
            player_id,
            number: number as u64,
            name,
            look: serde_json::from_str(&look).unwrap_or(serde_json::Value::Null),
            online: in_app || web_seen.is_some_and(|t| t >= alive),
            game: game.filter(|_| in_app),
            server,
        });
    }
    let mut statement = db
        .prepare(
            "SELECT a.player_id, a.name, a.about, a.number FROM party_invites i
             JOIN accounts a ON a.player_id = i.player_id
             WHERE i.party_id = ?1 AND i.time >= ?2 ORDER BY i.time",
        )
        .map_err(internal)?;
    let invited = statement
        .query_map(params![party_id, at - INVITE_LASTS_SECS], |r| {
            Ok(Person {
                player_id: r.get(0)?,
                name: r.get(1)?,
                about: r.get(2)?,
                number: r.get::<_, i64>(3)? as u64,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .map_err(internal)?;
    Ok(PartyView { party: Some(Party { party_id, leader_id, members, invited }), invites })
}

//! Messages between friends.
//!
//! Every message is signed by its sender, checked against the word filter
//! and, when the Hub has one set up, a moderation service. Only the two
//! players in a conversation can read it, again checked by a signature.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use super::{Db, Limit, Limits, Person, now, too_many, verify};
use crate::limits::Caller;
use crate::social::{fresh, now_ms};

/// The longest message, in characters.
pub const MAX_MESSAGE: usize = 500;

/// How many messages a conversation keeps; older ones go.
const KEEP: i64 = 500;

/// How many messages one read gives.
const PAGE: i64 = 100;

/// How old a signed read may be, in milliseconds.
const READ_SLACK_MS: i64 = 10 * 60 * 1000;

/// A message to send.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewMessage {
    pub from_id: String,
    pub to_id: String,
    pub text: String,
    /// Milliseconds since 1970; each message must be newer than the last.
    pub time: i64,
    /// [`message_message`] signed with the sender's identity key.
    pub signature: String,
}

/// What a sender signs.
pub fn message_message(from_id: &str, to_id: &str, text: &str, time: i64) -> Vec<u8> {
    format!("libroxel message\n{from_id}\n{to_id}\n{text}\n{time}").into_bytes()
}

/// What a player signs to read their messages.
pub fn read_messages_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel messages\n{player_id}\n{time}").into_bytes()
}

/// Blocking or unblocking a player.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BlockChange {
    pub player_id: String,
    pub other_id: String,
    /// `false` unblocks.
    pub block: bool,
    pub time: i64,
    /// [`block_message`] signed by `player_id`.
    pub signature: String,
}

pub fn block_message(player_id: &str, other_id: &str, block: bool, time: i64) -> Vec<u8> {
    format!("libroxel block\n{player_id}\n{other_id}\n{}\n{time}", u8::from(block)).into_bytes()
}

/// Marks every message to a player read.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadAll {
    pub player_id: String,
    pub time: i64,
    /// [`read_all_message`] signed by `player_id`.
    pub signature: String,
}

pub fn read_all_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel messages read\n{player_id}\n{time}").into_bytes()
}

/// One message, as the Hub keeps it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub message_id: String,
    pub from_id: String,
    pub to_id: String,
    /// With bad words masked.
    pub text: String,
    /// Seconds since 1970.
    pub sent: i64,
    pub read: bool,
}

/// Someone this player has messages with.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Conversation {
    pub with: Person,
    pub look: serde_json::Value,
    pub last: Message,
    /// Messages from them not read yet.
    pub unread: u32,
}

/// A moderation service the Hub asks about every message, such as
/// OpenAI's free moderation endpoint.
#[derive(Debug, Clone)]
pub struct Moderation {
    pub url: String,
    pub key: String,
}

impl Moderation {
    /// From `PLAYLIBRE_MODERATION_KEY` (and `PLAYLIBRE_MODERATION_URL`); `None` when unset.
    pub fn from_env() -> Option<Moderation> {
        let key = crate::setting("PLAYLIBRE_MODERATION_KEY").filter(|k| !k.trim().is_empty())?;
        let url = crate::setting("PLAYLIBRE_MODERATION_URL")
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| "https://api.openai.com/v1/moderations".to_owned());
        Some(Moderation { url, key: key.trim().to_owned() })
    }

    /// Whether the service flags `text`. A service that can't be reached lets it through.
    fn flags(&self, text: &str) -> bool {
        let agent: ureq::Agent =
            ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(4))).build().into();
        let answer = agent
            .post(&self.url)
            .header("Authorization", format!("Bearer {}", self.key))
            .send_json(serde_json::json!({ "model": "omni-moderation-latest", "input": text }))
            .and_then(|mut r| r.body_mut().read_json::<serde_json::Value>());
        match answer {
            Ok(answer) => answer["results"]
                .as_array()
                .is_some_and(|results| results.iter().any(|r| r["flagged"].as_bool() == Some(true))),
            Err(_) => false,
        }
    }
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS messages (
            message_id  TEXT PRIMARY KEY,
            from_id     TEXT NOT NULL,
            to_id       TEXT NOT NULL,
            text        TEXT NOT NULL,
            sent        INTEGER NOT NULL,
            read        INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS messages_from ON messages(from_id, to_id, sent);
        CREATE INDEX IF NOT EXISTS messages_to ON messages(to_id, from_id, sent);
        CREATE TABLE IF NOT EXISTS blocks (
            player_id   TEXT NOT NULL,
            blocked_id  TEXT NOT NULL,
            time        INTEGER NOT NULL,
            PRIMARY KEY (player_id, blocked_id)
        );
        CREATE INDEX IF NOT EXISTS blocks_blocked ON blocks(blocked_id);",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))
}

type Failure = (StatusCode, String);

fn internal(e: rusqlite::Error) -> Failure {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn friends(db: &Connection, a: &str, b: &str) -> Result<bool, Failure> {
    db.query_row(
        "SELECT EXISTS (SELECT 1 FROM friends WHERE accepted = 1
             AND ((from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1)))",
        params![a, b],
        |r| r.get(0),
    )
    .map_err(internal)
}

/// Whether either player blocked the other.
pub(crate) fn blocked(db: &Connection, a: &str, b: &str) -> bool {
    db.query_row(
        "SELECT EXISTS (SELECT 1 FROM blocks
             WHERE (player_id = ?1 AND blocked_id = ?2) OR (player_id = ?2 AND blocked_id = ?1))",
        params![a, b],
        |r| r.get(0),
    )
    .unwrap_or(false)
}

fn new_id() -> String {
    let mut bytes = [0u8; 10];
    getrandom::fill(&mut bytes).expect("the system has a random number source");
    data_encoding::BASE32_NOPAD.encode(&bytes).to_ascii_lowercase()
}

fn row(r: &rusqlite::Row, at: usize) -> rusqlite::Result<Message> {
    Ok(Message {
        message_id: r.get(at)?,
        from_id: r.get(at + 1)?,
        to_id: r.get(at + 2)?,
        text: r.get(at + 3)?,
        sent: r.get(at + 4)?,
        read: r.get(at + 5)?,
    })
}

const COLUMNS: &str = "m.message_id, m.from_id, m.to_id, unseal(m.text), m.sent, m.read";

pub(crate) async fn send_message(
    State(db): State<Db>,
    State(limits): State<Arc<Limits>>,
    State(moderation): State<Option<Arc<Moderation>>>,
    Extension(caller): Extension<Caller>,
    Json(message): Json<NewMessage>,
) -> Result<Json<Message>, Failure> {
    if !limits.take(Limit::Message, &caller.0) || !limits.take(Limit::Message, &message.from_id) {
        return Err(too_many("messages"));
    }
    let signed = message_message(&message.from_id, &message.to_id, &message.text, message.time);
    if !verify(&message.from_id, &signed, &message.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    let text = message.text.trim();
    if text.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "write something first".into()));
    }
    if text.chars().count() > MAX_MESSAGE {
        return Err((StatusCode::BAD_REQUEST, format!("a message is at most {MAX_MESSAGE} characters")));
    }
    let Some(mut text) = crate::filter::clean(text) else {
        return Err((StatusCode::BAD_REQUEST, "that message can't be sent".into()));
    };
    {
        let db = db.lock().unwrap();
        if crate::owner::is_banned(&db, &message.from_id) {
            return Err((StatusCode::FORBIDDEN, "this account is banned from PlayLibre".into()));
        }
        crate::moderate::not_muted(&db, &message.from_id)?;
        if blocked(&db, &message.from_id, &message.to_id) {
            return Err((StatusCode::FORBIDDEN, "you can't message this player".into()));
        }
        if !friends(&db, &message.from_id, &message.to_id)? {
            return Err((StatusCode::FORBIDDEN, "you can only message your friends".into()));
        }
        if !crate::may_message(&db, &message.from_id, &message.to_id) {
            return Err((StatusCode::FORBIDDEN, "messages are off for this account".into()));
        }
        let strict = |id: &str| crate::chat_level(&db, id) == crate::Chat::Strict;
        if strict(&message.from_id) || strict(&message.to_id) {
            text = crate::strict_filter(&text);
        }
    }
    if let Some(moderation) = moderation {
        let asked = text.clone();
        let flagged = tokio::task::spawn_blocking(move || moderation.flags(&asked)).await.unwrap_or(false);
        if flagged {
            return Err((StatusCode::BAD_REQUEST, "that message can't be sent".into()));
        }
    }
    let db = db.lock().unwrap();
    fresh(&db, &message.from_id, "message", message.time)?;
    let sent = Message {
        message_id: new_id(),
        from_id: message.from_id,
        to_id: message.to_id,
        text,
        sent: now(),
        read: false,
    };
    db.execute(
        "INSERT INTO messages (message_id, from_id, to_id, text, sent) VALUES (?1, ?2, ?3, seal(?4), ?5)",
        params![sent.message_id, sent.from_id, sent.to_id, sent.text, sent.sent],
    )
    .map_err(internal)?;
    let name = crate::follows::name_of(&db, &sent.from_id);
    crate::follows::notify(
        &db,
        &sent.to_id,
        "message",
        &sent.from_id,
        &format!("New message from {name}"),
        "/messages",
    );
    db.execute(
        "DELETE FROM messages WHERE message_id IN (
             SELECT message_id FROM messages
             WHERE (from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1)
             ORDER BY sent DESC, rowid DESC LIMIT -1 OFFSET ?3)",
        params![sent.from_id, sent.to_id, KEEP],
    )
    .map_err(internal)?;
    Ok(Json(sent))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReadSigned {
    time: i64,
    signature: String,
}

fn check_read(id: &str, signed: &ReadSigned) -> Result<(), Failure> {
    if !verify(id, &read_messages_message(id, signed.time), &signed.signature) {
        return Err((StatusCode::FORBIDDEN, "only you can read your messages".into()));
    }
    if (signed.time - now_ms()).abs() > READ_SLACK_MS {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    Ok(())
}

pub(crate) async fn conversations(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Query(signed): Query<ReadSigned>,
) -> Result<Json<Vec<Conversation>>, Failure> {
    check_read(&id, &signed)?;
    let db = db.lock().unwrap();
    let mut statement = db
        .prepare(&format!(
            "SELECT a.player_id, a.name, a.about, a.number, a.look, {COLUMNS},
                 (SELECT COUNT(*) FROM messages u WHERE u.from_id = a.player_id AND u.to_id = ?1 AND u.read = 0)
             FROM messages m
             JOIN accounts a ON a.player_id = CASE WHEN m.from_id = ?1 THEN m.to_id ELSE m.from_id END
             WHERE m.rowid IN (
                 SELECT MAX(rowid) FROM messages WHERE from_id = ?1 OR to_id = ?1
                 GROUP BY CASE WHEN from_id = ?1 THEN to_id ELSE from_id END)
             AND NOT {}
             AND a.player_id NOT IN (SELECT blocked_id FROM blocks WHERE player_id = ?1)
             ORDER BY m.sent DESC, m.rowid DESC LIMIT 200",
            crate::owner::banned_sql("a.player_id")
        ))
        .map_err(internal)?;
    let list = statement
        .query_map(params![id], |r| {
            let look: String = r.get(4)?;
            Ok(Conversation {
                with: Person {
                    player_id: r.get(0)?,
                    name: r.get(1)?,
                    about: r.get(2)?,
                    number: r.get::<_, i64>(3)? as u64,
                },
                look: serde_json::from_str(&look).unwrap_or(serde_json::Value::Null),
                last: row(r, 5)?,
                unread: r.get::<_, i64>(11)? as u32,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .map_err(internal)?;
    Ok(Json(list))
}

pub(crate) async fn conversation(
    State(db): State<Db>,
    UrlPath((id, other)): UrlPath<(String, String)>,
    Query(signed): Query<ReadSigned>,
) -> Result<Json<Vec<Message>>, Failure> {
    check_read(&id, &signed)?;
    let db = db.lock().unwrap();
    let mut statement = db
        .prepare(&format!(
            "SELECT * FROM (SELECT {COLUMNS}, m.rowid AS n FROM messages m
                 WHERE (m.from_id = ?1 AND m.to_id = ?2) OR (m.from_id = ?2 AND m.to_id = ?1)
                 ORDER BY m.sent DESC, m.rowid DESC LIMIT ?3)
             ORDER BY sent, n",
        ))
        .map_err(internal)?;
    let list: Vec<Message> = statement
        .query_map(params![id, other, PAGE], |r| row(r, 0))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .map_err(internal)?;
    db.execute(
        "UPDATE messages SET read = 1 WHERE from_id = ?2 AND to_id = ?1 AND read = 0",
        params![id, other],
    )
    .map_err(internal)?;
    db.execute(
        "UPDATE notifications SET seen = 1 WHERE player_id = ?1 AND kind = 'message' AND about = ?2",
        params![id, other],
    )
    .map_err(internal)?;
    Ok(Json(list))
}

pub(crate) async fn read_all(State(db): State<Db>, Json(r): Json<ReadAll>) -> Result<StatusCode, Failure> {
    if !verify(&r.player_id, &read_all_message(&r.player_id, r.time), &r.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    let db = db.lock().unwrap();
    fresh(&db, &r.player_id, "messages read", r.time)?;
    db.execute("UPDATE messages SET read = 1 WHERE to_id = ?1 AND read = 0", params![r.player_id])
        .map_err(internal)?;
    db.execute(
        "UPDATE notifications SET seen = 1 WHERE player_id = ?1 AND kind = 'message'",
        params![r.player_id],
    )
    .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

fn blocked_list(db: &Connection, id: &str) -> Result<Vec<Person>, Failure> {
    let mut statement = db
        .prepare(
            "SELECT a.player_id, a.number, a.name, a.about FROM blocks b
             JOIN accounts a ON a.player_id = b.blocked_id
             WHERE b.player_id = ?1 ORDER BY b.time DESC",
        )
        .map_err(internal)?;
    statement
        .query_map(params![id], |r| {
            Ok(Person {
                player_id: r.get(0)?,
                number: r.get::<_, i64>(1)? as u64,
                name: r.get(2)?,
                about: r.get(3)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .map_err(internal)
}

pub(crate) async fn change_block(
    State(db): State<Db>,
    Json(c): Json<BlockChange>,
) -> Result<Json<Vec<Person>>, Failure> {
    if !verify(&c.player_id, &block_message(&c.player_id, &c.other_id, c.block, c.time), &c.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    if c.player_id == c.other_id {
        return Err((StatusCode::BAD_REQUEST, "you can't block yourself".into()));
    }
    let db = db.lock().unwrap();
    fresh(&db, &c.player_id, "block", c.time)?;
    if c.block {
        let exists: bool = db
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM accounts WHERE player_id = ?1)",
                params![c.other_id],
                |r| r.get(0),
            )
            .map_err(internal)?;
        if !exists {
            return Err((StatusCode::NOT_FOUND, "there is nobody with that id".into()));
        }
        db.execute(
            "INSERT OR IGNORE INTO blocks (player_id, blocked_id, time) VALUES (?1, ?2, ?3)",
            params![c.player_id, c.other_id, now()],
        )
        .map_err(internal)?;
        db.execute(
            "DELETE FROM friends WHERE (from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1)",
            params![c.player_id, c.other_id],
        )
        .map_err(internal)?;
        db.execute(
            "UPDATE messages SET read = 1 WHERE from_id = ?2 AND to_id = ?1",
            params![c.player_id, c.other_id],
        )
        .map_err(internal)?;
        db.execute(
            "DELETE FROM notifications WHERE player_id = ?1 AND about = ?2 AND kind IN ('message', 'friend')",
            params![c.player_id, c.other_id],
        )
        .map_err(internal)?;
    } else {
        db.execute(
            "DELETE FROM blocks WHERE player_id = ?1 AND blocked_id = ?2",
            params![c.player_id, c.other_id],
        )
        .map_err(internal)?;
    }
    Ok(Json(blocked_list(&db, &c.player_id)?))
}

pub(crate) async fn blocks(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Query(signed): Query<ReadSigned>,
) -> Result<Json<Vec<Person>>, Failure> {
    check_read(&id, &signed)?;
    Ok(Json(blocked_list(&db.lock().unwrap(), &id)?))
}

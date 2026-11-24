//! Getting back into an account: recovery codes for two-step sign-in,
//! password reset by email, and the list of signed-in devices with
//! signing out everywhere.
//!
//! Resetting a password and signing out everywhere both move the account
//! to a new identity key made on the player's computer, so any key copied
//! before stops working.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::Digest;

use super::limits::{Caller, Limit, Limits, too_many};
use super::password::{PasswordSet, password_message};
use super::{Db, Mail, now, verify};

/// How many recovery codes a player gets at a time.
pub const RECOVERY_CODES: usize = 10;

/// The answer for a key that was signed out (by a reset or signing out everywhere).
pub const SIGNED_OUT: &str = "this device was signed out";

const RESET_LASTS_SECS: i64 = 30 * 60;
const RESET_TRIES: i64 = 5;

type Failure = (StatusCode, String);

fn internal(e: rusqlite::Error) -> Failure {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn hash(db: &Connection, kind: &str, code: &str) -> String {
    crate::seal::pepper(
        db,
        &data_encoding::HEXLOWER.encode(&sha2::Sha256::digest(format!("libroxel {kind}\n{code}"))),
    )
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS recovery_codes (
            player_id   TEXT NOT NULL,
            hash        TEXT NOT NULL,
            PRIMARY KEY (player_id, hash)
        );
        CREATE TABLE IF NOT EXISTS resets (
            player_id   TEXT PRIMARY KEY,
            code        TEXT NOT NULL,
            until       INTEGER NOT NULL,
            tries       INTEGER NOT NULL DEFAULT 0,
            sent_at     INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sessions (
            session_id  TEXT PRIMARY KEY,
            player_id   TEXT NOT NULL,
            device      TEXT NOT NULL,
            from_addr   TEXT NOT NULL,
            created     INTEGER NOT NULL,
            last_seen   INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS sessions_player ON sessions(player_id);
        CREATE TABLE IF NOT EXISTS moved (
            old_id      TEXT PRIMARY KEY,
            new_id      TEXT NOT NULL,
            at          INTEGER NOT NULL
        );",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))
}

// ---------------------------------------------------------------------------
// Recovery codes
// ---------------------------------------------------------------------------

fn tidy_code(code: &str) -> String {
    code.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase()
}

/// Whether `code` looks like a recovery code rather than one from the app.
pub(crate) fn is_recovery_code(code: &str) -> bool {
    tidy_code(code).chars().any(|c| c.is_ascii_alphabetic())
}

fn new_recovery_code() -> String {
    const LETTERS: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut bytes = [0u8; 10];
    getrandom::fill(&mut bytes).expect("the system has a random number source");
    let letters: String = bytes.iter().map(|b| LETTERS[*b as usize % LETTERS.len()] as char).collect();
    format!("{}-{}", &letters[..5], &letters[5..])
}

/// Replaces `player_id`'s recovery codes with new ones, given back once.
pub(crate) fn make_recovery_codes(db: &Connection, player_id: &str) -> Result<Vec<String>, Failure> {
    db.execute("DELETE FROM recovery_codes WHERE player_id = ?1", params![player_id]).map_err(internal)?;
    let codes: Vec<String> = (0..RECOVERY_CODES).map(|_| new_recovery_code()).collect();
    for code in &codes {
        db.execute(
            "INSERT OR IGNORE INTO recovery_codes (player_id, hash) VALUES (?1, ?2)",
            params![player_id, hash(db, "recovery", &tidy_code(code))],
        )
        .map_err(internal)?;
    }
    Ok(codes)
}

pub(crate) fn forget_recovery_codes(db: &Connection, player_id: &str) {
    let _ = db.execute("DELETE FROM recovery_codes WHERE player_id = ?1", params![player_id]);
}

/// Uses up one of `player_id`'s recovery codes, if `code` is one.
pub(crate) fn use_recovery_code(db: &Connection, player_id: &str, code: &str) -> bool {
    db.execute(
        "DELETE FROM recovery_codes WHERE player_id = ?1 AND hash = ?2",
        params![player_id, hash(db, "recovery", &tidy_code(code))],
    )
    .is_ok_and(|n| n == 1)
}

pub(crate) fn recovery_codes_left(db: &Connection, player_id: &str) -> u32 {
    db.query_row("SELECT COUNT(*) FROM recovery_codes WHERE player_id = ?1", params![player_id], |r| r.get(0))
        .unwrap_or(0)
}

/// Asking for a new set of recovery codes, signed with the identity key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecoveryAsk {
    pub player_id: String,
    /// Milliseconds since 1970.
    pub time: i64,
    pub signature: String,
}

pub fn recovery_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel recovery codes\n{player_id}\n{time}").into_bytes()
}

/// Recovery codes, each good for one sign-in in place of a code from the app.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RecoveryCodes {
    pub codes: Vec<String>,
}

pub(crate) async fn new_codes(
    State(db): State<Db>,
    Json(ask): Json<RecoveryAsk>,
) -> Result<Json<RecoveryCodes>, Failure> {
    if !verify(&ask.player_id, &recovery_message(&ask.player_id, ask.time), &ask.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    let db = db.lock().unwrap();
    crate::social::fresh(&db, &ask.player_id, "recovery codes", ask.time)?;
    let on: Option<i64> = db
        .query_row("SELECT 1 FROM two_step WHERE player_id = ?1", params![ask.player_id], |r| r.get(0))
        .optional()
        .map_err(internal)?;
    if on.is_none() {
        return Err((StatusCode::BAD_REQUEST, "turn on two-step sign-in first".into()));
    }
    Ok(Json(RecoveryCodes { codes: make_recovery_codes(&db, &ask.player_id)? }))
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

fn new_session_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the system has a random number source");
    data_encoding::HEXLOWER.encode(&bytes)
}

fn clip_device(device: Option<&str>) -> String {
    let device: String = device.unwrap_or("").trim().chars().filter(|c| !c.is_control()).take(40).collect();
    if device.is_empty() { "Unknown device".into() } else { device }
}

/// Starts a session for `player_id`, keeping the latest few.
pub(crate) fn start_session(db: &Connection, player_id: &str, device: Option<&str>, from: &str) -> String {
    let id = new_session_id();
    let now = now();
    let _ = db.execute(
        "INSERT INTO sessions (session_id, player_id, device, from_addr, created, last_seen)
         VALUES (pepper(?1), ?2, ?3, ?4, ?5, ?5)",
        params![id, player_id, clip_device(device), from, now],
    );
    let _ = db.execute(
        "DELETE FROM sessions WHERE player_id = ?1 AND session_id NOT IN
             (SELECT session_id FROM sessions WHERE player_id = ?1 ORDER BY last_seen DESC LIMIT 30)",
        params![player_id],
    );
    id
}

/// One device signed in to an account.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionInfo {
    pub device: String,
    pub from: String,
    pub signed_in_secs_ago: i64,
    pub seen_secs_ago: i64,
    /// Whether it's the device asking.
    pub this: bool,
}

pub(crate) fn sessions(db: &Connection, player_id: &str, this: Option<&str>) -> Vec<SessionInfo> {
    let now = now();
    if let Some(this) = this {
        let _ = db.execute(
            "UPDATE sessions SET last_seen = ?3 WHERE session_id = pepper(?1) AND player_id = ?2",
            params![this, player_id, now],
        );
    }
    let this = this.map(|t| crate::seal::pepper(db, t));
    db.prepare(
        "SELECT session_id, device, from_addr, created, last_seen FROM sessions
         WHERE player_id = ?1 ORDER BY last_seen DESC",
    )
    .and_then(|mut s| {
        s.query_map(params![player_id], |r| {
            let id: String = r.get(0)?;
            Ok(SessionInfo {
                device: r.get(1)?,
                from: r.get(2)?,
                signed_in_secs_ago: now - r.get::<_, i64>(3)?,
                seen_secs_ago: now - r.get::<_, i64>(4)?,
                this: this.as_deref() == Some(id.as_str()),
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
    })
    .unwrap_or_default()
}

/// Naming this device, for a player signed in without a session (such as
/// on the computer the account was made on).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionStart {
    pub player_id: String,
    pub device: String,
    /// Milliseconds since 1970.
    pub time: i64,
    pub signature: String,
}

pub fn session_message(player_id: &str, device: &str, time: i64) -> Vec<u8> {
    format!("libroxel session\n{player_id}\n{device}\n{time}").into_bytes()
}

/// A session started: the account (its key may be new) and the session's id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Session {
    pub player_id: String,
    pub session: String,
}

pub(crate) async fn new_session(
    State(db): State<Db>,
    Extension(caller): Extension<Caller>,
    Json(start): Json<SessionStart>,
) -> Result<Json<Session>, Failure> {
    if !verify(
        &start.player_id,
        &session_message(&start.player_id, &start.device, start.time),
        &start.signature,
    ) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    let db = db.lock().unwrap();
    if was_moved(&db, &start.player_id) {
        return Err((StatusCode::GONE, SIGNED_OUT.into()));
    }
    crate::social::fresh(&db, &start.player_id, "session", start.time)?;
    let known: Option<i64> = db
        .query_row("SELECT 1 FROM accounts WHERE player_id = ?1", params![start.player_id], |r| r.get(0))
        .optional()
        .map_err(internal)?;
    if known.is_none() {
        return Err((StatusCode::NOT_FOUND, "make the account first".into()));
    }
    let session = start_session(&db, &start.player_id, Some(&start.device), &caller.0);
    Ok(Json(Session { player_id: start.player_id, session }))
}

// ---------------------------------------------------------------------------
// Moving an account to a new key
// ---------------------------------------------------------------------------

/// Whether `player_id` was an account's key before it moved to another.
pub(crate) fn was_moved(db: &Connection, player_id: &str) -> bool {
    db.query_row("SELECT 1 FROM moved WHERE old_id = ?1", params![player_id], |_| Ok(()))
        .optional()
        .ok()
        .flatten()
        .is_some()
}

/// The columns that hold player ids, in every table.
const ID_COLUMNS: [&str; 7] =
    ["player_id", "from_id", "to_id", "author_id", "creator_id", "host_id", "reporter_id"];

/// Moves everything of `old` to `new`, with `password` as its new password.
fn move_account(db: &mut Connection, old: &str, password: &PasswordSet) -> Result<(), Failure> {
    let new = &password.player_id;
    let tx = db.transaction().map_err(internal)?;
    let tables: Vec<String> = tx
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .and_then(|mut s| s.query_map([], |r| r.get(0)).map(|rows| rows.filter_map(Result::ok).collect()))
        .map_err(internal)?;
    for table in ["sessions", "resets", "passwords"] {
        tx.execute(&format!("DELETE FROM {table} WHERE player_id = ?1"), params![old]).map_err(internal)?;
    }
    for table in &tables {
        let columns: Vec<String> = tx
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .and_then(|mut s| s.query_map([], |r| r.get(0)).map(|rows| rows.filter_map(Result::ok).collect()))
            .map_err(internal)?;
        for column in columns.iter().filter(|c| ID_COLUMNS.contains(&c.as_str())) {
            tx.execute(
                &format!("UPDATE OR REPLACE {table} SET {column} = ?2 WHERE {column} = ?1"),
                params![old, new],
            )
            .map_err(internal)?;
        }
    }
    tx.execute(
        "INSERT INTO passwords (player_id, salt, memory_kib, passes, login_id, sealed, set_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(player_id) DO UPDATE SET salt = ?2, memory_kib = ?3, passes = ?4, login_id = ?5,
             sealed = ?6, set_at = ?7",
        params![
            new,
            password.salt,
            password.memory_kib,
            password.passes,
            password.login_id,
            password.sealed,
            password.time
        ],
    )
    .map_err(internal)?;
    tx.execute("UPDATE moved SET new_id = ?2 WHERE new_id = ?1", params![old, new]).map_err(internal)?;
    tx.execute(
        "INSERT OR REPLACE INTO moved (old_id, new_id, at) VALUES (?1, ?2, ?3)",
        params![old, new, now()],
    )
    .map_err(internal)?;
    tx.commit().map_err(internal)
}

/// Checks a new key's password, as [`crate::password::set_password`] would.
fn check_new_password(db: &Connection, set: &PasswordSet) -> Result<(), Failure> {
    if !verify(&set.player_id, &password_message(set), &set.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    crate::password::check_password_set(set)?;
    let used: Option<i64> = db
        .query_row(
            "SELECT 1 FROM accounts WHERE player_id = ?1 UNION SELECT 1 FROM moved WHERE old_id = ?1",
            params![set.player_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(internal)?;
    if used.is_some() {
        return Err((StatusCode::CONFLICT, "that key is already used".into()));
    }
    Ok(())
}

fn two_step_check(db: &Connection, player_id: &str, code: Option<&str>) -> Result<(), Failure> {
    match crate::security::check_code(db, player_id, code) {
        Ok(()) => Ok(()),
        Err(why) if why == crate::security::NEEDS_CODE => Err((StatusCode::UNAUTHORIZED, why)),
        Err(why) => Err((StatusCode::FORBIDDEN, why)),
    }
}

/// Signing out every other device: the account moves to the new key in
/// `password`. Signed with the old identity key and the current password.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignOutEverywhere {
    pub player_id: String,
    /// The new key's password, signed by the new key.
    pub password: PasswordSet,
    /// A code from the authenticator app, or a recovery code.
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    /// Milliseconds since 1970.
    pub time: i64,
    /// [`sign_out_message`] signed with the old identity key.
    pub signature: String,
    /// The same, signed with the key the current password makes.
    pub login_signature: String,
}

pub fn sign_out_message(player_id: &str, password: &PasswordSet, time: i64) -> Vec<u8> {
    format!("libroxel sign out everywhere\n{player_id}\n{}\n{time}", password.signature).into_bytes()
}

pub(crate) async fn sign_out_everywhere(
    State(db): State<Db>,
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    Json(out): Json<SignOutEverywhere>,
) -> Result<Json<Session>, Failure> {
    if !limits.take(Limit::SignIn, &caller.0) {
        return Err(too_many("sign-in attempts"));
    }
    let message = sign_out_message(&out.player_id, &out.password, out.time);
    if !verify(&out.player_id, &message, &out.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    if (out.time - crate::social::now_ms()).abs() > 10 * 60 * 1000 {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    let mut db = db.lock().unwrap();
    let login: Option<(String, String)> = db
        .query_row(
            "SELECT p.login_id, a.name FROM passwords p JOIN accounts a ON a.player_id = p.player_id
             WHERE p.player_id = ?1",
            params![out.player_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(internal)?;
    let Some((login_id, name)) = login else {
        return Err((StatusCode::BAD_REQUEST, "set a password first".into()));
    };
    if limits.over(Limit::WrongPassword, &name) {
        return Err((StatusCode::TOO_MANY_REQUESTS, "too many wrong passwords; wait 15 minutes".into()));
    }
    if !verify(&login_id, &message, &out.login_signature) {
        limits.take(Limit::WrongPassword, &name);
        return Err((StatusCode::FORBIDDEN, "wrong password".into()));
    }
    if crate::owner::is_banned(&db, &out.player_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from Libroxel".into()));
    }
    check_new_password(&db, &out.password)?;
    two_step_check(&db, &out.player_id, out.code.as_deref())?;
    move_account(&mut db, &out.player_id, &out.password)?;
    let new = out.password.player_id.clone();
    let session = start_session(&db, &new, out.device.as_deref(), &caller.0);
    Ok(Json(Session { player_id: new, session }))
}

// ---------------------------------------------------------------------------
// Password reset by email
// ---------------------------------------------------------------------------

/// Asking for a reset code by mail.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResetAsk {
    pub name: String,
}

pub(crate) async fn ask_reset(
    State(db): State<Db>,
    State(mail): State<Arc<Mail>>,
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    Json(ask): Json<ResetAsk>,
) -> Result<StatusCode, Failure> {
    if !limits.take(Limit::Email, &caller.0) || !limits.take(Limit::Email, &format!("reset {}", ask.name)) {
        return Err(too_many("emails"));
    }
    if !mail.sends() {
        return Err((StatusCode::BAD_REQUEST, "this Hub can't send mail".into()));
    }
    let (to, code) = {
        let db = db.lock().unwrap();
        let found: Option<(String, String, bool)> = db
            .query_row(
                "SELECT a.player_id, unseal(e.email), e.verified FROM accounts a JOIN emails e ON e.player_id = a.player_id
                 WHERE a.name = ?1",
                params![ask.name.trim()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(internal)?;
        let Some((player_id, email, true)) = found else {
            return Err((StatusCode::NOT_FOUND, "that account has no proved email".into()));
        };
        let now = now();
        let sent: Option<i64> = db
            .query_row("SELECT sent_at FROM resets WHERE player_id = ?1", params![player_id], |r| r.get(0))
            .optional()
            .map_err(internal)?;
        if let Some(sent) = sent
            && now - sent < crate::email::RESEND_AFTER_SECS
        {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                format!(
                    "a code was just sent; ask again in {} seconds",
                    crate::email::RESEND_AFTER_SECS - (now - sent)
                ),
            ));
        }
        let code = crate::email::new_code();
        db.execute(
            "INSERT OR REPLACE INTO resets (player_id, code, until, tries, sent_at) VALUES (?1, ?2, ?3, 0, ?4)",
            params![player_id, hash(&db, "reset", &code), now + RESET_LASTS_SECS, now],
        )
        .map_err(internal)?;
        (email, code)
    };
    let subject = format!("{code} is your Libroxel reset code");
    let body = format!(
        "Hello!\n\nYour code to reset your Libroxel password is {code}\n\nIt works for 30 minutes. \
         Resetting signs you out everywhere.\n\nIf you didn't ask for it, ignore this mail: your \
         password stays the same.\n"
    );
    tokio::task::spawn_blocking(move || mail.send_letter(&to, &code, &subject, &body))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("could not send the mail: {e}")))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Finishing a reset: the code from the mail and the new key's password.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResetFinish {
    pub name: String,
    pub code: String,
    /// For accounts with two-step sign-in: a code from the app, or a recovery code.
    #[serde(default)]
    pub two_step: Option<String>,
    pub password: PasswordSet,
    #[serde(default)]
    pub device: Option<String>,
}

pub(crate) async fn finish_reset(
    State(db): State<Db>,
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    Json(reset): Json<ResetFinish>,
) -> Result<Json<Session>, Failure> {
    if !limits.take(Limit::SignIn, &caller.0) {
        return Err(too_many("sign-in attempts"));
    }
    let mut db = db.lock().unwrap();
    let found: Option<(String, String, i64, i64)> = db
        .query_row(
            "SELECT r.player_id, r.code, r.until, r.tries FROM resets r JOIN accounts a ON a.player_id = r.player_id
             WHERE a.name = ?1",
            params![reset.name.trim()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(internal)?;
    let Some((old, code, until, tries)) = found else {
        return Err((StatusCode::BAD_REQUEST, "ask for a reset code first".into()));
    };
    if until < now() || tries >= RESET_TRIES {
        return Err((StatusCode::BAD_REQUEST, "that code has run out; ask for a new one".into()));
    }
    if hash(&db, "reset", reset.code.trim()) != code {
        db.execute("UPDATE resets SET tries = tries + 1 WHERE player_id = ?1", params![old])
            .map_err(internal)?;
        return Err((StatusCode::BAD_REQUEST, "that's not the code we sent".into()));
    }
    if crate::owner::is_banned(&db, &old) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from Libroxel".into()));
    }
    check_new_password(&db, &reset.password)?;
    two_step_check(&db, &old, reset.two_step.as_deref())?;
    move_account(&mut db, &old, &reset.password)?;
    let new = reset.password.player_id.clone();
    crate::security::record_sign_in(&db, &new, &caller.0);
    let session = start_session(&db, &new, reset.device.as_deref(), &caller.0);
    Ok(Json(Session { player_id: new, session }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_codes_read_loosely() {
        let code = new_recovery_code();
        assert_eq!(code.len(), 11);
        assert!(is_recovery_code(&code));
        assert!(!is_recovery_code("123 456"));
        assert_eq!(tidy_code(&code.to_uppercase().replace('-', " ")), tidy_code(&code));
    }
}

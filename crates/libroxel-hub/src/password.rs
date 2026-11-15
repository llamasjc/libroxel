//! Signing in with a password, so an account works on another computer and
//! survives losing this one.
//!
//! An account still belongs to its identity key. Setting a password puts a
//! sealed copy of that key on the Hub: the password is stretched with
//! Argon2id on the player's computer into two keys, one that seals the
//! identity key (ChaCha20-Poly1305) and one that proves the player knows
//! the password when signing in. The password itself, and the key that
//! opens the sealed copy, never leave the player's computer, so the Hub
//! (or anyone listening to it) can't sign in as them. The Hub hands the
//! sealed copy only to someone who proves the password, and limits how
//! often anyone can try.

use argon2::{Algorithm, Argon2, Params, Version};
use axum::Json;
use axum::extract::{Extension, Path as UrlPath, State};
use axum::http::StatusCode;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::limits::{Caller, Limit, Limits, too_many};
use super::{Db, now, player_id_of, sign, verify};

/// The shortest password a Hub takes.
pub const MIN_PASSWORD: usize = 10;

/// How hard Argon2id works on a new password: 64 MiB, three passes.
pub const PASSWORD_MEMORY_KIB: u32 = 64 * 1024;
pub const PASSWORD_PASSES: u32 = 3;

/// The least a client accepts from a Hub when signing in (OWASP's floor),
/// so a Hub can't ask for a password stretched so little it's easy to guess.
const MIN_MEMORY_KIB: u32 = 19 * 1024;
const MIN_PASSES: u32 = 2;
/// The most, so a Hub can't make a client stretch for ever.
const MAX_MEMORY_KIB: u32 = 1024 * 1024;
const MAX_PASSES: u32 = 10;

/// How old a signed sign-in may be, in milliseconds.
const SIGN_IN_SLACK_MS: i64 = 10 * 60 * 1000;

/// Setting or changing an account's password, signed with its identity key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PasswordSet {
    pub player_id: String,
    /// Random bytes for Argon2id, as hex.
    pub salt: String,
    pub memory_kib: u32,
    pub passes: u32,
    /// The public half of the key the password makes for signing in.
    pub login_id: String,
    /// The identity key sealed with the password, as hex (nonce, then sealed bytes).
    pub sealed: String,
    /// Seconds since 1970; each change must be newer than the last.
    pub time: i64,
    /// [`password_message`] signed with the player's identity key.
    pub signature: String,
}

/// What a player signs to set their password.
pub fn password_message(set: &PasswordSet) -> Vec<u8> {
    format!(
        "libroxel password\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        set.player_id, set.salt, set.memory_kib, set.passes, set.login_id, set.sealed, set.time
    )
    .into_bytes()
}

/// How to stretch an account's password, as the Hub gives it out to sign in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignInSalt {
    pub salt: String,
    pub memory_kib: u32,
    pub passes: u32,
}

/// Signing in by name, signed with the key the password makes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignIn {
    pub name: String,
    /// Milliseconds since 1970.
    pub time: i64,
    pub signature: String,
}

/// What signing in signs.
pub fn sign_in_message(name: &str, time: i64) -> Vec<u8> {
    format!("libroxel sign in\n{}\n{time}", name.to_ascii_lowercase()).into_bytes()
}

/// What the Hub answers a good sign-in: the account and its sealed key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignedIn {
    pub player_id: String,
    pub sealed: String,
}

/// The two keys a password makes: one to sign in, one to seal.
struct Stretched {
    login: [u8; 32],
    seal: [u8; 32],
}

fn stretch(password: &str, salt: &[u8], memory_kib: u32, passes: u32) -> Result<Stretched, String> {
    let params =
        Params::new(memory_kib, passes, 1, Some(64)).map_err(|e| format!("bad password settings: {e}"))?;
    let mut out = [0u8; 64];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut out)
        .map_err(|e| format!("could not stretch the password: {e}"))?;
    let (login, seal) = out.split_at(32);
    Ok(Stretched { login: login.try_into().unwrap(), seal: seal.try_into().unwrap() })
}

/// Why `password` can't be used, if it can't.
pub fn password_problem(password: &str, name: &str) -> Option<String> {
    if password.chars().count() < MIN_PASSWORD {
        return Some(format!("a password is at least {MIN_PASSWORD} characters"));
    }
    if password.len() > 256 {
        return Some("that password is too long".into());
    }
    if !name.is_empty() && password.to_ascii_lowercase().contains(&name.to_ascii_lowercase()) {
        return Some("a password can't have your name in it".into());
    }
    None
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("the system has a random number source");
    bytes
}

/// Seals `secret_key` with `password` and signs it, ready for
/// [`crate::HubClient::set_password`]. Takes a moment: the password is
/// stretched the way [`PASSWORD_MEMORY_KIB`] and [`PASSWORD_PASSES`] say.
pub fn seal_password(secret_key: &[u8; 32], password: &str, time: i64) -> Result<PasswordSet, String> {
    seal_password_with(secret_key, password, time, PASSWORD_MEMORY_KIB, PASSWORD_PASSES)
}

/// [`seal_password`] with other stretching (tests use the floor, to be quick).
pub fn seal_password_with(
    secret_key: &[u8; 32],
    password: &str,
    time: i64,
    memory_kib: u32,
    passes: u32,
) -> Result<PasswordSet, String> {
    let player_id = player_id_of(secret_key);
    let salt = random::<16>();
    let keys = stretch(password, &salt, memory_kib, passes)?;
    let nonce = random::<12>();
    let sealed = ChaCha20Poly1305::new(Key::from_slice(&keys.seal))
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: secret_key, aad: player_id.as_bytes() })
        .map_err(|_| "could not seal the key".to_owned())?;
    let hex = |b: &[u8]| data_encoding::HEXLOWER.encode(b);
    let mut set = PasswordSet {
        player_id,
        salt: hex(&salt),
        memory_kib,
        passes,
        login_id: player_id_of(&keys.login),
        sealed: hex(&[nonce.as_slice(), &sealed].concat()),
        time,
        signature: String::new(),
    };
    set.signature = sign(secret_key, &password_message(&set));
    Ok(set)
}

/// Stretches `password` the way `salt` says and signs a sign-in for `name`.
/// The second half opens the sealed key the Hub sends back.
pub(crate) fn sign_in_with(
    name: &str,
    password: &str,
    salt: &SignInSalt,
    time: i64,
) -> Result<(SignIn, [u8; 32]), String> {
    if !(MIN_MEMORY_KIB..=MAX_MEMORY_KIB).contains(&salt.memory_kib)
        || !(MIN_PASSES..=MAX_PASSES).contains(&salt.passes)
    {
        return Err("this Hub asks for unsafe password settings".into());
    }
    let salt_bytes =
        data_encoding::HEXLOWER.decode(salt.salt.as_bytes()).map_err(|_| "the Hub sent a bad salt")?;
    let keys = stretch(password, &salt_bytes, salt.memory_kib, salt.passes)?;
    let signature = sign(&keys.login, &sign_in_message(name, time));
    Ok((SignIn { name: name.to_owned(), time, signature }, keys.seal))
}

/// Opens the sealed identity key from a sign-in, checking it is the account's.
pub(crate) fn open_sealed(seal: &[u8; 32], signed_in: &SignedIn) -> Result<[u8; 32], String> {
    let bytes =
        data_encoding::HEXLOWER.decode(signed_in.sealed.as_bytes()).map_err(|_| "the Hub sent a bad key")?;
    if bytes.len() < 12 {
        return Err("the Hub sent a bad key".into());
    }
    let (nonce, sealed) = bytes.split_at(12);
    let key = ChaCha20Poly1305::new(Key::from_slice(seal))
        .decrypt(Nonce::from_slice(nonce), Payload { msg: sealed, aad: signed_in.player_id.as_bytes() })
        .map_err(|_| "that password does not open this account".to_owned())?;
    let key: [u8; 32] = key.try_into().map_err(|_| "the Hub sent a bad key".to_owned())?;
    if player_id_of(&key) != signed_in.player_id {
        return Err("the Hub sent someone else's key".into());
    }
    Ok(key)
}

// ---------------------------------------------------------------------------
// The server side
// ---------------------------------------------------------------------------

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS passwords (
            player_id   TEXT PRIMARY KEY,
            salt        TEXT NOT NULL,
            memory_kib  INTEGER NOT NULL,
            passes      INTEGER NOT NULL,
            login_id    TEXT NOT NULL,
            sealed      TEXT NOT NULL,
            set_at      INTEGER NOT NULL
        );",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))
}

type Failure = (StatusCode, String);

fn internal(e: rusqlite::Error) -> Failure {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

pub(crate) async fn set_password(
    State(db): State<Db>,
    Json(set): Json<PasswordSet>,
) -> Result<StatusCode, Failure> {
    let bad = |why: &str| Err((StatusCode::BAD_REQUEST, why.to_owned()));
    if !verify(&set.player_id, &password_message(&set), &set.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    if (set.time - now()).abs() > 24 * 60 * 60 {
        return bad("this computer's clock is too far off");
    }
    if !(MIN_MEMORY_KIB..=MAX_MEMORY_KIB).contains(&set.memory_kib)
        || !(MIN_PASSES..=MAX_PASSES).contains(&set.passes)
    {
        return bad("those password settings are not safe");
    }
    let salt_ok =
        data_encoding::HEXLOWER.decode(set.salt.as_bytes()).is_ok_and(|s| (16..=64).contains(&s.len()));
    // A nonce, the 32-byte key and its 16-byte tag.
    let sealed_ok =
        data_encoding::HEXLOWER.decode(set.sealed.as_bytes()).is_ok_and(|s| s.len() == 12 + 32 + 16);
    if !salt_ok || !sealed_ok || set.login_id.parse::<iroh_base::PublicKey>().is_err() {
        return bad("that is not a sealed key the Hub can keep");
    }
    let db = db.lock().unwrap();
    if crate::owner::is_banned(&db, &set.player_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from this Hub".into()));
    }
    let has_account: Option<i64> = db
        .query_row("SELECT 1 FROM accounts WHERE player_id = ?1", params![set.player_id], |r| r.get(0))
        .optional()
        .map_err(internal)?;
    if has_account.is_none() {
        return bad("claim a name before setting a password");
    }
    let last: Option<i64> = db
        .query_row("SELECT set_at FROM passwords WHERE player_id = ?1", params![set.player_id], |r| r.get(0))
        .optional()
        .map_err(internal)?;
    if last.is_some_and(|last| set.time <= last) {
        return Err((StatusCode::CONFLICT, "a newer password is already set".into()));
    }
    db.execute(
        "INSERT INTO passwords (player_id, salt, memory_kib, passes, login_id, sealed, set_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(player_id) DO UPDATE SET salt = ?2, memory_kib = ?3, passes = ?4, login_id = ?5,
             sealed = ?6, set_at = ?7",
        params![set.player_id, set.salt, set.memory_kib, set.passes, set.login_id, set.sealed, set.time],
    )
    .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// The password row for an account name: (player_id, salt, memory, passes, login_id, sealed).
type PasswordRow = (String, String, u32, u32, String, String);

fn password_for(db: &Connection, name: &str) -> Result<Option<PasswordRow>, Failure> {
    db.query_row(
        "SELECT a.player_id, p.salt, p.memory_kib, p.passes, p.login_id, p.sealed
         FROM accounts a JOIN passwords p ON p.player_id = a.player_id WHERE a.name = ?1",
        params![name],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    )
    .optional()
    .map_err(internal)
}

pub(crate) async fn sign_in_salt(
    State(db): State<Db>,
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    UrlPath(name): UrlPath<String>,
) -> Result<Json<SignInSalt>, Failure> {
    if !limits.take(Limit::SignIn, &caller.0) {
        return Err(too_many("sign-in attempts"));
    }
    let db = db.lock().unwrap();
    let Some((_, salt, memory_kib, passes, _, _)) = password_for(&db, &name)? else {
        return Err((StatusCode::NOT_FOUND, "no one with that name has a password on this Hub".into()));
    };
    Ok(Json(SignInSalt { salt, memory_kib, passes }))
}

pub(crate) async fn sign_in(
    State(db): State<Db>,
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    Json(attempt): Json<SignIn>,
) -> Result<Json<SignedIn>, Failure> {
    if !limits.take(Limit::SignIn, &caller.0) {
        return Err(too_many("sign-in attempts"));
    }
    if limits.over(Limit::WrongPassword, &attempt.name) {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "too many wrong passwords for that name; wait 15 minutes and try again".into(),
        ));
    }
    if (attempt.time - super::social::now_ms()).abs() > SIGN_IN_SLACK_MS {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    let db = db.lock().unwrap();
    let wrong = || {
        limits.take(Limit::WrongPassword, &attempt.name);
        Err((StatusCode::FORBIDDEN, "wrong name or password".to_owned()))
    };
    let Some((player_id, _, _, _, login_id, sealed)) = password_for(&db, &attempt.name)? else {
        return wrong();
    };
    if !verify(&login_id, &sign_in_message(&attempt.name, attempt.time), &attempt.signature) {
        return wrong();
    }
    if crate::owner::is_banned(&db, &player_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from this Hub".into()));
    }
    // Each sign-in is used once.
    super::social::fresh(&db, &player_id, "sign in", attempt.time)?;
    Ok(Json(SignedIn { player_id, sealed }))
}

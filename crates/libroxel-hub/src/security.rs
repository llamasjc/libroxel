//! The checks big platforms make, for a Hub anyone can run:
//!
//! - **Two-step sign-in**: a code from an authenticator app (TOTP, RFC 6238)
//!   on top of the password when signing in on another computer.
//! - **A bot check for new accounts**: the player's computer works a small
//!   puzzle (proof of work) before the Hub makes the account, so no one can
//!   make thousands of names cheaply.
//! - **Sign-in alerts**: every sign-in on another computer is kept, and the
//!   account's owner sees them on their account page.
//! - **Verified badges**, which only whoever runs the Hub gives out.

use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use hmac::{Hmac, Mac};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::Digest;

use super::{Db, now, verify};

// ---------------------------------------------------------------------------
// The bot check
// ---------------------------------------------------------------------------

/// How hard the puzzle for a new account is: about a million tries, well
/// under a second on a player's computer.
pub const WORK_BITS: u32 = 20;

fn work_zero_bits(player_id: &str, name: &str, nonce: u64) -> u32 {
    let hash =
        sha2::Sha256::digest(format!("libroxel work\n{player_id}\n{}\n{nonce}", name.to_ascii_lowercase()));
    let mut bits = 0;
    for byte in hash {
        if byte == 0 {
            bits += 8;
        } else {
            bits += byte.leading_zeros();
            break;
        }
    }
    bits
}

/// Works the puzzle for making the account `name` for `player_id`: the
/// answer goes in [`crate::AccountUpdate::work`].
pub fn account_work(player_id: &str, name: &str) -> u64 {
    (0u64..).find(|&nonce| work_zero_bits(player_id, name, nonce) >= WORK_BITS).unwrap_or(0)
}

pub(crate) fn work_done(player_id: &str, name: &str, nonce: Option<u64>) -> bool {
    nonce.is_some_and(|nonce| work_zero_bits(player_id, name, nonce) >= WORK_BITS)
}

// ---------------------------------------------------------------------------
// Two-step codes (TOTP: 30-second steps, 6 digits, HMAC-SHA1, as every
// authenticator app expects)
// ---------------------------------------------------------------------------

const STEP_SECS: i64 = 30;

/// A new two-step secret, as the base32 text authenticator apps take.
pub fn new_two_step_secret() -> String {
    let mut bytes = [0u8; 20];
    getrandom::fill(&mut bytes).expect("the system has a random number source");
    data_encoding::BASE32_NOPAD.encode(&bytes)
}

/// The link an authenticator app reads (as a QR code, or pasted) to add the account.
pub fn two_step_link(secret: &str, name: &str, _hub: &str) -> String {
    // There's one Hub, so the authenticator app just says Libroxel.
    format!("otpauth://totp/Libroxel:{name}?secret={secret}&issuer=Libroxel&digits=6&period=30")
}

fn code_at(secret: &[u8], step: i64) -> String {
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(secret).expect("HMAC takes any key");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = (hash[19] & 0x0f) as usize;
    let number =
        u32::from_be_bytes([hash[offset] & 0x7f, hash[offset + 1], hash[offset + 2], hash[offset + 3]]);
    format!("{:06}", number % 1_000_000)
}

/// The code an authenticator app shows for `secret` at `unix_secs`.
pub fn two_step_code(secret: &str, unix_secs: i64) -> Option<String> {
    let secret = data_encoding::BASE32_NOPAD.decode(secret.trim().to_ascii_uppercase().as_bytes()).ok()?;
    Some(code_at(&secret, unix_secs.div_euclid(STEP_SECS)))
}

/// The step `code` is good for, allowing one step either side for clocks
/// that are a little off.
fn code_step(secret: &str, code: &str, unix_secs: i64) -> Option<i64> {
    let secret = data_encoding::BASE32_NOPAD.decode(secret.as_bytes()).ok()?;
    let code: String = code.chars().filter(char::is_ascii_digit).collect();
    let step = unix_secs.div_euclid(STEP_SECS);
    (step - 1..=step + 1).find(|&s| code_at(&secret, s) == code)
}

/// Turning two-step sign-in on (with a code, to show the app has it) or off.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TwoStepSet {
    pub player_id: String,
    /// The secret the authenticator app has, or `None` to turn it off.
    pub secret: Option<String>,
    /// A code from the app right now: for the new secret, or (turning it
    /// off) for the one in use.
    pub code: String,
    /// Seconds since 1970.
    pub time: i64,
    /// [`two_step_message`] signed with the player's identity key.
    pub signature: String,
}

/// What a player signs to change two-step sign-in.
pub fn two_step_message(set: &TwoStepSet) -> Vec<u8> {
    format!(
        "libroxel two-step\n{}\n{}\n{}\n{}",
        set.player_id,
        set.secret.as_deref().unwrap_or(""),
        set.code,
        set.time
    )
    .into_bytes()
}

/// The answer from signing in when the account needs a two-step code and none came.
pub const NEEDS_CODE: &str = "this account needs the code from your authenticator app";

/// Checks a two-step code for `player_id` (if they use two-step), and that
/// it hasn't been used before. `Ok` when they don't use it.
pub(crate) fn check_code(db: &Connection, player_id: &str, code: Option<&str>) -> Result<(), String> {
    let row: Option<(String, i64)> = db
        .query_row(
            "SELECT unseal(secret), last_step FROM two_step WHERE player_id = ?1",
            params![player_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((secret, last_step)) = row else { return Ok(()) };
    let Some(code) = code.filter(|c| !c.trim().is_empty()) else { return Err(NEEDS_CODE.into()) };
    if crate::recovery::is_recovery_code(code) {
        return if crate::recovery::use_recovery_code(db, player_id, code) {
            Ok(())
        } else {
            Err("wrong recovery code".into())
        };
    }
    match code_step(&secret, code, now()) {
        Some(step) if step > last_step => {
            db.execute("UPDATE two_step SET last_step = ?2 WHERE player_id = ?1", params![player_id, step])
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        Some(_) => Err("that code was already used; wait for the next one".into()),
        None => Err("wrong code".into()),
    }
}

pub(crate) async fn set_two_step(
    State(db): State<Db>,
    Json(set): Json<TwoStepSet>,
) -> Result<Json<crate::recovery::RecoveryCodes>, (StatusCode, String)> {
    if !verify(&set.player_id, &two_step_message(&set), &set.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    if (set.time - now()).abs() > 10 * 60 {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    let db = db.lock().unwrap();
    if crate::owner::is_banned(&db, &set.player_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from Libroxel".into()));
    }
    let internal = |e: rusqlite::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    match &set.secret {
        Some(secret) => {
            let secret = secret.trim().to_ascii_uppercase();
            let ok = data_encoding::BASE32_NOPAD.decode(secret.as_bytes()).is_ok_and(|s| s.len() >= 16);
            if !ok {
                return Err((StatusCode::BAD_REQUEST, "that is not a two-step secret".into()));
            }
            let Some(step) = code_step(&secret, &set.code, now()) else {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "that code does not match; check the app's clock".into(),
                ));
            };
            db.execute(
                "INSERT INTO two_step (player_id, secret, last_step) VALUES (?1, seal(?2), ?3)
                 ON CONFLICT(player_id) DO UPDATE SET secret = seal(?2), last_step = ?3",
                params![set.player_id, secret, step],
            )
            .map_err(internal)?;
            let codes = crate::recovery::make_recovery_codes(&db, &set.player_id)?;
            Ok(Json(crate::recovery::RecoveryCodes { codes }))
        }
        None => {
            check_code(&db, &set.player_id, Some(&set.code)).map_err(|e| (StatusCode::FORBIDDEN, e))?;
            db.execute("DELETE FROM two_step WHERE player_id = ?1", params![set.player_id])
                .map_err(internal)?;
            crate::recovery::forget_recovery_codes(&db, &set.player_id);
            Ok(Json(crate::recovery::RecoveryCodes::default()))
        }
    }
}

// ---------------------------------------------------------------------------
// Sign-in alerts and the account page's security part
// ---------------------------------------------------------------------------

/// What a player signs to read their own account's security.
pub fn security_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel security\n{player_id}\n{time}").into_bytes()
}

/// One sign-in on another computer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignInRecord {
    pub secs_ago: i64,
    /// The address it came from, as the Hub saw it.
    pub from: String,
}

/// How an account is kept safe, as only its owner sees it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Security {
    pub has_password: bool,
    pub two_step: bool,
    /// The account's email (only its owner sees it), if it has one.
    #[serde(default)]
    pub email: Option<String>,
    /// Whether that email is proved with a code from the mail.
    #[serde(default)]
    pub email_verified: bool,
    /// Whether this Hub mails codes (a Hub that can't keeps emails only).
    #[serde(default)]
    pub sends_mail: bool,
    /// The latest sign-ins on other computers, newest first.
    pub sign_ins: Vec<SignInRecord>,
    #[serde(default)]
    pub warnings: Vec<crate::Warning>,
    #[serde(default)]
    pub muted_secs: Option<i64>,
    /// Devices signed in, latest first.
    #[serde(default)]
    pub sessions: Vec<crate::recovery::SessionInfo>,
    #[serde(default)]
    pub recovery_codes_left: u32,
    #[serde(default)]
    pub standing: crate::Standing,
}

/// Remembers a sign-in on another computer.
pub(crate) fn record_sign_in(db: &Connection, player_id: &str, from: &str) {
    let _ = db.execute(
        "INSERT INTO sign_ins (player_id, time, from_addr) VALUES (?1, ?2, ?3)",
        params![player_id, now(), from],
    );
    // Only the latest few are kept.
    let _ = db.execute(
        "DELETE FROM sign_ins WHERE player_id = ?1 AND rowid NOT IN
             (SELECT rowid FROM sign_ins WHERE player_id = ?1 ORDER BY time DESC LIMIT 20)",
        params![player_id],
    );
}

#[derive(Debug, Deserialize)]
pub(crate) struct Signed {
    time: i64,
    signature: String,
    #[serde(default)]
    session: Option<String>,
}

pub(crate) async fn security(
    State(db): State<Db>,
    State(mail): State<std::sync::Arc<crate::Mail>>,
    UrlPath(id): UrlPath<String>,
    Query(signed): Query<Signed>,
) -> Result<Json<Security>, (StatusCode, String)> {
    if !verify(&id, &security_message(&id, signed.time), &signed.signature) {
        return Err((StatusCode::FORBIDDEN, "only you can see this".into()));
    }
    if (signed.time - crate::social::now_ms()).abs() > 10 * 60 * 1000 {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    let db = db.lock().unwrap();
    if crate::recovery::was_moved(&db, &id) {
        return Err((StatusCode::GONE, crate::recovery::SIGNED_OUT.into()));
    }
    let has = |table: &str| -> bool {
        db.query_row(&format!("SELECT 1 FROM {table} WHERE player_id = ?1"), params![id], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some()
    };
    let now = now();
    let sign_ins = db
        .prepare("SELECT time, from_addr FROM sign_ins WHERE player_id = ?1 ORDER BY time DESC LIMIT 20")
        .and_then(|mut s| {
            s.query_map(params![id], |r| {
                Ok(SignInRecord { secs_ago: now - r.get::<_, i64>(0)?, from: r.get(1)? })
            })
            .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default();
    let email = crate::email::email_of(&db, &id);
    Ok(Json(Security {
        has_password: has("passwords"),
        two_step: has("two_step"),
        email_verified: email.as_ref().is_some_and(|e| e.1),
        email: email.map(|e| e.0),
        sends_mail: mail.sends(),
        sign_ins,
        warnings: crate::moderate::unseen_warnings(&db, &id),
        muted_secs: crate::moderate::muted_for(&db, &id),
        sessions: crate::recovery::sessions(&db, &id, signed.session.as_deref()),
        recovery_codes_left: crate::recovery::recovery_codes_left(&db, &id),
        standing: crate::moderate::standing(&db, &id),
    }))
}

// ---------------------------------------------------------------------------
// Tables, and verified badges
// ---------------------------------------------------------------------------

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS two_step (
            player_id   TEXT PRIMARY KEY,
            secret      TEXT NOT NULL,
            last_step   INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sign_ins (
            player_id   TEXT NOT NULL,
            time        INTEGER NOT NULL,
            from_addr   TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS sign_ins_player ON sign_ins(player_id, time);
        CREATE TABLE IF NOT EXISTS verified (
            player_id   TEXT PRIMARY KEY,
            since       INTEGER NOT NULL
        );",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))
}

/// SQL that holds when the player id in `column` has a verified badge
/// (owners always do).
pub(crate) fn verified_sql(column: &str) -> String {
    format!(
        "(EXISTS (SELECT 1 FROM verified v WHERE v.player_id = {column})
          OR EXISTS (SELECT 1 FROM owners o WHERE o.player_id = {column}))"
    )
}

/// Gives `who` (an account name or player id) a verified badge, or takes it away.
pub fn set_verified(db: &Connection, who: &str, verified: bool) -> Result<(), String> {
    let id: String = db
        .query_row("SELECT player_id FROM accounts WHERE name = ?1 OR player_id = ?1", params![who], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("there is no account called {who}"))?;
    let sql = if verified {
        "INSERT OR IGNORE INTO verified (player_id, since) VALUES (?1, ?2)"
    } else {
        "DELETE FROM verified WHERE player_id = ?1 AND ?2 IS NOT NULL"
    };
    db.execute(sql, params![id, now()]).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_match_the_rfc_6238_examples() {
        // RFC 6238's SHA-1 secret, "12345678901234567890", and its first two times.
        let secret = data_encoding::BASE32_NOPAD.encode(b"12345678901234567890");
        assert_eq!(two_step_code(&secret, 59).unwrap(), "287082");
        assert_eq!(two_step_code(&secret, 1_111_111_109).unwrap(), "081804");
        assert_eq!(code_step(&secret, "287 082", 59), Some(1));
        assert_eq!(code_step(&secret, "287082", 59 + 30 * 5), None);
    }

    #[test]
    fn the_bot_check_is_tied_to_the_player_and_name() {
        let nonce = account_work("someone", "Ana");
        assert!(work_done("someone", "ana", Some(nonce)));
        assert!(!work_done("someone", "Ana", None));
    }
}

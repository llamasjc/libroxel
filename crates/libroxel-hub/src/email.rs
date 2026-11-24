//! Email on accounts: kept private, and proved with a 6-digit code the Hub
//! mails out.
//!
//! The Hub sends mail through any SMTP server (a provider such as Resend,
//! Brevo or Postmark, or your own), set with environment variables:
//!
//! | Variable | What |
//! | --- | --- |
//! | `LIBROXEL_SMTP_HOST` | the server, such as `smtp.resend.com` |
//! | `LIBROXEL_SMTP_PORT` | `587` (STARTTLS, the usual) or `465` (TLS from the start); a server on `localhost` is reached without TLS |
//! | `LIBROXEL_SMTP_USER`, `LIBROXEL_SMTP_PASSWORD` | the login it gives you |
//! | `LIBROXEL_MAIL_FROM` | who mail is from, such as `Libroxel <no-reply@libroxel.com>` |
//!
//! A Hub that can send mail asks every new account for an email. One that
//! can't (on a LAN, or for trying things out) keeps emails people give but
//! sends no codes.

use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::Digest;

use super::{Db, now, verify};
use crate::limits::{Caller, Limit, Limits, too_many};

/// How long a code works, in seconds.
pub const CODE_LASTS_SECS: i64 = 30 * 60;
/// How long to wait before asking for another code, in seconds.
pub const RESEND_AFTER_SECS: i64 = 60;
/// Wrong tries allowed for one code.
const MOST_TRIES: i64 = 5;

/// Where the Hub's mail goes.
#[derive(Clone, Default)]
pub enum Mail {
    /// It can't send mail: emails are kept, but no codes go out.
    #[default]
    Off,
    /// Through an SMTP server.
    Smtp(SmtpSettings),
    /// Into a list instead, for tests: each mail as (to, code).
    Outbox(Arc<Mutex<Vec<(String, String)>>>),
}

impl std::fmt::Debug for Mail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mail::Off => write!(f, "Off"),
            Mail::Smtp(s) => write!(f, "Smtp({}:{})", s.host, s.port),
            Mail::Outbox(_) => write!(f, "Outbox"),
        }
    }
}

impl Mail {
    /// The settings in `LIBROXEL_SMTP_*` and `LIBROXEL_MAIL_FROM`, if there are any.
    pub fn from_env() -> Result<Mail, String> {
        let var =
            |name: &str| std::env::var(name).ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        let Some(host) = var("LIBROXEL_SMTP_HOST") else { return Ok(Mail::Off) };
        let port = match var("LIBROXEL_SMTP_PORT") {
            Some(p) => p.parse().map_err(|_| format!("LIBROXEL_SMTP_PORT needs a number, not {p}"))?,
            None => 587,
        };
        let from = var("LIBROXEL_MAIL_FROM").ok_or("LIBROXEL_MAIL_FROM is needed with LIBROXEL_SMTP_HOST")?;
        from.parse::<lettre::message::Mailbox>()
            .map_err(|e| format!("LIBROXEL_MAIL_FROM ({from}) isn't an address: {e}"))?;
        Ok(Mail::Smtp(SmtpSettings {
            host,
            port,
            user: var("LIBROXEL_SMTP_USER").unwrap_or_default(),
            password: var("LIBROXEL_SMTP_PASSWORD").unwrap_or_default(),
            from,
        }))
    }

    /// Whether codes go out.
    pub fn sends(&self) -> bool {
        !matches!(self, Mail::Off)
    }

    /// Sends `code` to `to`. Blocks, so call it off the async threads.
    fn send(&self, to: &str, code: &str) -> Result<(), String> {
        let subject = format!("{code} is your Libroxel code");
        let body = format!(
            "Hello!\n\nYour Libroxel code is {code}\n\nType it into Libroxel to prove this email is yours. \
             It works for 30 minutes.\n\nIf you didn't ask for it, someone typed your email by mistake: \
             you can ignore this mail.\n"
        );
        self.send_letter(to, code, &subject, &body)
    }

    pub(crate) fn send_letter(&self, to: &str, code: &str, subject: &str, body: &str) -> Result<(), String> {
        match self {
            Mail::Off => Ok(()),
            Mail::Outbox(outbox) => {
                outbox.lock().unwrap().push((to.to_owned(), code.to_owned()));
                Ok(())
            }
            Mail::Smtp(settings) => settings.send(to, subject, body),
        }
    }
}

/// An SMTP server to send through.
#[derive(Clone)]
pub struct SmtpSettings {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// Who mail is from, such as `Libroxel <no-reply@libroxel.com>`.
    pub from: String,
}

impl SmtpSettings {
    fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), String> {
        use lettre::Transport;
        let message = lettre::Message::builder()
            .from(self.from.parse().map_err(|e| format!("bad sender: {e}"))?)
            .to(to.parse().map_err(|e| format!("bad address: {e}"))?)
            .subject(subject)
            .body(body.to_owned())
            .map_err(|e| format!("could not write the mail: {e}"))?;
        // A mail server on this computer (such as Postfix) is reached without TLS.
        let local = matches!(self.host.as_str(), "localhost" | "127.0.0.1" | "::1");
        let builder = if local {
            Ok(lettre::SmtpTransport::builder_dangerous(&self.host))
        } else if self.port == 465 {
            lettre::SmtpTransport::relay(&self.host)
        } else {
            lettre::SmtpTransport::starttls_relay(&self.host)
        }
        .map_err(|e| format!("could not reach the mail server: {e}"))?
        .port(self.port);
        let builder = if self.user.is_empty() {
            builder
        } else {
            builder.credentials(lettre::transport::smtp::authentication::Credentials::new(
                self.user.clone(),
                self.password.clone(),
            ))
        };
        builder.build().send(&message).map(|_| ()).map_err(|e| format!("the mail server said no: {e}"))
    }
}

/// Whether `email` looks like an address mail can go to.
pub fn valid_email(email: &str) -> bool {
    email.len() <= 254 && email.parse::<lettre::Address>().is_ok_and(|a| a.domain().contains('.'))
}

/// How emails are kept: trimmed and lower case.
pub(crate) fn tidy(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

/// Giving an account an email (or a new one), which sends a code to it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmailSet {
    pub player_id: String,
    pub email: String,
    /// When it was signed, in milliseconds since 1970.
    pub time: i64,
    /// [`email_message`] signed with the player's identity key.
    pub signature: String,
}

/// What a player signs to give their account an email.
pub fn email_message(player_id: &str, email: &str, time: i64) -> Vec<u8> {
    format!("libroxel email\n{player_id}\n{}\n{time}", tidy(email)).into_bytes()
}

/// The code from the mail, proving the email is theirs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmailCode {
    pub player_id: String,
    pub code: String,
    /// When it was signed, in milliseconds since 1970.
    pub time: i64,
    /// [`email_code_message`] signed with the player's identity key.
    pub signature: String,
}

/// What a player signs to send in the code from the mail.
pub fn email_code_message(player_id: &str, code: &str, time: i64) -> Vec<u8> {
    format!("libroxel email code\n{player_id}\n{}\n{time}", code.trim()).into_bytes()
}

/// What the Hub did with an email.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmailSent {
    /// Whether a code was mailed (a Hub that can't send mail keeps the email only).
    pub sent: bool,
    /// Whether that email was already proved.
    #[serde(default)]
    pub verified: bool,
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS emails (
            player_id   TEXT PRIMARY KEY,
            email       TEXT NOT NULL,
            verified    INTEGER NOT NULL DEFAULT 0,
            code        TEXT,
            code_until  INTEGER,
            tries       INTEGER NOT NULL DEFAULT 0,
            sent_at     INTEGER
        );",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))
}

/// An account's email, and whether it's proved.
pub(crate) fn email_of(db: &Connection, player_id: &str) -> Option<(String, bool)> {
    db.query_row("SELECT unseal(email), verified FROM emails WHERE player_id = ?1", params![player_id], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })
    .optional()
    .ok()
    .flatten()
}

fn hash(db: &Connection, code: &str) -> String {
    crate::seal::pepper(
        db,
        &data_encoding::HEXLOWER.encode(&sha2::Sha256::digest(format!("libroxel email code\n{code}"))),
    )
}

pub(crate) fn new_code() -> String {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).expect("the system has a random number source");
    format!("{:06}", u32::from_le_bytes(bytes) % 1_000_000)
}

/// A code to mail: (to, code).
pub(crate) type Outgoing = (String, String);

/// Keeps `email` for `player_id` and, when mail goes out, a fresh code for
/// it, which comes back to be sent (after the database is let go).
pub(crate) fn keep(
    db: &Connection,
    mail: &Mail,
    player_id: &str,
    email: &str,
) -> Result<(EmailSent, Option<Outgoing>), (StatusCode, String)> {
    let internal = |e: rusqlite::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    let email = tidy(email);
    if !valid_email(&email) {
        return Err((StatusCode::BAD_REQUEST, "that doesn't look like an email address".into()));
    }
    let now = now();
    let before: Option<(String, bool, Option<i64>)> = db
        .query_row(
            "SELECT unseal(email), verified, sent_at FROM emails WHERE player_id = ?1",
            params![player_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(internal)?;
    if let Some((old, true, _)) = &before
        && *old == email
    {
        return Ok((EmailSent { sent: false, verified: true }, None));
    }
    if let Some((_, _, Some(sent))) = before
        && mail.sends()
        && now - sent < RESEND_AFTER_SECS
    {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            format!("a code was just sent; ask again in {} seconds", RESEND_AFTER_SECS - (now - sent)),
        ));
    }
    let code = mail.sends().then(new_code);
    db.execute(
        "INSERT INTO emails (player_id, email, verified, code, code_until, tries, sent_at)
         VALUES (?1, seal(?2), 0, ?3, ?4, 0, ?5)
         ON CONFLICT(player_id) DO UPDATE SET email = seal(?2), verified = 0, code = ?3, code_until = ?4,
             tries = 0, sent_at = ?5",
        params![
            player_id,
            email,
            code.as_deref().map(|c| hash(db, c)),
            code.as_ref().map(|_| now + CODE_LASTS_SECS),
            code.as_ref().map(|_| now)
        ],
    )
    .map_err(internal)?;
    Ok((EmailSent { sent: code.is_some(), verified: false }, code.map(|c| (email, c))))
}

/// Mails what [`keep`] gave back, off the async threads.
pub(crate) async fn post(mail: Arc<Mail>, outgoing: Option<Outgoing>) -> Result<(), String> {
    let Some((to, code)) = outgoing else { return Ok(()) };
    tokio::task::spawn_blocking(move || mail.send(&to, &code)).await.map_err(|e| e.to_string())?
}

fn fresh(time: i64) -> Result<(), (StatusCode, String)> {
    if (time - crate::social::now_ms()).abs() > 10 * 60 * 1000 {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    Ok(())
}

pub(crate) async fn set_email(
    State(db): State<Db>,
    State(mail): State<Arc<Mail>>,
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    Json(set): Json<EmailSet>,
) -> Result<Json<EmailSent>, (StatusCode, String)> {
    if !verify(&set.player_id, &email_message(&set.player_id, &set.email, set.time), &set.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    fresh(set.time)?;
    if !limits.take(Limit::Email, &caller.0) {
        return Err(too_many("emails"));
    }
    let (sent, outgoing) = {
        let db = db.lock().unwrap();
        let known: Option<String> = db
            .query_row("SELECT name FROM accounts WHERE player_id = ?1", params![set.player_id], |r| r.get(0))
            .optional()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if known.is_none() {
            return Err((StatusCode::NOT_FOUND, "make the account first".into()));
        }
        keep(&db, &mail, &set.player_id, &set.email)?
    };
    post(mail, outgoing)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("could not send the mail: {e}")))?;
    Ok(Json(sent))
}

pub(crate) async fn verify_email(
    State(db): State<Db>,
    Json(sent): Json<EmailCode>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !verify(&sent.player_id, &email_code_message(&sent.player_id, &sent.code, sent.time), &sent.signature)
    {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    fresh(sent.time)?;
    let db = db.lock().unwrap();
    let internal = |e: rusqlite::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    let row: Option<(bool, Option<String>, Option<i64>, i64)> = db
        .query_row(
            "SELECT verified, code, code_until, tries FROM emails WHERE player_id = ?1",
            params![sent.player_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(internal)?;
    let Some((verified, code, until, tries)) = row else {
        return Err((StatusCode::NOT_FOUND, "this account has no email yet".into()));
    };
    if verified {
        return Ok(StatusCode::NO_CONTENT);
    }
    let (Some(code), Some(until)) = (code, until) else {
        return Err((StatusCode::BAD_REQUEST, "no code was sent; ask for one".into()));
    };
    if until < now() || tries >= MOST_TRIES {
        return Err((StatusCode::BAD_REQUEST, "that code has run out; ask for a new one".into()));
    }
    if hash(&db, sent.code.trim()) != code {
        db.execute("UPDATE emails SET tries = tries + 1 WHERE player_id = ?1", params![sent.player_id])
            .map_err(internal)?;
        return Err((StatusCode::BAD_REQUEST, "that's not the code we sent".into()));
    }
    db.execute(
        "UPDATE emails SET verified = 1, code = NULL, code_until = NULL WHERE player_id = ?1",
        params![sent.player_id],
    )
    .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_checked() {
        for good in ["alice@example.com", "a.b+libroxel@mail.co.uk", "  Alice@Example.com "] {
            assert!(valid_email(good.trim()), "{good}");
        }
        for bad in ["", "alice", "alice@", "@example.com", "alice@localhost", "a b@example.com"] {
            assert!(!valid_email(bad), "{bad}");
        }
    }
}

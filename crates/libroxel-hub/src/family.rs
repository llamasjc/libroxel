use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::Digest;

use super::{Db, now, verify};
use crate::email::{Mail, tidy, valid_email};
pub const CONSENT_AGE: u32 = 13;
pub const CONSENT_LASTS_SECS: i64 = 7 * 24 * 60 * 60;
const RESEND_AFTER_SECS: i64 = 60;
const MOST_TRIES: i64 = 5;
const LOCKED_SECS: i64 = 15 * 60;
const MOST_GAP_SECS: i64 = 120;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Chat {
    Off,
    Strict,
    #[default]
    Standard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Messages {
    #[default]
    Everyone,
    Friends,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FamilySettings {
    pub chat: Chat,
    pub messages: Messages,
    pub friend_requests: bool,
    #[serde(default)]
    pub spend_limit: Option<u32>,
    #[serde(default)]
    pub playtime_mins: Option<u32>,
}

impl FamilySettings {
    fn standard() -> Self {
        Self {
            chat: Chat::Standard,
            messages: Messages::Everyone,
            friend_requests: true,
            spend_limit: None,
            playtime_mins: None,
        }
    }

    fn child() -> Self {
        Self { chat: Chat::Strict, messages: Messages::Friends, ..Self::standard() }
    }

    fn unapproved() -> Self {
        Self { friend_requests: false, ..Self::child() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Birthday {
    pub player_id: String,
    pub born: String,
    #[serde(default)]
    pub parent_email: Option<String>,
    #[serde(default)]
    pub utc_offset_mins: i32,
    pub time: i64,
    pub signature: String,
}

pub fn birthday_message(b: &Birthday) -> Vec<u8> {
    format!(
        "libroxel birthday\n{}\n{}\n{}\n{}\n{}",
        b.player_id,
        b.born.trim(),
        b.parent_email.as_deref().map(tidy).unwrap_or_default(),
        b.utc_offset_mins,
        b.time
    )
    .into_bytes()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParentEmail {
    pub player_id: String,
    pub parent_email: String,
    pub time: i64,
    pub signature: String,
}

pub fn parent_email_message(player_id: &str, parent_email: &str, time: i64) -> Vec<u8> {
    format!("libroxel parent email\n{player_id}\n{}\n{time}", tidy(parent_email)).into_bytes()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Consent {
    pub player_id: String,
    pub code: String,
    pub pin: String,
    pub time: i64,
    pub signature: String,
}

pub fn consent_message(c: &Consent) -> Vec<u8> {
    format!("libroxel consent\n{}\n{}\n{}\n{}", c.player_id, c.code.trim(), c.pin, c.time).into_bytes()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FamilyChange {
    pub player_id: String,
    pub pin: String,
    #[serde(default)]
    pub new_pin: Option<String>,
    pub settings: FamilySettings,
    pub time: i64,
    pub signature: String,
}

pub fn family_message(c: &FamilyChange) -> Vec<u8> {
    format!(
        "libroxel family\n{}\n{}\n{}\n{}\n{}",
        c.player_id,
        c.pin,
        c.new_pin.as_deref().unwrap_or(""),
        serde_json::to_string(&c.settings).unwrap_or_default(),
        c.time
    )
    .into_bytes()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Family {
    pub has_birthday: bool,
    pub under_13: bool,
    pub consent: bool,
    pub parent_email: Option<String>,
    pub has_pin: bool,
    pub settings: FamilySettings,
    pub played_mins: u32,
    pub sends_mail: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ChatRule {
    pub chat: Chat,
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS family (
            player_id    TEXT PRIMARY KEY,
            born         TEXT NOT NULL,
            utc_offset   INTEGER NOT NULL DEFAULT 0,
            parent_email TEXT,
            consent      INTEGER NOT NULL DEFAULT 0,
            code         TEXT,
            code_until   INTEGER,
            sent_at      INTEGER,
            pin          TEXT,
            tries        INTEGER NOT NULL DEFAULT 0,
            locked_until INTEGER,
            settings     TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS playtime (
            player_id   TEXT NOT NULL,
            day         INTEGER NOT NULL,
            secs        INTEGER NOT NULL,
            PRIMARY KEY (player_id, day)
        );",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))
}

type Failure = (StatusCode, String);

fn internal(e: rusqlite::Error) -> Failure {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn bad(why: &str) -> Failure {
    (StatusCode::BAD_REQUEST, why.to_owned())
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub fn parse_date(text: &str) -> Option<(i64, i64, i64)> {
    let mut parts = text.trim().split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&m) || d < 1 {
        return None;
    }
    (civil_from_days(days_from_civil(y, m, d)) == (y, m, d)).then_some((y, m, d))
}

pub fn age_on(born: (i64, i64, i64), today: i64) -> i64 {
    let (y, m, d) = civil_from_days(today);
    let mut age = y - born.0;
    if (m, d) < (born.1, born.2) {
        age -= 1;
    }
    age
}

fn today(utc_offset_mins: i32) -> i64 {
    (now() + i64::from(utc_offset_mins) * 60).div_euclid(86_400)
}

fn pin_ok(pin: &str) -> bool {
    pin.len() == 4 && pin.chars().all(|c| c.is_ascii_digit())
}

fn hash(db: &Connection, player_id: &str, kind: &str, secret: &str) -> String {
    crate::seal::pepper(
        db,
        &data_encoding::HEXLOWER
            .encode(&sha2::Sha256::digest(format!("libroxel {kind}\n{player_id}\n{secret}"))),
    )
}

fn fresh(time: i64) -> Result<(), Failure> {
    if (time - crate::social::now_ms()).abs() > 10 * 60 * 1000 {
        return Err(bad("this computer's clock is too far off"));
    }
    Ok(())
}

struct Row {
    born: String,
    utc_offset: i32,
    parent_email: Option<String>,
    consent: bool,
    code: Option<String>,
    code_until: Option<i64>,
    sent_at: Option<i64>,
    pin: Option<String>,
    tries: i64,
    locked_until: Option<i64>,
    settings: FamilySettings,
}

fn row(db: &Connection, player_id: &str) -> Result<Option<Row>, Failure> {
    db.query_row(
        "SELECT born, utc_offset, unseal(parent_email), consent, code, code_until, sent_at, pin, tries, locked_until,
             settings FROM family WHERE player_id = ?1",
        params![player_id],
        |r| {
            let settings: String = r.get(10)?;
            Ok(Row {
                born: r.get(0)?,
                utc_offset: r.get(1)?,
                parent_email: r.get(2)?,
                consent: r.get(3)?,
                code: r.get(4)?,
                code_until: r.get(5)?,
                sent_at: r.get(6)?,
                pin: r.get(7)?,
                tries: r.get(8)?,
                locked_until: r.get(9)?,
                settings: serde_json::from_str(&settings).unwrap_or_else(|_| FamilySettings::unapproved()),
            })
        },
    )
    .optional()
    .map_err(internal)
}

impl Row {
    fn under_13(&self) -> bool {
        parse_date(&self.born).is_none_or(|b| age_on(b, today(self.utc_offset)) < i64::from(CONSENT_AGE))
    }

    fn applied(&self) -> FamilySettings {
        if !self.under_13() {
            return self.settings;
        }
        if !self.consent {
            return FamilySettings {
                playtime_mins: self.settings.playtime_mins,
                ..FamilySettings::unapproved()
            };
        }
        let mut s = self.settings;
        if s.chat == Chat::Standard {
            s.chat = Chat::Strict;
        }
        s
    }
}

pub fn settings_of(db: &Connection, player_id: &str) -> FamilySettings {
    match row(db, player_id).ok().flatten() {
        Some(row) => row.applied(),
        None => FamilySettings { chat: Chat::Strict, ..FamilySettings::standard() },
    }
}

pub fn chat_level(db: &Connection, player_id: &str) -> Chat {
    settings_of(db, player_id).chat
}

fn are_friends(db: &Connection, a: &str, b: &str) -> bool {
    db.query_row(
        "SELECT 1 FROM friends WHERE accepted = 1 AND ((from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1))",
        params![a, b],
        |_| Ok(()),
    )
    .optional()
    .ok()
    .flatten()
    .is_some()
}

pub fn may_message(db: &Connection, from_id: &str, to_id: &str) -> bool {
    let (from, to) = (settings_of(db, from_id), settings_of(db, to_id));
    if from.chat == Chat::Off || to.chat == Chat::Off {
        return false;
    }
    let allows = |s: FamilySettings, other: &str, me: &str| match s.messages {
        Messages::Everyone => true,
        Messages::Friends => are_friends(db, me, other),
        Messages::Off => false,
    };
    allows(to, from_id, to_id) && allows(from, to_id, from_id)
}

pub fn may_friend(db: &Connection, player_id: &str) -> bool {
    settings_of(db, player_id).friend_requests
}

pub fn may_spend(db: &Connection, player_id: &str, spent: u32, amount: u32) -> bool {
    settings_of(db, player_id).spend_limit.is_none_or(|most| spent.saturating_add(amount) <= most)
}

fn played_secs(db: &Connection, player_id: &str, day: i64) -> i64 {
    db.query_row(
        "SELECT secs FROM playtime WHERE player_id = ?1 AND day = ?2",
        params![player_id, day],
        |r| r.get(0),
    )
    .optional()
    .ok()
    .flatten()
    .unwrap_or(0)
}

fn offset_of(db: &Connection, player_id: &str) -> i32 {
    db.query_row("SELECT utc_offset FROM family WHERE player_id = ?1", params![player_id], |r| r.get(0))
        .optional()
        .ok()
        .flatten()
        .unwrap_or(0)
}
pub(crate) fn count_play(db: &Connection, player_id: &str) {
    let last: Option<(bool, Option<String>, i64)> = db
        .query_row(
            "SELECT online, game, last_seen FROM presence WHERE player_id = ?1",
            params![player_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .ok()
        .flatten();
    let Some((true, Some(_), last_seen)) = last else { return };
    let secs = (now() - last_seen).clamp(0, MOST_GAP_SECS);
    if secs == 0 {
        return;
    }
    let day = today(offset_of(db, player_id));
    let _ = db.execute(
        "INSERT INTO playtime (player_id, day, secs) VALUES (?1, ?2, ?3)
         ON CONFLICT(player_id, day) DO UPDATE SET secs = secs + ?3",
        params![player_id, day, secs],
    );
}

pub fn playtime_up(db: &Connection, player_id: &str) -> Option<String> {
    let most = settings_of(db, player_id).playtime_mins?;
    let played = played_secs(db, player_id, today(offset_of(db, player_id)));
    (played >= i64::from(most) * 60).then(|| format!("you've played your {most} minutes for today"))
}

pub fn strict_filter(text: &str) -> String {
    const SPOKEN: &[&str] =
        &["zero", "oh", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
    let words: Vec<&str> = text.split_inclusive(char::is_whitespace).collect();
    let spoken: Vec<bool> = words
        .iter()
        .map(|w| {
            SPOKEN.contains(
                &w.trim().trim_matches(|c: char| !c.is_alphanumeric()).to_ascii_lowercase().as_str(),
            )
        })
        .collect();
    let in_run = |i: usize| {
        let (mut a, mut b) = (i, i);
        while a > 0 && spoken[a - 1] {
            a -= 1;
        }
        while b + 1 < spoken.len() && spoken[b + 1] {
            b += 1;
        }
        spoken[i] && b - a >= 2
    };
    let mut out = String::with_capacity(text.len());
    for (i, word) in words.into_iter().enumerate() {
        let core = word.trim_end();
        let lower = core.to_ascii_lowercase();
        let linky = in_run(i)
            || lower.contains("://")
            || lower.starts_with("www.")
            || (core.contains('@') && core.len() > 1)
            || [".com", ".net", ".org", ".gg", ".io", ".co", ".me", ".tv", ".ly"].iter().any(|end| {
                let bare = lower.trim_end_matches(|c: char| !c.is_alphanumeric());
                bare.ends_with(end) || lower.contains(&format!("{end}/"))
            });
        if linky {
            out.extend(std::iter::repeat_n('#', core.chars().count()));
            out.push_str(&word[core.len()..]);
            continue;
        }
        out.push_str(word);
    }
    let chars: Vec<char> = out.chars().collect();
    let mut hidden = chars.clone();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let (start, mut end, mut digits) = (i, i, 0);
        let mut j = i;
        while j < chars.len()
            && (chars[j].is_ascii_digit() || matches!(chars[j], ' ' | '-' | '.' | '(' | ')'))
        {
            if chars[j].is_ascii_digit() {
                digits += 1;
                end = j;
            }
            j += 1;
        }
        if digits >= 3 {
            for c in &mut hidden[start..=end] {
                if c.is_ascii_digit() {
                    *c = '#';
                }
            }
        }
        i = end + 1;
    }
    hidden.into_iter().collect()
}

fn account_name(db: &Connection, player_id: &str) -> Result<Option<String>, Failure> {
    db.query_row("SELECT name FROM accounts WHERE player_id = ?1", params![player_id], |r| r.get(0))
        .optional()
        .map_err(internal)
}

type Letter = (String, String, String);

async fn post(mail: Arc<Mail>, letter: Option<Letter>) -> Result<(), Failure> {
    let Some((to, code, name)) = letter else { return Ok(()) };
    let subject = format!("{code} is the code to approve {name} on Libroxel");
    let body = format!(
        "Hello!\n\n{name} made a Libroxel account and is under 13. They can chat with a strict filter, but friend requests stay off until a parent approves.\n\n\
         To approve, open Libroxel on their device, go to Avatar > Parent settings, and type this code with a \
         4-digit PIN of your own:\n\n{code}\n\nThe PIN lets you change chat, messages, friend requests, spending \
         and playtime later. The code works for 7 days.\n\nIf you don't approve, do nothing.\n"
    );
    tokio::task::spawn_blocking(move || mail.send_letter(&to, &code, &subject, &body))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("could not send the mail: {e}")))
}

fn new_parent_code(
    db: &Connection,
    mail: &Mail,
    player_id: &str,
    email: &str,
    name: String,
) -> Result<Option<Letter>, Failure> {
    let code = mail.sends().then(crate::email::new_code);
    let now = now();
    db.execute(
        "UPDATE family SET parent_email = seal(?2), code = ?3, code_until = ?4, sent_at = ?5, tries = 0
         WHERE player_id = ?1",
        params![
            player_id,
            email,
            code.as_deref().map(|c| hash(db, player_id, "consent", c)),
            code.as_ref().map(|_| now + CONSENT_LASTS_SECS),
            code.as_ref().map(|_| now)
        ],
    )
    .map_err(internal)?;
    Ok(code.map(|c| (email.to_owned(), c, name)))
}

pub(crate) async fn set_birthday(
    State(db): State<Db>,
    State(mail): State<Arc<Mail>>,
    Json(b): Json<Birthday>,
) -> Result<Json<Family>, Failure> {
    if !verify(&b.player_id, &birthday_message(&b), &b.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    fresh(b.time)?;
    let born = parse_date(&b.born).ok_or_else(|| bad("that isn't a date"))?;
    if !(-14 * 60..=14 * 60).contains(&b.utc_offset_mins) {
        return Err(bad("that time zone is off"));
    }
    let age = age_on(born, today(b.utc_offset_mins));
    if !(0..=120).contains(&age) {
        return Err(bad("that birthday can't be right"));
    }
    let under_13 = age < i64::from(CONSENT_AGE);
    let parent = b.parent_email.as_deref().map(tidy).filter(|e| !e.is_empty());
    if parent.as_deref().is_some_and(|e| !valid_email(e)) {
        return Err(bad("that parent email doesn't look right"));
    }
    let letter = {
        let db = db.lock().unwrap();
        let Some(name) = account_name(&db, &b.player_id)? else {
            return Err((StatusCode::NOT_FOUND, "make the account first".into()));
        };
        if row(&db, &b.player_id)?.is_some() {
            return Err((StatusCode::CONFLICT, "the birthday is already set".into()));
        }
        let settings = if under_13 { FamilySettings::child() } else { FamilySettings::standard() };
        db.execute(
            "INSERT INTO family (player_id, born, utc_offset, settings) VALUES (?1, ?2, ?3, ?4)",
            params![
                b.player_id,
                format!("{:04}-{:02}-{:02}", born.0, born.1, born.2),
                b.utc_offset_mins,
                serde_json::to_string(&settings).unwrap_or_default()
            ],
        )
        .map_err(internal)?;
        match parent.filter(|_| under_13) {
            Some(email) => new_parent_code(&db, &mail, &b.player_id, &email, name)?,
            None => None,
        }
    };
    post(mail.clone(), letter).await?;
    let db = db.lock().unwrap();
    Ok(Json(view(&db, &mail, &b.player_id)?))
}

pub(crate) async fn ask_parent(
    State(db): State<Db>,
    State(mail): State<Arc<Mail>>,
    Json(ask): Json<ParentEmail>,
) -> Result<StatusCode, Failure> {
    if !verify(
        &ask.player_id,
        &parent_email_message(&ask.player_id, &ask.parent_email, ask.time),
        &ask.signature,
    ) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    fresh(ask.time)?;
    let email = tidy(&ask.parent_email);
    if !valid_email(&email) {
        return Err(bad("that parent email doesn't look right"));
    }
    let letter = {
        let db = db.lock().unwrap();
        let name = account_name(&db, &ask.player_id)?
            .ok_or((StatusCode::NOT_FOUND, "make the account first".into()))?;
        let Some(row) = row(&db, &ask.player_id)? else { return Err(bad("give your birthday first")) };
        if row.consent {
            return Err((StatusCode::CONFLICT, "a parent already approved".into()));
        }
        if let Some(sent) = row.sent_at
            && now() - sent < RESEND_AFTER_SECS
        {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                format!("a code was just sent; ask again in {} seconds", RESEND_AFTER_SECS - (now() - sent)),
            ));
        }
        new_parent_code(&db, &mail, &ask.player_id, &email, name)?
    };
    post(mail, letter).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn check_try(db: &Connection, player_id: &str, row: &Row, right: bool) -> Result<(), Failure> {
    let now = now();
    if row.locked_until.is_some_and(|until| until > now) {
        return Err((StatusCode::TOO_MANY_REQUESTS, "too many wrong tries; wait 15 minutes".into()));
    }
    if right {
        db.execute(
            "UPDATE family SET tries = 0, locked_until = NULL WHERE player_id = ?1",
            params![player_id],
        )
        .map_err(internal)?;
        return Ok(());
    }
    let tries = row.tries + 1;
    let locked = (tries >= MOST_TRIES).then_some(now + LOCKED_SECS);
    db.execute(
        "UPDATE family SET tries = ?2, locked_until = ?3 WHERE player_id = ?1",
        params![player_id, if locked.is_some() { 0 } else { tries }, locked],
    )
    .map_err(internal)?;
    Err((StatusCode::FORBIDDEN, "that's not right".into()))
}

pub(crate) async fn consent(
    State(db): State<Db>,
    State(mail): State<Arc<Mail>>,
    Json(c): Json<Consent>,
) -> Result<Json<Family>, Failure> {
    if !verify(&c.player_id, &consent_message(&c), &c.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    fresh(c.time)?;
    if !pin_ok(&c.pin) {
        return Err(bad("a PIN is 4 digits"));
    }
    let db = db.lock().unwrap();
    let Some(row) = row(&db, &c.player_id)? else { return Err(bad("give your birthday first")) };
    if row.consent {
        return Err((StatusCode::CONFLICT, "a parent already approved".into()));
    }
    let (Some(code), Some(until)) = (&row.code, row.code_until) else {
        return Err(bad("no code was sent; ask for one"));
    };
    if until < now() {
        return Err(bad("that code has run out; ask for a new one"));
    }
    check_try(&db, &c.player_id, &row, hash(&db, &c.player_id, "consent", c.code.trim()) == *code)?;
    db.execute(
        "UPDATE family SET consent = 1, code = NULL, code_until = NULL, pin = ?2 WHERE player_id = ?1",
        params![c.player_id, hash(&db, &c.player_id, "pin", &c.pin)],
    )
    .map_err(internal)?;
    Ok(Json(view(&db, &mail, &c.player_id)?))
}

pub(crate) async fn change(
    State(db): State<Db>,
    State(mail): State<Arc<Mail>>,
    Json(c): Json<FamilyChange>,
) -> Result<Json<Family>, Failure> {
    if !verify(&c.player_id, &family_message(&c), &c.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    fresh(c.time)?;
    if c.new_pin.as_deref().is_some_and(|p| !pin_ok(p)) {
        return Err(bad("a PIN is 4 digits"));
    }
    if c.settings.playtime_mins.is_some_and(|m| m > 24 * 60) {
        return Err(bad("a day has 1440 minutes"));
    }
    let db = db.lock().unwrap();
    let Some(row) = row(&db, &c.player_id)? else { return Err(bad("give your birthday first")) };
    if row.under_13() && !row.consent {
        return Err((StatusCode::FORBIDDEN, "a parent needs to approve first".into()));
    }
    match &row.pin {
        Some(pin) => check_try(&db, &c.player_id, &row, hash(&db, &c.player_id, "pin", &c.pin) == *pin)?,
        None if c.new_pin.is_none() => return Err(bad("set a PIN first")),
        None => {}
    }
    db.execute(
        "UPDATE family SET settings = ?2, pin = COALESCE(?3, pin) WHERE player_id = ?1",
        params![
            c.player_id,
            serde_json::to_string(&c.settings).unwrap_or_default(),
            c.new_pin.as_deref().map(|p| hash(&db, &c.player_id, "pin", p))
        ],
    )
    .map_err(internal)?;
    Ok(Json(view(&db, &mail, &c.player_id)?))
}

fn view(db: &Connection, mail: &Mail, player_id: &str) -> Result<Family, Failure> {
    let row = row(db, player_id)?;
    let played = played_secs(db, player_id, today(offset_of(db, player_id)));
    Ok(Family {
        has_birthday: row.is_some(),
        under_13: row.as_ref().is_some_and(Row::under_13),
        consent: row.as_ref().is_some_and(|r| r.consent),
        parent_email: row.as_ref().and_then(|r| r.parent_email.clone()),
        has_pin: row.as_ref().is_some_and(|r| r.pin.is_some()),
        settings: settings_of(db, player_id),
        played_mins: (played / 60) as u32,
        sends_mail: mail.sends(),
    })
}

pub fn family_read_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel family read\n{player_id}\n{time}").into_bytes()
}

#[derive(Debug, Deserialize)]
pub(crate) struct Signed {
    time: i64,
    signature: String,
}

pub(crate) async fn family(
    State(db): State<Db>,
    State(mail): State<Arc<Mail>>,
    UrlPath(id): UrlPath<String>,
    Query(signed): Query<Signed>,
) -> Result<Json<Family>, Failure> {
    if !verify(&id, &family_read_message(&id, signed.time), &signed.signature) {
        return Err((StatusCode::FORBIDDEN, "only you can see this".into()));
    }
    fresh(signed.time)?;
    let db = db.lock().unwrap();
    Ok(Json(view(&db, &mail, &id)?))
}

pub(crate) async fn chat_rule(State(db): State<Db>, UrlPath(id): UrlPath<String>) -> Json<ChatRule> {
    Json(ChatRule { chat: chat_level(&db.lock().unwrap(), &id) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_count_whole_years() {
        let today = days_from_civil(2026, 11, 22);
        assert_eq!(age_on((2013, 11, 22), today), 13);
        assert_eq!(age_on((2013, 11, 23), today), 12);
        assert_eq!(age_on((2000, 2, 29), today), 26);
        assert_eq!(parse_date("2015-02-29"), None);
        assert_eq!(parse_date("2016-02-29"), Some((2016, 2, 29)));
        assert_eq!(parse_date("2016-13-01"), None);
    }

    #[test]
    fn strict_chat_hides_ways_to_get_in_touch() {
        assert_eq!(strict_filter("call me 555-123-4567"), "call me ###-###-####");
        assert_eq!(strict_filter("add me ana@mail.com ok"), "add me ############ ok");
        assert_eq!(strict_filter("go to discord.gg/abc"), "go to ##############");
        assert_eq!(strict_filter("I scored 12 points"), "I scored 12 points");
        assert_eq!(strict_filter("hi @ana"), "hi ####");
        assert_eq!(strict_filter("five five five two"), "#### #### #### ###");
        assert_eq!(strict_filter("I have one or two"), "I have one or two");
    }
}

use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::social::now_ms;
use crate::wallet::{self, CURRENCY, Failed, Sale};
use crate::{Db, now, verify};

pub const CREATOR_SHARE: u32 = 70;
pub const HOLD_DAYS: i64 = 3;
pub const PAYOUT_AGE: i64 = 18;
pub const MAX_PASSES: usize = 50;
const MAX_PASS_NAME: usize = 50;
const MAX_PASS_ABOUT: usize = 300;
const MAX_DETAILS: usize = 200;
const PLAY_CAP_SECS: i64 = 2 * 60 * 60;
const DAY: i64 = 86_400;
const CLOCK_SLACK_MS: i64 = 10 * 60 * 1000;

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    let _ = db.execute("ALTER TABLE payouts RENAME COLUMN sparks TO credits", []);
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS passes (
            pass_id     INTEGER PRIMARY KEY,
            game_id     TEXT NOT NULL,
            name        TEXT NOT NULL,
            about       TEXT NOT NULL,
            price       INTEGER NOT NULL,
            created     INTEGER NOT NULL,
            removed     INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS passes_game ON passes(game_id);
        CREATE TABLE IF NOT EXISTS settled (
            purchase_id INTEGER PRIMARY KEY,
            buyer_id    TEXT NOT NULL,
            seller_id   TEXT NOT NULL,
            paid        INTEGER NOT NULL,
            earned      INTEGER NOT NULL,
            cashable    INTEGER NOT NULL,
            time        INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS settled_buyer ON settled(buyer_id);
        CREATE INDEX IF NOT EXISTS settled_seller ON settled(seller_id, time);
        CREATE TABLE IF NOT EXISTS engagement (
            day         INTEGER NOT NULL,
            game_id     TEXT NOT NULL,
            creator_id  TEXT NOT NULL,
            secs        INTEGER NOT NULL,
            earned      INTEGER NOT NULL,
            PRIMARY KEY (day, game_id)
        );
        CREATE INDEX IF NOT EXISTS engagement_creator ON engagement(creator_id, day);
        CREATE TABLE IF NOT EXISTS engagement_days (
            day         INTEGER PRIMARY KEY,
            pool        INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS payouts (
            payout_id   INTEGER PRIMARY KEY,
            player_id   TEXT NOT NULL,
            credits     INTEGER NOT NULL,
            cents       INTEGER NOT NULL,
            details     TEXT NOT NULL,
            state       TEXT NOT NULL,
            note        TEXT NOT NULL DEFAULT '',
            asked       INTEGER NOT NULL,
            done        INTEGER
        );
        CREATE INDEX IF NOT EXISTS payouts_player ON payouts(player_id, asked);
        CREATE TABLE IF NOT EXISTS earnings_settings (
            id          INTEGER PRIMARY KEY CHECK (id = 0),
            pool        INTEGER NOT NULL DEFAULT 0,
            provider    TEXT NOT NULL DEFAULT '',
            cents_per_thousand INTEGER NOT NULL DEFAULT 0,
            minimum     INTEGER NOT NULL DEFAULT 0
        );
        INSERT OR IGNORE INTO earnings_settings (id) VALUES (0);",
    )
    .map_err(|e| format!("could not set up earnings: {e}"))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EarningsSettings {
    pub pool: u32,
    pub provider: String,
    pub cents_per_thousand: u32,
    pub minimum: u32,
}

impl EarningsSettings {
    pub fn payouts_on(&self) -> bool {
        !self.provider.is_empty() && self.cents_per_thousand > 0
    }
}

pub fn settings(db: &Connection) -> EarningsSettings {
    db.query_row(
        "SELECT pool, provider, cents_per_thousand, minimum FROM earnings_settings WHERE id = 0",
        [],
        |r| {
            Ok(EarningsSettings {
                pool: r.get(0)?,
                provider: r.get(1)?,
                cents_per_thousand: r.get(2)?,
                minimum: r.get(3)?,
            })
        },
    )
    .unwrap_or_default()
}

pub fn set_settings(db: &Connection, s: &EarningsSettings) -> Result<(), String> {
    db.execute(
        "UPDATE earnings_settings SET pool = ?1, provider = ?2, cents_per_thousand = ?3, minimum = ?4 WHERE id = 0",
        params![s.pool, s.provider.trim(), s.cents_per_thousand, s.minimum],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

fn share(amount: i64) -> i64 {
    amount * i64::from(CREATOR_SHARE) / 100
}

pub fn settle(db: &Connection, now: i64) -> rusqlite::Result<()> {
    settle_sales(db, now)?;
    settle_engagement(db, now)
}

fn settle_sales(db: &Connection, now: i64) -> rusqlite::Result<()> {
    let due: Vec<(i64, String, String, i64, i64)> = {
        let mut s = db.prepare(
            "SELECT p.purchase_id, p.buyer_id, p.seller_id, p.amount, p.time FROM purchases p
             WHERE p.time <= ?1 AND NOT EXISTS (SELECT 1 FROM settled s WHERE s.purchase_id = p.purchase_id)
             ORDER BY p.purchase_id",
        )?;
        s.query_map(params![now - HOLD_DAYS * DAY], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    for (id, buyer, seller, amount, time) in due {
        // "libroxel" is the platform's own account id (from before the
        // rename to PlayLibre), kept in the shop's records.
        let to_creator = seller != "libroxel" && seller != buyer;
        if to_creator && crate::owner::is_banned(db, &seller) {
            continue;
        }
        let bought: i64 = db.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM ledger WHERE player_id = ?1 AND reason = 'bought' AND time <= ?2",
            params![buyer, time],
            |r| r.get(0),
        )?;
        let used: i64 = db.query_row(
            "SELECT COALESCE(SUM(paid), 0) FROM settled WHERE buyer_id = ?1",
            params![buyer],
            |r| r.get(0),
        )?;
        let paid = amount.min((bought - used).max(0));
        let earned = if to_creator { share(paid) } else { 0 };
        let cashable = earned;
        if earned > 0 {
            wallet::credit(db, &seller, earned as u32, "earned", &format!("sale-{id}"))?;
        }
        db.execute(
            "INSERT INTO settled (purchase_id, buyer_id, seller_id, paid, earned, cashable, time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, buyer, seller, paid, earned, cashable, now],
        )?;
    }
    Ok(())
}

fn settle_engagement(db: &Connection, now: i64) -> rusqlite::Result<()> {
    let today = now.div_euclid(DAY);
    let pool = settings(db).pool;
    for day in today - 3..today {
        let done: Option<i64> = db
            .query_row("SELECT 1 FROM engagement_days WHERE day = ?1", params![day], |r| r.get(0))
            .optional()?;
        if done.is_some() {
            continue;
        }
        if pool > 0 {
            pay_engagement(db, day, i64::from(pool))?;
        }
        db.execute("INSERT INTO engagement_days (day, pool) VALUES (?1, ?2)", params![day, pool])?;
    }
    Ok(())
}

fn pay_engagement(db: &Connection, day: i64, pool: i64) -> rusqlite::Result<()> {
    let games: Vec<(String, String)> = {
        let mut s = db.prepare(&format!(
            "SELECT game_id, author_id FROM games WHERE author_id IS NOT NULL AND NOT {}",
            crate::owner::banned_sql("author_id")
        ))?;
        s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    let mut counted = Vec::new();
    for (game, author) in games {
        let own = crate::analytics::player_code(db, &game, &author)?;
        let creator = crate::groups::earner(db, &game, &author);
        let secs: i64 = db.query_row(
            "SELECT COALESCE(SUM(MIN(secs, ?4)), 0) FROM (
                SELECT player, SUM(last_seen - started) AS secs FROM play_sessions
                WHERE game_id = ?1 AND started >= ?2 AND started < ?2 + 86400 AND player <> ?3
                GROUP BY player)",
            params![game, day * DAY, own, PLAY_CAP_SECS],
            |r| r.get(0),
        )?;
        if secs > 0 {
            counted.push((game, creator, secs));
        }
    }
    let total: i64 = counted.iter().map(|(_, _, s)| s).sum();
    for (game, creator, secs) in counted {
        let earned = pool * secs / total;
        db.execute(
            "INSERT OR IGNORE INTO engagement (day, game_id, creator_id, secs, earned) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![day, game, creator, secs, earned],
        )?;
        if earned > 0 {
            wallet::credit(db, &creator, earned as u32, "engagement", &format!("{day}-{game}"))?;
        }
    }
    Ok(())
}

pub fn payable(db: &Connection, player_id: &str) -> i64 {
    let sum = |sql: &str| db.query_row(sql, params![player_id], |r| r.get::<_, i64>(0)).unwrap_or(0);
    let earned = sum("SELECT COALESCE(SUM(cashable), 0) FROM settled WHERE seller_id = ?1")
        + sum("SELECT COALESCE(SUM(earned), 0) FROM engagement WHERE creator_id = ?1")
        + sum("SELECT COALESCE(SUM(amount), 0) FROM ledger WHERE player_id = ?1 AND reason = 'group pay'");
    let out =
        sum("SELECT COALESCE(SUM(credits), 0) FROM payouts WHERE player_id = ?1 AND state <> 'refused'");
    (earned - out).clamp(0, wallet::balance(db, player_id).max(0))
}

pub fn payout_needs(db: &Connection, player_id: &str) -> Vec<String> {
    let mut needs = Vec::new();
    let born: Option<String> = db
        .query_row("SELECT born FROM family WHERE player_id = ?1", params![player_id], |r| r.get(0))
        .optional()
        .ok()
        .flatten();
    let adult = born
        .and_then(|b| crate::family::parse_date(&b))
        .is_some_and(|b| crate::family::age_on(b, now().div_euclid(DAY)) >= PAYOUT_AGE);
    if !adult {
        needs.push(format!("be {PAYOUT_AGE} or older"));
    }
    if !crate::email::email_of(db, player_id).is_some_and(|(_, verified)| verified) {
        needs.push("verify your email".into());
    }
    let two_step: Option<i64> = db
        .query_row("SELECT 1 FROM two_step WHERE player_id = ?1", params![player_id], |r| r.get(0))
        .optional()
        .ok()
        .flatten();
    if two_step.is_none() {
        needs.push("turn on two-step sign-in".into());
    }
    needs
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pass {
    pub pass_id: i64,
    pub game_id: String,
    pub name: String,
    pub about: String,
    pub price: u32,
    #[serde(default)]
    pub owned: bool,
    #[serde(default)]
    pub sold: u32,
}

pub fn passes(db: &Connection, game_id: &str, player_id: Option<&str>) -> rusqlite::Result<Vec<Pass>> {
    let mut s = db.prepare(
        "SELECT p.pass_id, p.game_id, p.name, p.about, p.price,
                EXISTS (SELECT 1 FROM owned o WHERE o.player_id = ?2 AND o.item_id = 'pass-' || p.pass_id)
                OR EXISTS (SELECT 1 FROM games g WHERE g.game_id = p.game_id AND g.author_id = ?2),
                (SELECT COUNT(*) FROM purchases s WHERE s.item_id = 'pass-' || p.pass_id)
         FROM passes p WHERE p.game_id = ?1 AND p.removed = 0 ORDER BY p.price, p.pass_id",
    )?;
    s.query_map(params![game_id, player_id.unwrap_or("")], |r| {
        Ok(Pass {
            pass_id: r.get(0)?,
            game_id: r.get(1)?,
            name: r.get(2)?,
            about: r.get(3)?,
            price: r.get(4)?,
            owned: r.get(5)?,
            sold: r.get(6)?,
        })
    })?
    .collect()
}

pub fn owns_pass(db: &Connection, player_id: &str, pass_id: i64) -> bool {
    wallet::owns(db, player_id, &format!("pass-{pass_id}"))
        || db
            .query_row(
                "SELECT 1 FROM passes p JOIN games g ON g.game_id = p.game_id
                 WHERE p.pass_id = ?1 AND g.author_id = ?2",
                params![pass_id, player_id],
                |_| Ok(()),
            )
            .optional()
            .ok()
            .flatten()
            .is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PassAction {
    Add,
    Edit,
    Remove,
}

impl PassAction {
    fn word(self) -> &'static str {
        match self {
            PassAction::Add => "add",
            PassAction::Edit => "edit",
            PassAction::Remove => "remove",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassChange {
    pub player_id: String,
    pub action: PassAction,
    #[serde(default)]
    pub pass_id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub price: u32,
    pub time: i64,
    pub signature: String,
}

pub fn pass_message(game_id: &str, c: &PassChange) -> Vec<u8> {
    format!(
        "libroxel pass\n{}\n{game_id}\n{}\n{}\n{}\n{}\n{}\n{}",
        c.player_id,
        c.action.word(),
        c.pass_id,
        c.name,
        c.about,
        c.price,
        c.time
    )
    .into_bytes()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuyPass {
    pub player_id: String,
    pub pass_id: i64,
    pub price: u32,
    pub time: i64,
    pub signature: String,
}

pub fn buy_pass_message(player_id: &str, pass_id: i64, price: u32, time: i64) -> Vec<u8> {
    format!("libroxel buy pass\n{player_id}\n{pass_id}\n{price}\n{time}").into_bytes()
}

pub fn earnings_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel earnings\n{player_id}\n{time}").into_bytes()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CashOut {
    pub player_id: String,
    pub credits: u32,
    pub details: String,
    pub time: i64,
    pub signature: String,
}

pub fn cash_out_message(c: &CashOut) -> Vec<u8> {
    format!("libroxel cash out\n{}\n{}\n{}\n{}", c.player_id, c.credits, c.details, c.time).into_bytes()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EarnedDay {
    pub day: i64,
    pub sales: i64,
    pub engagement: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameEarnings {
    pub game_id: String,
    pub title: String,
    pub passes: i64,
    pub engagement: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaleLine {
    pub name: String,
    pub kind: String,
    pub game_id: Option<String>,
    pub price: i64,
    pub earned: i64,
    pub settled: bool,
    pub secs_ago: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payout {
    pub payout_id: i64,
    pub player_id: String,
    pub credits: i64,
    pub cents: i64,
    pub details: String,
    pub state: String,
    pub note: String,
    pub asked_secs_ago: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Earnings {
    pub currency: String,
    pub share_percent: u32,
    pub hold_days: i64,
    pub balance: i64,
    pub pending: i64,
    pub total: i64,
    pub last_30: i64,
    pub items: i64,
    pub days: Vec<EarnedDay>,
    pub games: Vec<GameEarnings>,
    pub sales: Vec<SaleLine>,
    pub payouts_on: bool,
    pub provider: String,
    pub cents_per_thousand: u32,
    pub minimum: u32,
    pub payable: i64,
    pub needs: Vec<String>,
    pub payouts: Vec<Payout>,
}

fn payout_row(r: &rusqlite::Row, now: i64) -> rusqlite::Result<Payout> {
    Ok(Payout {
        payout_id: r.get(0)?,
        player_id: r.get(1)?,
        credits: r.get(2)?,
        cents: r.get(3)?,
        details: r.get(4)?,
        state: r.get(5)?,
        note: r.get(6)?,
        asked_secs_ago: now - r.get::<_, i64>(7)?,
    })
}

const PAYOUT_COLUMNS: &str = "payout_id, player_id, credits, cents, details, state, note, asked";

pub fn earnings(db: &Connection, player_id: &str, now: i64) -> rusqlite::Result<Earnings> {
    let one = |sql: &str, p: &[&dyn rusqlite::ToSql]| db.query_row(sql, p, |r| r.get::<_, i64>(0));
    let since = now - 30 * DAY;
    let today = now.div_euclid(DAY);
    let sales_total =
        one("SELECT COALESCE(SUM(earned), 0) FROM settled WHERE seller_id = ?1", &[&player_id])?;
    let engagement_total =
        one("SELECT COALESCE(SUM(earned), 0) FROM engagement WHERE creator_id = ?1", &[&player_id])?;
    let pending = one(
        "SELECT COALESCE(SUM(amount), 0) FROM purchases p WHERE seller_id = ?1 AND buyer_id <> ?1
         AND NOT EXISTS (SELECT 1 FROM settled s WHERE s.purchase_id = p.purchase_id)",
        &[&player_id],
    )?;
    let items = one(
        "SELECT COALESCE(SUM(s.earned), 0) FROM settled s JOIN purchases p ON p.purchase_id = s.purchase_id
         WHERE s.seller_id = ?1 AND p.game_id IS NULL",
        &[&player_id],
    )?;

    let mut days: Vec<EarnedDay> =
        (today - 29..=today).map(|day| EarnedDay { day, ..Default::default() }).collect();
    {
        let mut s = db.prepare(
            "SELECT p.time / 86400, SUM(s.earned) FROM settled s JOIN purchases p ON p.purchase_id = s.purchase_id
             WHERE s.seller_id = ?1 AND p.time >= ?2 GROUP BY 1",
        )?;
        for row in
            s.query_map(params![player_id, since], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
        {
            let (day, sum) = row?;
            if let Some(d) = days.iter_mut().find(|d| d.day == day) {
                d.sales += sum;
            }
        }
        let mut s = db.prepare(
            "SELECT day, SUM(earned) FROM engagement WHERE creator_id = ?1 AND day >= ?2 GROUP BY day",
        )?;
        for row in
            s.query_map(params![player_id, today - 29], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
        {
            let (day, sum) = row?;
            if let Some(d) = days.iter_mut().find(|d| d.day == day) {
                d.engagement += sum;
            }
        }
    }
    let last_30 = days.iter().map(|d| d.sales + d.engagement).sum();

    let games = {
        let mut s = db.prepare(
            "SELECT g.game_id, g.title,
                (SELECT COALESCE(SUM(s.earned), 0) FROM settled s JOIN purchases p ON p.purchase_id = s.purchase_id
                 WHERE s.seller_id = ?1 AND p.game_id = g.game_id),
                (SELECT COALESCE(SUM(e.earned), 0) FROM engagement e WHERE e.creator_id = ?1 AND e.game_id = g.game_id)
             FROM games g WHERE g.author_id = ?1 ORDER BY g.updated DESC",
        )?;
        s.query_map(params![player_id], |r| {
            Ok(GameEarnings {
                game_id: r.get(0)?,
                title: r.get(1)?,
                passes: r.get(2)?,
                engagement: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };

    let sales = {
        let mut s = db.prepare(
            "SELECT COALESCE(ps.name, i.name, p.item_id), p.kind, p.game_id, p.amount, s.earned, s.purchase_id IS NOT NULL, p.time
             FROM purchases p
             LEFT JOIN settled s ON s.purchase_id = p.purchase_id
             LEFT JOIN passes ps ON 'pass-' || ps.pass_id = p.item_id
             LEFT JOIN items i ON i.item_id = p.item_id
             WHERE p.seller_id = ?1 AND p.buyer_id <> ?1 ORDER BY p.purchase_id DESC LIMIT 30",
        )?;
        s.query_map(params![player_id], |r| {
            let price: i64 = r.get(3)?;
            Ok(SaleLine {
                name: r.get(0)?,
                kind: r.get(1)?,
                game_id: r.get(2)?,
                price,
                earned: r.get::<_, Option<i64>>(4)?.unwrap_or_else(|| share(price)),
                settled: r.get(5)?,
                secs_ago: now - r.get::<_, i64>(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };

    let payouts = {
        let mut s = db.prepare(&format!(
            "SELECT {PAYOUT_COLUMNS} FROM payouts WHERE player_id = ?1 ORDER BY payout_id DESC LIMIT 20"
        ))?;
        s.query_map(params![player_id], |r| payout_row(r, now))?.collect::<rusqlite::Result<Vec<_>>>()?
    };

    let set = settings(db);
    Ok(Earnings {
        currency: CURRENCY.into(),
        share_percent: CREATOR_SHARE,
        hold_days: HOLD_DAYS,
        balance: wallet::balance(db, player_id),
        pending: share(pending),
        total: sales_total + engagement_total,
        last_30,
        items,
        days,
        games,
        sales,
        payouts_on: set.payouts_on(),
        payable: payable(db, player_id),
        needs: if set.payouts_on() { payout_needs(db, player_id) } else { Vec::new() },
        provider: set.provider,
        cents_per_thousand: set.cents_per_thousand,
        minimum: set.minimum,
        payouts,
    })
}

pub fn ask_payout(db: &Connection, player_id: &str, credits: u32, details: &str) -> Result<i64, Failed> {
    let set = settings(db);
    let refuse = |code: StatusCode, why: String| Err((code, why));
    if !set.payouts_on() {
        return refuse(StatusCode::FORBIDDEN, "cashing out isn't open on this Hub".into());
    }
    let needs = payout_needs(db, player_id);
    if !needs.is_empty() {
        return refuse(StatusCode::FORBIDDEN, format!("first {}", needs.join(", ")));
    }
    let details = details.trim();
    if details.is_empty() || details.chars().count() > MAX_DETAILS {
        return refuse(StatusCode::BAD_REQUEST, format!("say where {} should pay you", set.provider));
    }
    if credits < set.minimum.max(1) {
        return refuse(
            StatusCode::BAD_REQUEST,
            format!("the least you can cash out is {} {CURRENCY}", set.minimum),
        );
    }
    let waiting: Option<i64> = db
        .query_row(
            "SELECT 1 FROM payouts WHERE player_id = ?1 AND state = 'waiting'",
            params![player_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(internal)?;
    if waiting.is_some() {
        return refuse(StatusCode::CONFLICT, "you already have a cash out waiting".into());
    }
    if i64::from(credits) > payable(db, player_id) {
        return refuse(
            StatusCode::PAYMENT_REQUIRED,
            format!("you can cash out up to {} {CURRENCY}", payable(db, player_id)),
        );
    }
    let cents = i64::from(credits) * i64::from(set.cents_per_thousand) / 1000;
    db.execute(
        "INSERT INTO payouts (player_id, credits, cents, details, state, asked) VALUES (?1, ?2, ?3, ?4, 'waiting', ?5)",
        params![player_id, credits, cents, details, now()],
    )
    .map_err(internal)?;
    let id = db.last_insert_rowid();
    if let Err(e) = wallet::debit(db, player_id, credits, "cash-out", &id.to_string()) {
        let _ = db.execute("DELETE FROM payouts WHERE payout_id = ?1", params![id]);
        return Err(e.into());
    }
    Ok(id)
}

pub fn payouts(db: &Connection, state: &str) -> Vec<Payout> {
    let now = now();
    let Ok(mut s) = db.prepare(&format!(
        "SELECT {PAYOUT_COLUMNS} FROM payouts WHERE state = ?1 OR ?1 = '' ORDER BY payout_id"
    )) else {
        return Vec::new();
    };
    s.query_map(params![state], |r| payout_row(r, now))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

pub fn finish_payout(db: &Connection, payout_id: i64, paid: bool, note: &str) -> Result<(), String> {
    let row: Option<(String, u32)> = db
        .query_row(
            "SELECT player_id, credits FROM payouts WHERE payout_id = ?1 AND state = 'waiting'",
            params![payout_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((player_id, credits)) = row else {
        return Err(format!("no cash out {payout_id} is waiting"));
    };
    db.execute(
        "UPDATE payouts SET state = ?2, note = ?3, done = ?4 WHERE payout_id = ?1",
        params![payout_id, if paid { "paid" } else { "refused" }, note.trim(), now()],
    )
    .map_err(|e| e.to_string())?;
    if !paid {
        wallet::credit(db, &player_id, credits, "cash-out-back", &payout_id.to_string())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn internal(e: rusqlite::Error) -> Failed {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn bad(why: &str) -> Failed {
    (StatusCode::BAD_REQUEST, why.to_owned())
}

fn signed_read(player_id: &str, message: &[u8], signature: &str, time: i64) -> Result<(), Failed> {
    if !verify(player_id, message, signature) {
        return Err((StatusCode::FORBIDDEN, "only you can see this".into()));
    }
    if (time - now_ms()).abs() > CLOCK_SLACK_MS {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub(crate) struct Signed {
    time: i64,
    signature: String,
}

pub(crate) async fn get_earnings(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<Signed>,
) -> Result<Json<Earnings>, Failed> {
    signed_read(&id, &earnings_message(&id, q.time), &q.signature, q.time)?;
    let db = db.lock().unwrap();
    settle(&db, now()).map_err(internal)?;
    earnings(&db, &id, now()).map(Json).map_err(internal)
}

pub(crate) async fn cash_out(State(db): State<Db>, Json(c): Json<CashOut>) -> Result<Json<Earnings>, Failed> {
    let db = db.lock().unwrap();
    wallet::check(&db, &c.player_id, &cash_out_message(&c), &c.signature, c.time)?;
    settle(&db, now()).map_err(internal)?;
    ask_payout(&db, &c.player_id, c.credits, &c.details)?;
    earnings(&db, &c.player_id, now()).map(Json).map_err(internal)
}

#[derive(Debug, Deserialize)]
pub(crate) struct PassQuery {
    player: Option<String>,
}

pub(crate) async fn list_passes(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<PassQuery>,
) -> Result<Json<Vec<Pass>>, Failed> {
    passes(&db.lock().unwrap(), &id, q.player.as_deref()).map(Json).map_err(internal)
}

pub(crate) async fn owns(
    State(db): State<Db>,
    UrlPath((pass_id, player)): UrlPath<(i64, String)>,
) -> Json<bool> {
    Json(owns_pass(&db.lock().unwrap(), &player, pass_id))
}

pub(crate) async fn change_pass(
    State(db): State<Db>,
    UrlPath(game_id): UrlPath<String>,
    Json(c): Json<PassChange>,
) -> Result<Json<Vec<Pass>>, Failed> {
    let db = db.lock().unwrap();
    wallet::check(&db, &c.player_id, &pass_message(&game_id, &c), &c.signature, c.time)?;
    let author: Option<Option<String>> = db
        .query_row("SELECT author_id FROM games WHERE game_id = ?1", params![game_id], |r| r.get(0))
        .optional()
        .map_err(internal)?;
    if author.flatten().as_deref() != Some(c.player_id.as_str())
        && !crate::groups::may_edit(&db, &game_id, &c.player_id)
    {
        return Err((StatusCode::FORBIDDEN, "only the game's creator can sell passes for it".into()));
    }
    if c.action != PassAction::Remove {
        let name = c.name.trim();
        if name.is_empty() || name.chars().count() > MAX_PASS_NAME {
            return Err(bad("a pass needs a short name"));
        }
        if c.about.chars().count() > MAX_PASS_ABOUT {
            return Err(bad("that description is too long"));
        }
        if crate::filter::clean(name).as_deref() != Some(name)
            || crate::filter::clean(&c.about).as_deref() != Some(c.about.as_str())
        {
            return Err(bad("pick friendlier words"));
        }
        if c.price == 0 || c.price > wallet::MAX_PRICE {
            return Err(bad(&format!("a pass costs 1 to {} {CURRENCY}", wallet::MAX_PRICE)));
        }
    }
    let changed = match c.action {
        PassAction::Add => {
            if passes(&db, &game_id, None).map_err(internal)?.len() >= MAX_PASSES {
                return Err(bad("that game has as many passes as it can"));
            }
            db.execute(
                "INSERT INTO passes (game_id, name, about, price, created) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![game_id, c.name.trim(), c.about.trim(), c.price, now()],
            )
        }
        PassAction::Edit => db.execute(
            "UPDATE passes SET name = ?3, about = ?4, price = ?5 WHERE pass_id = ?1 AND game_id = ?2 AND removed = 0",
            params![c.pass_id, game_id, c.name.trim(), c.about.trim(), c.price],
        ),
        PassAction::Remove => db.execute(
            "UPDATE passes SET removed = 1 WHERE pass_id = ?1 AND game_id = ?2",
            params![c.pass_id, game_id],
        ),
    }
    .map_err(internal)?;
    if changed == 0 {
        return Err((StatusCode::NOT_FOUND, "that pass is gone".into()));
    }
    passes(&db, &game_id, Some(&c.player_id)).map(Json).map_err(internal)
}

pub(crate) async fn buy_pass(State(db): State<Db>, Json(b): Json<BuyPass>) -> Result<Json<Pass>, Failed> {
    let db = db.lock().unwrap();
    wallet::check(
        &db,
        &b.player_id,
        &buy_pass_message(&b.player_id, b.pass_id, b.price, b.time),
        &b.signature,
        b.time,
    )?;
    let found: Option<(String, u32, Option<String>)> = db
        .query_row(
            &format!(
                "SELECT p.game_id, p.price, g.author_id FROM passes p JOIN games g ON g.game_id = p.game_id
                 WHERE p.pass_id = ?1 AND p.removed = 0 AND NOT {} AND NOT {}",
                crate::hidden_sql("game", "g.game_id"),
                crate::owner::banned_sql("g.author_id")
            ),
            params![b.pass_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(internal)?;
    let Some((game_id, price, Some(seller_id))) = found else {
        return Err((StatusCode::NOT_FOUND, "that pass isn't for sale".into()));
    };
    if price != b.price {
        return Err((StatusCode::CONFLICT, "the price changed; look again".into()));
    }
    if owns_pass(&db, &b.player_id, b.pass_id) {
        return Err((StatusCode::CONFLICT, "you already have it".into()));
    }
    let item_id = format!("pass-{}", b.pass_id);
    let sale = Sale {
        buyer_id: b.player_id.clone(),
        seller_id: crate::groups::earner(&db, &game_id, &seller_id),
        game_id: Some(game_id.clone()),
        item_id: item_id.clone(),
        kind: "pass".into(),
        amount: price,
    };
    let seller = seller_id;
    wallet::buy(&db, &sale)?;
    if let Ok(name) = db.query_row("SELECT name FROM passes WHERE pass_id = ?1", params![b.pass_id], |r| {
        r.get::<_, String>(0)
    }) {
        crate::follows::notify(
            &db,
            &seller,
            "sale",
            "",
            &format!("{} bought your pass {name}", crate::follows::name_of(&db, &b.player_id)),
            "/creator",
        );
    }
    db.execute(
        "INSERT OR IGNORE INTO owned (player_id, item_id, time) VALUES (?1, ?2, ?3)",
        params![b.player_id, item_id, now()],
    )
    .map_err(internal)?;
    passes(&db, &game_id, Some(&b.player_id))
        .map_err(internal)?
        .into_iter()
        .find(|p| p.pass_id == b.pass_id)
        .map(Json)
        .ok_or_else(|| internal(rusqlite::Error::QueryReturnedNoRows))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(db: &Connection, id: &str, author: &str) {
        db.execute(
            "INSERT INTO games (game_id, secret, title, author, description, place, version, plays, created, updated, author_id)
             VALUES (?1, '', ?1, '', '', '', 1, 0, 0, 0, ?2)",
            params![id, author],
        )
        .unwrap();
    }

    fn sell(db: &Connection, buyer: &str, seller: &str, amount: u32) {
        let sale = Sale {
            buyer_id: buyer.into(),
            seller_id: seller.into(),
            game_id: Some("g".into()),
            item_id: "pass-1".into(),
            kind: "pass".into(),
            amount,
        };
        wallet::buy(db, &sale).unwrap();
    }

    #[test]
    fn sales_pay_the_creator_after_the_hold() {
        let db = crate::open_db(None).unwrap();
        game(&db, "g", "maker");
        wallet::top_up(&db, "fan", 100, "pay_1").unwrap();
        wallet::credit(&db, "fan", 100, "daily", "1").unwrap();
        sell(&db, "fan", "maker", 100);
        sell(&db, "fan", "maker", 100);
        settle(&db, now()).unwrap();
        assert_eq!(wallet::balance(&db, "maker"), 0);
        assert_eq!(earnings(&db, "maker", now()).unwrap().pending, 140);
        settle(&db, now() + HOLD_DAYS * DAY + 1).unwrap();
        settle(&db, now() + HOLD_DAYS * DAY + 1).unwrap();
        assert_eq!(wallet::balance(&db, "maker"), 70);
        assert_eq!(payable(&db, "maker"), 70);
        let e = earnings(&db, "maker", now()).unwrap();
        assert_eq!((e.total, e.pending, e.games[0].passes), (70, 0, 70));
    }

    #[test]
    fn free_credits_are_taken_back_once() {
        let db = crate::open_db(None).unwrap();
        wallet::top_up(&db, "fan", 100, "pay_1").unwrap();
        wallet::credit(&db, "fan", 25, "daily", "1").unwrap();
        wallet::credit(&db, "fan", 100, "badge", "explorer").unwrap();
        wallet::credit(&db, "bo", 100, "badge", "explorer").unwrap();
        wallet::debit(&db, "bo", 60, "buy", "1").unwrap();
        assert_eq!(wallet::free_to_take(&db).unwrap(), vec![("bo".into(), 40), ("fan".into(), 125)]);
        wallet::take_back_free(&db).unwrap();
        assert_eq!((wallet::balance(&db, "fan"), wallet::balance(&db, "bo")), (100, 0));
        assert!(wallet::take_back_free(&db).unwrap().is_empty());
    }

    #[test]
    fn own_and_platform_sales_earn_nothing() {
        let db = crate::open_db(None).unwrap();
        wallet::top_up(&db, "maker", 200, "pay_1").unwrap();
        sell(&db, "maker", "maker", 50);
        sell(&db, "maker", "libroxel", 50);
        settle(&db, now() + HOLD_DAYS * DAY + 1).unwrap();
        assert_eq!((wallet::balance(&db, "maker"), payable(&db, "maker")), (100, 0));
    }

    #[test]
    fn the_engagement_pool_splits_by_playtime_and_skips_the_creator() {
        let db = crate::open_db(None).unwrap();
        game(&db, "a", "ann");
        game(&db, "b", "bob");
        set_settings(&db, &EarningsSettings { pool: 900, ..Default::default() }).unwrap();
        let day = now().div_euclid(DAY) - 1;
        let at = day * DAY + 100;
        for (g, p, secs) in
            [("a", "x", 600), ("a", "y", 600), ("b", "x", 600), ("a", "ann", 5000), ("b", "z", 99_999)]
        {
            let code = crate::analytics::player_code(&db, g, p).unwrap();
            db.execute(
                "INSERT INTO play_sessions (game_id, player, started, last_seen) VALUES (?1, ?2, ?3, ?4)",
                params![g, code, at, at + secs],
            )
            .unwrap();
        }
        settle(&db, now()).unwrap();
        settle(&db, now()).unwrap();
        let (ann, bob) = (wallet::balance(&db, "ann"), wallet::balance(&db, "bob"));
        assert_eq!(ann, 900 * 1200 / (1200 + 600 + PLAY_CAP_SECS));
        assert_eq!(bob, 900 * (600 + PLAY_CAP_SECS) / (1200 + 600 + PLAY_CAP_SECS));
        assert_eq!(payable(&db, "ann"), ann);
    }

    #[test]
    fn cashing_out_stays_shut_until_set_up() {
        let db = crate::open_db(None).unwrap();
        db.execute(
            "INSERT OR IGNORE INTO engagement (day, game_id, creator_id, secs, earned) VALUES (1, 'g', 'ann', 1, 5000)",
            [],
        )
        .unwrap();
        wallet::credit(&db, "ann", 5000, "engagement", "1-g").unwrap();
        assert_eq!(ask_payout(&db, "ann", 1000, "ann@example.com").unwrap_err().0, StatusCode::FORBIDDEN);
        set_settings(
            &db,
            &EarningsSettings { pool: 0, provider: "PayPal".into(), cents_per_thousand: 350, minimum: 1000 },
        )
        .unwrap();
        let err = ask_payout(&db, "ann", 1000, "ann@example.com").unwrap_err().1;
        assert!(err.contains("18") && err.contains("email") && err.contains("two-step"), "{err}");
        db.execute("INSERT INTO family (player_id, born, settings) VALUES ('ann', '1990-01-01', '{}')", [])
            .unwrap();
        db.execute("INSERT INTO two_step (player_id, secret, last_step) VALUES ('ann', 'x', 0)", []).unwrap();
        db.execute(
            "INSERT INTO emails (player_id, email, verified) VALUES ('ann', seal('ann@example.com'), 1)",
            [],
        )
        .unwrap();
        assert_eq!(payout_needs(&db, "ann"), Vec::<String>::new());
        assert!(ask_payout(&db, "ann", 999, "ann@example.com").is_err());
        assert!(ask_payout(&db, "ann", 6000, "ann@example.com").is_err());
        let id = ask_payout(&db, "ann", 2000, "ann@example.com").unwrap();
        assert!(ask_payout(&db, "ann", 1000, "ann@example.com").is_err());
        assert_eq!((wallet::balance(&db, "ann"), payable(&db, "ann")), (3000, 3000));
        assert_eq!(payouts(&db, "waiting")[0].cents, 700);
        finish_payout(&db, id, false, "wrong address").unwrap();
        assert_eq!((wallet::balance(&db, "ann"), payable(&db, "ann")), (5000, 5000));
        let id = ask_payout(&db, "ann", 5000, "ann@example.com").unwrap();
        finish_payout(&db, id, true, "PAYPAL-123").unwrap();
        assert_eq!((wallet::balance(&db, "ann"), payable(&db, "ann")), (0, 0));
        assert!(finish_payout(&db, id, true, "").is_err());
    }
}

use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{Db, hidden_sql, now, owner, verify};
use crate::social::now_ms;
use crate::wallet::{self, Failed, MAX_PRICE, Refused, Sale, Wallet, check};

pub const MAX_STOCK: u32 = 100_000;
pub const MAX_SALE_DAYS: i64 = 90;
pub const RESALE_FEE: u32 = 30;
pub const TRADE_ITEMS: usize = 4;
pub const OPEN_TRADES: i64 = 50;
const DAY: i64 = 24 * 60 * 60;
const CLOCK_SLACK_MS: i64 = 10 * 60 * 1000;

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS limited (
            item_id     TEXT PRIMARY KEY,
            stock       INTEGER,
            ends        INTEGER,
            sold        INTEGER NOT NULL,
            made        INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS serials (
            item_id     TEXT NOT NULL,
            serial      INTEGER NOT NULL,
            owner_id    TEXT NOT NULL,
            time        INTEGER NOT NULL,
            PRIMARY KEY (item_id, serial)
        );
        CREATE INDEX IF NOT EXISTS serials_owner ON serials(owner_id, item_id);
        CREATE TABLE IF NOT EXISTS transfers (
            transfer_id INTEGER PRIMARY KEY,
            item_id     TEXT NOT NULL,
            serial      INTEGER NOT NULL,
            from_id     TEXT NOT NULL,
            to_id       TEXT NOT NULL,
            reason      TEXT NOT NULL,
            reference   TEXT NOT NULL,
            time        INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS transfers_item ON transfers(item_id, serial, transfer_id);
        CREATE TABLE IF NOT EXISTS resale (
            item_id     TEXT NOT NULL,
            serial      INTEGER NOT NULL,
            seller_id   TEXT NOT NULL,
            price       INTEGER NOT NULL,
            time        INTEGER NOT NULL,
            PRIMARY KEY (item_id, serial)
        );
        CREATE TABLE IF NOT EXISTS resales (
            sale_id     INTEGER PRIMARY KEY,
            item_id     TEXT NOT NULL,
            serial      INTEGER NOT NULL,
            seller_id   TEXT NOT NULL,
            buyer_id    TEXT NOT NULL,
            price       INTEGER NOT NULL,
            fee         INTEGER NOT NULL,
            time        INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS resales_item ON resales(item_id, sale_id);
        CREATE INDEX IF NOT EXISTS resales_buyer ON resales(buyer_id, time);
        CREATE TABLE IF NOT EXISTS trades (
            trade_id    INTEGER PRIMARY KEY,
            from_id     TEXT NOT NULL,
            to_id       TEXT NOT NULL,
            give        TEXT NOT NULL,
            get         TEXT NOT NULL,
            state       TEXT NOT NULL,
            from_ok     INTEGER NOT NULL,
            to_ok       INTEGER NOT NULL,
            made        INTEGER NOT NULL,
            updated     INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS trades_from ON trades(from_id, state);
        CREATE INDEX IF NOT EXISTS trades_to ON trades(to_id, state);",
    )
    .map_err(|e| format!("could not set up limited items: {e}"))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Limited {
    pub stock: Option<u32>,
    pub sold: u32,
    pub ends_in_secs: Option<i64>,
    pub on_sale: bool,
    pub lowest: Option<u32>,
    pub average: Option<u32>,
}

struct Row {
    stock: Option<u32>,
    ends: Option<i64>,
    sold: u32,
}

fn row(db: &Connection, item_id: &str) -> rusqlite::Result<Option<Row>> {
    db.query_row("SELECT stock, ends, sold FROM limited WHERE item_id = ?1", params![item_id], |r| {
        Ok(Row { stock: r.get(0)?, ends: r.get(1)?, sold: r.get(2)? })
    })
    .optional()
}

impl Row {
    fn on_sale(&self, now: i64) -> bool {
        self.stock.is_none_or(|stock| self.sold < stock) && self.ends.is_none_or(|ends| now < ends)
    }
}

pub fn is_limited(db: &Connection, item_id: &str) -> bool {
    row(db, item_id).ok().flatten().is_some()
}

fn lowest(db: &Connection, item_id: &str) -> Option<u32> {
    db.query_row(
        &format!(
            "SELECT MIN(price) FROM resale WHERE item_id = ?1 AND NOT {}",
            owner::banned_sql("seller_id")
        ),
        params![item_id],
        |r| r.get(0),
    )
    .ok()
    .flatten()
}

fn average(db: &Connection, item_id: &str) -> Option<u32> {
    db.query_row(
        "SELECT CAST(AVG(price) AS INTEGER) FROM (SELECT price FROM resales WHERE item_id = ?1 ORDER BY sale_id DESC LIMIT 20)",
        params![item_id],
        |r| r.get(0),
    )
    .ok()
    .flatten()
}

pub fn summary(db: &Connection, item_id: &str) -> Option<Limited> {
    let row = row(db, item_id).ok().flatten()?;
    let now = now();
    Some(Limited {
        stock: row.stock,
        sold: row.sold,
        ends_in_secs: row.ends.map(|ends| (ends - now).max(0)),
        on_sale: row.on_sale(now),
        lowest: lowest(db, item_id),
        average: average(db, item_id),
    })
}

pub fn limited_message(
    player_id: &str,
    item_id: &str,
    stock: Option<u32>,
    days: Option<u32>,
    time: i64,
) -> Vec<u8> {
    let or_none = |v: Option<u32>| v.map(|v| v.to_string()).unwrap_or_default();
    format!("libroxel limited\n{player_id}\n{item_id}\n{}\n{}\n{time}", or_none(stock), or_none(days))
        .into_bytes()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MakeLimited {
    pub player_id: String,
    #[serde(default)]
    pub stock: Option<u32>,
    #[serde(default)]
    pub days: Option<u32>,
    pub time: i64,
    pub signature: String,
}

fn internal(e: rusqlite::Error) -> Failed {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn refused(r: Refused) -> Failed {
    r.into()
}

pub(crate) async fn make_limited(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Json(m): Json<MakeLimited>,
) -> Result<Json<Limited>, Failed> {
    let db = db.lock().unwrap();
    check(
        &db,
        &m.player_id,
        &limited_message(&m.player_id, &id, m.stock, m.days, m.time),
        &m.signature,
        m.time,
    )?;
    let bad = |why: &str| Err((StatusCode::BAD_REQUEST, why.to_owned()));
    if m.stock.is_none() && m.days.is_none() {
        return bad("give it a stock, a number of days on sale, or both");
    }
    if m.stock.is_some_and(|s| s == 0 || s > MAX_STOCK) {
        return bad(&format!("the stock has to be from 1 to {MAX_STOCK}"));
    }
    if m.days.is_some_and(|d| d == 0 || i64::from(d) > MAX_SALE_DAYS) {
        return bad(&format!("it can be on sale for 1 to {MAX_SALE_DAYS} days"));
    }
    let item: Option<(String, u32)> = db
        .query_row("SELECT creator_id, price FROM items WHERE item_id = ?1", params![id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()
        .map_err(internal)?;
    let Some((creator, price)) = item else {
        return Err((StatusCode::NOT_FOUND, "that isn't in the catalog".into()));
    };
    if creator != m.player_id {
        return Err((StatusCode::FORBIDDEN, "only its creator can make it limited".into()));
    }
    if price == 0 {
        return bad("give it a price first");
    }
    if row(&db, &id).map_err(internal)?.is_some() {
        return Err((StatusCode::CONFLICT, "it is already limited".into()));
    }
    let owners: i64 = db
        .query_row("SELECT COUNT(*) FROM owned WHERE item_id = ?1", params![id], |r| r.get(0))
        .map_err(internal)?;
    if owners > 0 {
        return Err((StatusCode::CONFLICT, "people already have it, so it can't be limited now".into()));
    }
    let now = now();
    db.execute(
        "INSERT INTO limited (item_id, stock, ends, sold, made) VALUES (?1, ?2, ?3, 0, ?4)",
        params![id, m.stock, m.days.map(|d| now + i64::from(d) * DAY), now],
    )
    .map_err(internal)?;
    Ok(Json(summary(&db, &id).expect("it was just made")))
}

fn move_serial(
    tx: &Connection,
    item_id: &str,
    serial: u32,
    from: &str,
    to: &str,
    reason: &str,
    reference: &str,
) -> rusqlite::Result<bool> {
    let now = now();
    let moved = tx.execute(
        "UPDATE serials SET owner_id = ?4, time = ?5 WHERE item_id = ?1 AND serial = ?2 AND owner_id = ?3",
        params![item_id, serial, from, to, now],
    )?;
    if moved == 0 {
        return Ok(false);
    }
    tx.execute(
        "INSERT INTO transfers (item_id, serial, from_id, to_id, reason, reference, time)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![item_id, serial, from, to, reason, reference, now],
    )?;
    tx.execute("DELETE FROM resale WHERE item_id = ?1 AND serial = ?2", params![item_id, serial])?;
    tx.execute(
        "UPDATE trades SET state = 'gone', updated = ?2
         WHERE state IN ('offered', 'accepted') AND (instr(give, ?1) > 0 OR instr(get, ?1) > 0)",
        params![piece(item_id, serial), now],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO owned (player_id, item_id, time) VALUES (?1, ?2, ?3)",
        params![to, item_id, now],
    )?;
    tx.execute(
        "DELETE FROM owned WHERE player_id = ?1 AND item_id = ?2
         AND NOT EXISTS (SELECT 1 FROM serials WHERE owner_id = ?1 AND item_id = ?2)",
        params![from, item_id],
    )?;
    Ok(true)
}

/// The first sale of a limited item: the next serial, the Credits and the
/// record of it, all at once or not at all.
pub(crate) fn buy_first(db: &Connection, sale: &Sale) -> Result<u32, Failed> {
    let spent = u32::try_from(wallet::spent(db, &sale.buyer_id)).unwrap_or(u32::MAX);
    if !crate::family::may_spend(db, &sale.buyer_id, spent, sale.amount) {
        return Err(refused(Refused::Parent));
    }
    let tx = db.unchecked_transaction().map_err(internal)?;
    let Some(row) = row(&tx, &sale.item_id).map_err(internal)? else {
        return Err((StatusCode::NOT_FOUND, "that isn't limited".into()));
    };
    if !row.on_sale(now()) {
        return Err((StatusCode::GONE, "it is sold out; look for one for resale".into()));
    }
    if wallet::balance(&tx, &sale.buyer_id) < i64::from(sale.amount) {
        return Err(refused(Refused::Short));
    }
    let serial = row.sold + 1;
    tx.execute(
        "INSERT INTO purchases (buyer_id, seller_id, game_id, item_id, kind, amount, time)
         VALUES (?1, ?2, NULL, ?3, 'item', ?4, ?5)",
        params![sale.buyer_id, sale.seller_id, sale.item_id, sale.amount, now()],
    )
    .map_err(internal)?;
    let purchase = tx.last_insert_rowid().to_string();
    wallet::entry(&tx, &sale.buyer_id, -i64::from(sale.amount), "buy", &purchase).map_err(internal)?;
    tx.execute("UPDATE limited SET sold = ?2 WHERE item_id = ?1", params![sale.item_id, serial])
        .map_err(internal)?;
    tx.execute(
        "INSERT INTO serials (item_id, serial, owner_id, time) VALUES (?1, ?2, ?3, ?4)",
        params![sale.item_id, serial, sale.buyer_id, now()],
    )
    .map_err(internal)?;
    tx.execute(
        "INSERT INTO transfers (item_id, serial, from_id, to_id, reason, reference, time)
         VALUES (?1, ?2, '', ?3, 'bought', ?4, ?5)",
        params![sale.item_id, serial, sale.buyer_id, purchase, now()],
    )
    .map_err(internal)?;
    tx.execute(
        "INSERT OR IGNORE INTO owned (player_id, item_id, time) VALUES (?1, ?2, ?3)",
        params![sale.buyer_id, sale.item_id, now()],
    )
    .map_err(internal)?;
    tx.commit().map_err(internal)?;
    Ok(serial)
}

pub(crate) fn resale_spent(db: &Connection, player_id: &str, since: i64) -> i64 {
    db.query_row(
        "SELECT COALESCE(SUM(price), 0) FROM resales WHERE buyer_id = ?1 AND time >= ?2",
        params![player_id, since],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

pub fn fee(price: u32) -> u32 {
    price * RESALE_FEE / 100
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResaleAction {
    List,
    Unlist,
    Buy,
}

impl ResaleAction {
    fn word(self) -> &'static str {
        match self {
            ResaleAction::List => "list",
            ResaleAction::Unlist => "unlist",
            ResaleAction::Buy => "buy",
        }
    }
}

pub fn resale_message(
    player_id: &str,
    action: ResaleAction,
    item_id: &str,
    serial: u32,
    price: u32,
    time: i64,
) -> Vec<u8> {
    format!("libroxel resale\n{player_id}\n{}\n{item_id}\n{serial}\n{price}\n{time}", action.word())
        .into_bytes()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResaleChange {
    pub player_id: String,
    pub action: ResaleAction,
    pub item_id: String,
    pub serial: u32,
    #[serde(default)]
    pub price: u32,
    pub time: i64,
    pub signature: String,
}

fn owner_of(db: &Connection, item_id: &str, serial: u32) -> rusqlite::Result<Option<String>> {
    db.query_row(
        "SELECT owner_id FROM serials WHERE item_id = ?1 AND serial = ?2",
        params![item_id, serial],
        |r| r.get(0),
    )
    .optional()
}

fn item_shown(db: &Connection, item_id: &str) -> rusqlite::Result<bool> {
    Ok(db
        .query_row(
            &format!(
                "SELECT 1 FROM items i WHERE i.item_id = ?1 AND NOT {} AND NOT {}",
                hidden_sql("item", "i.item_id"),
                owner::banned_sql("i.creator_id")
            ),
            params![item_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

pub(crate) async fn change_resale(
    State(db): State<Db>,
    Json(c): Json<ResaleChange>,
) -> Result<Json<Wallet>, Failed> {
    let db = db.lock().unwrap();
    let message = resale_message(&c.player_id, c.action, &c.item_id, c.serial, c.price, c.time);
    check(&db, &c.player_id, &message, &c.signature, c.time)?;
    let not_found = || Err((StatusCode::NOT_FOUND, "there is no such limited item".into()));
    if !item_shown(&db, &c.item_id).map_err(internal)? {
        return not_found();
    }
    let Some(holder) = owner_of(&db, &c.item_id, c.serial).map_err(internal)? else {
        return not_found();
    };
    match c.action {
        ResaleAction::List => {
            if holder != c.player_id {
                return Err((StatusCode::FORBIDDEN, "that one isn't yours".into()));
            }
            if c.price == 0 || c.price > MAX_PRICE {
                return Err((StatusCode::BAD_REQUEST, format!("the price has to be from 1 to {MAX_PRICE}")));
            }
            db.execute(
                "INSERT INTO resale (item_id, serial, seller_id, price, time) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(item_id, serial) DO UPDATE SET seller_id = ?3, price = ?4, time = ?5",
                params![c.item_id, c.serial, c.player_id, c.price, now()],
            )
            .map_err(internal)?;
        }
        ResaleAction::Unlist => {
            db.execute(
                "DELETE FROM resale WHERE item_id = ?1 AND serial = ?2 AND seller_id = ?3",
                params![c.item_id, c.serial, c.player_id],
            )
            .map_err(internal)?;
        }
        ResaleAction::Buy => resell(&db, &c, &holder)?,
    }
    Ok(Json(wallet::wallet(&db, &c.player_id)))
}

fn resell(db: &Connection, c: &ResaleChange, holder: &str) -> Result<(), Failed> {
    if holder == c.player_id {
        return Err((StatusCode::CONFLICT, "that one is already yours".into()));
    }
    let listed: Option<(String, u32)> = db
        .query_row(
            "SELECT seller_id, price FROM resale WHERE item_id = ?1 AND serial = ?2",
            params![c.item_id, c.serial],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(internal)?;
    let Some((seller, price)) = listed.filter(|(seller, _)| seller == holder) else {
        return Err((StatusCode::NOT_FOUND, "that one isn't for sale".into()));
    };
    if owner::is_banned(db, &seller) {
        return Err((StatusCode::NOT_FOUND, "that one isn't for sale".into()));
    }
    if price != c.price {
        return Err((StatusCode::CONFLICT, "the price changed; look again".into()));
    }
    let spent = u32::try_from(wallet::spent(db, &c.player_id)).unwrap_or(u32::MAX);
    if !crate::family::may_spend(db, &c.player_id, spent, price) {
        return Err(refused(Refused::Parent));
    }
    let tx = db.unchecked_transaction().map_err(internal)?;
    if wallet::balance(&tx, &c.player_id) < i64::from(price) {
        return Err(refused(Refused::Short));
    }
    let fee = fee(price);
    tx.execute(
        "INSERT INTO resales (item_id, serial, seller_id, buyer_id, price, fee, time)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![c.item_id, c.serial, seller, c.player_id, price, fee, now()],
    )
    .map_err(internal)?;
    let sale = tx.last_insert_rowid().to_string();
    let paid = wallet::entry(&tx, &c.player_id, -i64::from(price), "resale-buy", &sale).map_err(internal)?;
    let got = wallet::entry(&tx, &seller, i64::from(price - fee), "resale-sold", &sale).map_err(internal)?;
    let moved =
        move_serial(&tx, &c.item_id, c.serial, &seller, &c.player_id, "resale", &sale).map_err(internal)?;
    if !(paid && got && moved) {
        return Err((StatusCode::CONFLICT, "that one isn't for sale any more".into()));
    }
    tx.commit().map_err(internal)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResaleListing {
    pub serial: u32,
    pub seller: String,
    pub seller_id: String,
    pub price: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PricePoint {
    pub serial: u32,
    pub price: u32,
    pub secs_ago: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemSales {
    pub item_id: String,
    pub limited: Limited,
    pub fee_percent: u32,
    pub owners: u32,
    pub listings: Vec<ResaleListing>,
    pub history: Vec<PricePoint>,
}

pub(crate) async fn item_sales(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<ItemSales>, Failed> {
    let db = db.lock().unwrap();
    let shown = item_shown(&db, &id).map_err(internal)?;
    let Some(limited) = summary(&db, &id).filter(|_| shown) else {
        return Err((StatusCode::NOT_FOUND, "there is no such limited item".into()));
    };
    let owners: u32 = db
        .query_row("SELECT COUNT(DISTINCT owner_id) FROM serials WHERE item_id = ?1", params![id], |r| {
            r.get(0)
        })
        .map_err(internal)?;
    let listings = db
        .prepare(&format!(
            "SELECT r.serial, COALESCE(a.name, 'Someone'), r.seller_id, r.price
             FROM resale r LEFT JOIN accounts a ON a.player_id = r.seller_id
             WHERE r.item_id = ?1 AND NOT {} ORDER BY r.price, r.time LIMIT 100",
            owner::banned_sql("r.seller_id")
        ))
        .and_then(|mut s| {
            s.query_map(params![id], |r| {
                Ok(ResaleListing {
                    serial: r.get(0)?,
                    seller: r.get(1)?,
                    seller_id: r.get(2)?,
                    price: r.get(3)?,
                })
            })
            .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .map_err(internal)?;
    let now = now();
    let history = db
        .prepare("SELECT serial, price, time FROM resales WHERE item_id = ?1 ORDER BY sale_id DESC LIMIT 100")
        .and_then(|mut s| {
            s.query_map(params![id], |r| {
                Ok(PricePoint { serial: r.get(0)?, price: r.get(1)?, secs_ago: now - r.get::<_, i64>(2)? })
            })
            .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .map_err(internal)?;
    Ok(Json(ItemSales { item_id: id, limited, fee_percent: RESALE_FEE, owners, listings, history }))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Piece {
    pub item_id: String,
    pub serial: u32,
}

fn piece(item_id: &str, serial: u32) -> String {
    format!(",{item_id}#{serial},")
}

fn pieces_text(pieces: &[Piece]) -> String {
    pieces.iter().map(|p| format!("{}#{}", p.item_id, p.serial)).collect::<Vec<_>>().join(",")
}

fn stored(pieces: &[Piece]) -> String {
    format!(",{},", pieces_text(pieces))
}

fn unstored(text: &str) -> Vec<Piece> {
    text.split(',')
        .filter_map(|p| {
            let (item_id, serial) = p.rsplit_once('#')?;
            Some(Piece { item_id: item_id.to_owned(), serial: serial.parse().ok()? })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Owned {
    pub item_id: String,
    pub name: String,
    pub serial: u32,
    pub stock: Option<u32>,
    pub listed: Option<u32>,
}

pub(crate) async fn serials(State(db): State<Db>, UrlPath(id): UrlPath<String>) -> Json<Vec<Owned>> {
    let db = db.lock().unwrap();
    Json(owned_by(&db, &id))
}

pub fn owned_by(db: &Connection, player_id: &str) -> Vec<Owned> {
    db.prepare(&format!(
        "SELECT s.item_id, i.name, s.serial, l.stock, r.price
         FROM serials s JOIN items i ON i.item_id = s.item_id JOIN limited l ON l.item_id = s.item_id
         LEFT JOIN resale r ON r.item_id = s.item_id AND r.serial = s.serial
         WHERE s.owner_id = ?1 AND NOT {}
         ORDER BY i.name, s.serial",
        hidden_sql("item", "s.item_id")
    ))
    .and_then(|mut s| {
        s.query_map(params![player_id], |r| {
            Ok(Owned {
                item_id: r.get(0)?,
                name: r.get(1)?,
                serial: r.get(2)?,
                stock: r.get(3)?,
                listed: r.get(4)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
    })
    .unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TradeAction {
    Offer,
    Accept,
    Decline,
    Cancel,
    Confirm,
}

impl TradeAction {
    fn word(self) -> &'static str {
        match self {
            TradeAction::Offer => "offer",
            TradeAction::Accept => "accept",
            TradeAction::Decline => "decline",
            TradeAction::Cancel => "cancel",
            TradeAction::Confirm => "confirm",
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn trade_message(
    player_id: &str,
    action: TradeAction,
    trade_id: i64,
    to_id: &str,
    give: &[Piece],
    get: &[Piece],
    time: i64,
) -> Vec<u8> {
    format!(
        "libroxel trade\n{player_id}\n{}\n{trade_id}\n{to_id}\n{}\n{}\n{time}",
        action.word(),
        pieces_text(give),
        pieces_text(get)
    )
    .into_bytes()
}

pub fn trades_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel trades\n{player_id}\n{time}").into_bytes()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeChange {
    pub player_id: String,
    pub action: TradeAction,
    #[serde(default)]
    pub trade_id: i64,
    #[serde(default)]
    pub to_id: String,
    #[serde(default)]
    pub give: Vec<Piece>,
    #[serde(default)]
    pub get: Vec<Piece>,
    pub time: i64,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradePiece {
    pub item_id: String,
    pub serial: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trade {
    pub trade_id: i64,
    pub from_id: String,
    pub from: String,
    pub to_id: String,
    pub to: String,
    pub give: Vec<TradePiece>,
    pub get: Vec<TradePiece>,
    pub state: String,
    pub from_confirmed: bool,
    pub to_confirmed: bool,
    pub secs_ago: i64,
}

fn name_of(db: &Connection, player_id: &str) -> String {
    db.query_row("SELECT name FROM accounts WHERE player_id = ?1", params![player_id], |r| r.get(0))
        .unwrap_or_else(|_| "Someone".into())
}

fn named(db: &Connection, pieces: &[Piece]) -> Vec<TradePiece> {
    pieces
        .iter()
        .map(|p| TradePiece {
            item_id: p.item_id.clone(),
            serial: p.serial,
            name: db
                .query_row("SELECT name FROM items WHERE item_id = ?1", params![p.item_id], |r| r.get(0))
                .unwrap_or_else(|_| "Gone".into()),
        })
        .collect()
}

const TRADE_COLUMNS: &str = "trade_id, from_id, to_id, give, get, state, from_ok, to_ok, updated";

fn trade_row(db: &Connection, r: &rusqlite::Row, now: i64) -> rusqlite::Result<Trade> {
    let (from_id, to_id): (String, String) = (r.get(1)?, r.get(2)?);
    Ok(Trade {
        trade_id: r.get(0)?,
        from: name_of(db, &from_id),
        to: name_of(db, &to_id),
        from_id,
        to_id,
        give: named(db, &unstored(&r.get::<_, String>(3)?)),
        get: named(db, &unstored(&r.get::<_, String>(4)?)),
        state: r.get(5)?,
        from_confirmed: r.get(6)?,
        to_confirmed: r.get(7)?,
        secs_ago: now - r.get::<_, i64>(8)?,
    })
}

fn trade(db: &Connection, trade_id: i64) -> rusqlite::Result<Option<Trade>> {
    let now = now();
    db.query_row(&format!("SELECT {TRADE_COLUMNS} FROM trades WHERE trade_id = ?1"), params![trade_id], |r| {
        trade_row(db, r, now)
    })
    .optional()
}

pub fn trades_of(db: &Connection, player_id: &str) -> Vec<Trade> {
    let now = now();
    db.prepare(&format!(
        "SELECT {TRADE_COLUMNS} FROM trades WHERE from_id = ?1 OR to_id = ?1
         ORDER BY state NOT IN ('offered', 'accepted'), updated DESC LIMIT 100"
    ))
    .and_then(|mut s| {
        s.query_map(params![player_id], |r| trade_row(db, r, now))
            .map(|rows| rows.filter_map(Result::ok).collect())
    })
    .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradesSigned {
    pub time: i64,
    pub signature: String,
}

pub(crate) async fn list_trades(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Query(signed): Query<TradesSigned>,
) -> Result<Json<Vec<Trade>>, Failed> {
    if !verify(&id, &trades_message(&id, signed.time), &signed.signature) {
        return Err((StatusCode::FORBIDDEN, "only you can see this".into()));
    }
    if (signed.time - now_ms()).abs() > CLOCK_SLACK_MS {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    Ok(Json(trades_of(&db.lock().unwrap(), &id)))
}

fn all_owned_by(db: &Connection, pieces: &[Piece], player_id: &str) -> rusqlite::Result<bool> {
    for p in pieces {
        if owner_of(db, &p.item_id, p.serial)?.as_deref() != Some(player_id) || !item_shown(db, &p.item_id)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn offer(db: &Connection, c: &TradeChange) -> Result<i64, Failed> {
    let bad = |why: &str| Err((StatusCode::BAD_REQUEST, why.to_owned()));
    if c.to_id == c.player_id {
        return bad("you can't trade with yourself");
    }
    if c.give.is_empty() && c.get.is_empty() {
        return bad("a trade needs at least one item");
    }
    if c.give.len() > TRADE_ITEMS || c.get.len() > TRADE_ITEMS {
        return bad(&format!("each side can put in up to {TRADE_ITEMS} items"));
    }
    let mut all: Vec<&Piece> = c.give.iter().chain(&c.get).collect();
    all.sort();
    all.dedup();
    if all.len() != c.give.len() + c.get.len() {
        return bad("each item can be in a trade once");
    }
    let other = db
        .query_row("SELECT 1 FROM accounts WHERE player_id = ?1", params![c.to_id], |_| Ok(()))
        .optional()
        .map_err(internal)?
        .is_some();
    if !other || owner::is_banned(db, &c.to_id) {
        return Err((StatusCode::NOT_FOUND, "there is no one with that id here".into()));
    }
    if !all_owned_by(db, &c.give, &c.player_id).map_err(internal)? {
        return Err((StatusCode::CONFLICT, "you don't have everything you offered".into()));
    }
    if !all_owned_by(db, &c.get, &c.to_id).map_err(internal)? {
        return Err((StatusCode::CONFLICT, "they don't have everything you asked for".into()));
    }
    let open: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM trades WHERE from_id = ?1 AND state IN ('offered', 'accepted')",
            params![c.player_id],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if open >= OPEN_TRADES {
        return Err((StatusCode::TOO_MANY_REQUESTS, format!("you can have up to {OPEN_TRADES} open trades")));
    }
    let now = now();
    db.execute(
        "INSERT INTO trades (from_id, to_id, give, get, state, from_ok, to_ok, made, updated)
         VALUES (?1, ?2, ?3, ?4, 'offered', 0, 0, ?5, ?5)",
        params![c.player_id, c.to_id, stored(&c.give), stored(&c.get), now],
    )
    .map_err(internal)?;
    Ok(db.last_insert_rowid())
}

fn finish(
    db: &Connection,
    id: i64,
    from: &str,
    to: &str,
    give: &[Piece],
    get: &[Piece],
) -> Result<bool, Failed> {
    let tx = db.unchecked_transaction().map_err(internal)?;
    let reference = id.to_string();
    let ok =
        all_owned_by(&tx, give, from).map_err(internal)? && all_owned_by(&tx, get, to).map_err(internal)?;
    if !ok {
        drop(tx);
        db.execute("UPDATE trades SET state = 'gone', updated = ?2 WHERE trade_id = ?1", params![id, now()])
            .map_err(internal)?;
        return Ok(false);
    }
    for p in give {
        if !move_serial(&tx, &p.item_id, p.serial, from, to, "trade", &reference).map_err(internal)? {
            return Ok(false);
        }
    }
    for p in get {
        if !move_serial(&tx, &p.item_id, p.serial, to, from, "trade", &reference).map_err(internal)? {
            return Ok(false);
        }
    }
    tx.execute(
        "UPDATE trades SET state = 'done', from_ok = 1, to_ok = 1, updated = ?2 WHERE trade_id = ?1",
        params![id, now()],
    )
    .map_err(internal)?;
    tx.commit().map_err(internal)?;
    Ok(true)
}

pub(crate) async fn change_trade(
    State(db): State<Db>,
    Json(c): Json<TradeChange>,
) -> Result<Json<Trade>, Failed> {
    let db = db.lock().unwrap();
    let message = trade_message(&c.player_id, c.action, c.trade_id, &c.to_id, &c.give, &c.get, c.time);
    check(&db, &c.player_id, &message, &c.signature, c.time)?;
    let id = if c.action == TradeAction::Offer {
        offer(&db, &c)?
    } else {
        let found: Option<(String, String, String, String, String, bool, bool)> = db
            .query_row(
                "SELECT from_id, to_id, give, get, state, from_ok, to_ok FROM trades WHERE trade_id = ?1",
                params![c.trade_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )
            .optional()
            .map_err(internal)?;
        let Some((from, to, give, get, state, from_ok, to_ok)) =
            found.filter(|(from, to, ..)| *from == c.player_id || *to == c.player_id)
        else {
            return Err((StatusCode::NOT_FOUND, "there is no such trade".into()));
        };
        let is_to = to == c.player_id;
        let set = |state: &str| {
            db.execute(
                "UPDATE trades SET state = ?2, updated = ?3 WHERE trade_id = ?1",
                params![c.trade_id, state, now()],
            )
            .map_err(internal)
        };
        let closed = || Err((StatusCode::CONFLICT, format!("that trade is {state}")));
        match c.action {
            TradeAction::Offer => unreachable!(),
            TradeAction::Accept | TradeAction::Decline => {
                if !is_to {
                    return Err((
                        StatusCode::FORBIDDEN,
                        "only the one it was offered to can answer it".into(),
                    ));
                }
                if state != "offered" {
                    return closed();
                }
                set(if c.action == TradeAction::Accept { "accepted" } else { "declined" })?;
            }
            TradeAction::Cancel => {
                if state != "offered" && state != "accepted" {
                    return closed();
                }
                set("cancelled")?;
            }
            TradeAction::Confirm => {
                if state != "accepted" {
                    return closed();
                }
                let (from_ok, to_ok) = if is_to { (from_ok, true) } else { (true, to_ok) };
                if from_ok && to_ok {
                    if owner::is_banned(&db, if is_to { &from } else { &to }) {
                        return Err((StatusCode::CONFLICT, "that trade can't go ahead".into()));
                    }
                    if !finish(&db, c.trade_id, &from, &to, &unstored(&give), &unstored(&get))? {
                        return Err((
                            StatusCode::CONFLICT,
                            "the items in that trade aren't all there any more".into(),
                        ));
                    }
                } else {
                    db.execute(
                        "UPDATE trades SET from_ok = ?2, to_ok = ?3, updated = ?4 WHERE trade_id = ?1",
                        params![c.trade_id, from_ok, to_ok, now()],
                    )
                    .map_err(internal)?;
                }
            }
        }
        c.trade_id
    };
    trade(&db, id)
        .map_err(internal)?
        .map(Json)
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "it went missing".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_read_back() {
        let pieces =
            vec![Piece { item_id: "a#b".into(), serial: 3 }, Piece { item_id: "c".into(), serial: 12 }];
        assert_eq!(unstored(&stored(&pieces)), pieces);
        assert!(!stored(&pieces).contains(&piece("c", 1)));
        assert!(stored(&pieces).contains(&piece("c", 12)));
    }

    #[test]
    fn the_fee_rounds_down() {
        assert_eq!((fee(100), fee(9), fee(1)), (30, 2, 0));
    }
}

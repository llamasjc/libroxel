use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{Db, now, verify};
use crate::social::now_ms;

pub const CURRENCY: &str = "Credits";
pub const MAX_PRICE: u32 = 100_000;
const MONTH_SECS: i64 = 30 * 24 * 60 * 60;
const CLOCK_SLACK_MS: i64 = 10 * 60 * 1000;

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS wallets (
            player_id   TEXT PRIMARY KEY,
            balance     INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS ledger (
            entry_id    INTEGER PRIMARY KEY,
            player_id   TEXT NOT NULL,
            amount      INTEGER NOT NULL,
            reason      TEXT NOT NULL,
            reference   TEXT NOT NULL,
            time        INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS ledger_player ON ledger(player_id, entry_id);
        CREATE UNIQUE INDEX IF NOT EXISTS ledger_once ON ledger(player_id, reason, reference) WHERE reference <> '';
        CREATE TABLE IF NOT EXISTS purchases (
            purchase_id INTEGER PRIMARY KEY,
            buyer_id    TEXT NOT NULL,
            seller_id   TEXT NOT NULL,
            game_id     TEXT,
            item_id     TEXT NOT NULL,
            kind        TEXT NOT NULL,
            amount      INTEGER NOT NULL,
            time        INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS purchases_buyer ON purchases(buyer_id, time);
        CREATE INDEX IF NOT EXISTS purchases_seller ON purchases(seller_id, time);
        CREATE TABLE IF NOT EXISTS owned (
            player_id   TEXT NOT NULL,
            item_id     TEXT NOT NULL,
            time        INTEGER NOT NULL,
            PRIMARY KEY (player_id, item_id)
        );
        CREATE TABLE IF NOT EXISTS wallet_clock (
            player_id   TEXT PRIMARY KEY,
            time        INTEGER NOT NULL
        );",
    )
    .map_err(|e| format!("could not set up wallets: {e}"))?;
    let _ = db.execute("ALTER TABLE items ADD COLUMN price INTEGER NOT NULL DEFAULT 0", []);
    for (id, name, price, parts) in shop_items() {
        let data = serde_json::json!({ "name": name, "parts": parts }).to_string();
        db.execute(
            "INSERT OR IGNORE INTO items (item_id, creator_id, name, kind, data, wears, created, price)
             VALUES (?1, 'playlibre', ?2, 'hat', ?3, 0, 0, ?4)",
            params![id, name, data, price],
        )
        .map_err(|e| format!("could not set up the shop: {e}"))?;
    }
    Ok(())
}

fn shop_items() -> Vec<(&'static str, &'static str, u32, serde_json::Value)> {
    use serde_json::json;
    let part = |size: [f32; 3], at: [f32; 3], turn: [f32; 3], shape: &str, color: [u8; 3], neon: bool| json!({ "size": size, "at": at, "turn": turn, "shape": shape, "color": color, "neon": neon });
    let flat = [0.0, 0.0, 0.0];
    vec![
        (
            "shop-party-hat",
            "Party hat",
            50,
            json!([
                part([1.2, 0.4, 1.2], [0.0, 0.9, 0.0], flat, "Block", [226, 86, 150], false),
                part([0.8, 0.4, 0.8], [0.0, 1.3, 0.0], flat, "Block", [250, 220, 70], false),
                part([0.45, 0.4, 0.45], [0.0, 1.7, 0.0], flat, "Block", [226, 86, 150], false),
                part([0.35, 0.35, 0.35], [0.0, 2.05, 0.0], flat, "Ball", [240, 240, 240], false),
            ]),
        ),
        (
            "shop-headphones",
            "Headphones",
            80,
            json!([
                part([1.9, 0.2, 0.3], [0.0, 0.85, 0.0], flat, "Block", [40, 40, 46], false),
                part([0.2, 0.9, 0.3], [-0.9, 0.4, 0.0], flat, "Block", [40, 40, 46], false),
                part([0.2, 0.9, 0.3], [0.9, 0.4, 0.0], flat, "Block", [40, 40, 46], false),
                part([0.3, 0.6, 0.6], [-0.95, -0.1, 0.0], flat, "Block", [70, 150, 230], false),
                part([0.3, 0.6, 0.6], [0.95, -0.1, 0.0], flat, "Block", [70, 150, 230], false),
            ]),
        ),
        (
            "shop-viking-helmet",
            "Viking helmet",
            120,
            json!([
                part([1.75, 0.5, 1.55], [0.0, 0.95, 0.0], flat, "Block", [150, 150, 160], false),
                part([1.8, 0.15, 1.6], [0.0, 0.75, 0.0], flat, "Block", [120, 80, 50], false),
                part([0.25, 0.7, 0.25], [-1.0, 1.2, 0.0], [0.0, 0.0, 25.0], "Block", [240, 230, 200], false),
                part([0.25, 0.7, 0.25], [1.0, 1.2, 0.0], [0.0, 0.0, -25.0], "Block", [240, 230, 200], false),
            ]),
        ),
        (
            "shop-star-crown",
            "Star crown",
            200,
            json!([
                part([1.7, 0.35, 1.5], [0.0, 0.88, 0.0], flat, "Block", [250, 200, 60], false),
                part([0.5, 0.5, 0.5], [0.0, 1.4, -0.5], flat, "Ball", [250, 250, 140], true),
            ]),
        ),
    ]
}

pub fn balance(db: &Connection, player_id: &str) -> i64 {
    db.query_row("SELECT balance FROM wallets WHERE player_id = ?1", params![player_id], |r| r.get(0))
        .optional()
        .ok()
        .flatten()
        .unwrap_or(0)
}

pub(crate) fn entry(
    db: &Connection,
    player_id: &str,
    amount: i64,
    reason: &str,
    reference: &str,
) -> rusqlite::Result<bool> {
    let added = db.execute(
        "INSERT OR IGNORE INTO ledger (player_id, amount, reason, reference, time) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![player_id, amount, reason, reference, now()],
    )?;
    if added == 0 {
        return Ok(false);
    }
    db.execute(
        "INSERT INTO wallets (player_id, balance) VALUES (?1, ?2)
         ON CONFLICT(player_id) DO UPDATE SET balance = balance + ?2",
        params![player_id, amount],
    )?;
    Ok(true)
}

pub fn credit(
    db: &Connection,
    player_id: &str,
    amount: u32,
    reason: &str,
    reference: &str,
) -> rusqlite::Result<bool> {
    let tx = db.unchecked_transaction()?;
    let added = entry(&tx, player_id, amount.into(), reason, reference)?;
    tx.commit()?;
    Ok(added)
}

pub fn debit(
    db: &Connection,
    player_id: &str,
    amount: u32,
    reason: &str,
    reference: &str,
) -> Result<bool, Refused> {
    if balance(db, player_id) < i64::from(amount) {
        return Err(Refused::Short);
    }
    let tx = db.unchecked_transaction()?;
    let taken = entry(&tx, player_id, -i64::from(amount), reason, reference)?;
    tx.commit()?;
    Ok(taken)
}

pub(crate) fn move_credits(
    db: &Connection,
    from: &str,
    to: &str,
    amount: u32,
    reason: &str,
    reference: &str,
) -> rusqlite::Result<()> {
    entry(db, from, -i64::from(amount), reason, reference)?;
    entry(db, to, i64::from(amount), reason, reference)?;
    Ok(())
}

pub fn top_up(db: &Connection, player_id: &str, amount: u32, payment_id: &str) -> rusqlite::Result<bool> {
    credit(db, player_id, amount, "bought", payment_id)
}

const FREE_REASONS: &str = "'daily', 'badge'";

pub fn history(db: &Connection, player_id: &str) -> Vec<(i64, String, String, i64)> {
    db.prepare("SELECT amount, reason, reference, time FROM ledger WHERE player_id = ?1 ORDER BY entry_id")
        .and_then(|mut s| {
            s.query_map(params![player_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default()
}

pub fn free_to_take(db: &Connection) -> rusqlite::Result<Vec<(String, i64)>> {
    let mut s = db.prepare(&format!(
        "SELECT player_id, SUM(owed) FROM (
            SELECT player_id, amount AS owed FROM ledger WHERE reason IN ({FREE_REASONS})
            UNION ALL SELECT seller_id, earned - cashable FROM settled
            UNION ALL SELECT player_id, amount FROM ledger WHERE reason = 'taken-back')
         GROUP BY player_id ORDER BY player_id"
    ))?;
    let owed: Vec<(String, i64)> =
        s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok(owed
        .into_iter()
        .map(|(id, owed)| {
            let have = balance(db, &id);
            (id, owed.min(have))
        })
        .filter(|(_, take)| *take > 0)
        .collect())
}

pub fn take_back_free(db: &Connection) -> rusqlite::Result<Vec<(String, i64)>> {
    let tx = db.unchecked_transaction()?;
    let list = free_to_take(&tx)?;
    let reference = now_ms().to_string();
    for (id, take) in &list {
        entry(&tx, id, -take, "taken-back", &reference)?;
    }
    tx.commit()?;
    Ok(list)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sale {
    pub buyer_id: String,
    pub seller_id: String,
    pub game_id: Option<String>,
    pub item_id: String,
    pub kind: String,
    pub amount: u32,
}

#[derive(Debug)]
pub enum Refused {
    Parent,
    Short,
    Failed(rusqlite::Error),
}

impl From<rusqlite::Error> for Refused {
    fn from(e: rusqlite::Error) -> Self {
        Refused::Failed(e)
    }
}

impl From<Refused> for (StatusCode, String) {
    fn from(r: Refused) -> Self {
        match r {
            Refused::Parent => (StatusCode::FORBIDDEN, "a parent turned off spending on this account".into()),
            Refused::Short => (StatusCode::PAYMENT_REQUIRED, format!("you don't have enough {CURRENCY}")),
            Refused::Failed(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        }
    }
}

pub fn spent(db: &Connection, player_id: &str) -> i64 {
    db.query_row(
        "SELECT COALESCE(SUM(amount), 0) FROM purchases WHERE buyer_id = ?1 AND time >= ?2",
        params![player_id, now() - MONTH_SECS],
        |r| r.get::<_, i64>(0),
    )
    .unwrap_or(0)
        + crate::limited::resale_spent(db, player_id, now() - MONTH_SECS)
}

pub fn buy(db: &Connection, sale: &Sale) -> Result<i64, Refused> {
    let spent = u32::try_from(spent(db, &sale.buyer_id)).unwrap_or(u32::MAX);
    if !crate::family::may_spend(db, &sale.buyer_id, spent, sale.amount) {
        return Err(Refused::Parent);
    }
    if balance(db, &sale.buyer_id) < i64::from(sale.amount) {
        return Err(Refused::Short);
    }
    let tx = db.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO purchases (buyer_id, seller_id, game_id, item_id, kind, amount, time)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![sale.buyer_id, sale.seller_id, sale.game_id, sale.item_id, sale.kind, sale.amount, now()],
    )?;
    let id = tx.last_insert_rowid();
    entry(&tx, &sale.buyer_id, -i64::from(sale.amount), "buy", &id.to_string())?;
    tx.commit()?;
    Ok(id)
}

pub fn owns(db: &Connection, player_id: &str, item_id: &str) -> bool {
    db.query_row(
        "SELECT 1 FROM owned WHERE player_id = ?1 AND item_id = ?2
         UNION SELECT 1 FROM items WHERE item_id = ?2 AND (price = 0 OR creator_id = ?1)",
        params![player_id, item_id],
        |_| Ok(()),
    )
    .optional()
    .ok()
    .flatten()
    .is_some()
}

/// Whether `item_id` is in the player's inventory: bought, got for free,
/// or made by them.
pub fn has(db: &Connection, player_id: &str, item_id: &str) -> bool {
    db.query_row(
        "SELECT 1 FROM owned WHERE player_id = ?1 AND item_id = ?2
         UNION SELECT 1 FROM items WHERE item_id = ?2 AND creator_id = ?1",
        params![player_id, item_id],
        |_| Ok(()),
    )
    .optional()
    .ok()
    .flatten()
    .is_some()
}

pub(crate) fn may_wear(db: &Connection, player_id: &str, look: &serde_json::Value) -> bool {
    let Some(hat) = look.get("item").and_then(playlibre_avatar::CustomHat::from_json) else { return true };
    let Ok(mut statement) = db.prepare(
        "SELECT i.data FROM items i WHERE i.price > 0 AND i.creator_id <> ?1
         AND NOT EXISTS (SELECT 1 FROM owned o WHERE o.player_id = ?1 AND o.item_id = i.item_id)",
    ) else {
        return false;
    };
    let rows = statement.query_map(params![player_id], |r| r.get::<_, String>(0));
    let Ok(rows) = rows else { return false };
    !rows.filter_map(Result::ok).any(|data| {
        serde_json::from_str(&data)
            .ok()
            .and_then(|d| playlibre_avatar::CustomHat::from_json(&d))
            .is_some_and(|sold| sold.parts == hat.parts)
    })
}

fn keep_for_wearers(db: &Connection, item_id: &str) -> rusqlite::Result<()> {
    let data: String =
        db.query_row("SELECT data FROM items WHERE item_id = ?1", params![item_id], |r| r.get(0))?;
    let Some(sold) =
        serde_json::from_str(&data).ok().and_then(|d| playlibre_avatar::CustomHat::from_json(&d))
    else {
        return Ok(());
    };
    let mut statement = db.prepare("SELECT player_id, look FROM accounts WHERE look LIKE '%\"item\"%'")?;
    let wearers: Vec<String> = statement
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(Result::ok)
        .filter(|(_, look)| {
            serde_json::from_str::<serde_json::Value>(look)
                .ok()
                .and_then(|l| l.get("item").and_then(playlibre_avatar::CustomHat::from_json))
                .is_some_and(|hat| hat.parts == sold.parts)
        })
        .map(|(id, _)| id)
        .collect();
    for id in wearers {
        db.execute(
            "INSERT OR IGNORE INTO owned (player_id, item_id, time) VALUES (?1, ?2, ?3)",
            params![id, item_id, now()],
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub amount: i64,
    pub reason: String,
    pub secs_ago: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wallet {
    pub currency: String,
    pub balance: i64,
    /// Free Credits once a day are gone: always 0 and false, kept so apps
    /// from before still read the wallet.
    #[serde(default)]
    pub daily: i64,
    #[serde(default)]
    pub daily_ready: bool,
    pub owned: Vec<String>,
    /// Everything the player owns or made, to wear.
    #[serde(default)]
    pub items: Vec<crate::Item>,
    pub history: Vec<Entry>,
    pub may_spend: Option<i64>,
}

pub fn wallet(db: &Connection, player_id: &str) -> Wallet {
    let now = now();
    let mut history = Vec::new();
    if let Ok(mut s) = db.prepare(
        "SELECT amount, reason, time FROM ledger WHERE player_id = ?1 ORDER BY entry_id DESC LIMIT 30",
    ) && let Ok(rows) = s.query_map(params![player_id], |r| {
        Ok(Entry { amount: r.get(0)?, reason: r.get(1)?, secs_ago: now - r.get::<_, i64>(2)? })
    }) {
        history = rows.filter_map(Result::ok).collect();
    }
    let mut owned = Vec::new();
    if let Ok(mut s) = db.prepare("SELECT item_id FROM owned WHERE player_id = ?1 ORDER BY time DESC")
        && let Ok(rows) = s.query_map(params![player_id], |r| r.get(0))
    {
        owned = rows.filter_map(Result::ok).collect();
    }
    let limit = crate::family::settings_of(db, player_id).spend_limit;
    Wallet {
        currency: CURRENCY.into(),
        balance: balance(db, player_id),
        daily: 0,
        daily_ready: false,
        owned,
        items: crate::items_where(
            db,
            "i.creator_id = ?1 OR i.item_id IN (SELECT item_id FROM owned WHERE player_id = ?1)",
            params![player_id],
            "i.kind, i.name COLLATE NOCASE",
        ),
        history,
        may_spend: limit.map(|most| (i64::from(most) - spent(db, player_id)).max(0)),
    }
}

pub fn wallet_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel wallet\n{player_id}\n{time}").into_bytes()
}

pub fn buy_message(player_id: &str, item_id: &str, price: u32, time: i64) -> Vec<u8> {
    format!("libroxel buy\n{player_id}\n{item_id}\n{price}\n{time}").into_bytes()
}

pub fn price_message(player_id: &str, item_id: &str, price: u32, time: i64) -> Vec<u8> {
    format!("libroxel price\n{player_id}\n{item_id}\n{price}\n{time}").into_bytes()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalletSigned {
    pub time: i64,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Buy {
    pub player_id: String,
    pub item_id: String,
    pub price: u32,
    pub time: i64,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetPrice {
    pub player_id: String,
    pub price: u32,
    pub time: i64,
    pub signature: String,
}

pub(crate) type Failed = (StatusCode, String);

fn internal(e: rusqlite::Error) -> Failed {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

pub(crate) fn check(
    db: &Connection,
    player_id: &str,
    message: &[u8],
    signature: &str,
    time: i64,
) -> Result<(), Failed> {
    if !verify(player_id, message, signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    if (time - now_ms()).abs() > CLOCK_SLACK_MS {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    let has_account = db
        .query_row("SELECT 1 FROM accounts WHERE player_id = ?1", params![player_id], |_| Ok(()))
        .optional()
        .map_err(internal)?
        .is_some();
    if !has_account {
        return Err((StatusCode::FORBIDDEN, "you need an account for that".into()));
    }
    if crate::owner::is_banned(db, player_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from PlayLibre".into()));
    }
    let last: Option<i64> = db
        .query_row("SELECT time FROM wallet_clock WHERE player_id = ?1", params![player_id], |r| r.get(0))
        .optional()
        .map_err(internal)?;
    if last.is_some_and(|last| time <= last) {
        return Err((StatusCode::CONFLICT, "that was already sent".into()));
    }
    db.execute(
        "INSERT INTO wallet_clock (player_id, time) VALUES (?1, ?2)
         ON CONFLICT(player_id) DO UPDATE SET time = ?2",
        params![player_id, time],
    )
    .map_err(internal)?;
    Ok(())
}

pub(crate) async fn get_wallet(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Query(signed): Query<WalletSigned>,
) -> Result<Json<Wallet>, Failed> {
    if !verify(&id, &wallet_message(&id, signed.time), &signed.signature) {
        return Err((StatusCode::FORBIDDEN, "only you can see this".into()));
    }
    if (signed.time - now_ms()).abs() > CLOCK_SLACK_MS {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    Ok(Json(wallet(&db.lock().unwrap(), &id)))
}

/// Apps from before still offer free Credits once a day; the Hub no longer
/// gives them.
pub(crate) async fn no_daily() -> Failed {
    (StatusCode::GONE, "PlayLibre no longer gives free Credits".into())
}

pub(crate) async fn buy_item(State(db): State<Db>, Json(b): Json<Buy>) -> Result<Json<Wallet>, Failed> {
    let db = db.lock().unwrap();
    check(&db, &b.player_id, &buy_message(&b.player_id, &b.item_id, b.price, b.time), &b.signature, b.time)?;
    let item: Option<(String, u32)> = db
        .query_row(
            &format!(
                "SELECT creator_id, price FROM items i WHERE item_id = ?1 AND NOT {} AND NOT {}",
                super::hidden_sql("item", "i.item_id"),
                crate::owner::banned_sql("i.creator_id")
            ),
            params![b.item_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(internal)?;
    let Some((seller_id, price)) = item else {
        return Err((StatusCode::NOT_FOUND, "that isn't in the shop".into()));
    };
    if price != b.price {
        return Err((StatusCode::CONFLICT, "the price changed; look again".into()));
    }
    if has(&db, &b.player_id, &b.item_id) {
        return Err((StatusCode::CONFLICT, "you already have it".into()));
    }
    let sale = Sale {
        buyer_id: b.player_id.clone(),
        seller_id,
        game_id: None,
        item_id: b.item_id.clone(),
        kind: "item".into(),
        amount: price,
    };
    if crate::limited::is_limited(&db, &b.item_id) {
        crate::limited::buy_first(&db, &sale)?;
        return Ok(Json(wallet(&db, &b.player_id)));
    }
    if price > 0 {
        buy(&db, &sale)?;
    }
    db.execute(
        "INSERT OR IGNORE INTO owned (player_id, item_id, time) VALUES (?1, ?2, ?3)",
        params![b.player_id, b.item_id, now()],
    )
    .map_err(internal)?;
    Ok(Json(wallet(&db, &b.player_id)))
}

pub(crate) async fn set_price(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Json(p): Json<SetPrice>,
) -> Result<StatusCode, Failed> {
    let db = db.lock().unwrap();
    check(&db, &p.player_id, &price_message(&p.player_id, &id, p.price, p.time), &p.signature, p.time)?;
    if p.price > MAX_PRICE {
        return Err((StatusCode::BAD_REQUEST, format!("the most an item can cost is {MAX_PRICE}")));
    }
    if p.price == 0 && crate::limited::is_limited(&db, &id) {
        return Err((StatusCode::BAD_REQUEST, "a limited item can't be free".into()));
    }
    let changed = db
        .execute(
            "UPDATE items SET price = ?1 WHERE item_id = ?2 AND creator_id = ?3",
            params![p.price, id, p.player_id],
        )
        .map_err(internal)?;
    if changed == 0 {
        return Err((StatusCode::FORBIDDEN, "only its creator can price it".into()));
    }
    keep_for_wearers(&db, &id).map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_count_once_and_debits_need_the_credits() {
        let db = crate::open_db(None).unwrap();
        assert!(credit(&db, "a", 100, "badge", "first-win").unwrap());
        assert!(!credit(&db, "a", 100, "badge", "first-win").unwrap());
        assert!(top_up(&db, "a", 40, "pay_1").unwrap());
        assert!(!top_up(&db, "a", 40, "pay_1").unwrap());
        assert_eq!(balance(&db, "a"), 140);
        assert!(matches!(debit(&db, "a", 141, "x", ""), Err(Refused::Short)));
        assert!(debit(&db, "a", 40, "x", "").unwrap());
        let sale = Sale {
            buyer_id: "a".into(),
            seller_id: "b".into(),
            game_id: Some("g".into()),
            item_id: "pass".into(),
            kind: "pass".into(),
            amount: 60,
        };
        buy(&db, &sale).unwrap();
        assert_eq!((balance(&db, "a"), spent(&db, "a")), (40, 60));
        assert!(matches!(buy(&db, &sale), Err(Refused::Short)));
    }
}

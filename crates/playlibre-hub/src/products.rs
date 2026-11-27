//! Developer products: things a game sells over and over, such as coins or
//! a revive. Each purchase leaves a receipt that the game's server hands to
//! `MarketplaceService.ProcessReceipt` until a script says it granted it.

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::earnings::PassAction;
use crate::wallet::{self, CURRENCY, Failed, Sale};
use crate::{Db, now, verify};

pub const MAX_PRODUCTS: usize = 100;
const MAX_NAME: usize = 50;
const MAX_ABOUT: usize = 300;

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS products (
            product_id  INTEGER PRIMARY KEY,
            game_id     TEXT NOT NULL,
            name        TEXT NOT NULL,
            about       TEXT NOT NULL,
            price       INTEGER NOT NULL,
            created     INTEGER NOT NULL,
            removed     INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS products_game ON products(game_id);
        CREATE TABLE IF NOT EXISTS receipts (
            receipt_id  INTEGER PRIMARY KEY,
            product_id  INTEGER NOT NULL,
            game_id     TEXT NOT NULL,
            player_id   TEXT NOT NULL,
            price       INTEGER NOT NULL,
            time        INTEGER NOT NULL,
            granted     INTEGER
        );
        CREATE INDEX IF NOT EXISTS receipts_waiting ON receipts(game_id, player_id) WHERE granted IS NULL;",
    )
    .map_err(|e| format!("could not set up products: {e}"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Product {
    pub product_id: i64,
    pub game_id: String,
    pub name: String,
    pub about: String,
    pub price: u32,
    #[serde(default)]
    pub sold: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub receipt_id: i64,
    pub product_id: i64,
    pub game_id: String,
    pub player_id: String,
    pub price: u32,
    pub time: i64,
    #[serde(default)]
    pub granted: bool,
}

pub fn products(db: &Connection, game_id: &str) -> rusqlite::Result<Vec<Product>> {
    let mut s = db.prepare(
        "SELECT p.product_id, p.game_id, p.name, p.about, p.price,
                (SELECT COUNT(*) FROM receipts r WHERE r.product_id = p.product_id)
         FROM products p WHERE p.game_id = ?1 AND p.removed = 0 ORDER BY p.price, p.product_id",
    )?;
    s.query_map(params![game_id], |r| {
        Ok(Product {
            product_id: r.get(0)?,
            game_id: r.get(1)?,
            name: r.get(2)?,
            about: r.get(3)?,
            price: r.get(4)?,
            sold: r.get(5)?,
        })
    })?
    .collect()
}

fn receipt_row(r: &rusqlite::Row) -> rusqlite::Result<Receipt> {
    Ok(Receipt {
        receipt_id: r.get(0)?,
        product_id: r.get(1)?,
        game_id: r.get(2)?,
        player_id: r.get(3)?,
        price: r.get(4)?,
        time: r.get(5)?,
        granted: r.get::<_, Option<i64>>(6)?.is_some(),
    })
}

const RECEIPT_COLUMNS: &str = "receipt_id, product_id, game_id, player_id, price, time, granted";

/// The receipts of `player_id` in `game_id` that no server has granted yet.
pub fn waiting(db: &Connection, game_id: &str, player_id: &str) -> rusqlite::Result<Vec<Receipt>> {
    let mut s = db.prepare(&format!(
        "SELECT {RECEIPT_COLUMNS} FROM receipts
         WHERE game_id = ?1 AND player_id = ?2 AND granted IS NULL ORDER BY receipt_id LIMIT 100"
    ))?;
    s.query_map(params![game_id, player_id], receipt_row)?.collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductChange {
    pub player_id: String,
    pub action: PassAction,
    #[serde(default)]
    pub product_id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub price: u32,
    pub time: i64,
    pub signature: String,
}

pub fn product_message(game_id: &str, c: &ProductChange) -> Vec<u8> {
    format!(
        "playlibre product\n{}\n{game_id}\n{}\n{}\n{}\n{}\n{}\n{}",
        c.player_id,
        c.action.word(),
        c.product_id,
        c.name,
        c.about,
        c.price,
        c.time
    )
    .into_bytes()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuyProduct {
    pub player_id: String,
    pub product_id: i64,
    pub price: u32,
    pub time: i64,
    pub signature: String,
}

pub fn buy_product_message(player_id: &str, product_id: i64, price: u32, time: i64) -> Vec<u8> {
    format!("playlibre buy product\n{player_id}\n{product_id}\n{price}\n{time}").into_bytes()
}

/// A server saying a script granted a receipt. Saying it twice changes
/// nothing, so it needs no time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Granted {
    pub server_id: String,
    pub signature: String,
}

pub fn granted_message(server_id: &str, receipt_id: i64) -> Vec<u8> {
    format!("playlibre granted\n{server_id}\n{receipt_id}").into_bytes()
}

fn internal(e: rusqlite::Error) -> Failed {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn bad(why: &str) -> Failed {
    (StatusCode::BAD_REQUEST, why.to_owned())
}

pub fn buy(db: &Connection, b: &BuyProduct) -> Result<Receipt, Failed> {
    let found: Option<(String, u32, Option<String>)> = db
        .query_row(
            &format!(
                "SELECT p.game_id, p.price, g.author_id FROM products p JOIN games g ON g.game_id = p.game_id
                 WHERE p.product_id = ?1 AND p.removed = 0 AND NOT {} AND NOT {}",
                crate::hidden_sql("game", "g.game_id"),
                crate::owner::banned_sql("g.author_id")
            ),
            params![b.product_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(internal)?;
    let Some((game_id, price, Some(seller_id))) = found else {
        return Err((StatusCode::NOT_FOUND, "that isn't for sale".into()));
    };
    if price != b.price {
        return Err((StatusCode::CONFLICT, "the price changed; look again".into()));
    }
    let sale = Sale {
        buyer_id: b.player_id.clone(),
        seller_id,
        game_id: Some(game_id.clone()),
        item_id: format!("product-{}", b.product_id),
        kind: "product".into(),
        amount: price,
    };
    let receipt_id = wallet::buy(db, &sale)?;
    let time = now();
    db.execute(
        "INSERT INTO receipts (receipt_id, product_id, game_id, player_id, price, time) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![receipt_id, b.product_id, game_id, b.player_id, price, time],
    )
    .map_err(internal)?;
    Ok(Receipt {
        receipt_id,
        product_id: b.product_id,
        game_id,
        player_id: b.player_id.clone(),
        price,
        time,
        granted: false,
    })
}

/// Marks a receipt granted, when `server_id` may: a server trusted with the
/// game, or the buyer's own app.
pub fn grant(db: &Connection, receipt_id: i64, server_id: &str) -> Result<(), Failed> {
    let found: Option<(String, String)> = db
        .query_row(
            "SELECT game_id, player_id FROM receipts WHERE receipt_id = ?1",
            params![receipt_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(internal)?;
    let Some((game_id, buyer)) = found else {
        return Err((StatusCode::NOT_FOUND, "no such receipt".into()));
    };
    if buyer != server_id && !crate::data::trusted(db, server_id, &game_id) {
        return Err((StatusCode::FORBIDDEN, "only the game's own servers grant its receipts".into()));
    }
    db.execute(
        "UPDATE receipts SET granted = ?2 WHERE receipt_id = ?1 AND granted IS NULL",
        params![receipt_id, now()],
    )
    .map_err(internal)?;
    Ok(())
}

pub(crate) async fn list_products(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<Vec<Product>>, Failed> {
    products(&db.lock().unwrap(), &id).map(Json).map_err(internal)
}

pub(crate) async fn change_product(
    State(db): State<Db>,
    UrlPath(game_id): UrlPath<String>,
    Json(c): Json<ProductChange>,
) -> Result<Json<Vec<Product>>, Failed> {
    let db = db.lock().unwrap();
    wallet::check(&db, &c.player_id, &product_message(&game_id, &c), &c.signature, c.time)?;
    let author: Option<Option<String>> = db
        .query_row("SELECT author_id FROM games WHERE game_id = ?1", params![game_id], |r| r.get(0))
        .optional()
        .map_err(internal)?;
    if author.flatten().as_deref() != Some(c.player_id.as_str()) {
        return Err((StatusCode::FORBIDDEN, "only the game's creator can sell things in it".into()));
    }
    if c.action != PassAction::Remove {
        let name = c.name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME {
            return Err(bad("a product needs a short name"));
        }
        if c.about.chars().count() > MAX_ABOUT {
            return Err(bad("that description is too long"));
        }
        if crate::filter::clean(name).as_deref() != Some(name)
            || crate::filter::clean(&c.about).as_deref() != Some(c.about.as_str())
        {
            return Err(bad("pick friendlier words"));
        }
        if c.price == 0 || c.price > wallet::MAX_PRICE {
            return Err(bad(&format!("a product costs 1 to {} {CURRENCY}", wallet::MAX_PRICE)));
        }
    }
    let changed = match c.action {
        PassAction::Add => {
            if products(&db, &game_id).map_err(internal)?.len() >= MAX_PRODUCTS {
                return Err(bad("that game has as many products as it can"));
            }
            db.execute(
                "INSERT INTO products (game_id, name, about, price, created) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![game_id, c.name.trim(), c.about.trim(), c.price, now()],
            )
        }
        PassAction::Edit => db.execute(
            "UPDATE products SET name = ?3, about = ?4, price = ?5 WHERE product_id = ?1 AND game_id = ?2 AND removed = 0",
            params![c.product_id, game_id, c.name.trim(), c.about.trim(), c.price],
        ),
        PassAction::Remove => db.execute(
            "UPDATE products SET removed = 1 WHERE product_id = ?1 AND game_id = ?2",
            params![c.product_id, game_id],
        ),
    }
    .map_err(internal)?;
    if changed == 0 {
        return Err((StatusCode::NOT_FOUND, "that product is gone".into()));
    }
    products(&db, &game_id).map(Json).map_err(internal)
}

pub(crate) async fn buy_product(
    State(db): State<Db>,
    Json(b): Json<BuyProduct>,
) -> Result<Json<Receipt>, Failed> {
    let db = db.lock().unwrap();
    wallet::check(
        &db,
        &b.player_id,
        &buy_product_message(&b.player_id, b.product_id, b.price, b.time),
        &b.signature,
        b.time,
    )?;
    buy(&db, &b).map(Json)
}

pub(crate) async fn list_waiting(
    State(db): State<Db>,
    UrlPath((game_id, player)): UrlPath<(String, String)>,
) -> Result<Json<Vec<Receipt>>, Failed> {
    waiting(&db.lock().unwrap(), &game_id, &player).map(Json).map_err(internal)
}

pub(crate) async fn mark_granted(
    State(db): State<Db>,
    UrlPath(receipt_id): UrlPath<i64>,
    Json(g): Json<Granted>,
) -> Result<Json<()>, Failed> {
    if !verify(&g.server_id, &granted_message(&g.server_id, receipt_id), &g.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match".into()));
    }
    grant(&db.lock().unwrap(), receipt_id, &g.server_id).map(Json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipts_wait_until_a_trusted_server_grants_them() {
        let db = crate::open_db(None).unwrap();
        db.execute(
            "INSERT INTO games (game_id, secret, title, author, description, place, version, plays, created, updated, author_id)
             VALUES ('g', '', 'g', '', '', '', 1, 0, 0, 0, 'maker')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO products (game_id, name, about, price, created) VALUES ('g', 'Coins', '', 10, 0)",
            [],
        )
        .unwrap();
        wallet::top_up(&db, "fan", 25, "pay_1").unwrap();
        let b = |price| BuyProduct {
            player_id: "fan".into(),
            product_id: 1,
            price,
            time: 0,
            signature: String::new(),
        };
        assert_eq!(buy(&db, &b(9)).unwrap_err().0, StatusCode::CONFLICT);
        let one = buy(&db, &b(10)).unwrap();
        let two = buy(&db, &b(10)).unwrap();
        assert_eq!(buy(&db, &b(10)).unwrap_err().0, StatusCode::PAYMENT_REQUIRED);
        assert_eq!(wallet::balance(&db, "fan"), 5);
        assert_eq!(waiting(&db, "g", "fan").unwrap().len(), 2);
        assert_eq!(grant(&db, one.receipt_id, "stranger").unwrap_err().0, StatusCode::FORBIDDEN);
        grant(&db, one.receipt_id, "maker").unwrap();
        grant(&db, one.receipt_id, "maker").unwrap();
        grant(&db, two.receipt_id, "fan").unwrap();
        assert!(waiting(&db, "g", "fan").unwrap().is_empty());
        assert_eq!(products(&db, "g").unwrap()[0].sold, 2);
        crate::earnings::settle(&db, now() + crate::earnings::HOLD_DAYS * 86_400 + 1).unwrap();
        assert_eq!(wallet::balance(&db, "maker"), 14);
    }
}

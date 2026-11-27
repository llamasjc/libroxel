use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use hmac::{Hmac, Mac};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::wallet::{self, Failed};
use crate::{Db, now};

const SIGNATURE_AGE_SECS: i64 = 5 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pack {
    pub id: u32,
    pub credits: u32,
    pub cents: u32,
}

pub const PACKS: [Pack; 5] = [
    Pack { id: 1, credits: 400, cents: 499 },
    Pack { id: 2, credits: 800, cents: 999 },
    Pack { id: 3, credits: 1700, cents: 1999 },
    Pack { id: 4, credits: 4500, cents: 4999 },
    Pack { id: 5, credits: 10000, cents: 9999 },
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Packs {
    pub on: bool,
    pub currency: String,
    pub packs: Vec<Pack>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkout {
    pub player_id: String,
    pub pack: u32,
    pub time: i64,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckoutUrl {
    pub url: String,
}

pub fn checkout_message(player_id: &str, pack: u32, time: i64) -> Vec<u8> {
    format!("libroxel checkout\n{player_id}\n{pack}\n{time}").into_bytes()
}

#[derive(Debug, Clone)]
pub struct Stripe {
    pub key: String,
    pub webhook_secret: String,
    pub site: String,
    pub api: String,
}

impl Stripe {
    pub fn from_env() -> Option<Stripe> {
        let get = |name: &str| crate::setting(name).map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        Some(Stripe {
            key: get("PLAYLIBRE_STRIPE_KEY")?,
            webhook_secret: get("PLAYLIBRE_STRIPE_WEBHOOK_SECRET")?,
            site: get("PLAYLIBRE_SITE").unwrap_or_else(|| "https://playlibre.net".into()),
            api: get("PLAYLIBRE_STRIPE_API").unwrap_or_else(|| "https://api.stripe.com".into()),
        })
    }

    fn new_session(&self, player_id: &str, pack: Pack) -> Result<(String, String), String> {
        let agent: ureq::Agent =
            ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).build().into();
        let back = format!("{}/credits", self.site.trim_end_matches('/'));
        let name = format!("{} {}", pack.credits, wallet::CURRENCY);
        let form = [
            ("mode", "payment".to_owned()),
            ("success_url", format!("{back}?paid=1")),
            ("cancel_url", back),
            ("client_reference_id", player_id.to_owned()),
            ("line_items[0][quantity]", "1".into()),
            ("line_items[0][price_data][currency]", "usd".into()),
            ("line_items[0][price_data][unit_amount]", pack.cents.to_string()),
            ("line_items[0][price_data][product_data][name]", name),
            ("metadata[player_id]", player_id.to_owned()),
            ("metadata[pack]", pack.id.to_string()),
        ];
        let answer: serde_json::Value = agent
            .post(format!("{}/v1/checkout/sessions", self.api))
            .header("Authorization", format!("Bearer {}", self.key))
            .send_form(form.iter().map(|(k, v)| (*k, v.as_str())))
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not start checkout: {e}"))?;
        match (answer["id"].as_str(), answer["url"].as_str()) {
            (Some(id), Some(url)) => Ok((id.to_owned(), url.to_owned())),
            _ => Err("could not start checkout".into()),
        }
    }
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS checkouts (
            session_id  TEXT PRIMARY KEY,
            player_id   TEXT NOT NULL,
            credits     INTEGER NOT NULL,
            cents       INTEGER NOT NULL,
            payment_id  TEXT,
            state       TEXT NOT NULL,
            time        INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS checkouts_payment ON checkouts(payment_id);",
    )
    .map_err(|e| format!("could not set up checkouts: {e}"))
}

pub(crate) async fn packs(State(stripe): State<Option<Arc<Stripe>>>) -> Json<Packs> {
    Json(Packs { on: stripe.is_some(), currency: wallet::CURRENCY.into(), packs: PACKS.to_vec() })
}

pub(crate) async fn checkout(
    State(db): State<Db>,
    State(stripe): State<Option<Arc<Stripe>>>,
    Json(c): Json<Checkout>,
) -> Result<Json<CheckoutUrl>, Failed> {
    let Some(stripe) = stripe else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "buying Credits is not on for this Hub".into()));
    };
    let Some(pack) = PACKS.iter().copied().find(|p| p.id == c.pack) else {
        return Err((StatusCode::NOT_FOUND, "no such pack".into()));
    };
    {
        let db = db.lock().unwrap();
        wallet::check(
            &db,
            &c.player_id,
            &checkout_message(&c.player_id, c.pack, c.time),
            &c.signature,
            c.time,
        )?;
        if crate::family::settings_of(&db, &c.player_id).spend_limit == Some(0) {
            return Err(wallet::Refused::Parent.into());
        }
    }
    let player_id = c.player_id.clone();
    let (session_id, url) = tokio::task::spawn_blocking(move || stripe.new_session(&player_id, pack))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
    db.lock()
        .unwrap()
        .execute(
            "INSERT OR IGNORE INTO checkouts (session_id, player_id, credits, cents, state, time)
             VALUES (?1, ?2, ?3, ?4, 'open', ?5)",
            params![session_id, c.player_id, pack.credits, pack.cents, now()],
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(CheckoutUrl { url }))
}

pub fn signed_by_stripe(secret: &str, header: &str, body: &[u8], now: i64) -> bool {
    let mut time = None;
    let mut signatures = Vec::new();
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", t)) => time = t.parse::<i64>().ok(),
            Some(("v1", v)) => signatures.push(v),
            _ => {}
        }
    }
    let Some(time) = time else { return false };
    if (now - time).abs() > SIGNATURE_AGE_SECS {
        return false;
    }
    signatures.iter().any(|v| {
        let Ok(want) = data_encoding::HEXLOWER_PERMISSIVE.decode(v.as_bytes()) else { return false };
        let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else { return false };
        mac.update(time.to_string().as_bytes());
        mac.update(b".");
        mac.update(body);
        mac.verify_slice(&want).is_ok()
    })
}

pub fn paid(db: &Connection, session_id: &str, payment_id: &str) -> rusqlite::Result<bool> {
    let tx = db.unchecked_transaction()?;
    let found: Option<(String, u32)> = tx
        .query_row(
            "SELECT player_id, credits FROM checkouts WHERE session_id = ?1",
            params![session_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((player_id, credits)) = found else { return Ok(false) };
    tx.execute(
        "UPDATE checkouts SET state = 'paid', payment_id = ?2 WHERE session_id = ?1 AND state = 'open'",
        params![session_id, payment_id],
    )?;
    let added = wallet::entry(&tx, &player_id, credits.into(), "bought", session_id)?;
    tx.commit()?;
    Ok(added)
}

pub fn refunded(db: &Connection, payment_id: &str) -> rusqlite::Result<bool> {
    let tx = db.unchecked_transaction()?;
    let found: Option<(String, String, i64)> = tx
        .query_row(
            "SELECT session_id, player_id, credits FROM checkouts WHERE payment_id = ?1 AND state = 'paid'",
            params![payment_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((session_id, player_id, credits)) = found else { return Ok(false) };
    let take = credits.min(wallet::balance(&tx, &player_id).max(0));
    if take > 0 {
        wallet::entry(&tx, &player_id, -take, "refunded", &session_id)?;
    }
    tx.execute("UPDATE checkouts SET state = 'refunded' WHERE session_id = ?1", params![session_id])?;
    tx.commit()?;
    Ok(true)
}

pub(crate) async fn stripe_event(
    State(db): State<Db>,
    State(stripe): State<Option<Arc<Stripe>>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<StatusCode, Failed> {
    let Some(stripe) = stripe else { return Err((StatusCode::NOT_FOUND, String::new())) };
    let header = headers.get("stripe-signature").and_then(|h| h.to_str().ok()).unwrap_or_default();
    if !signed_by_stripe(&stripe.webhook_secret, header, &body, now()) {
        return Err((StatusCode::BAD_REQUEST, "bad signature".into()));
    }
    let event: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let object = &event["data"]["object"];
    let text = |key: &str| object[key].as_str().unwrap_or_default().to_owned();
    let db = db.lock().unwrap();
    let done = match event["type"].as_str().unwrap_or_default() {
        "checkout.session.completed" | "checkout.session.async_payment_succeeded"
            if object["payment_status"] == "paid" =>
        {
            paid(&db, &text("id"), &text("payment_intent")).map(|_| ())
        }
        "charge.refunded" | "charge.dispute.created" => refunded(&db, &text("payment_intent")).map(|_| ()),
        _ => Ok(()),
    };
    done.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &str, time: i64, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("{time}.").as_bytes());
        mac.update(body);
        format!("t={time},v1={}", data_encoding::HEXLOWER.encode(&mac.finalize().into_bytes()))
    }

    #[test]
    fn only_stripe_signatures_count() {
        let body = br#"{"id":"evt_1"}"#;
        let header = sign("whsec_x", 1000, body);
        assert!(signed_by_stripe("whsec_x", &header, body, 1100));
        assert!(!signed_by_stripe("whsec_y", &header, body, 1100));
        assert!(!signed_by_stripe("whsec_x", &header, b"{}", 1100));
        assert!(!signed_by_stripe("whsec_x", &header, body, 1000 + SIGNATURE_AGE_SECS + 1));
    }

    #[test]
    fn a_payment_counts_once_and_a_refund_takes_it_back() {
        let db = crate::open_db(None).unwrap();
        db.execute(
            "INSERT INTO checkouts (session_id, player_id, credits, cents, state, time) VALUES ('cs_1', 'a', 400, 499, 'open', 0)",
            [],
        )
        .unwrap();
        assert!(paid(&db, "cs_1", "pi_1").unwrap());
        assert!(!paid(&db, "cs_1", "pi_1").unwrap());
        assert!(!paid(&db, "cs_unknown", "pi_2").unwrap());
        assert_eq!(wallet::balance(&db, "a"), 400);
        wallet::debit(&db, "a", 100, "buy", "1").unwrap();
        assert!(refunded(&db, "pi_1").unwrap());
        assert!(!refunded(&db, "pi_1").unwrap());
        assert_eq!(wallet::balance(&db, "a"), 0);
    }
}

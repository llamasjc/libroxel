use std::collections::HashSet;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    Db, MAX_DESCRIPTION, clip, clip_to, copies, filter, hidden_sql, moderate, new_game_id, now, owner, verify,
};
use crate::limits::{Caller, Limit, Limits, too_many};

pub const MAX_MODEL: usize = 4 * 1024 * 1024;
pub const MAX_PLUGIN: usize = 256 * 1024;

pub const PERMISSIONS: [&str; 3] = ["add", "change", "scripts"];

type Failure = (StatusCode, String);

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS market (
            asset_id    TEXT PRIMARY KEY,
            creator_id  TEXT NOT NULL,
            kind        TEXT NOT NULL,
            name        TEXT NOT NULL,
            about       TEXT NOT NULL,
            data        TEXT NOT NULL,
            permissions TEXT NOT NULL,
            price       INTEGER NOT NULL,
            version     INTEGER NOT NULL,
            installs    INTEGER NOT NULL,
            signed_at   INTEGER NOT NULL,
            created     INTEGER NOT NULL,
            updated     INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS market_creator ON market(creator_id);",
    )
    .map_err(|e| format!("could not set up the marketplace: {e}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    Model,
    Plugin,
}

impl AssetKind {
    pub fn word(self) -> &'static str {
        match self {
            AssetKind::Model => "model",
            AssetKind::Plugin => "plugin",
        }
    }

    fn of(word: &str) -> Option<AssetKind> {
        match word {
            "model" => Some(AssetKind::Model),
            "plugin" => Some(AssetKind::Plugin),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewAsset {
    #[serde(default)]
    pub asset_id: Option<String>,
    pub creator_id: String,
    pub kind: AssetKind,
    pub name: String,
    #[serde(default)]
    pub about: String,
    pub data: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub price: u32,
    pub time: i64,
    pub signature: String,
}

#[allow(clippy::too_many_arguments)]
pub fn asset_message(
    creator_id: &str,
    asset_id: Option<&str>,
    kind: AssetKind,
    name: &str,
    about: &str,
    data: &str,
    permissions: &[String],
    price: u32,
    time: i64,
) -> Vec<u8> {
    let hash = data_encoding::HEXLOWER.encode(&Sha256::digest(data.as_bytes()));
    format!(
        "libroxel market\n{creator_id}\n{}\n{}\n{name}\n{about}\n{hash}\n{}\n{price}\n{time}",
        asset_id.unwrap_or_default(),
        kind.word(),
        permissions.join(",")
    )
    .into_bytes()
}

pub fn unmarket_message(asset_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel unmarket\n{asset_id}\n{time}").into_bytes()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unmarket {
    pub time: i64,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub asset_id: String,
    pub kind: AssetKind,
    pub name: String,
    pub about: String,
    pub creator: String,
    pub creator_id: String,
    pub permissions: Vec<String>,
    pub price: u32,
    pub version: u32,
    pub installs: u32,
    pub size: u64,
    pub updated_secs_ago: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetData {
    pub asset_id: String,
    pub kind: AssetKind,
    pub version: u32,
    pub permissions: Vec<String>,
    pub data: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct MarketQuery {
    pub q: Option<String>,
    pub kind: Option<String>,
    pub sort: Option<String>,
    pub creator: Option<String>,
}

const COLUMNS: &str = "m.asset_id, m.kind, m.name, m.about, COALESCE(a.name, 'Someone'), m.creator_id,
    m.permissions, m.price, m.version, m.installs, LENGTH(m.data), m.updated";

fn row(r: &rusqlite::Row, now: i64) -> rusqlite::Result<Asset> {
    let kind: String = r.get(1)?;
    let permissions: String = r.get(6)?;
    Ok(Asset {
        asset_id: r.get(0)?,
        kind: AssetKind::of(&kind).unwrap_or(AssetKind::Model),
        name: r.get(2)?,
        about: r.get(3)?,
        creator: r.get(4)?,
        creator_id: r.get(5)?,
        permissions: split(&permissions),
        price: r.get::<_, i64>(7)? as u32,
        version: r.get::<_, i64>(8)? as u32,
        installs: r.get::<_, i64>(9)? as u32,
        size: r.get::<_, i64>(10)? as u64,
        updated_secs_ago: now - r.get::<_, i64>(11)?,
    })
}

fn split(permissions: &str) -> Vec<String> {
    permissions.split(',').filter(|p| !p.is_empty()).map(str::to_owned).collect()
}

fn listed() -> String {
    format!("NOT {} AND NOT {}", hidden_sql("market", "m.asset_id"), owner::banned_sql("m.creator_id"))
}

pub(crate) async fn list(State(db): State<Db>, Query(query): Query<MarketQuery>) -> Json<Vec<Asset>> {
    let db = db.lock().unwrap();
    let pattern = format!("%{}%", query.q.unwrap_or_default().trim().replace(['%', '_'], ""));
    let kind = query.kind.as_deref().and_then(AssetKind::of).map(AssetKind::word);
    let order = match query.sort.as_deref() {
        Some("new") => "m.created DESC",
        Some("updated") => "m.updated DESC",
        _ => "m.installs DESC, m.updated DESC",
    };
    let sql = format!(
        "SELECT {COLUMNS} FROM market m LEFT JOIN accounts a ON a.player_id = m.creator_id
         WHERE (m.name LIKE ?1 OR m.about LIKE ?1 OR COALESCE(a.name, '') LIKE ?1)
           AND (?2 IS NULL OR m.kind = ?2) AND (?3 IS NULL OR m.creator_id = ?3) AND {}
         ORDER BY {order} LIMIT 200",
        listed()
    );
    let now = now();
    let rows = db
        .prepare(&sql)
        .and_then(|mut s| {
            s.query_map(params![pattern, kind, query.creator], |r| row(r, now))
                .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default();
    Json(rows)
}

fn find(db: &Connection, id: &str) -> Option<Asset> {
    let sql = format!(
        "SELECT {COLUMNS} FROM market m LEFT JOIN accounts a ON a.player_id = m.creator_id
         WHERE m.asset_id = ?1 AND {}",
        listed()
    );
    let now = now();
    db.query_row(&sql, params![id], |r| row(r, now)).optional().ok().flatten()
}

pub(crate) async fn info(State(db): State<Db>, UrlPath(id): UrlPath<String>) -> Result<Json<Asset>, Failure> {
    let db = db.lock().unwrap();
    find(&db, &id)
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "there is nothing in the marketplace with that id".into()))
}

pub(crate) async fn data(
    State(db): State<Db>,
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<AssetData>, Failure> {
    if !limits.take(Limit::Download, &caller.0) {
        return Err(too_many("marketplace downloads"));
    }
    let db = db.lock().unwrap();
    let Some(asset) = find(&db, &id) else {
        return Err((StatusCode::NOT_FOUND, "there is nothing in the marketplace with that id".into()));
    };
    if asset.price > 0 {
        return Err((StatusCode::PAYMENT_REQUIRED, "paid items aren't open yet".into()));
    }
    let data: String = db
        .query_row("SELECT data FROM market WHERE asset_id = ?1", params![id], |r| r.get(0))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let _ = db.execute("UPDATE market SET installs = installs + 1 WHERE asset_id = ?1", params![id]);
    Ok(Json(AssetData {
        asset_id: asset.asset_id,
        kind: asset.kind,
        version: asset.version,
        permissions: asset.permissions,
        data,
    }))
}

fn plugin_prints(source: &str) -> HashSet<String> {
    let code: String = source.chars().filter(|c| !c.is_whitespace()).collect();
    HashSet::from([format!("p{}", &data_encoding::HEXLOWER.encode(&Sha256::digest(&code))[..32])])
}

fn model_ok(data: &str) -> Result<(), &'static str> {
    let doc: serde_json::Value = serde_json::from_str(data).map_err(|_| "that is not a saved model")?;
    if doc.get("format").and_then(serde_json::Value::as_str) != Some("libroxel-place") {
        return Err("that is not a saved model");
    }
    let children = doc.get("children").and_then(serde_json::Value::as_array).ok_or("that model is empty")?;
    if children.len() != 1 {
        return Err("a model is one thing, with everything else inside it");
    }
    Ok(())
}

pub(crate) async fn publish(
    State(db): State<Db>,
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    Json(asset): Json<NewAsset>,
) -> Result<Json<Asset>, Failure> {
    if !limits.take(Limit::Share, &caller.0) {
        return Err(too_many("games and items shared"));
    }
    let bad = |why: &str| Err((StatusCode::BAD_REQUEST, why.to_owned()));
    let name = clip(&asset.name);
    if name.is_empty() {
        return bad("it needs a name");
    }
    if asset.about.chars().count() > MAX_DESCRIPTION {
        return bad("that description is too long");
    }
    if asset.price > 0 {
        return bad("paid items aren't open yet, so it has to be free for now");
    }
    let mut permissions = asset.permissions.clone();
    permissions.sort();
    permissions.dedup();
    if permissions.len() != asset.permissions.len() || permissions != asset.permissions {
        return bad("list permissions once each, in order");
    }
    if let Some(p) = permissions.iter().find(|p| !PERMISSIONS.contains(&p.as_str())) {
        return bad(&format!("there is no permission {p}"));
    }
    match asset.kind {
        AssetKind::Model => {
            if !permissions.is_empty() {
                return bad("only plugins ask for permissions");
            }
            if asset.data.len() > MAX_MODEL {
                return bad("that model is too big");
            }
            if let Err(why) = model_ok(&asset.data) {
                return bad(why);
            }
        }
        AssetKind::Plugin => {
            if asset.data.trim().is_empty() {
                return bad("that plugin has no code");
            }
            if asset.data.len() > MAX_PLUGIN {
                return bad("that plugin is too big");
            }
        }
    }
    let asset_id = asset.asset_id.as_deref().map(str::trim).filter(|id| !id.is_empty());
    let message = asset_message(
        &asset.creator_id,
        asset_id,
        asset.kind,
        &asset.name,
        &asset.about,
        &asset.data,
        &asset.permissions,
        asset.price,
        asset.time,
    );
    if !verify(&asset.creator_id, &message, &asset.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that creator".into()));
    }
    let now = now();
    if (asset.time - now).abs() > 24 * 60 * 60 {
        return bad("this computer's clock is too far off");
    }
    let (Some(name), Some(about)) =
        (filter::clean(&name), filter::clean(&clip_to(&asset.about, MAX_DESCRIPTION)))
    else {
        return bad("that name or description isn't OK here");
    };
    let prints = match asset.kind {
        AssetKind::Model => copies::prints(&asset.data),
        AssetKind::Plugin => plugin_prints(&asset.data),
    };
    let db = db.lock().unwrap();
    if owner::is_banned(&db, &asset.creator_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from Libroxel".into()));
    }
    moderate::not_muted(&db, &asset.creator_id)?;
    let internal = |e: rusqlite::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    let (id, version) = match asset_id {
        Some(id) => {
            let mine: Option<(String, String, i64, i64)> = db
                .query_row(
                    "SELECT creator_id, kind, version, signed_at FROM market WHERE asset_id = ?1",
                    params![id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()
                .map_err(internal)?;
            let Some((creator, kind, version, signed_at)) = mine else {
                return Err((
                    StatusCode::NOT_FOUND,
                    "there is nothing in the marketplace with that id".into(),
                ));
            };
            if creator != asset.creator_id {
                return Err((StatusCode::FORBIDDEN, "someone else published that".into()));
            }
            if kind != asset.kind.word() {
                return bad("a new version has to be the same kind of thing");
            }
            if asset.time <= signed_at {
                return Err((StatusCode::CONFLICT, "a newer version is already here".into()));
            }
            (id.to_owned(), version + 1)
        }
        None => (new_game_id(), 1),
    };
    let key = format!("market:{id}");
    if let Some(original) = copies::copied_from(&db, &key, &asset.creator_id, &prints) {
        eprintln!("{} tried to publish a copy of {original} to the marketplace", asset.creator_id);
        return Err((
            StatusCode::CONFLICT,
            format!("this looks copied from “{original}”, which someone else made"),
        ));
    }
    db.execute(
        "INSERT INTO market (asset_id, creator_id, kind, name, about, data, permissions, price, version, installs,
             signed_at, created, updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11, ?11)
         ON CONFLICT(asset_id) DO UPDATE SET name = ?4, about = ?5, data = ?6, permissions = ?7, price = ?8,
             version = ?9, signed_at = ?10, updated = ?11",
        params![
            id,
            asset.creator_id,
            asset.kind.word(),
            name,
            about,
            asset.data,
            permissions.join(","),
            asset.price,
            version,
            asset.time,
            now
        ],
    )
    .map_err(internal)?;
    copies::keep(&db, &key, &asset.creator_id, &prints).map_err(internal)?;
    find(&db, &id).map(Json).ok_or((StatusCode::INTERNAL_SERVER_ERROR, "it went missing".into()))
}

pub(crate) async fn remove(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Json(signed): Json<Unmarket>,
) -> Result<StatusCode, Failure> {
    let db = db.lock().unwrap();
    let creator: Option<String> = db
        .query_row("SELECT creator_id FROM market WHERE asset_id = ?1", params![id], |r| r.get(0))
        .optional()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let Some(creator) = creator else {
        return Err((StatusCode::NOT_FOUND, "there is nothing in the marketplace with that id".into()));
    };
    if (signed.time - now()).abs() > 24 * 60 * 60
        || !verify(&creator, &unmarket_message(&id, signed.time), &signed.signature)
    {
        return Err((StatusCode::FORBIDDEN, "only its creator can take it down".into()));
    }
    take_down(&db, &id).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn take_down(db: &Connection, id: &str) -> rusqlite::Result<usize> {
    let gone = db.execute("DELETE FROM market WHERE asset_id = ?1", params![id])?;
    copies::forget(db, &format!("market:{id}"));
    Ok(gone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_is_one_thing() {
        assert!(
            model_ok(r#"{"format":"libroxel-place","version":1,"children":[{"class":"model","name":"M"}]}"#)
                .is_ok()
        );
        assert!(model_ok(r#"{"format":"libroxel-place","version":1,"children":[]}"#).is_err());
        assert!(model_ok("print(1)").is_err());
    }

    #[test]
    fn plugins_match_whatever_the_spacing() {
        assert_eq!(plugin_prints("print( 1 )\n"), plugin_prints("print(1)"));
        assert_ne!(plugin_prints("print(1)"), plugin_prints("print(2)"));
    }
}

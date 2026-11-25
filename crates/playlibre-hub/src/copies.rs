use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::assets::{ASSET_PREFIX, valid_hash};

const SCRIPTS: [&str; 3] = ["script", "local_script", "module_script"];
const SHORTEST_SCRIPT: usize = 200;
const ENOUGH: usize = 5;
const COMMON: usize = 3;

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS prints (
            game_id     TEXT NOT NULL,
            owner       TEXT NOT NULL,
            print       TEXT NOT NULL,
            PRIMARY KEY (game_id, print)
        );
        CREATE INDEX IF NOT EXISTS prints_print ON prints(print);",
    )
    .map_err(|e| format!("could not set up the Hub's copy check: {e}"))?;
    fill(db).map_err(|e| format!("could not set up the Hub's copy check: {e}"))
}

fn fill(db: &Connection) -> rusqlite::Result<()> {
    let games: Vec<(String, String, String)> = db
        .prepare(
            "SELECT game_id, COALESCE(author_id, game_id), place FROM games
             WHERE game_id NOT IN (SELECT DISTINCT game_id FROM prints)",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (game_id, owner, place) in games {
        keep(db, &game_id, &owner, &prints(&place))?;
    }
    Ok(())
}

pub(crate) fn prints(place: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    if let Ok(value) = serde_json::from_str::<Value>(place) {
        walk(&value, &mut out);
    }
    out
}

fn walk(value: &Value, out: &mut HashSet<String>) {
    match value {
        Value::Object(map) => {
            if map.get("class").and_then(Value::as_str).is_some_and(|c| SCRIPTS.contains(&c))
                && let Some(source) =
                    map.get("properties").and_then(|p| p.get("source")).and_then(Value::as_str)
            {
                let code: String = source.chars().filter(|c| !c.is_whitespace()).collect();
                if code.len() >= SHORTEST_SCRIPT {
                    out.insert(format!("s{}", &data_encoding::HEXLOWER.encode(&Sha256::digest(&code))[..32]));
                }
            }
            map.values().for_each(|v| walk(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
        Value::String(text) => {
            if let Some(hash) = text.strip_prefix(ASSET_PREFIX).and_then(|rest| rest.get(..64))
                && valid_hash(hash)
            {
                out.insert(format!("a{hash}"));
            }
        }
        _ => {}
    }
}

pub(crate) fn keep(
    db: &Connection,
    game_id: &str,
    owner: &str,
    prints: &HashSet<String>,
) -> rusqlite::Result<()> {
    db.execute("DELETE FROM prints WHERE game_id = ?1", params![game_id])?;
    let mut insert =
        db.prepare("INSERT OR IGNORE INTO prints (game_id, owner, print) VALUES (?1, ?2, ?3)")?;
    for print in prints {
        insert.execute(params![game_id, owner, print])?;
    }
    Ok(())
}

pub(crate) fn forget(db: &Connection, game_id: &str) {
    let _ = db.execute("DELETE FROM prints WHERE game_id = ?1", params![game_id]);
}

pub(crate) fn copied_from(
    db: &Connection,
    game_id: &str,
    owner: &str,
    prints: &HashSet<String>,
) -> Option<String> {
    let plugin = !prints.is_empty() && prints.iter().all(|p| p.starts_with('p'));
    let enough = if plugin { 1 } else { ENOUGH };
    let market = game_id.starts_with("market:");
    if prints.len() < enough {
        return None;
    }
    let mut find = db.prepare("SELECT game_id, owner FROM prints WHERE print = ?1").ok()?;
    let mut shared: HashMap<String, usize> = HashMap::new();
    for print in prints {
        let all: Vec<(String, String)> = find
            .query_map(params![print], |r| Ok((r.get(0)?, r.get(1)?)))
            .ok()?
            .filter_map(Result::ok)
            .collect();
        if all
            .iter()
            .any(|(game, _)| game.starts_with("starter:") || (!market && game.starts_with("market:")))
        {
            continue;
        }
        let rows: Vec<(String, String)> =
            all.into_iter().filter(|(game, other)| game != game_id && other != owner).collect();
        let owners: HashSet<&str> = rows.iter().map(|(_, other)| other.as_str()).collect();
        if owners.len() >= COMMON {
            continue;
        }
        for (game, _) in rows {
            *shared.entry(game).or_default() += 1;
        }
    }
    let (game, count) = shared.into_iter().max_by_key(|(_, count)| *count)?;
    if count < enough || count * 2 < prints.len() {
        return None;
    }
    match game.strip_prefix("market:") {
        Some(id) => db.query_row("SELECT name FROM market WHERE asset_id = ?1", params![id], |r| r.get(0)),
        None => db.query_row("SELECT title FROM games WHERE game_id = ?1", params![game], |r| r.get(0)),
    }
    .optional()
    .ok()?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(scripts: &[String], assets: &[&str]) -> String {
        let mut children: Vec<Value> = scripts
            .iter()
            .map(|s| serde_json::json!({ "class": "script", "name": "S", "properties": { "source": s } }))
            .collect();
        for asset in assets {
            children.push(serde_json::json!({
                "class": "decal", "name": "D", "properties": { "texture": format!("{ASSET_PREFIX}{asset}.png") }
            }));
        }
        serde_json::json!({ "format": "libroxel-place", "version": 1, "children": children }).to_string()
    }

    fn script(n: usize) -> String {
        format!("local value{n} = {n}\n{}", "print(value) ".repeat(30))
    }

    #[test]
    fn finds_scripts_and_assets() {
        let hash = "a".repeat(64);
        let found = prints(&place(&[script(1), "print(1)".into()], &[&hash]));
        assert_eq!(found.len(), 2);
        assert!(found.contains(&format!("a{hash}")));
        let spaced = prints(&place(&[script(1).replace(' ', "\n  ")], &[]));
        assert!(spaced.is_subset(&found));
    }

    #[test]
    fn a_copy_by_someone_else_is_caught() {
        let db = crate::open_db(None).unwrap();
        let scripts: Vec<String> = (0..6).map(script).collect();
        db.execute(
            "INSERT INTO games (game_id, secret, title, author, description, place, version, plays, created, updated)
             VALUES ('orig', 's', 'Original', 'a', '', '', 1, 0, 0, 0)",
            [],
        )
        .unwrap();
        keep(&db, "orig", "alice", &prints(&place(&scripts, &[]))).unwrap();
        let copy = prints(&place(&scripts, &[]));
        assert_eq!(copied_from(&db, "new", "bob", &copy).as_deref(), Some("Original"));
        assert_eq!(copied_from(&db, "new", "alice", &copy), None);
        assert_eq!(copied_from(&db, "orig", "bob", &copy), None);
        let own: Vec<String> = (10..30).map(script).chain(scripts[..2].iter().cloned()).collect();
        assert_eq!(copied_from(&db, "new", "bob", &prints(&place(&own, &[]))), None);
        for other in ["c", "d", "e"] {
            keep(&db, other, other, &prints(&place(&scripts, &[]))).unwrap();
        }
        assert_eq!(copied_from(&db, "new", "bob", &copy), None);
    }
}

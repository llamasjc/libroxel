//! What whoever runs the Hub can do: name its **owners** (the only accounts
//! that may list Official servers) and **ban** accounts.
//!
//! These are run on the Hub's own computer (`libroxel-hub owners`, `ban`
//! and so on), straight on its database, so no one can do them over the
//! network.

use rusqlite::{Connection, OptionalExtension, params};

use super::now;

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS owners (
            player_id   TEXT PRIMARY KEY,
            since       INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS banned (
            player_id   TEXT PRIMARY KEY,
            reason      TEXT NOT NULL,
            since       INTEGER NOT NULL
        );",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))
}

/// SQL that holds when the player id in `column` is banned.
pub(crate) fn banned_sql(column: &str) -> String {
    format!("EXISTS (SELECT 1 FROM banned b WHERE b.player_id = {column})")
}

pub(crate) fn is_banned(db: &Connection, player_id: &str) -> bool {
    db.query_row("SELECT 1 FROM banned WHERE player_id = ?1", params![player_id], |_| Ok(()))
        .optional()
        .ok()
        .flatten()
        .is_some()
}

pub(crate) fn is_owner(db: &Connection, player_id: &str) -> bool {
    db.query_row("SELECT 1 FROM owners WHERE player_id = ?1", params![player_id], |_| Ok(()))
        .optional()
        .ok()
        .flatten()
        .is_some()
}

/// The player id for `who`: an account name, or a player id as it is.
fn resolve(db: &Connection, who: &str) -> Result<String, String> {
    let found: Option<String> = db
        .query_row("SELECT player_id FROM accounts WHERE name = ?1", params![who], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    match found {
        Some(id) => Ok(id),
        None if who.parse::<iroh_base::PublicKey>().is_ok() => Ok(who.to_owned()),
        None => Err(format!("there is no account called {who}")),
    }
}

/// An account, as whoever runs the Hub sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountRow {
    pub player_id: String,
    pub name: String,
    pub joined_secs_ago: i64,
    pub has_password: bool,
    pub owner: bool,
    pub banned: Option<String>,
}

/// Accounts whose names match `search`, by name.
pub fn accounts(db: &Connection, search: &str) -> Vec<AccountRow> {
    let pattern = format!("%{}%", search.trim().replace(['%', '_'], ""));
    let now = now();
    let Ok(mut statement) = db.prepare(
        "SELECT a.player_id, a.name, a.created,
             EXISTS (SELECT 1 FROM passwords p WHERE p.player_id = a.player_id),
             EXISTS (SELECT 1 FROM owners o WHERE o.player_id = a.player_id),
             (SELECT b.reason FROM banned b WHERE b.player_id = a.player_id)
         FROM accounts a WHERE a.name LIKE ?1 ORDER BY a.name",
    ) else {
        return Vec::new();
    };
    statement
        .query_map(params![pattern], |r| {
            Ok(AccountRow {
                player_id: r.get(0)?,
                name: r.get(1)?,
                joined_secs_ago: now - r.get::<_, i64>(2)?,
                has_password: r.get(3)?,
                owner: r.get(4)?,
                banned: r.get(5)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

/// Makes `who` (an account name or player id) an owner of the Hub. Returns their player id.
pub fn add_owner(db: &Connection, who: &str) -> Result<String, String> {
    let id = resolve(db, who)?;
    db.execute("INSERT OR IGNORE INTO owners (player_id, since) VALUES (?1, ?2)", params![id, now()])
        .map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn remove_owner(db: &Connection, who: &str) -> Result<(), String> {
    let id = resolve(db, who)?;
    match db.execute("DELETE FROM owners WHERE player_id = ?1", params![id]) {
        Ok(0) => Err(format!("{who} is not an owner")),
        Ok(_) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// The owners' player ids and account names (if they have one).
pub fn owners(db: &Connection) -> Vec<(String, Option<String>)> {
    let Ok(mut statement) = db.prepare(
        "SELECT o.player_id, a.name FROM owners o LEFT JOIN accounts a ON a.player_id = o.player_id ORDER BY o.since",
    ) else {
        return Vec::new();
    };
    statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

/// Bans `who`: they can't change their account, sign in, host, publish,
/// share, report or befriend anyone, and their page, games and items stop
/// being listed. Their name stays taken. Returns their player id.
pub fn ban(db: &Connection, who: &str, reason: &str) -> Result<String, String> {
    let id = resolve(db, who)?;
    if is_owner(db, &id) {
        return Err(format!("{who} is an owner; remove them as one first"));
    }
    db.execute(
        "INSERT INTO banned (player_id, reason, since) VALUES (?1, ?2, ?3)
         ON CONFLICT(player_id) DO UPDATE SET reason = ?2",
        params![id, reason.trim(), now()],
    )
    .map_err(|e| e.to_string())?;
    let _ = db.execute("DELETE FROM presence WHERE player_id = ?1", params![id]);
    let _ = db.execute("DELETE FROM web_presence WHERE player_id = ?1", params![id]);
    let _ = db.execute("DELETE FROM servers WHERE host_id = ?1", params![id]);
    Ok(id)
}

/// Renames `who` to `name`, if no one else has it. The old name is freed.
pub fn rename(db: &Connection, who: &str, name: &str) -> Result<String, String> {
    if !super::valid_account_name(name) {
        return Err("a name is 3 to 20 letters, digits or underscores".into());
    }
    let id = resolve(db, who)?;
    let taken: Option<String> = db
        .query_row("SELECT player_id FROM accounts WHERE name = ?1", params![name], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    if taken.is_some_and(|owner| owner != id) {
        return Err(format!("someone already has the name {name}"));
    }
    let changed = db
        .execute("UPDATE accounts SET name = ?2 WHERE player_id = ?1", params![id, name])
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err(format!("{who} has no account here"));
    }
    db.execute("DELETE FROM released_names WHERE name = ?1", params![name]).map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn unban(db: &Connection, who: &str) -> Result<(), String> {
    let id = resolve(db, who)?;
    match db.execute("DELETE FROM banned WHERE player_id = ?1", params![id]) {
        Ok(0) => Err(format!("{who} is not banned")),
        Ok(_) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

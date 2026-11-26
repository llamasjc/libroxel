//! Encryption of private data in the Hub's database, with a key kept beside it.

use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, OptionalExtension, params};

const SEALED: &str = "enc1:";
const PEPPERED: &str = "p1:";
/// Sealed into the database to tell the right key from a wrong one. Its
/// value predates the rename to PlayLibre; changing it would lock out
/// databases sealed with it.
const CHECK: &str = "libroxel";

/// The key file for the database at `path`.
pub fn key_path(path: &Path) -> PathBuf {
    path.with_extension("key")
}

fn random_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).expect("the system has a random number source");
    key
}

fn write_key(path: &Path, key: &[u8; 32]) -> std::io::Result<()> {
    let hex = data_encoding::HEXLOWER.encode(key);
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
        file.write_all(hex.as_bytes())
    }
    #[cfg(not(unix))]
    std::fs::write(path, hex)
}

fn read_key(path: &Path) -> Result<Option<[u8; 32]>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };
    data_encoding::HEXLOWER_PERMISSIVE
        .decode(text.trim().as_bytes())
        .ok()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .map(Some)
        .ok_or_else(|| format!("{} is not a Hub key", path.display()))
}

fn has_check(db: &Connection) -> Result<bool, String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS seal_check (id INTEGER PRIMARY KEY CHECK (id = 1), value TEXT NOT NULL);",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))?;
    db.query_row("SELECT 1 FROM seal_check", [], |_| Ok(()))
        .optional()
        .map(|row| row.is_some())
        .map_err(|e| e.to_string())
}

/// Loads (or makes) the key for `db` and teaches it `seal`, `unseal` and `pepper`.
pub(crate) fn install(db: &Connection, path: Option<&Path>) -> Result<(), String> {
    let sealed_before = has_check(db)?;
    let key = match path.map(key_path) {
        None => random_key(),
        Some(file) => match read_key(&file)? {
            Some(key) => key,
            None if sealed_before => {
                return Err(format!(
                    "the Hub's key {} is missing; without it its data can't be read",
                    file.display()
                ));
            }
            None => {
                let key = random_key();
                write_key(&file, &key).map_err(|e| format!("could not write {}: {e}", file.display()))?;
                key
            }
        },
    };
    add_functions(db, key).map_err(|e| format!("could not set up the Hub database: {e}"))?;
    if sealed_before {
        let fine = db
            .query_row("SELECT unseal(value) FROM seal_check", [], |r| r.get::<_, String>(0))
            .is_ok_and(|v| v == CHECK);
        if !fine {
            return Err("the Hub's key doesn't match its database".into());
        }
    } else {
        db.execute("INSERT INTO seal_check (id, value) VALUES (1, seal(?1))", params![CHECK])
            .map_err(|e| e.to_string())?;
    }
    seal_old_rows(db).map_err(|e| format!("could not encrypt the Hub's data: {e}"))
}

fn add_functions(db: &Connection, key: [u8; 32]) -> rusqlite::Result<()> {
    let cipher = XChaCha20Poly1305::new(&key.into());
    let open = cipher.clone();
    db.create_scalar_function("seal", 1, FunctionFlags::SQLITE_UTF8, move |ctx| {
        let Some(text) = ctx.get::<Option<String>>(0)? else { return Ok(None) };
        if text.starts_with(SEALED) {
            return Ok(Some(text));
        }
        let mut nonce = [0u8; 24];
        getrandom::fill(&mut nonce).map_err(|e| rusqlite::Error::UserFunctionError(e.to_string().into()))?;
        let sealed = cipher
            .encrypt(XNonce::from_slice(&nonce), text.as_bytes())
            .map_err(|_| rusqlite::Error::UserFunctionError("could not encrypt".into()))?;
        let mut bytes = nonce.to_vec();
        bytes.extend_from_slice(&sealed);
        Ok(Some(format!("{SEALED}{}", data_encoding::BASE64.encode(&bytes))))
    })?;
    db.create_scalar_function(
        "unseal",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        move |ctx| {
            let Some(text) = ctx.get::<Option<String>>(0)? else { return Ok(None) };
            let Some(body) = text.strip_prefix(SEALED) else { return Ok(Some(text)) };
            let bad = || rusqlite::Error::UserFunctionError("could not decrypt".into());
            let bytes = data_encoding::BASE64.decode(body.as_bytes()).map_err(|_| bad())?;
            if bytes.len() < 24 {
                return Err(bad());
            }
            let (nonce, sealed) = bytes.split_at(24);
            let plain = open.decrypt(XNonce::from_slice(nonce), sealed).map_err(|_| bad())?;
            String::from_utf8(plain).map(Some).map_err(|_| bad())
        },
    )?;
    db.create_scalar_function(
        "pepper",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        move |ctx| {
            let Some(text) = ctx.get::<Option<String>>(0)? else { return Ok(None) };
            if text.starts_with(PEPPERED) {
                return Ok(Some(text));
            }
            let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(&key).expect("any key length");
            mac.update(text.as_bytes());
            Ok(Some(format!("{PEPPERED}{}", data_encoding::HEXLOWER.encode(&mac.finalize().into_bytes()))))
        },
    )
}

/// `text` hashed with the Hub's key.
pub(crate) fn pepper(db: &Connection, text: &str) -> String {
    db.query_row("SELECT pepper(?1)", params![text], |r| r.get(0)).expect("the pepper function is set up")
}

/// Encrypts rows kept by Hubs from before encryption.
fn seal_old_rows(db: &Connection) -> rusqlite::Result<()> {
    for (table, column) in [
        ("emails", "email"),
        ("family", "parent_email"),
        ("two_step", "secret"),
        ("messages", "text"),
        // A stolen copy of the database alone gives nothing to guess
        // passwords against: the salt, the key a password makes and the
        // sealed identity key all need the Hub's key file too.
        ("passwords", "salt"),
        ("passwords", "login_id"),
        ("passwords", "sealed"),
    ] {
        db.execute(
            &format!("UPDATE {table} SET {column} = seal({column}) WHERE {column} IS NOT NULL AND {column} NOT LIKE '{SEALED}%'"),
            [],
        )?;
    }
    for (table, column) in [
        ("emails", "code"),
        ("family", "code"),
        ("family", "pin"),
        ("resets", "code"),
        ("recovery_codes", "hash"),
        ("sessions", "session_id"),
        ("games", "secret"),
        ("servers", "secret"),
    ] {
        db.execute(
            &format!(
                "UPDATE OR IGNORE {table} SET {column} = pepper({column}) WHERE {column} IS NOT NULL AND {column} NOT LIKE '{PEPPERED}%'"
            ),
            [],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seals_and_opens() {
        let db = crate::open_db(None).unwrap();
        let sealed: String = db.query_row("SELECT seal('a@b.c')", [], |r| r.get(0)).unwrap();
        assert!(sealed.starts_with(SEALED) && !sealed.contains("a@b.c"));
        let open: String = db.query_row("SELECT unseal(?1)", params![sealed], |r| r.get(0)).unwrap();
        assert_eq!(open, "a@b.c");
        assert_eq!(pepper(&db, "123456"), pepper(&db, "123456"));
        assert_ne!(pepper(&db, "123456"), pepper(&db, "123457"));
    }

    #[test]
    fn needs_its_key() {
        let dir = std::env::temp_dir().join(format!("playlibre-seal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hub.sqlite");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(key_path(&path));
        let db = crate::open_db(Some(&path)).unwrap();
        db.execute("INSERT INTO emails (player_id, email) VALUES ('p', 'a@b.c')", []).unwrap();
        drop(db);
        let db = crate::open_db(Some(&path)).unwrap();
        let raw: String = db.query_row("SELECT email FROM emails", [], |r| r.get(0)).unwrap();
        assert!(raw.starts_with(SEALED));
        assert_eq!(crate::email::email_of(&db, "p"), Some(("a@b.c".into(), false)));
        drop(db);
        std::fs::remove_file(key_path(&path)).unwrap();
        assert!(crate::open_db(Some(&path)).is_err());
        std::fs::write(key_path(&path), data_encoding::HEXLOWER.encode(&[7u8; 32])).unwrap();
        assert!(crate::open_db(Some(&path)).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

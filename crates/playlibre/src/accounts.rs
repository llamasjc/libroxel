//! Accounts saved on this computer, so a player can sign out and switch
//! between them without typing a password each time.
//!
//! The account in use is the identity key at [`crate::identity_path`]. Each
//! account that has a name on a Hub is also kept in an `accounts` folder
//! beside it, as `<player id>.key` with its name in `<player id>.name`.

use std::path::{Path, PathBuf};

use playlibre_net::Identity;

/// An account saved on this computer.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub player_id: String,
    pub name: String,
}

/// Keeps the account in use, named `name`, among the saved ones.
pub fn remember(name: &str) -> Result<(), String> {
    remember_at(&crate::identity_path(), name)
}

/// The accounts saved on this computer, by name.
pub fn saved() -> Vec<Saved> {
    saved_at(&crate::identity_path())
}

/// Uses the saved account `player_id` from now on.
pub fn switch_to(player_id: &str) -> Result<(), String> {
    switch_at(&crate::identity_path(), player_id)
}

/// Stops using the account in use (it stays saved, named `name`, if it has
/// a name) and starts a new, empty one: to make another account or sign in.
pub fn sign_out(name: Option<&str>) -> Result<(), String> {
    sign_out_at(&crate::identity_path(), name)
}

/// Notes that the account `player_id` signed in with a password too easy to
/// guess (`why`), so the app asks for a new one before anything else.
pub fn mark_weak_password(player_id: &str, why: &str) -> Result<(), String> {
    let path = weak_path(&crate::identity_path());
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&path, format!("{player_id}\n{why}"))
        .map_err(|e| format!("Could not save a note on this computer: {e}"))
}

/// Why the account in use has to change its password, if it does.
pub fn weak_password() -> Option<String> {
    let identity = crate::identity_path();
    let note = std::fs::read_to_string(weak_path(&identity)).ok()?;
    let (player_id, why) = note.split_once('\n')?;
    (load(&identity).ok()?.player_id() == player_id).then(|| why.to_owned())
}

/// Forgets the note, once the account has a new password.
pub fn clear_weak_password() {
    let _ = std::fs::remove_file(weak_path(&crate::identity_path()));
}

/// This device's session on the Hub for the account in use, if it has one.
pub fn session() -> Option<String> {
    let identity = crate::identity_path();
    let note = std::fs::read_to_string(session_path(&identity)).ok()?;
    let (player_id, session) = note.trim().split_once('\n')?;
    (load(&identity).ok()?.player_id() == player_id).then(|| session.to_owned())
}

/// Keeps `session` as this device's session for `player_id`.
pub fn set_session(player_id: &str, session: &str) {
    let path = session_path(&crate::identity_path());
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, format!("{player_id}\n{session}"));
}

/// Uses `key` from now on in place of the account in use, which was moved
/// to it; the old key is dropped.
pub fn moved_to(key: &[u8; 32]) -> Result<(), String> {
    moved_at(&crate::identity_path(), key)
}

/// Drops the account in use, whose key was signed out, and starts a new, empty one.
pub fn signed_out() -> Result<(), String> {
    moved_at(&crate::identity_path(), &Identity::generate().secret_bytes())
}

fn moved_at(identity: &Path, key: &[u8; 32]) -> Result<(), String> {
    if let Ok(old) = load(identity) {
        let dir = folder(identity);
        let _ = std::fs::remove_file(dir.join(format!("{}.key", old.player_id())));
        let _ = std::fs::remove_file(dir.join(format!("{}.name", old.player_id())));
    }
    Identity::from_secret_bytes(key).save_over(identity).map_err(|e| format!("Could not save the key: {e}"))
}

fn session_path(identity: &Path) -> PathBuf {
    identity.parent().map(|dir| dir.join("session")).unwrap_or_else(|| PathBuf::from("session"))
}

fn weak_path(identity: &Path) -> PathBuf {
    identity.parent().map(|dir| dir.join("weak-password")).unwrap_or_else(|| PathBuf::from("weak-password"))
}

fn folder(identity: &Path) -> PathBuf {
    identity.parent().map(|dir| dir.join("accounts")).unwrap_or_else(|| PathBuf::from("accounts"))
}

fn load(identity: &Path) -> Result<Identity, String> {
    Identity::load_or_create(identity).map_err(|e| format!("Could not load your identity: {e}"))
}

fn remember_at(identity: &Path, name: &str) -> Result<(), String> {
    let me = load(identity)?;
    let dir = folder(identity);
    let id = me.player_id();
    let failed = |e: std::io::Error| format!("Could not save the account: {e}");
    me.save_over(&dir.join(format!("{id}.key"))).map_err(failed)?;
    std::fs::write(dir.join(format!("{id}.name")), name).map_err(failed)
}

fn saved_at(identity: &Path) -> Vec<Saved> {
    let mut found: Vec<Saved> = std::fs::read_dir(folder(identity))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()? != "key" {
                return None;
            }
            let player_id = path.file_stem()?.to_str()?.to_owned();
            let name = std::fs::read_to_string(path.with_extension("name")).unwrap_or_default();
            let name = if name.trim().is_empty() { player_id.clone() } else { name.trim().to_owned() };
            Some(Saved { player_id, name })
        })
        .collect();
    found.sort_by_key(|s| s.name.to_ascii_lowercase());
    found
}

fn switch_at(identity: &Path, player_id: &str) -> Result<(), String> {
    let bytes = std::fs::read(folder(identity).join(format!("{player_id}.key")))
        .map_err(|e| format!("That account isn't saved here any more: {e}"))?;
    let bytes: [u8; 32] =
        bytes.as_slice().try_into().map_err(|_| "That saved account is damaged.".to_owned())?;
    let account = Identity::from_secret_bytes(&bytes);
    if account.player_id() != player_id {
        return Err("That saved account is damaged.".into());
    }
    account.save_over(identity).map_err(|e| format!("Could not switch accounts: {e}"))
}

fn sign_out_at(identity: &Path, name: Option<&str>) -> Result<(), String> {
    if let Some(name) = name {
        remember_at(identity, name)?;
    }
    Identity::generate().save_over(identity).map_err(|e| format!("Could not sign out: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_out_keeps_the_account_to_switch_back_to() {
        let dir = std::env::temp_dir().join(format!("playlibre-switch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let identity = dir.join("identity.key");
        let admin = load(&identity).unwrap().player_id();
        remember_at(&identity, "Admin").unwrap();

        // Sign out: a new, empty identity is in use; Admin stays saved.
        sign_out_at(&identity, Some("Admin")).unwrap();
        let fresh = load(&identity).unwrap().player_id();
        assert_ne!(fresh, admin);
        remember_at(&identity, "llamas").unwrap();
        let names: Vec<_> = saved_at(&identity).into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["Admin", "llamas"]);

        // Switch back and forth.
        switch_at(&identity, &admin).unwrap();
        assert_eq!(load(&identity).unwrap().player_id(), admin);
        switch_at(&identity, &fresh).unwrap();
        assert_eq!(load(&identity).unwrap().player_id(), fresh);
        assert!(switch_at(&identity, "nobody").is_err());

        // A key moved away is dropped, saved copy and all.
        moved_at(&identity, &[9u8; 32]).unwrap();
        assert_eq!(load(&identity).unwrap().player_id(), Identity::from_secret_bytes(&[9u8; 32]).player_id());
        assert!(saved_at(&identity).iter().all(|s| s.player_id != fresh));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

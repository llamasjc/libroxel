//! A copy of unsaved work, written now and then while the editor has
//! changes that aren't saved, so a crash or a closed window doesn't lose
//! them. The next start offers the copy back. Saving the game (or choosing
//! to throw the copy away) deletes it.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use libroxel_instance::{InstanceTree, place};
use serde_json::{Value as Json, json};

/// How long unsaved changes may wait before a copy is written.
pub const EVERY: Duration = Duration::from_secs(60);

const GAME: &str = "unsaved.lrx";
const ABOUT: &str = "unsaved.json";

/// Where the copy lives.
pub fn dir() -> PathBuf {
    crate::server::data_dir().join("recovery")
}

/// A copy of unsaved work found on start.
#[derive(Debug, Clone, PartialEq)]
pub struct Recovered {
    /// The game, as a `.lrx` file's text.
    pub text: String,
    /// The file it was opened from, if it had one.
    pub file: Option<PathBuf>,
    /// When the copy was written, in seconds since 1970.
    pub saved_at: u64,
}

impl Recovered {
    /// What to call it: its file's name, or "an unsaved game".
    pub fn name(&self) -> String {
        self.file
            .as_deref()
            .and_then(Path::file_name)
            .map_or_else(|| "an unsaved game".to_owned(), |n| n.to_string_lossy().into_owned())
    }

    /// How long ago it was written, in words.
    pub fn age(&self, now: u64) -> String {
        let minutes = now.saturating_sub(self.saved_at) / 60;
        match minutes {
            0 => "just now".to_owned(),
            1 => "a minute ago".to_owned(),
            2..60 => format!("{minutes} minutes ago"),
            60..120 => "an hour ago".to_owned(),
            120..2880 => format!("{} hours ago", minutes / 60),
            _ => format!("{} days ago", minutes / 1440),
        }
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Writes a copy of `tree` (opened from `file`, if it was) into `dir`.
/// The game is written beside its final name first, so a crash part-way
/// through leaves the previous copy whole.
pub fn write(dir: &Path, tree: &InstanceTree, file: Option<&Path>) -> Result<(), String> {
    let text = place::save(tree).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let part = dir.join(format!("{GAME}.part"));
    std::fs::write(&part, text).map_err(|e| e.to_string())?;
    std::fs::rename(&part, dir.join(GAME)).map_err(|e| e.to_string())?;
    let about = json!({ "file": file.map(|f| f.to_string_lossy().into_owned()), "saved_at": now() });
    std::fs::write(dir.join(ABOUT), about.to_string()).map_err(|e| e.to_string())
}

/// The copy in `dir`, if there is a readable one.
pub fn read(dir: &Path) -> Option<Recovered> {
    let text = std::fs::read_to_string(dir.join(GAME)).ok()?;
    if text.trim().is_empty() {
        return None;
    }
    let about: Json = std::fs::read_to_string(dir.join(ABOUT))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Json::Null);
    Some(Recovered {
        text,
        file: about.get("file").and_then(Json::as_str).map(PathBuf::from),
        saved_at: about.get("saved_at").and_then(Json::as_u64).unwrap_or(0),
    })
}

/// Deletes the copy in `dir`.
pub fn clear(dir: &Path) {
    for name in [GAME, ABOUT] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    let _ = std::fs::remove_file(dir.join(format!("{GAME}.part")));
}

/// Keeps the copy up to date for one frame of the editor.
#[derive(Debug, Default)]
pub struct Autosave {
    /// When the changes not yet in a copy began (seconds of app time).
    pending_since: Option<f64>,
    /// Whether this run has written a copy (so it is ours to delete).
    wrote: bool,
    /// The app was in the background last frame.
    backgrounded: bool,
}

impl Autosave {
    /// Takes over a copy found on start that the player opened, so saving
    /// deletes it like one written this run.
    pub fn adopt(&mut self) {
        self.wrote = true;
    }

    /// Writes a copy once changes have waited [`EVERY`], and deletes it once
    /// they are saved. `held` stops both, while a found copy is being
    /// offered: it mustn't be overwritten or deleted before the player chooses.
    pub fn tick(
        &mut self,
        dir: &Path,
        now: f64,
        dirty: bool,
        held: bool,
        save: impl FnOnce(&Path) -> Result<(), String>,
    ) -> Option<String> {
        if held {
            return None;
        }
        if !dirty {
            self.pending_since = None;
            if self.wrote {
                clear(dir);
                self.wrote = false;
            }
            return None;
        }
        let since = *self.pending_since.get_or_insert(now);
        // Going into the background, a phone may close the app without
        // asking: keep a copy straight away.
        let backgrounded = libroxel_engine::sounds::silent();
        let leaving = backgrounded && !std::mem::replace(&mut self.backgrounded, backgrounded);
        self.backgrounded = backgrounded;
        if now - since < EVERY.as_secs_f64() && !leaving {
            return None;
        }
        self.pending_since = Some(now);
        match save(dir) {
            Ok(()) => {
                self.wrote = true;
                None
            }
            Err(e) => Some(format!("Could not keep a copy of unsaved work: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libroxel_instance::{ClassKind, Property, PropertyValue, Vec3};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("libroxel-autosave-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn world() -> InstanceTree {
        let mut tree = InstanceTree::new();
        let ws = tree.workspace();
        let part = tree.create(ClassKind::Part, "Tower").unwrap();
        tree.set_parent(part, Some(ws)).unwrap();
        tree.set(part, Property::Position, PropertyValue::Vec3(Vec3::new(1.0, 2.0, 3.0))).unwrap();
        tree
    }

    #[test]
    fn a_copy_comes_back_as_it_was() {
        let dir = scratch("back");
        assert_eq!(read(&dir), None);
        write(&dir, &world(), Some(Path::new("/games/castle.lrx"))).unwrap();
        let found = read(&dir).unwrap();
        assert_eq!(found.file.as_deref(), Some(Path::new("/games/castle.lrx")));
        assert_eq!(found.name(), "castle.lrx");
        assert!(now() - found.saved_at < 60);
        let (tree, _) = place::load(&found.text).unwrap();
        let tower = tree.find_first_child(tree.workspace(), "Tower").unwrap().unwrap();
        assert_eq!(tree.get_vec3(tower, Property::Position), Some(Vec3::new(1.0, 2.0, 3.0)));
        clear(&dir);
        assert_eq!(read(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copies_are_written_after_a_minute_and_deleted_once_saved() {
        let dir = scratch("tick");
        let mut auto = Autosave::default();
        let save = |d: &Path| write(d, &world(), None);
        // Saved work: nothing to do.
        auto.tick(&dir, 0.0, false, false, save);
        assert_eq!(read(&dir), None);
        // A change: no copy straight away...
        auto.tick(&dir, 10.0, true, false, save);
        auto.tick(&dir, 69.0, true, false, save);
        assert_eq!(read(&dir), None);
        // ...but a minute after it, there is one.
        auto.tick(&dir, 70.5, true, false, save);
        assert_eq!(read(&dir).unwrap().name(), "an unsaved game");
        // Saving deletes it.
        auto.tick(&dir, 71.0, false, false, save);
        assert_eq!(read(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_copy_being_offered_is_left_alone() {
        let dir = scratch("held");
        write(&dir, &world(), None).unwrap();
        let mut auto = Autosave::default();
        // Starting up with nothing unsaved doesn't delete a copy from last time.
        auto.tick(&dir, 0.0, false, true, |_| Ok(()));
        auto.tick(&dir, 1.0, false, false, |_| Ok(()));
        assert!(read(&dir).is_some(), "only copies this run wrote are deleted");
        // Once the player opens it, saving deletes it.
        auto.adopt();
        auto.tick(&dir, 2.0, false, false, |_| Ok(()));
        assert_eq!(read(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ages_read_naturally() {
        let r = Recovered { text: String::new(), file: None, saved_at: 1_000_000 };
        assert_eq!(r.age(1_000_030), "just now");
        assert_eq!(r.age(1_000_000 + 60 * 7), "7 minutes ago");
        assert_eq!(r.age(1_000_000 + 3600 * 5), "5 hours ago");
        assert_eq!(r.age(1_000_000 + 86400 * 3), "3 days ago");
    }
}

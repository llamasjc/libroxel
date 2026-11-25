//! Opening a game from a file: PlayLibre's own `.lrx` places, and place
//! and model files saved by the reference editor.

use std::path::Path;

use playlibre_instance::{InstanceId, InstanceTree, place};

/// A game read from a file.
pub struct Opened {
    pub tree: InstanceTree,
    /// What to tell the creator about it (what came across, what didn't).
    pub notes: Vec<String>,
    /// Whether it came from another engine's file, so saving it should make
    /// a new `.lrx` rather than write over the original.
    pub imported: bool,
}

/// Whether `path` is a file [`open`] understands.
pub fn is_game_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lrx")) || playlibre_rbx::is_place_file(path)
}

/// Whether `path` holds a model (instances to add to a game) rather than a whole place.
pub fn is_model_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "rbxm" | "rbxmx"))
}

/// Runs `read` on a file's contents, turning a crash inside the readers
/// (a damaged or deliberately broken file can trip them) into an error, so
/// a bad download can't close the app.
fn guarded<T>(path: &Path, read: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(read)).unwrap_or_else(|_| {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Err(format!("{name} is damaged and couldn't be read"))
    })
}

/// Opens a place (or a model, set in an empty world).
pub fn open(path: &Path) -> Result<Opened, String> {
    guarded(path, || open_unguarded(path))
}

fn open_unguarded(path: &Path) -> Result<Opened, String> {
    if playlibre_rbx::is_place_file(path) {
        let (tree, report) = playlibre_rbx::open(path).map_err(|e| e.to_string())?;
        let mut notes = report.summary();
        notes.extend(save_made_files(&report));
        return Ok(Opened { tree, notes, imported: true });
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let (tree, report) = place::load(&text).map_err(|e| e.to_string())?;
    Ok(Opened { tree, notes: report.warnings, imported: false })
}

/// Adds a model file's instances under `parent`, returning them and what to tell the creator.
pub fn insert_model(
    tree: &mut InstanceTree,
    path: &Path,
    parent: InstanceId,
) -> Result<(Vec<InstanceId>, Vec<String>), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let (made, report) = guarded(path, || {
        let dom = playlibre_rbx::read_dom(&bytes).map_err(|e| e.to_string())?;
        Ok(playlibre_rbx::import_model(tree, &dom, parent))
    })?;
    let mut notes = report.summary();
    notes.extend(save_made_files(&report));
    Ok((made, notes))
}

/// Saves the models an import made (union shapes) where models are looked
/// for, returning a note if that failed.
fn save_made_files(report: &playlibre_rbx::Report) -> Option<String> {
    if report.files.is_empty() {
        return None;
    }
    let dir = crate::mesh_import::meshes_dir();
    let saved = std::fs::create_dir_all(&dir).and_then(|_| {
        for (name, bytes) in &report.files {
            let path = dir.join(name);
            // Names follow contents, so one already there is the same file.
            if !path.exists() {
                std::fs::write(path, bytes)?;
            }
        }
        Ok(())
    });
    saved.err().map(|e| format!("Couldn't save the union shapes in {}: {e}", dir.display()))
}

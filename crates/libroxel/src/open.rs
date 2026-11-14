//! Opening a game from a file: Libroxel's own `.lrx` places, and place
//! and model files saved by Roblox Studio.

use std::path::Path;

use libroxel_instance::{InstanceId, InstanceTree, place};

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
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lrx")) || libroxel_rbx::is_roblox_file(path)
}

/// Whether `path` holds a model (instances to add to a game) rather than a whole place.
pub fn is_model_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "rbxm" | "rbxmx"))
}

/// Opens a place (or a model, set in an empty world).
pub fn open(path: &Path) -> Result<Opened, String> {
    if libroxel_rbx::is_roblox_file(path) {
        let (tree, report) = libroxel_rbx::open(path).map_err(|e| e.to_string())?;
        return Ok(Opened { tree, notes: report.summary(), imported: true });
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
    let dom = libroxel_rbx::read_dom(&bytes).map_err(|e| e.to_string())?;
    let (made, report) = libroxel_rbx::import_model(tree, &dom, parent);
    Ok((made, report.summary()))
}

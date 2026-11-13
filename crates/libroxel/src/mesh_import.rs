//! Bringing your own 3D models in: drop an .obj file on the window and it
//! is copied into the games folder's `meshes` folder and added as a part
//! that draws it (its `MeshId`), at the model's own size.

use std::path::{Path, PathBuf};

use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue, Vec3};

/// The biggest a model comes in at, in studs along its longest side.
const LARGEST: f32 = 50.0;

/// Where imported models are kept.
pub fn meshes_dir() -> PathBuf {
    crate::games_dir().join("meshes")
}

/// The size a part showing a model `natural` studs big starts at: as it
/// is, but shrunk to fit if it's huge and never flatter than a sliver.
pub fn starting_size(natural: bevy::math::Vec3) -> Vec3 {
    let longest = natural.max_element();
    let shrink = if longest > LARGEST { LARGEST / longest } else { 1.0 };
    let side = |v: f32| (v * shrink).max(0.05);
    Vec3::new(side(natural.x), side(natural.y), side(natural.z))
}

/// Copies the model at `from` into `into` and adds a part showing it to
/// the workspace, standing at `at`. Returns the new part.
pub fn import(tree: &mut InstanceTree, from: &Path, into: &Path, at: Vec3) -> Result<InstanceId, String> {
    let text = std::fs::read_to_string(from).map_err(|e| format!("Couldn't read {}: {e}", from.display()))?;
    let model = libroxel_engine::mesh_files::parse_obj(&text)
        .ok_or_else(|| format!("{} has no faces to draw", from.display()))?;
    let file_name =
        from.file_name().and_then(|n| n.to_str()).ok_or("The model's file name isn't readable")?;
    std::fs::create_dir_all(into).map_err(|e| format!("Couldn't make {}: {e}", into.display()))?;
    let copy = into.join(file_name);
    if copy != from {
        std::fs::copy(from, &copy).map_err(|e| format!("Couldn't copy the model: {e}"))?;
    }
    let name = from.file_stem().and_then(|n| n.to_str()).unwrap_or("Mesh");
    let size = starting_size(model.size);
    let part = tree.create(ClassKind::Part, name).map_err(|e| e.to_string())?;
    let set = |tree: &mut InstanceTree, p, v| tree.set(part, p, v).map_err(|e| e.to_string());
    set(tree, Property::MeshId, PropertyValue::String(file_name.to_owned()))?;
    set(tree, Property::Size, PropertyValue::Vec3(size))?;
    set(tree, Property::Position, PropertyValue::Vec3(at + Vec3::new(0.0, size.y / 2.0, 0.0)))?;
    set(tree, Property::Anchored, PropertyValue::Bool(true))?;
    tree.set_parent(part, Some(tree.workspace())).map_err(|e| e.to_string())?;
    Ok(part)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dropped_model_is_copied_in_and_becomes_a_part_at_its_own_size() {
        let dir = std::env::temp_dir().join(format!("libroxel-mesh-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("ramp.obj");
        std::fs::write(&source, "v 0 0 0\nv 6 0 0\nv 6 2 0\nv 0 0 3\nf 1 2 3\nf 1 3 4\n").unwrap();
        let mut tree = InstanceTree::new();
        let part = import(&mut tree, &source, &dir.join("meshes"), Vec3::new(1.0, 0.0, 0.0)).unwrap();
        assert!(dir.join("meshes").join("ramp.obj").exists());
        assert_eq!(tree.name(part).unwrap(), "ramp");
        assert_eq!(tree.get_string(part, Property::MeshId).as_deref(), Some("ramp.obj"));
        assert_eq!(tree.get_vec3(part, Property::Size), Some(Vec3::new(6.0, 2.0, 3.0)));
        assert_eq!(tree.get_vec3(part, Property::Position), Some(Vec3::new(1.0, 1.0, 0.0)));
        assert_eq!(tree.parent(part).unwrap(), Some(tree.workspace()));
        std::fs::write(dir.join("empty.obj"), "# nothing here\n").unwrap();
        assert!(import(&mut tree, &dir.join("empty.obj"), &dir, Vec3::ZERO).is_err());
        // Huge models are shrunk to fit; flat ones keep a little thickness.
        let big = starting_size(bevy::math::Vec3::new(500.0, 100.0, 0.0));
        assert_eq!(big, Vec3::new(50.0, 10.0, 0.05));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! How much memory a big map's parts take in the tree: a part should cost
//! well under a kilobyte, so a 60,000-part map fits on a small tablet.
//! `cargo test --release -p playlibre-instance --test part_memory -- --nocapture`
//! prints the bytes per part.

use playlibre_instance::{ClassKind, Color3, InstanceTree, Property, PropertyValue, Vec3};

/// The memory the process holds now, in bytes.
fn resident() -> f64 {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let pages: f64 = statm.split_whitespace().nth(1).and_then(|p| p.parse().ok()).unwrap_or(0.0);
    pages * 4096.0
}

const MODELS: usize = 3000;
const PARTS_EACH: usize = 20;

#[test]
fn a_part_takes_little_memory() {
    let before = resident();
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let v = |x, y, z| PropertyValue::Vec3(Vec3::new(x, y, z));
    for m in 0..MODELS {
        let model = tree.create(ClassKind::Model, format!("Model{m}")).unwrap();
        tree.set_parent(model, Some(ws)).unwrap();
        for p in 0..PARTS_EACH {
            let part = tree.create(ClassKind::Part, format!("Part{p}")).unwrap();
            tree.set_parent(part, Some(model)).unwrap();
            tree.set(part, Property::Position, v(m as f32, p as f32, 0.0)).unwrap();
            tree.set(part, Property::Size, v(2.0, 1.0, 2.0)).unwrap();
            tree.set(part, Property::Color, PropertyValue::Color3(Color3::new(0.5, 0.2, 0.1))).unwrap();
            tree.set(part, Property::Anchored, PropertyValue::Bool(true)).unwrap();
        }
    }
    // What a game keeps while it plays (its logs of changes are read and
    // let go each frame).
    tree.drain_changes();
    let parts = MODELS * PARTS_EACH;
    let per_part = (resident() - before) / parts as f64;
    println!("part memory: {per_part:.0} bytes a part ({parts} parts in {MODELS} models)");
    // (Only where the process's memory can be read.)
    if before > 0.0 {
        assert!(per_part < 750.0, "{per_part:.0} bytes a part");
    }
    drop(tree);
}

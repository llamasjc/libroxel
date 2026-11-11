//! Times play-session frames in a world with many parts:
//! `cargo run -p libroxel-engine --example bench -- 2000`.

use libroxel_engine::CharacterInput;
use libroxel_engine::session::start_on;
use libroxel_instance::{ClassKind, InstanceTree, Property, PropertyValue, Vec3};

fn main() {
    let count: usize = std::env::args().nth(1).and_then(|n| n.parse().ok()).unwrap_or(1000);
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let floor = tree.create(ClassKind::Part, "Floor").unwrap();
    tree.set_parent(floor, Some(ws)).unwrap();
    tree.set(floor, Property::Size, PropertyValue::Vec3(Vec3::new(400.0, 1.0, 400.0))).unwrap();
    tree.set(floor, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, -0.5, 0.0))).unwrap();
    tree.set(floor, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    let side = (count as f32).sqrt().ceil() as usize;
    for i in 0..count {
        let id = tree.create(ClassKind::Part, "Brick").unwrap();
        tree.set_parent(id, Some(ws)).unwrap();
        let (x, z) = ((i % side) as f32 * 5.0 - 100.0, (i / side) as f32 * 5.0 - 100.0);
        // Every fourth brick is loose and falls; the rest are scenery.
        let loose = i % 4 == 0;
        tree.set(id, Property::Position, PropertyValue::Vec3(Vec3::new(x, if loose { 4.0 } else { 1.0 }, z)))
            .unwrap();
        tree.set(id, Property::Size, PropertyValue::Vec3(Vec3::new(2.0, 2.0, 2.0))).unwrap();
        tree.set(id, Property::Anchored, PropertyValue::Bool(!loose)).unwrap();
    }
    let mut session = start_on(tree, true).unwrap();
    let frames = 120;
    let start = std::time::Instant::now();
    for _ in 0..frames {
        session.step(1.0 / 60.0, CharacterInput::default());
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0 / frames as f64;
    println!("{count} parts: {ms:.2} ms per frame");
}

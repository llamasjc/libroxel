//! Loose parts tipping over edges and settling flat.

use libroxel_engine::CharacterInput;
use libroxel_engine::session::start_on;
use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue, Vec3};

const DT: f32 = 1.0 / 60.0;

fn part(tree: &mut InstanceTree, name: &str, pos: Vec3, size: Vec3, anchored: bool) -> InstanceId {
    let id = tree.create(ClassKind::Part, name).unwrap();
    tree.set_parent(id, Some(tree.workspace())).unwrap();
    tree.set(id, Property::Position, PropertyValue::Vec3(pos)).unwrap();
    tree.set(id, Property::Size, PropertyValue::Vec3(size)).unwrap();
    tree.set(id, Property::Anchored, PropertyValue::Bool(anchored)).unwrap();
    id
}

/// A floor, and a ledge 10 studs high whose edge is at x = 0.
fn world() -> InstanceTree {
    let mut tree = InstanceTree::new();
    part(&mut tree, "Floor", Vec3::new(0.0, -0.5, 0.0), Vec3::new(200.0, 1.0, 200.0), true);
    part(&mut tree, "Ledge", Vec3::new(-10.0, 5.0, 0.0), Vec3::new(20.0, 10.0, 20.0), true);
    tree
}

/// Runs for `seconds`; gives each watched part's final position and orientation.
fn run(tree: InstanceTree, seconds: f32, watch: &[InstanceId]) -> Vec<(Vec3, Vec3)> {
    let mut session = start_on(tree, false).unwrap();
    let shared = session.runtime().tree().clone();
    for _ in 0..(seconds / DT).round() as usize {
        session.step(DT, CharacterInput::default());
    }
    let t = shared.borrow();
    watch
        .iter()
        .map(|id| {
            (t.get_vec3(*id, Property::Position).unwrap(), t.get_vec3(*id, Property::Orientation).unwrap())
        })
        .collect()
}

/// Whether every angle is a whole number of right angles.
fn lies_flat(orientation: Vec3) -> bool {
    [orientation.x, orientation.z].iter().all(|a| {
        let off = a.rem_euclid(90.0);
        off.min(90.0 - off) < 0.5
    })
}

#[test]
fn a_crate_hanging_over_an_edge_tips_off_and_lands_flat() {
    let mut tree = world();
    // Three quarters of it hangs past the edge.
    let crate_ = part(&mut tree, "Crate", Vec3::new(1.0, 12.0, 0.0), Vec3::new(4.0, 4.0, 4.0), false);
    let [(pos, orientation)] = run(tree, 4.0, &[crate_])[..] else { unreachable!() };
    assert!(pos.y < 2.2 && pos.y > 1.8, "it fell to the floor and rests there: {pos:?}");
    assert!(pos.x > 0.0, "it fell off the side it hung over: {pos:?}");
    assert!(lies_flat(orientation), "{orientation:?}");
}

#[test]
fn a_crate_mostly_on_the_ledge_stays() {
    let mut tree = world();
    let crate_ = part(&mut tree, "Crate", Vec3::new(-1.0, 12.0, 0.0), Vec3::new(4.0, 4.0, 4.0), false);
    let [(pos, orientation)] = run(tree, 2.0, &[crate_])[..] else { unreachable!() };
    assert!((pos.y - 12.0).abs() < 0.1 && (pos.x + 1.0).abs() < 0.1, "{pos:?}");
    assert_eq!(orientation, Vec3::ZERO);
}

#[test]
fn a_tilted_block_lands_on_a_side() {
    let mut tree = world();
    let block = part(&mut tree, "Block", Vec3::new(20.0, 8.0, 0.0), Vec3::new(2.0, 6.0, 2.0), false);
    tree.set(block, Property::Orientation, PropertyValue::Vec3(Vec3::new(0.0, 0.0, 35.0))).unwrap();
    // A plank stood on end stays standing.
    let plank = part(&mut tree, "Plank", Vec3::new(30.0, 4.0, 0.0), Vec3::new(1.0, 8.0, 1.0), false);
    let [(pos, orientation), (plank_pos, plank_turn)] = run(tree, 4.0, &[block, plank])[..] else {
        unreachable!()
    };
    assert!(lies_flat(orientation), "{orientation:?}");
    // It fell over onto a long side (tilted past halfway to it), so it lies low.
    assert!(pos.y < 1.2 && pos.y > 0.8, "{pos:?} {orientation:?}");
    assert_eq!(plank_turn, Vec3::ZERO);
    assert!((plank_pos.y - 4.0).abs() < 0.1, "{plank_pos:?}");
}

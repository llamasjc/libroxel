//! Parts with a model collide as the model's shape, not its box: here, two
//! stilts with a gap between them, which things fall through.

use playlibre_engine::CharacterInput;
use playlibre_engine::session::start_on;
use playlibre_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue, Vec3};

const DT: f32 = 1.0 / 60.0;

/// Two 2-stud cubes, 4 studs apart, as an .obj.
fn stilts() -> String {
    let mut out = String::new();
    let mut base = 0;
    for x0 in [-4.0f32, 2.0] {
        for y in [0.0, 2.0] {
            for z in [-1.0, 1.0] {
                for x in [x0, x0 + 2.0] {
                    out += &format!("v {x} {y} {z}\n");
                }
            }
        }
        // The cube's corners are numbered by x, then z, then y.
        let faces = [[1, 2, 4, 3], [5, 7, 8, 6], [1, 5, 6, 2], [3, 4, 8, 7], [1, 3, 7, 5], [2, 6, 8, 4]];
        for f in faces {
            out += &format!("f {} {} {} {}\n", f[0] + base, f[1] + base, f[2] + base, f[3] + base);
        }
        base += 8;
    }
    out
}

fn part(tree: &mut InstanceTree, name: &str, pos: Vec3, size: Vec3, anchored: bool) -> InstanceId {
    let id = tree.create(ClassKind::Part, name).unwrap();
    tree.set_parent(id, Some(tree.workspace())).unwrap();
    tree.set(id, Property::Position, PropertyValue::Vec3(pos)).unwrap();
    tree.set(id, Property::Size, PropertyValue::Vec3(size)).unwrap();
    tree.set(id, Property::Anchored, PropertyValue::Bool(anchored)).unwrap();
    id
}

fn world() -> InstanceTree {
    let mut tree = InstanceTree::new();
    part(&mut tree, "Floor", Vec3::new(0.0, -0.5, 0.0), Vec3::new(200.0, 1.0, 200.0), true);
    // The stilts, 8 wide, standing 10 studs up so there's room below.
    let stilts = part(&mut tree, "Stilts", Vec3::new(0.0, 11.0, 0.0), Vec3::new(8.0, 2.0, 2.0), true);
    tree.set(stilts, Property::MeshId, PropertyValue::String("stilts.obj".into())).unwrap();
    tree
}

#[test]
fn things_fall_through_a_models_gaps_and_rest_on_its_solid_bits() {
    let dir = std::env::temp_dir().join(format!("playlibre-mesh-collisions-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("stilts.obj"), stilts()).unwrap();
    playlibre_engine::pictures::set_folders(vec![dir.clone()]);

    // Loose crates: one over the gap, one over a stilt.
    let mut tree = world();
    let gap = part(&mut tree, "OverGap", Vec3::new(0.0, 16.0, 0.0), Vec3::new(1.0, 1.0, 1.0), false);
    let stilt = part(&mut tree, "OverStilt", Vec3::new(3.0, 16.0, 0.0), Vec3::new(1.0, 1.0, 1.0), false);
    let mut session = start_on(tree, false).unwrap();
    let shared = session.runtime().tree().clone();
    for _ in 0..180 {
        session.step(DT, CharacterInput::default());
    }
    {
        let t = shared.borrow();
        let y = |id| t.get_vec3(id, Property::Position).unwrap().y;
        assert!(y(gap) < 2.0, "fell through the gap to the floor: {}", y(gap));
        assert!((y(stilt) - 12.5).abs() < 0.3, "rests on the stilt: {}", y(stilt));
    }

    // A character walks the same way: dropped over the gap it lands on
    // the floor; over a stilt it stands on top.
    let mut ends = Vec::new();
    for x in [0.0, 3.0] {
        let mut session = start_on(world(), true).unwrap();
        let shared = session.runtime().tree().clone();
        let root = session.character().unwrap().root;
        shared
            .borrow_mut()
            .set(root, Property::Position, PropertyValue::Vec3(Vec3::new(x, 18.0, 0.0)))
            .unwrap();
        for _ in 0..180 {
            session.step(DT, CharacterInput::default());
        }
        ends.push(shared.borrow().get_vec3(root, Property::Position).unwrap());
    }
    assert!(ends[0].y < 8.0, "over the gap: {:?}", ends[0]);
    assert!(ends[1].y > 12.0, "over a stilt: {:?}", ends[1]);
    let _ = std::fs::remove_dir_all(dir);
}

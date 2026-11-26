use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use playlibre_engine::{CharacterInput, PlaySession};
use playlibre_instance::{ClassKind, InstanceTree, Property, PropertyValue, Vec3};

const DT: f32 = 1.0 / 60.0;
const MODELS: usize = 3000;
const PARTS_EACH: usize = 20;

fn heavy(streaming: bool) -> InstanceTree {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    tree.set(ws, Property::StreamingEnabled, PropertyValue::Bool(streaming)).unwrap();
    let floor = tree.create(ClassKind::Part, "Floor").unwrap();
    tree.set_parent(floor, Some(ws)).unwrap();
    tree.set(floor, Property::Size, PropertyValue::Vec3(Vec3::new(2000.0, 1.0, 2000.0))).unwrap();
    tree.set(floor, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, -0.5, 0.0))).unwrap();
    tree.set(floor, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    let words = "a fairly long description for each part so the world weighs a lot ".repeat(4);
    for m in 0..MODELS {
        let model = tree.create(ClassKind::Model, format!("Model{m}")).unwrap();
        tree.set_parent(model, Some(ws)).unwrap();
        for p in 0..PARTS_EACH {
            let part = tree.create(ClassKind::Part, format!("Part{p}")).unwrap();
            tree.set_parent(part, Some(model)).unwrap();
            let x = (m % 60) as f32 * 30.0 - 900.0;
            let z = (m / 60) as f32 * 30.0 - 900.0;
            tree.set(part, Property::Position, PropertyValue::Vec3(Vec3::new(x, 1.0 + p as f32, z))).unwrap();
            tree.set(part, Property::Size, PropertyValue::Vec3(Vec3::new(2.0, 1.0, 2.0))).unwrap();
            tree.set(part, Property::Anchored, PropertyValue::Bool(true)).unwrap();
            tree.set_attribute(part, "Note", Some(PropertyValue::String(words.clone()))).unwrap();
        }
    }
    tree
}

#[test]
fn joining_a_heavy_game_comes_in_over_many_short_steps() {
    join_heavy(false);
}

#[test]
fn joining_a_heavy_streamed_game_comes_in_over_many_short_steps() {
    join_heavy(true);
}

/// Without streaming every part comes, as the game's scripts expect;
/// with it, the ones near the player.
fn join_heavy(streaming: bool) {
    let tree = Rc::new(RefCell::new(heavy(streaming)));
    let mut host = PlaySession::host(tree, 0, None, true).unwrap();
    let port = host.host_address().unwrap().port();
    for _ in 0..3 {
        host.step(DT, CharacterInput::default());
    }
    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Bea")
            .unwrap();
    let near = |x: f32, z: f32| !streaming || x.hypot(z) < 900.0;
    let want =
        (0..MODELS).filter(|m| near((m % 60) as f32 * 30.0 - 900.0, (m / 60) as f32 * 30.0 - 900.0)).count()
            * PARTS_EACH;
    let mut slowest_guest = Duration::ZERO;
    let mut slowest_host = Duration::ZERO;
    let mut steps = 0;
    let began = Instant::now();
    loop {
        let t = Instant::now();
        host.step(DT, CharacterInput::default());
        slowest_host = slowest_host.max(t.elapsed());
        let t = Instant::now();
        guest.step(DT, CharacterInput::default());
        slowest_guest = slowest_guest.max(t.elapsed());
        steps += 1;
        assert!(!guest.disconnected(), "lost the connection after {steps} steps");
        let have = {
            let t = guest.runtime().tree().borrow();
            t.descendants_of_class(t.workspace(), ClassKind::Part)
                .into_iter()
                .filter_map(|p| t.get_vec3(p, Property::Position))
                .filter(|at| at.y >= 1.0 && near(at.x, at.z))
                .count()
        };
        if have >= want && guest.character().is_some() {
            break;
        }
        assert!(
            began.elapsed() < Duration::from_secs(120),
            "only {have} of {want} parts after {steps} steps"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(slowest_guest < Duration::from_millis(100), "a guest step took {slowest_guest:?}");
    assert!(slowest_host < Duration::from_millis(250), "a host step took {slowest_host:?}");
}

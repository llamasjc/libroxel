//! Whole-game tests: a character walking, a kill brick, respawning.

use libroxel_engine::session::{RESPAWN_DELAY, start_on};
use libroxel_engine::{CharacterInput, LogLevel};
use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue, Vec3};

const DT: f32 = 1.0 / 60.0;

fn part(tree: &mut InstanceTree, class: ClassKind, name: &str, pos: Vec3, size: Vec3) -> InstanceId {
    let id = tree.create(class, name).unwrap();
    tree.set_parent(id, Some(tree.workspace())).unwrap();
    tree.set(id, Property::Position, PropertyValue::Vec3(pos)).unwrap();
    tree.set(id, Property::Size, PropertyValue::Vec3(size)).unwrap();
    tree.set(id, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    id
}

fn script(tree: &mut InstanceTree, name: &str, source: &str) {
    let id = tree.create(ClassKind::Script, name).unwrap();
    tree.set_parent(id, Some(tree.workspace())).unwrap();
    tree.set(id, Property::Source, PropertyValue::String(source.into())).unwrap();
}

fn world() -> InstanceTree {
    let mut tree = InstanceTree::new();
    part(&mut tree, ClassKind::Part, "Floor", Vec3::new(0.0, -0.5, 0.0), Vec3::new(100.0, 1.0, 100.0));
    part(&mut tree, ClassKind::SpawnLocation, "Spawn", Vec3::new(0.0, 0.5, 0.0), Vec3::new(6.0, 1.0, 6.0));
    tree
}

#[test]
fn character_spawns_on_the_spawn_and_walks() {
    let mut session = start_on(world(), true).unwrap();
    let character = session.character().unwrap();
    let tree = session.runtime().tree().clone();
    for _ in 0..30 {
        session.step(DT, CharacterInput::default());
    }
    let start = tree.borrow().get_vec3(character.root, Property::Position).unwrap();
    assert!((start.y - 3.0).abs() < 0.01, "standing on the spawn: {start:?}");

    let forward = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), jump: false };
    for _ in 0..60 {
        session.step(DT, forward);
    }
    let end = tree.borrow().get_vec3(character.root, Property::Position).unwrap();
    assert!((end.x - start.x - 16.0).abs() < 0.5, "walks 16 studs in a second: {start:?} -> {end:?}");

    // Jumping leaves the ground and comes back.
    let jump = CharacterInput { jump: true, ..Default::default() };
    session.step(DT, jump);
    let mut peak: f32 = 0.0;
    for _ in 0..60 {
        session.step(DT, CharacterInput::default());
        peak = peak.max(tree.borrow().get_vec3(character.root, Property::Position).unwrap().y);
    }
    assert!(peak > 5.0, "jumped: peak {peak}");
}

#[test]
fn kill_brick_kills_and_the_character_respawns() {
    let mut tree = world();
    part(&mut tree, ClassKind::Part, "Lava", Vec3::new(10.0, 0.5, 0.0), Vec3::new(4.0, 1.0, 4.0));
    script(
        &mut tree,
        "KillBrick",
        r#"
        local lava = workspace:WaitForChild("Lava")
        lava.Touched:Connect(function(hit)
            local humanoid = hit.Parent and hit.Parent:FindFirstChild("Humanoid")
            if humanoid then
                humanoid.Health = 0
                print("ouch")
            end
        end)
        "#,
    );
    let mut session = start_on(tree, true).unwrap();
    let first = session.character().unwrap();
    let walk = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), jump: false };
    for _ in 0..60 {
        session.step(DT, walk);
    }
    let log = session.take_log();
    assert!(log.iter().any(|l| l.text == "ouch"), "{log:?}");
    assert!(log.iter().any(|l| l.text.contains("died")), "{log:?}");

    let frames = (RESPAWN_DELAY / DT) as usize + 5;
    for _ in 0..frames {
        session.step(DT, CharacterInput::default());
    }
    let second = session.character().unwrap();
    assert_ne!(first.root, second.root, "a new character was built");
    let tree = session.runtime().tree().clone();
    assert!(tree.borrow().is_destroyed(first.model).unwrap());
    let pos = tree.borrow().get_vec3(second.root, Property::Position).unwrap();
    assert!(pos.x.abs() < 0.1, "back at the spawn: {pos:?}");
}

#[test]
fn script_errors_reach_the_log() {
    let mut tree = world();
    script(&mut tree, "Broken", "local x = nil; x.y = 1");
    let mut session = start_on(tree, false).unwrap();
    let log = session.take_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].level, LogLevel::Error);
    assert!(log[0].text.starts_with("Broken:"), "{}", log[0].text);
}

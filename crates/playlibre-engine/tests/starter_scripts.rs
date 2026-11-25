//! Scripts in the starter player's folders, and the force field a spawn
//! location gives.

use playlibre_engine::session::start_on;
use playlibre_engine::{CharacterInput, LogLevel};
use playlibre_instance::{ClassKind, InstanceTree, Property, PropertyValue, Vec3, place};

fn add_script(
    tree: &mut InstanceTree,
    class: ClassKind,
    parent: playlibre_instance::InstanceId,
    source: &str,
) {
    let script = tree.create(class, "Test").unwrap();
    tree.set(script, Property::Source, PropertyValue::String(source.into())).unwrap();
    tree.set_parent(script, Some(parent)).unwrap();
}

fn world() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let floor = tree.create(ClassKind::Part, "Floor").unwrap();
    tree.set(floor, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, -0.5, 0.0))).unwrap();
    tree.set(floor, Property::Size, PropertyValue::Vec3(Vec3::new(100.0, 1.0, 100.0))).unwrap();
    tree.set(floor, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    let ws = tree.workspace();
    tree.set_parent(floor, Some(ws)).unwrap();
    tree
}

fn printed(session: &mut playlibre_engine::PlaySession) -> Vec<String> {
    let log = session.take_log();
    assert!(log.iter().all(|l| l.level != LogLevel::Error), "{log:?}");
    log.into_iter().map(|l| l.text).collect()
}

#[test]
fn every_tree_has_the_starter_folders_and_saving_keeps_just_one_of_each() {
    let mut tree = InstanceTree::new();
    let folder = tree.starter_scripts(ClassKind::StarterPlayerScripts).unwrap();
    add_script(&mut tree, ClassKind::LocalScript, folder, "print('hi')");
    assert!(tree.destroy(folder).is_err(), "the folders stay");
    let text = place::save(&tree).unwrap();
    let (loaded, _) = place::load(&text).unwrap();
    let starter = loaded.service(ClassKind::StarterPlayer).unwrap();
    let kids = loaded.children(starter).unwrap();
    assert_eq!(kids.len(), 2, "no doubled folders");
    let folder = loaded.starter_scripts(ClassKind::StarterPlayerScripts).unwrap();
    assert_eq!(loaded.children(folder).unwrap().len(), 1);
}

#[test]
fn player_scripts_run_once_and_character_scripts_run_on_every_spawn() {
    let mut tree = world();
    let players = tree.starter_scripts(ClassKind::StarterPlayerScripts).unwrap();
    add_script(
        &mut tree,
        ClassKind::LocalScript,
        players,
        "print('player script in ' .. script.Parent.ClassName .. ' of ' .. script.Parent.Parent.Name)",
    );
    let characters = tree.starter_scripts(ClassKind::StarterCharacterScripts).unwrap();
    add_script(
        &mut tree,
        ClassKind::LocalScript,
        characters,
        "print('local character script, health ' .. script.Parent:FindFirstChildOfClass('Humanoid').Health)",
    );
    add_script(
        &mut tree,
        ClassKind::Script,
        characters,
        "print('server character script in ' .. script.Parent.Name)",
    );
    // Instance.new can't make the folders themselves.
    let ws = tree.workspace();
    add_script(
        &mut tree,
        ClassKind::Script,
        ws,
        "print(pcall(function() return Instance.new('StarterPlayerScripts') end))",
    );
    let mut session = start_on(tree, true).unwrap();
    for _ in 0..5 {
        session.step(1.0 / 60.0, CharacterInput::default());
    }
    let first = printed(&mut session);
    let count = |log: &[String], text: &str| log.iter().filter(|l| l.contains(text)).count();
    assert_eq!(count(&first, "player script in PlayerScripts of "), 1, "{first:?}");
    assert_eq!(count(&first, "local character script, health 100"), 1, "{first:?}");
    assert_eq!(count(&first, "server character script in "), 1, "{first:?}");
    assert!(first.iter().any(|l| l.starts_with("false")), "{first:?}");

    // Die, respawn: the character scripts run again, the player script doesn't.
    let tree = session.runtime().tree().clone();
    {
        let mut t = tree.borrow_mut();
        let humanoid = t.descendants_of_class(t.workspace(), ClassKind::Humanoid)[0];
        t.set(humanoid, Property::Health, PropertyValue::Number(0.0)).unwrap();
    }
    for _ in 0..(60 * 7) {
        session.step(1.0 / 60.0, CharacterInput::default());
    }
    let after = printed(&mut session);
    assert_eq!(count(&after, "player script in"), 0, "{after:?}");
    assert_eq!(count(&after, "local character script"), 1, "{after:?}");
    assert_eq!(count(&after, "server character script"), 1, "{after:?}");
}

#[test]
fn a_spawn_shield_stops_damage_then_wears_off() {
    let mut tree = world();
    let spawn = tree.create(ClassKind::SpawnLocation, "Spawn").unwrap();
    tree.set(spawn, Property::Size, PropertyValue::Vec3(Vec3::new(6.0, 1.0, 6.0))).unwrap();
    tree.set(spawn, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    tree.set(spawn, Property::SpawnShield, PropertyValue::Number(1.0)).unwrap();
    let ws = tree.workspace();
    tree.set_parent(spawn, Some(ws)).unwrap();
    add_script(
        &mut tree,
        ClassKind::Script,
        ws,
        r#"
        local player = game.Players:GetPlayers()[1] or game.Players.PlayerAdded:Wait()
        local character = player.Character or player.CharacterAdded:Wait()
        local humanoid = character:WaitForChild("Humanoid")
        print("shielded", character:FindFirstChildOfClass("ForceField") ~= nil)
        humanoid:TakeDamage(30)
        print("health", humanoid.Health)
        task.wait(1.5)
        print("shielded", character:FindFirstChildOfClass("ForceField") ~= nil)
        humanoid:TakeDamage(30)
        print("health", humanoid.Health)
        "#,
    );
    let mut session = start_on(tree, true).unwrap();
    for _ in 0..120 {
        session.step(1.0 / 60.0, CharacterInput::default());
    }
    let log = printed(&mut session);
    let lines: Vec<&String> =
        log.iter().filter(|l| l.starts_with("shielded") || l.starts_with("health")).collect();
    assert_eq!(lines, ["shielded true", "health 100", "shielded false", "health 70"], "{log:?}");
}

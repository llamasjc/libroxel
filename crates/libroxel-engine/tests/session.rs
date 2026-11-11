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

#[test]
fn players_join_get_leaderstats_and_respawn() {
    let mut tree = world();
    part(&mut tree, ClassKind::Part, "Lava", Vec3::new(30.0, 0.5, 0.0), Vec3::new(4.0, 1.0, 4.0));
    script(
        &mut tree,
        "Stats",
        r#"
        local Players = game:GetService("Players")
        Players.PlayerAdded:Connect(function(player)
            local stats = Instance.new("Folder", player)
            stats.Name = "leaderstats"
            local deaths = Instance.new("IntValue", stats)
            deaths.Name = "Deaths"
            player.CharacterAdded:Connect(function(character)
                print("spawned", character.Name)
                character:WaitForChild("Humanoid").Died:Connect(function()
                    deaths.Value += 1
                end)
            end)
        end)
        "#,
    );
    let mut session = start_on(tree, true).unwrap();
    let tree = session.runtime().tree().clone();
    let player = session.player().unwrap();
    let first = session.character().unwrap();
    assert_eq!(tree.borrow().get_ref(player, Property::Character), Some(first.model));

    // Kill the character directly, then wait out the respawn.
    tree.borrow_mut().set(first.humanoid, Property::Health, PropertyValue::Number(0.0)).unwrap();
    for _ in 0..((RESPAWN_DELAY / DT) as usize + 10) {
        session.step(DT, CharacterInput::default());
    }
    let second = session.character().unwrap();
    assert_ne!(second.model, first.model);
    let t = tree.borrow();
    assert_eq!(t.get_ref(player, Property::Character), Some(second.model));
    let stats = t.find_first_child(player, "leaderstats").unwrap().unwrap();
    let deaths = t.find_first_child(stats, "Deaths").unwrap().unwrap();
    assert_eq!(t.get_number(deaths, Property::NumberValue), Some(1.0));
    let log: Vec<String> = session.take_log().into_iter().map(|e| e.text).collect();
    assert_eq!(log.iter().filter(|l| l.as_str() == "spawned Player1").count(), 2, "{log:?}");
}

#[test]
fn guis_are_copied_and_parts_can_be_clicked() {
    let mut tree = world();
    let starter = tree.service(ClassKind::StarterGui).unwrap();
    let screen = tree.create(ClassKind::ScreenGui, "Hud").unwrap();
    tree.set_parent(screen, Some(starter)).unwrap();
    let label = tree.create(ClassKind::TextLabel, "Status").unwrap();
    tree.set_parent(label, Some(screen)).unwrap();
    let local = tree.create(ClassKind::LocalScript, "Show").unwrap();
    tree.set_parent(local, Some(screen)).unwrap();
    let source = r#"
        local label = script.Parent.Status
        label.Text = "Hello " .. game:GetService("Players").LocalPlayer.Name
        workspace.Button.ClickDetector.MouseClick:Connect(function(player)
            label.Text = "Clicked by " .. player.Name
        end)
        workspace.Button.Changed:Connect(function(property)
            if property == "Color" then label.Text = "Recoloured" end
        end)
    "#;
    tree.set(local, Property::Source, PropertyValue::String(source.into())).unwrap();
    let button = part(&mut tree, ClassKind::Part, "Button", Vec3::new(4.0, 1.0, 0.0), Vec3::ONE);
    let detector = tree.create(ClassKind::ClickDetector, "ClickDetector").unwrap();
    tree.set_parent(detector, Some(button)).unwrap();
    let far = part(&mut tree, ClassKind::Part, "Far", Vec3::new(90.0, 1.0, 0.0), Vec3::ONE);
    let far_detector = tree.create(ClassKind::ClickDetector, "ClickDetector").unwrap();
    tree.set_parent(far_detector, Some(far)).unwrap();

    let mut session = start_on(tree, true).unwrap();
    session.step(DT, CharacterInput::default());
    let tree = session.runtime().tree().clone();
    let gui = session.player_gui().unwrap();
    let text = |tree: &InstanceTree| {
        let hud = tree.find_first_child(gui, "Hud").unwrap().unwrap();
        let status = tree.find_first_child(hud, "Status").unwrap().unwrap();
        tree.get_string(status, Property::Text).unwrap()
    };
    assert_eq!(text(&tree.borrow()), "Hello Player1");
    assert!(session.click_part(far), "clickable, but too far away");
    assert_eq!(text(&tree.borrow()), "Hello Player1");
    assert!(session.click_part(button));
    assert_eq!(text(&tree.borrow()), "Clicked by Player1");
    let floor = tree.borrow().find_first_child(tree.borrow().workspace(), "Floor").unwrap().unwrap();
    assert!(!session.click_part(floor), "no click detector");

    tree.borrow_mut()
        .set(button, Property::Color, PropertyValue::Color3(libroxel_instance::Color3::WHITE))
        .unwrap();
    session.step(DT, CharacterInput::default());
    assert_eq!(text(&tree.borrow()), "Recoloured");
    let errors: Vec<_> = session.take_log().into_iter().filter(|l| l.level == LogLevel::Error).collect();
    assert!(errors.is_empty(), "{errors:?}");
}

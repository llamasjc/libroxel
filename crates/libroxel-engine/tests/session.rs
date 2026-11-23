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
    let spawn = part(
        &mut tree,
        ClassKind::SpawnLocation,
        "Spawn",
        Vec3::new(0.0, 0.5, 0.0),
        Vec3::new(6.0, 1.0, 6.0),
    );
    tree.set(spawn, Property::SpawnShield, PropertyValue::Number(0.0)).unwrap();
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

    let forward = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), jump: false, ..Default::default() };
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
    let walk = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), jump: false, ..Default::default() };
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

#[test]
fn launch_pads_explosions_and_the_camera() {
    let mut tree = world();
    let crate_id =
        part(&mut tree, ClassKind::Part, "Crate", Vec3::new(20.0, 1.0, 0.0), Vec3::new(2.0, 2.0, 2.0));
    tree.set(crate_id, Property::Anchored, PropertyValue::Bool(false)).unwrap();
    script(
        &mut tree,
        "Pad",
        r#"
        local Players = game:GetService("Players")
        print("camera", workspace.CurrentCamera ~= nil)
        Players.PlayerAdded:Connect(function(player)
            player.CharacterAdded:Connect(function(character)
                task.wait(0.5)
                character.HumanoidRootPart.AssemblyLinearVelocity = Vector3.new(0, 120, 0)
                task.wait(1.5)
                local boom = Instance.new("Explosion")
                boom.Position = workspace.Crate.Position + Vector3.new(-2, 0, 0)
                boom.BlastRadius = 3
                boom.Hit:Connect(function(part) print("hit", part.Name) end)
                boom.Parent = workspace
            end)
        end)
        "#,
    );
    let mut session = start_on(tree, true).unwrap();
    let character = session.character().unwrap();
    let tree = session.runtime().tree().clone();
    assert!(session.camera().is_some());
    let mut peak: f32 = 0.0;
    for _ in 0..90 {
        session.step(DT, CharacterInput::default());
        peak = peak.max(tree.borrow().get_vec3(character.root, Property::Position).unwrap().y);
    }
    assert!(peak > 30.0, "launched: peak {peak}");
    for _ in 0..40 {
        session.step(DT, CharacterInput::default());
    }
    let moved = tree.borrow().get_vec3(crate_id, Property::Position).unwrap();
    assert!(moved.x > 21.0, "the blast threw the crate away: {moved:?}");
    let log: Vec<String> = session.take_log().into_iter().map(|l| l.text).collect();
    assert!(log.iter().any(|l| l == "camera true"), "{log:?}");
    assert!(log.iter().any(|l| l == "hit Crate"), "{log:?}");
    assert!(!log.iter().any(|l| l.contains("died")), "the character was out of reach: {log:?}");
}

#[test]
fn npcs_walk_where_they_are_sent_and_teams_pick_spawns() {
    let mut tree = world();
    // A second spawn for the red team, away from the neutral one.
    let red_spawn = part(
        &mut tree,
        ClassKind::SpawnLocation,
        "RedSpawn",
        Vec3::new(30.0, 0.5, 0.0),
        Vec3::new(6.0, 1.0, 6.0),
    );
    tree.set(red_spawn, Property::Neutral, PropertyValue::Bool(false)).unwrap();
    let red = libroxel_instance::Color3::new(1.0, 0.0, 0.0);
    tree.set(red_spawn, Property::TeamColor, PropertyValue::Color3(red)).unwrap();
    let teams = tree.service(ClassKind::Teams).unwrap();
    let team = tree.create(ClassKind::Team, "Red").unwrap();
    tree.set_parent(team, Some(teams)).unwrap();
    tree.set(team, Property::TeamColor, PropertyValue::Color3(red)).unwrap();
    script(
        &mut tree,
        "GuardScript",
        r#"
        local guard = Instance.new("Model")
        guard.Name = "Guard"
        local root = Instance.new("Part", guard)
        root.Name = "HumanoidRootPart"
        root.Size = Vector3.new(2, 4, 1)
        root.Position = Vector3.new(-20, 3, 0)
        local head = Instance.new("Part", guard)
        head.Name = "Head"
        head.Size = Vector3.new(1, 1, 1)
        head.Position = Vector3.new(-20, 5.5, 0)
        local humanoid = Instance.new("Humanoid", guard)
        guard.Parent = workspace
        humanoid.MoveToFinished:Connect(function(reached)
            print("arrived", reached)
        end)
        humanoid.Died:Connect(function() print("guard down") end)
        game:GetService("Players").PlayerAdded:Connect(function(player)
            print("team", player.Team and player.Team.Name, player.Neutral, #game:GetService("Teams").Red:GetPlayers())
        end)
        task.wait(0.5)
        humanoid:MoveTo(Vector3.new(-10, 0, 0))
        "#,
    );
    let mut session = start_on(tree, true).unwrap();
    let character = session.character().unwrap();
    let tree = session.runtime().tree().clone();
    let at = tree.borrow().get_vec3(character.root, Property::Position).unwrap();
    assert!((at.x - 30.0).abs() < 0.01, "spawned on the team's spawn: {at:?}");
    for _ in 0..150 {
        session.step(DT, CharacterInput::default());
    }
    let (root, head) = {
        let t = tree.borrow();
        let guard = t.find_first_child(t.workspace(), "Guard").unwrap();
        let guard = guard.unwrap();
        (
            t.find_first_child(guard, "HumanoidRootPart").unwrap().unwrap(),
            t.find_first_child(guard, "Head").unwrap().unwrap(),
        )
    };
    let pos = tree.borrow().get_vec3(root, Property::Position).unwrap();
    let head_pos = tree.borrow().get_vec3(head, Property::Position).unwrap();
    assert!((pos.x + 10.0).abs() < 1.5, "walked over: {pos:?}");
    assert!((head_pos.x - pos.x).abs() < 0.01 && head_pos.y > pos.y, "the head came along: {head_pos:?}");
    session.runtime().run("kill", "workspace.Guard.Humanoid.Health = 0").unwrap();
    session.step(DT, CharacterInput::default());
    session.step(DT, CharacterInput::default());
    let log: Vec<String> = session.take_log().into_iter().map(|l| l.text).collect();
    assert!(log.iter().any(|l| l == "team Red false 1"), "{log:?}");
    assert!(log.iter().any(|l| l == "arrived true"), "{log:?}");
    assert!(log.iter().any(|l| l == "guard down"), "{log:?}");
}

#[test]
fn welds_events_gravity_and_loading_characters() {
    let mut tree = world();
    script(
        &mut tree,
        "Setup",
        r#"
        local players = game:GetService("Players")
        players.RespawnTime = 0.5
        players.PlayerAdded:Connect(function(player)
            player.CharacterRemoving:Connect(function(c) print("removing character", c.Name) end)
            player.CharacterAdded:Connect(function(c)
                c:WaitForChild("Humanoid").HealthChanged:Connect(function(h) print("health", h) end)
            end)
        end)
        -- A cart: a loose base with a welded seat that rides along.
        local base = Instance.new("Part")
        base.Name = "Base"
        base.Size = Vector3.new(4, 1, 4)
        base.Position = Vector3.new(20, 5, 0)
        base.Anchored = false
        base.Parent = workspace
        local seat = Instance.new("Part")
        seat.Name = "Seat"
        seat.Size = Vector3.new(2, 1, 2)
        seat.Position = Vector3.new(20, 6, 0)
        seat.Anchored = false
        seat.Parent = workspace
        local weld = Instance.new("WeldConstraint")
        weld.Part0 = base
        weld.Part1 = seat
        weld.Parent = base

        local folder = Instance.new("Folder", workspace)
        folder.Name = "Bin"
        folder.DescendantAdded:Connect(function(d) print("added", d.Name) end)
        folder.DescendantRemoving:Connect(function(d) print("removing", d.Name) end)
        folder.ChildRemoved:Connect(function(c) print("child removed", c.Name) end)
        local box = Instance.new("Model")
        box.Name = "Box"
        local inner = Instance.new("Part", box)
        inner.Name = "Inner"
        inner.Anchored = true
        inner.Position = Vector3.new(-30, 1, -30)
        box.Destroying:Connect(function() print("box destroying") end)
        task.wait(0.1)
        box.Parent = folder
        task.wait(0.1)
        box:Destroy()

        "#,
    );
    let mut session = start_on(tree, true).unwrap();
    let tree = session.runtime().tree().clone();
    for _ in 0..60 {
        session.step(DT, CharacterInput::default());
    }
    let find = |name: &str| {
        let t = tree.borrow();
        t.find_first_child(t.workspace(), name).unwrap().unwrap()
    };
    let (base, seat) = (find("Base"), find("Seat"));
    let base_pos = tree.borrow().get_vec3(base, Property::Position).unwrap();
    let seat_pos = tree.borrow().get_vec3(seat, Property::Position).unwrap();
    assert!((base_pos.y - 0.5).abs() < 0.05, "the base fell to the floor: {base_pos:?}");
    assert!(
        (seat_pos - base_pos - Vec3::new(0.0, 1.0, 0.0)).length() < 0.01,
        "the seat rode along: {seat_pos:?}"
    );

    // Pushing the base moves the whole cart.
    session.runtime().run("push", "workspace.Base.AssemblyLinearVelocity = Vector3.new(10, 0, 0)").unwrap();
    for _ in 0..10 {
        session.step(DT, CharacterInput::default());
    }
    let base_pos = tree.borrow().get_vec3(base, Property::Position).unwrap();
    let seat_pos = tree.borrow().get_vec3(seat, Property::Position).unwrap();
    // It slides a little way before rubbing on the floor stops it.
    assert!(base_pos.x > 20.3, "pushed: {base_pos:?}");
    assert!((seat_pos.x - base_pos.x).abs() < 0.01, "together: {seat_pos:?}");

    // Low gravity: a jump goes much higher.
    session.runtime().run("moon", "workspace.Gravity = 30").unwrap();
    let character = session.character().unwrap();
    session.step(DT, CharacterInput { jump: true, ..Default::default() });
    let mut peak: f32 = 0.0;
    for _ in 0..90 {
        session.step(DT, CharacterInput::default());
        peak = peak.max(tree.borrow().get_vec3(character.root, Property::Position).unwrap().y);
    }
    assert!(peak > 20.0, "floaty jump: {peak}");
    session.runtime().run("earth", "workspace.Gravity = 196.2").unwrap();

    // LoadCharacter swaps the character at once; dying respawns after RespawnTime.
    session.runtime().run("hurt", "workspace.Player1.Humanoid:TakeDamage(10)").unwrap();
    session.step(DT, CharacterInput::default());
    session.runtime().run("reload", "game:GetService('Players').Player1:LoadCharacter()").unwrap();
    session.step(DT, CharacterInput::default());
    let fresh = session.character().unwrap();
    assert_ne!(fresh.model, character.model);
    assert!(!tree.borrow().exists(character.model) || tree.borrow().is_destroyed(character.model).unwrap());
    session.runtime().run("kill", "workspace.Player1.Humanoid.Health = 0").unwrap();
    for _ in 0..40 {
        session.step(DT, CharacterInput::default());
    }
    assert_ne!(session.character().unwrap().model, fresh.model, "respawned after half a second");

    let log: Vec<String> = session.take_log().into_iter().map(|l| l.text).collect();
    for line in [
        "added Box",
        "added Inner",
        "removing Box",
        "child removed Box",
        "box destroying",
        "health 90",
        "removing character Player1",
        "health 0",
    ] {
        assert!(log.iter().any(|l| l == line), "missing {line:?} in {log:?}");
    }
}

#[test]
fn characters_wait_for_load_character_when_auto_loading_is_off() {
    let mut tree = world();
    let players = tree.service(ClassKind::Players).unwrap();
    tree.set(players, Property::CharacterAutoLoads, PropertyValue::Bool(false)).unwrap();
    script(
        &mut tree,
        "Lobby",
        r#"
        game:GetService("Players").PlayerAdded:Connect(function(player)
            task.wait(0.5)
            player:LoadCharacter()
        end)
        "#,
    );
    let mut session = start_on(tree, true).unwrap();
    assert!(session.character().is_none(), "no character until a script loads one");
    for _ in 0..40 {
        session.step(DT, CharacterInput::default());
    }
    assert!(session.character().is_some());
    session.runtime().run("kill", "workspace.Player1.Humanoid.Health = 0").unwrap();
    for _ in 0..((RESPAWN_DELAY / DT) as usize + 10) {
        session.step(DT, CharacterInput::default());
    }
    let t = session.runtime().tree().clone();
    let c = session.character().unwrap();
    assert_eq!(
        t.borrow().get_number(c.humanoid, Property::Health),
        Some(0.0),
        "stays dead without auto-loading"
    );
}

#[test]
fn the_first_character_waits_so_scripts_can_turn_auto_loading_off() {
    // A game that turns auto-loading off a moment after starting (after a
    // require or two) and spawns the character itself, once.
    let mut tree = world();
    script(
        &mut tree,
        "Join",
        r#"
        local Players = game:GetService("Players")
        task.wait(0.1)
        Players.CharacterAutoLoads = false
        local spawned = 0
        for _, p in ipairs(Players:GetPlayers()) do
            p.CharacterAdded:Connect(function() spawned += 1; print("spawned", spawned) end)
            task.wait(0.2)
            p:LoadCharacter()
        end
        "#,
    );
    let shared = std::rc::Rc::new(std::cell::RefCell::new(tree));
    let mut session = libroxel_engine::PlaySession::start(shared, true).unwrap();
    assert!(session.character().is_none(), "not right away");
    for _ in 0..120 {
        session.step(DT, CharacterInput::default());
    }
    let log: Vec<String> = session.take_log().into_iter().map(|l| l.text).collect();
    assert!(log.iter().any(|l| l == "spawned 1"), "{log:?}");
    assert!(!log.iter().any(|l| l == "spawned 2"), "only the script's character: {log:?}");

    // Left on, the character comes by itself a moment after joining.
    let shared = std::rc::Rc::new(std::cell::RefCell::new(world()));
    let mut session = libroxel_engine::PlaySession::start(shared, true).unwrap();
    for _ in 0..40 {
        session.step(DT, CharacterInput::default());
    }
    assert!(session.character().is_some());
}

#[test]
fn sitting_and_driving_a_kart() {
    let mut tree = world();
    script(
        &mut tree,
        "KartMaker",
        r#"
        local kart = Instance.new("Model")
        kart.Name = "Kart"
        local chassis = Instance.new("Part", kart)
        chassis.Name = "Chassis"
        chassis.Size = Vector3.new(4, 1, 4)
        chassis.Position = Vector3.new(0, 0.5, -20)
        chassis.Anchored = false
        local seat = Instance.new("VehicleSeat", kart)
        seat.Size = Vector3.new(2, 1, 2)
        seat.Position = Vector3.new(0, 1.5, -20)
        seat.Anchored = false
        local weld = Instance.new("WeldConstraint", chassis)
        weld.Part0 = chassis
        weld.Part1 = seat
        kart.Parent = workspace

        local bench = Instance.new("Seat")
        bench.Name = "Bench"
        bench.Anchored = true
        bench.Size = Vector3.new(2, 1, 2)
        bench.Position = Vector3.new(15, 0.5, 0)
        bench.Parent = workspace

        game:GetService("Players").PlayerAdded:Connect(function(player)
            player.CharacterAdded:Connect(function(character)
                character:WaitForChild("Humanoid").Seated:Connect(function(active, part)
                    print("seated", active, part and part.Name)
                end)
            end)
        end)
        "#,
    );
    let mut session = start_on(tree, true).unwrap();
    let tree = session.runtime().tree().clone();
    let character = session.character().unwrap();
    let find = |name: &str| {
        let t = tree.borrow();
        t.find_first_child(t.workspace(), name).unwrap().unwrap()
    };
    let idle = CharacterInput::default();
    for _ in 0..10 {
        session.step(DT, idle);
    }
    // Walk onto the bench: sitting holds the character on it.
    let east = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), ..Default::default() };
    for _ in 0..90 {
        session.step(DT, east);
    }
    let bench = find("Bench");
    assert_eq!(tree.borrow().get_ref(bench, Property::Occupant), Some(character.humanoid));
    assert_eq!(tree.borrow().get_bool(character.humanoid, Property::Sit), Some(true));
    let sat = tree.borrow().get_vec3(character.root, Property::Position).unwrap();
    assert!((sat.x - 15.0).abs() < 0.01 && (sat.y - 1.0).abs() < 0.01, "on the bench: {sat:?}");

    // Jumping gets up.
    session.step(DT, CharacterInput { jump: true, ..Default::default() });
    for _ in 0..10 {
        session.step(DT, idle);
    }
    assert_eq!(tree.borrow().get_ref(bench, Property::Occupant), None);
    assert_eq!(tree.borrow().get_bool(character.humanoid, Property::Sit), Some(false));

    // A script puts the character in the kart; the forward key drives it.
    session.runtime().run("drive", "workspace.Kart.VehicleSeat:Sit(workspace.Player1.Humanoid)").unwrap();
    session.step(DT, idle);
    let kart = find("Kart");
    let (chassis, seat) = {
        let t = tree.borrow();
        (
            t.find_first_child(kart, "Chassis").unwrap().unwrap(),
            t.find_first_child(kart, "VehicleSeat").unwrap().unwrap(),
        )
    };
    let start = tree.borrow().get_vec3(chassis, Property::Position).unwrap();
    let gas = CharacterInput { throttle: 1.0, ..Default::default() };
    for _ in 0..60 {
        session.step(DT, gas);
    }
    let after = tree.borrow().get_vec3(chassis, Property::Position).unwrap();
    assert!(after.z < start.z - 12.0, "drove forward (toward -Z): {start:?} -> {after:?}");
    assert_eq!(tree.borrow().get_number(seat, Property::Throttle), Some(1.0));
    let rider = tree.borrow().get_vec3(character.root, Property::Position).unwrap();
    let seat_pos = tree.borrow().get_vec3(seat, Property::Position).unwrap();
    assert!(
        (rider.x - seat_pos.x).abs() < 0.01 && (rider.z - seat_pos.z).abs() < 0.01,
        "rode along: {rider:?} {seat_pos:?} {:?}",
        tree.borrow().get_vec3(seat, Property::Orientation)
    );

    // Steering right turns it clockwise (its yaw goes down).
    let turn = CharacterInput { throttle: 1.0, steer: 1.0, ..Default::default() };
    for _ in 0..60 {
        session.step(DT, turn);
    }
    let yaw = tree.borrow().get_vec3(seat, Property::Orientation).unwrap().y;
    assert!(yaw < -20.0, "turned right: {yaw}");

    // Setting Sit to false gets out, and the seat's controls reset.
    session.runtime().run("out", "workspace.Player1.Humanoid.Sit = false").unwrap();
    session.step(DT, idle);
    assert_eq!(tree.borrow().get_ref(seat, Property::Occupant), None);
    assert_eq!(tree.borrow().get_number(seat, Property::Throttle), Some(0.0));

    let log: Vec<String> = session.take_log().into_iter().map(|l| l.text).collect();
    for line in ["seated true Bench", "seated false nil", "seated true VehicleSeat"] {
        assert!(log.iter().any(|l| l == line), "missing {line:?} in {log:?}");
    }
    assert!(session.runtime().run("bad", "workspace.Bench.Occupant = nil").is_err());
}

#[test]
fn a_respawned_guis_old_script_stops() {
    let mut tree = world();
    let starter = tree.service(ClassKind::StarterGui).unwrap();
    let gui = tree.create(ClassKind::ScreenGui, "HUD").unwrap();
    tree.set_parent(gui, Some(starter)).unwrap();
    let frame = tree.create(ClassKind::Frame, "Help").unwrap();
    tree.set_parent(frame, Some(gui)).unwrap();
    let local = tree.create(ClassKind::LocalScript, "HudScript").unwrap();
    tree.set_parent(local, Some(gui)).unwrap();
    let source = r#"
        local hud = script.Parent
        print("hud started")
        game:GetService("UserInputService").InputBegan:Connect(function()
            hud.Help.Visible = not hud.Help.Visible
        end)
        while true do
            task.wait(0.1)
            local _ = hud.Help
        end
    "#;
    tree.set(local, Property::Source, PropertyValue::String(source.into())).unwrap();
    let players = tree.service(ClassKind::Players).unwrap();
    tree.set(players, Property::RespawnTime, PropertyValue::Number(0.2)).unwrap();
    let mut session = start_on(tree, true).unwrap();
    for _ in 0..10 {
        session.step(DT, CharacterInput::default());
    }
    session.runtime().run("kill", "workspace.Player1.Humanoid.Health = 0").unwrap();
    for _ in 0..60 {
        session.step(DT, CharacterInput::default());
    }
    session.input_began(libroxel_script::UserInput::Key(libroxel_instance::KeyCode::H), false);
    for _ in 0..30 {
        session.step(DT, CharacterInput::default());
    }
    let log = session.take_log();
    let errors: Vec<_> = log.iter().filter(|l| l.level == LogLevel::Error).collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(log.iter().filter(|l| l.text == "hud started").count(), 2, "{log:?}");
}

#[test]
fn starter_player_sets_speeds_and_the_fall_height_is_the_workspaces() {
    let mut tree = world();
    let starter = tree.service(ClassKind::StarterPlayer).unwrap();
    tree.set(starter, Property::CharacterWalkSpeed, PropertyValue::Number(30.0)).unwrap();
    tree.set(starter, Property::CharacterJumpPower, PropertyValue::Number(80.0)).unwrap();
    let ws = tree.workspace();
    tree.set(ws, Property::FallenPartsDestroyHeight, PropertyValue::Number(-20.0)).unwrap();
    let rock = part(&mut tree, ClassKind::Part, "Rock", Vec3::new(200.0, -15.0, 0.0), Vec3::ONE);
    tree.set(rock, Property::Anchored, PropertyValue::Bool(false)).unwrap();
    let mut session = start_on(tree, true).unwrap();
    let character = session.character().unwrap();
    let tree = session.runtime().tree().clone();
    assert_eq!(tree.borrow().get_number(character.humanoid, Property::WalkSpeed), Some(30.0));
    assert_eq!(tree.borrow().get_number(character.humanoid, Property::JumpPower), Some(80.0));
    for _ in 0..60 {
        session.step(DT, CharacterInput::default());
    }
    assert!(tree.borrow().is_destroyed(rock).unwrap_or(true), "the rock fell below -20 and was removed");
}

#[test]
fn the_pointer_entering_and_leaving_gui_objects_fires_events() {
    let mut tree = world();
    let starter = tree.service(ClassKind::StarterGui).unwrap();
    let screen = tree.create(ClassKind::ScreenGui, "Menu").unwrap();
    tree.set_parent(screen, Some(starter)).unwrap();
    let panel = tree.create(ClassKind::Frame, "Panel").unwrap();
    tree.set_parent(panel, Some(screen)).unwrap();
    let button = tree.create(ClassKind::TextButton, "Play").unwrap();
    tree.set_parent(button, Some(panel)).unwrap();
    let local = tree.create(ClassKind::LocalScript, "Hover").unwrap();
    tree.set_parent(local, Some(screen)).unwrap();
    let source = r#"
        local panel = script.Parent.Panel
        for _, g in { panel, panel.Play } do
            g.MouseEnter:Connect(function() print("enter " .. g.Name) end)
            g.MouseLeave:Connect(function() print("leave " .. g.Name) end)
        end
    "#;
    tree.set(local, Property::Source, PropertyValue::String(source.into())).unwrap();
    let mut session = start_on(tree, true).unwrap();
    session.step(DT, CharacterInput::default());
    let gui = session.player_gui().unwrap();
    let (panel, button) = {
        let t = session.runtime().tree().clone();
        let t = t.borrow();
        let menu = t.find_first_child(gui, "Menu").unwrap().unwrap();
        let panel = t.find_first_child(menu, "Panel").unwrap().unwrap();
        (panel, t.find_first_child(panel, "Play").unwrap().unwrap())
    };
    session.take_log();
    session.gui_hover(Some(button));
    session.gui_hover(Some(button));
    session.gui_hover(Some(panel));
    session.gui_hover(None);
    let printed: Vec<_> = session.take_log().into_iter().map(|l| l.text).collect();
    assert_eq!(printed, ["enter Panel", "enter Play", "leave Play", "leave Panel"]);
}

#[test]
fn sounds_play_stop_and_end() {
    let mut tree = world();
    let bell = part(&mut tree, ClassKind::Part, "Bell", Vec3::new(0.0, 5.0, 0.0), Vec3::new(2.0, 2.0, 2.0));
    let ding = tree.create(ClassKind::Sound, "Ding").unwrap();
    tree.set(ding, Property::SoundId, PropertyValue::String("libroxel://chime".into())).unwrap();
    tree.set_parent(ding, Some(bell)).unwrap();
    script(
        &mut tree,
        "Ring",
        r#"
        local ding = workspace.Bell.Ding
        assert(ding.Volume == 0.5 and ding.PlaybackSpeed == 1 and not ding.Playing and not ding.Looped)
        ding.Ended:Connect(function(id) print("ended " .. id) end)
        ding:Play()
        assert(ding.Playing)
        ding:Pause()
        assert(not ding.Playing)
        ding:Resume()
        print("playing", ding.Playing)
        "#,
    );
    let mut session = start_on(tree, false).unwrap();
    let shared = session.runtime().tree().clone();
    session.step(DT, CharacterInput::default());
    assert_eq!(session.take_sound_restarts(), vec![ding]);
    assert!(session.take_sound_restarts().is_empty());
    assert_eq!(shared.borrow().get_bool(ding, Property::Playing), Some(true));

    // What the renderer hears: a sound in a part, quieter far away.
    let heard = libroxel_engine::sounds::audible_sounds(
        &shared.borrow(),
        Some(bevy::math::Vec3::new(0.0, 5.0, 40.0)),
    );
    assert_eq!(heard.len(), 1);
    assert!(heard[0].1.playing && heard[0].1.volume < 0.5 * 0.5 && heard[0].1.volume > 0.0);

    session.sound_ended(ding);
    assert_eq!(shared.borrow().get_bool(ding, Property::Playing), Some(false));
    let log: Vec<String> = session.take_log().into_iter().map(|e| e.text).collect();
    assert!(log.contains(&"playing true".to_string()), "{log:?}");
    assert!(log.contains(&"ended libroxel://chime".to_string()), "{log:?}");
}

#[test]
fn using_a_tool_swings_the_arm() {
    let mut tree = world();
    let pack = tree.service(ClassKind::StarterPack).unwrap();
    let sword = tree.create(ClassKind::Tool, "Sword").unwrap();
    tree.set_parent(sword, Some(pack)).unwrap();
    let handle = tree.create(ClassKind::Part, "Handle").unwrap();
    tree.set(handle, Property::Size, PropertyValue::Vec3(Vec3::new(0.4, 0.4, 3.0))).unwrap();
    tree.set_parent(handle, Some(sword)).unwrap();
    let mut session = start_on(tree, true).unwrap();
    let shared = session.runtime().tree().clone();
    for _ in 0..30 {
        session.step(DT, CharacterInput::default());
    }
    session.toggle_tool(0);
    session.step(DT, CharacterInput::default());
    let character = session.character().unwrap();
    let held = |t: &InstanceTree| {
        let tool = t.find_first_child_of_class(character.model, ClassKind::Tool).unwrap().unwrap();
        let handle = t.find_first_child(tool, "Handle").unwrap().unwrap();
        let root = t.get_vec3(character.root, Property::Position).unwrap();
        (
            t.get_vec3(handle, Property::Position).unwrap().y - root.y,
            t.get_vec3(handle, Property::Orientation).unwrap().x,
        )
    };
    let (rest, tilt) = held(&shared.borrow());
    assert!((rest - 1.6).abs() < 0.05 && tilt.abs() < 0.01, "{rest} {tilt}");

    session.input_began(libroxel_script::UserInput::MouseButton1, false);
    let mut highest = rest;
    let mut lowest = rest;
    for _ in 0..30 {
        session.step(DT, CharacterInput::default());
        let (y, _) = held(&shared.borrow());
        highest = highest.max(y);
        lowest = lowest.min(y);
    }
    session.input_ended(libroxel_script::UserInput::MouseButton1, false);
    assert!(highest > rest + 1.0, "raised to {highest}");
    assert!(lowest < rest - 1.0, "brought down to {lowest}");
    // Back to holding it out.
    let (y, tilt) = held(&shared.borrow());
    assert!((y - rest).abs() < 0.05 && tilt.abs() < 0.01, "{y} {tilt}");
}

#[test]
fn characters_sound_when_they_jump() {
    let mut session = start_on(world(), true).unwrap();
    let shared = session.runtime().tree().clone();
    for _ in 0..60 {
        session.step(DT, CharacterInput::default());
    }
    let root = session.character().unwrap().root;
    let jumping = shared.borrow().find_first_child(root, "Jumping").unwrap().unwrap();
    assert!(session.take_sound_restarts().is_empty(), "standing still is quiet");
    let mut heard = Vec::new();
    for i in 0..30 {
        let jump = CharacterInput { jump: i == 0, ..Default::default() };
        session.step(DT, jump);
        heard.extend(session.take_sound_restarts());
    }
    assert_eq!(heard, vec![jumping]);
    assert_eq!(shared.borrow().get_bool(jumping, Property::Playing), Some(true));
}

#[test]
fn characters_have_r6_joints_that_scripts_can_turn() {
    let mut tree = world();
    script(
        &mut tree,
        "Pose",
        r#"
        local Players = game:GetService("Players")
        local function pose(character)
            local torso = character:WaitForChild("Torso")
            local rs = torso:WaitForChild("Right Shoulder")
            assert(rs:IsA("Motor6D") and rs.Part0 == torso and rs.Part1 == character["Right Arm"])
            for _, name in { "Left Shoulder", "Left Hip", "Right Hip", "Neck" } do
                assert(torso:FindFirstChild(name), name)
            end
            assert(character.HumanoidRootPart:FindFirstChild("RootJoint"))
            assert(character.Humanoid.RigType == Enum.HumanoidRigType.R6, tostring(character.Humanoid.RigType))
            rs.C0 = rs.C0 * CFrame.Angles(0, 0, math.rad(90))
            print("posed")
        end
        Players.PlayerAdded:Connect(function(p)
            p.CharacterAdded:Connect(pose)
            if p.Character then pose(p.Character) end
        end)
        for _, p in Players:GetPlayers() do
            if p.Character then pose(p.Character) end
        end
        "#,
    );
    let mut session = start_on(tree, true).unwrap();
    let character = session.character().unwrap();
    let tree = session.runtime().tree().clone();
    for _ in 0..30 {
        session.step(DT, CharacterInput::default());
    }
    let log: Vec<String> = session.take_log().into_iter().map(|l| l.text).collect();
    assert!(log.iter().any(|l| l == "posed"), "{log:?}");
    let t = tree.borrow();
    let arm = t.find_first_child(character.model, "Right Arm").unwrap().unwrap();
    let torso = t.find_first_child(character.model, "Torso").unwrap().unwrap();
    let a = t.get_vec3(arm, Property::Position).unwrap();
    let b = t.get_vec3(torso, Property::Position).unwrap();
    // The arm points straight forward (-Z) at shoulder height, to the right.
    assert!(a.z - b.z < -0.6, "arm forward: arm {a:?} torso {b:?}");
    assert!(a.y - b.y > 0.5, "arm up at the shoulder: arm {a:?} torso {b:?}");
    assert!(a.x - b.x > 1.0, "arm on the right: arm {a:?} torso {b:?}");
}

#[test]
fn each_key_uses_the_nearest_prompt_for_that_key() {
    let mut tree = world();
    let near = part(&mut tree, ClassKind::Part, "Near", Vec3::new(3.0, 1.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
    let far = part(&mut tree, ClassKind::Part, "Far", Vec3::new(-6.0, 1.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
    for (on, name) in [(near, "E"), (far, "H")] {
        let p = tree.create(ClassKind::ProximityPrompt, name).unwrap();
        tree.set_parent(p, Some(on)).unwrap();
    }
    script(
        &mut tree,
        "Keys",
        r#"
        workspace.Far.H.KeyboardKeyCode = Enum.KeyCode.H
        game:GetService("ProximityPromptService").PromptTriggered:Connect(function(p) print("used " .. p.Name) end)
        "#,
    );
    let mut session = start_on(tree, true).unwrap();
    for _ in 0..30 {
        session.step(DT, CharacterInput::default());
    }
    assert_eq!(session.active_prompts().len(), 2);
    session.input_began(libroxel_script::UserInput::Key(libroxel_instance::KeyCode::H), false);
    session.input_ended(libroxel_script::UserInput::Key(libroxel_instance::KeyCode::H), false);
    session.step(DT, CharacterInput::default());
    let log: Vec<String> = session.take_log().into_iter().map(|l| l.text).collect();
    assert!(log.iter().any(|l| l == "used H"), "{log:?}");
    assert!(!log.iter().any(|l| l == "used E"), "{log:?}");
}

#[test]
fn reset_kills_the_character_and_it_respawns() {
    let mut session = start_on(world(), true).unwrap();
    let first = session.character().unwrap();
    let tree = session.runtime().tree().clone();
    session.step(DT, CharacterInput::default());
    assert!(session.can_reset());
    session.reset_character();
    session.step(DT, CharacterInput::default());
    assert_eq!(tree.borrow().get_number(first.humanoid, Property::Health), Some(0.0));
    for _ in 0..((RESPAWN_DELAY + 1.0) / DT) as usize {
        session.step(DT, CharacterInput::default());
    }
    assert_ne!(session.character().unwrap().model, first.model);
}

#[test]
fn reset_can_be_turned_off() {
    let mut tree = world();
    script(&mut tree, "NoReset", r#"game:GetService("StarterGui"):SetCore("ResetButtonCallback", false)"#);
    let mut session = start_on(tree, true).unwrap();
    session.step(DT, CharacterInput::default());
    assert!(!session.can_reset());
}

#[test]
fn walking_is_as_fast_at_a_low_frame_rate() {
    for dt in [1.0 / 60.0, 1.0 / 30.0, 1.0 / 15.0] {
        let mut session = start_on(world(), true).unwrap();
        let character = session.character().unwrap();
        let tree = session.runtime().tree().clone();
        for _ in 0..30 {
            session.step(DT, CharacterInput::default());
        }
        let forward = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), ..Default::default() };
        for _ in 0..10 {
            session.step(dt, forward);
        }
        let start = tree.borrow().get_vec3(character.root, Property::Position).unwrap();
        let frames = (1.0 / dt).round() as usize;
        for _ in 0..frames {
            session.step(dt, forward);
        }
        let end = tree.borrow().get_vec3(character.root, Property::Position).unwrap();
        let speed = (end.x - start.x) / (frames as f32 * dt);
        assert!((speed - 16.0).abs() < 0.2, "{speed} at {dt}");
    }
}

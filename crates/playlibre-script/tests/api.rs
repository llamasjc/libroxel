//! End-to-end tests of the script-facing API.

use playlibre_instance::{ClassKind, Property, PropertyValue, Vec3};
use playlibre_script::{OutputLevel, OutputLine, ScriptRuntime};

fn runtime() -> ScriptRuntime {
    ScriptRuntime::new_with_tree(Default::default()).expect("runtime starts")
}

fn run(rt: &ScriptRuntime, src: &str) {
    if let Err(err) = rt.run("test", src) {
        panic!("script failed: {err}");
    }
}

fn printed(rt: &ScriptRuntime) -> Vec<String> {
    rt.take_output().into_iter().map(|l| l.text).collect()
}

#[test]
fn creates_and_parents_parts() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local part = Instance.new("Part")
        part.Name = "Brick"
        part.Position = Vector3.new(1, 2, 3)
        part.Parent = workspace
        print(part.ClassName, part.Parent == workspace, workspace.Brick == part)
        print(part:GetFullName())
    "#,
    );
    assert_eq!(printed(&rt), vec!["Part true true", "Workspace.Brick"]);

    let tree = rt.tree().borrow();
    let brick = tree.find_first_child(tree.workspace(), "Brick").unwrap().unwrap();
    assert_eq!(tree.class(brick).unwrap(), ClassKind::Part);
    assert_eq!(tree.get(brick, Property::Position).unwrap(), PropertyValue::Vec3(Vec3::new(1.0, 2.0, 3.0)));
}

#[test]
fn instances_keep_identity() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local part = Instance.new("Part", workspace)
        local seen = {}
        seen[part] = true
        print(seen[workspace:FindFirstChild("Part")], rawequal(part, workspace:GetChildren()[1]))
    "#,
    );
    assert_eq!(printed(&rt), vec!["true true"]);
}

#[test]
fn vector_maths() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local a = Vector3.new(1, 2, 3)
        local b = Vector3.new(4, 5, 6)
        print(tostring(a + b), tostring(b - a), tostring(a * 2), tostring(2 * a), tostring(-a))
        print(a:Dot(b), Vector3.new(3, 4, 0).Magnitude, a == Vector3.new(1, 2, 3), typeof(a))
        local p = Instance.new("Part")
        p.Position += Vector3.new(0, 1, 0)
        print(p.Position.Y)
    "#,
    );
    assert_eq!(printed(&rt), vec!["5, 7, 9 3, 3, 3 2, 4, 6 2, 4, 6 -1, -2, -3", "32 5 true Vector3", "1"]);
}

#[test]
fn heartbeat_drives_scripts() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local part = Instance.new("Part", workspace)
        local conn
        conn = game:GetService("RunService").Heartbeat:Connect(function(dt)
            part.Position += Vector3.new(dt, 0, 0)
            if part.Position.X >= 1 then
                conn:Disconnect()
            end
        end)
    "#,
    );
    assert_eq!(rt.connection_count(), 1);
    for _ in 0..10 {
        assert!(rt.step(0.25).is_empty());
    }
    assert_eq!(rt.connection_count(), 0, "handler disconnected itself");
    let tree = rt.tree().borrow();
    let part = tree.children(tree.workspace()).unwrap()[0];
    assert_eq!(tree.get(part, Property::Position).unwrap(), PropertyValue::Vec3(Vec3::new(1.0, 0.0, 0.0)));
}

#[test]
fn handler_errors_are_reported_not_fatal() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local RunService = game:GetService("RunService")
        RunService.Heartbeat:Connect(function() error("boom") end)
        RunService.Heartbeat:Connect(function() print("still running") end)
    "#,
    );
    let errors = rt.step(0.016);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("boom"), "{}", errors[0].message);
    assert_eq!(printed(&rt), vec!["still running"]);
}

#[test]
fn friendly_errors() {
    let rt = runtime();
    let err = |src: &str| rt.run("bad", src).unwrap_err().message;
    assert!(err(r#"Instance.new("Spaceship")"#).contains("Spaceship is not a valid class name"));
    assert!(err(r#"Instance.new("Workspace")"#).contains("Workspace cannot be created"));
    assert!(err(r#"workspace.Parent = nil"#).contains("Parent property is locked"));
    assert!(err(r#"local _ = workspace.Nope"#).contains("Nope is not a valid member of Workspace"));
    assert!(err(r#"Instance.new("Part").Position = 5"#).contains("Position expects a Vector3"));
    assert!(
        err(r#"Instance.new("Folder").Position = Vector3.zero"#)
            .contains("Position is not a valid member of Folder")
    );
    let cycle = r#"
        local a = Instance.new("Folder"); local b = Instance.new("Folder", a)
        a.Parent = b
    "#;
    assert!(err(cycle).contains("cycle"));
}

#[test]
fn destroy_and_queries() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local model = Instance.new("Model", workspace)
        local part = Instance.new("Part", model)
        print(#workspace:GetDescendants(), part:IsDescendantOf(workspace), workspace:IsA("Model"))
        print(workspace:FindFirstChildOfClass("Model") == model)
        model:Destroy()
        print(part.Parent, #workspace:GetChildren(), part.Name)
    "#,
    );
    assert_eq!(printed(&rt), vec!["2 true true", "true", "nil 0 Part"]);
}

#[test]
fn scripts_cannot_tamper_with_builtins() {
    let rt = runtime();
    // Sandboxed: a script's globals don't leak into other scripts, and the
    // shared libraries are read-only.
    run(&rt, "leaked = 1");
    run(&rt, "print(leaked)");
    assert!(rt.run("evil", "Vector3.new = nil").is_err());
    let out = rt.take_output();
    assert_eq!(out, vec![OutputLine { level: OutputLevel::Print, text: "nil".into() }]);
}

#[test]
fn task_wait_resumes_after_the_right_time() {
    let rt = runtime();
    run(
        &rt,
        r#"
        print("start")
        local waited = task.wait(1)
        print("after", math.floor(waited * 100 + 0.5) / 100)
        wait()
        print("next frame")
    "#,
    );
    assert_eq!(printed(&rt), vec!["start"]);
    assert_eq!(rt.waiting_count(), 1);
    for _ in 0..3 {
        assert!(rt.step(0.25).is_empty());
    }
    assert!(printed(&rt).is_empty(), "0.75s is not enough");
    rt.step(0.25);
    assert_eq!(printed(&rt), vec!["after 1"]);
    rt.step(0.25);
    assert_eq!(printed(&rt), vec!["next frame"]);
    assert_eq!(rt.waiting_count(), 0);
}

#[test]
fn spawn_and_delay() {
    let rt = runtime();
    run(
        &rt,
        r#"
        task.spawn(function(x) print("spawned", x) task.wait(0.5) print("spawn done") end, 7)
        task.delay(1, function(a, b) print("delayed", a, b) end, "x", "y")
        print("main")
    "#,
    );
    assert_eq!(printed(&rt), vec!["spawned 7", "main"]);
    rt.step(0.5);
    assert_eq!(printed(&rt), vec!["spawn done"]);
    rt.step(0.5);
    assert_eq!(printed(&rt), vec!["delayed x y"]);
}

#[test]
fn errors_after_a_wait_come_from_step() {
    let rt = runtime();
    run(&rt, "task.wait(0.1) error('late')");
    let errors = rt.step(0.2);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].script, "test");
    assert!(errors[0].message.contains("late"));
}

#[test]
fn heartbeat_handlers_may_wait() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local busy = false
        game:GetService("RunService").Heartbeat:Connect(function()
            if busy then return end
            busy = true
            print("tick")
            task.wait(1)
            busy = false
        end)
    "#,
    );
    for _ in 0..8 {
        assert!(rt.step(0.25).is_empty());
    }
    assert_eq!(printed(&rt).len(), 2, "once at t=0.25, again at t=1.5");
}

#[test]
fn tree_scripts_get_their_own_script_global() {
    use playlibre_instance::InstanceTree;
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    for (name, src, disabled) in [
        ("A", "print(script.Name, script.Parent == workspace)", false),
        ("B", "print('B ran')", true),
        ("C", "error('C broke')", false),
    ] {
        let s = tree.create(ClassKind::Script, name).unwrap();
        tree.set_parent(s, Some(ws)).unwrap();
        tree.set(s, Property::Source, PropertyValue::String(src.into())).unwrap();
        tree.set(s, Property::Disabled, PropertyValue::Bool(disabled)).unwrap();
    }
    let rt = ScriptRuntime::new_with_tree(tree).unwrap();
    let errors = rt.run_tree_scripts();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].script, "C");
    assert_eq!(printed(&rt), vec!["A true"]);
}

#[test]
fn touched_and_clone_and_wait_for_child() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local lava = Instance.new("Part", workspace)
        lava.Name = "Lava"
        lava.Touched:Connect(function(hit)
            print("touched by", hit.Name)
        end)
        local copy = lava:Clone()
        copy.Name = "Lava2"
        copy.Parent = workspace
        print(#workspace:GetChildren(), copy ~= lava)
        task.spawn(function()
            local thing = workspace:WaitForChild("Later")
            print("found", thing.Name)
        end)
        task.delay(0.5, function() Instance.new("Folder", workspace).Name = "Later" end)
    "#,
    );
    assert_eq!(printed(&rt), vec!["2 true"]);
    let (lava, lava2) = {
        let tree = rt.tree().borrow();
        let ws = tree.workspace();
        (
            tree.find_first_child(ws, "Lava").unwrap().unwrap(),
            tree.find_first_child(ws, "Lava2").unwrap().unwrap(),
        )
    };
    assert!(rt.fire_touched(lava, lava2).is_empty());
    assert!(rt.fire_touched(lava2, lava).is_empty(), "clones don't copy connections");
    assert_eq!(printed(&rt), vec!["touched by Lava2"]);
    rt.step(0.5);
    rt.step(0.1);
    assert_eq!(printed(&rt), vec!["found Later"]);
}

#[test]
fn endless_loops_are_stopped() {
    let rt = runtime();
    let started = std::time::Instant::now();
    let err = rt.run("spin", "while true do end").unwrap_err();
    assert!(err.message.contains("without waiting"), "{}", err.message);
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    // The runtime still works afterwards.
    run(&rt, "print('alive')");
    assert_eq!(printed(&rt), vec!["alive"]);
}

#[test]
fn enums_set_shapes_and_materials() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local ball = Instance.new("Part", workspace)
        ball.Name = "Ball"
        ball.Shape = Enum.PartType.Ball
        ball.Material = "Neon"
        ball.Orientation = Vector3.new(0, 90, 0)
        print(ball.Shape, ball.Shape == Enum.PartType.Ball, ball.Material.Name, typeof(ball.Material))
        print(pcall(function() ball.Shape = Enum.Material.Wood end))
    "#,
    );
    let out = printed(&rt);
    assert_eq!(out[0], "Enum.PartType.Ball true Neon EnumItem");
    assert!(out[1].starts_with("false"), "a material is not a shape: {}", out[1]);
    let tree = rt.tree().borrow();
    let ball = tree.find_first_child(tree.workspace(), "Ball").unwrap().unwrap();
    assert_eq!(
        tree.get_enum(ball, Property::Material),
        Some(playlibre_instance::EnumValue::Material(playlibre_instance::Material::Neon))
    );
}

#[test]
fn modules_run_once_and_can_wait() {
    let rt = runtime();
    {
        let mut tree = rt.tree().borrow_mut();
        let storage = tree.service(ClassKind::ReplicatedStorage).unwrap();
        let module = tree.create(ClassKind::ModuleScript, "Util").unwrap();
        tree.set_parent(module, Some(storage)).unwrap();
        let source = r#"
            print("loading", script.Name)
            task.wait(0.5)
            local Util = {}
            function Util.double(n) return n * 2 end
            return Util
        "#;
        tree.set(module, Property::Source, PropertyValue::String(source.into())).unwrap();
        let bad = tree.create(ClassKind::ModuleScript, "Bad").unwrap();
        tree.set_parent(bad, Some(storage)).unwrap();
        tree.set(bad, Property::Source, PropertyValue::String("return 1, 2".into())).unwrap();
    }
    run(
        &rt,
        r#"
        local storage = game:GetService("ReplicatedStorage")
        local Util = require(storage.Util)
        print(Util.double(21), require(storage.Util) == Util)
        print(pcall(require, storage.Bad))
        print(pcall(require, workspace))
    "#,
    );
    assert_eq!(printed(&rt), vec!["loading Util"], "waits inside the module");
    rt.step(0.6);
    let out = printed(&rt);
    assert_eq!(out[0], "42 true", "the second require reuses the first result");
    assert!(out[1].contains("exactly one value"), "{}", out[1]);
    assert!(out[2].contains("ModuleScript"), "{}", out[2]);
}

#[test]
fn players_service_and_leaderstats() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local Players = game:GetService("Players")
        Players.PlayerAdded:Connect(function(player)
            local stats = Instance.new("Folder")
            stats.Name = "leaderstats"
            stats.Parent = player
            local coins = Instance.new("IntValue", stats)
            coins.Name = "Coins"
            coins.Value = 9.6
            print("joined", player.Name, #Players:GetPlayers(), coins.Value)
            player.CharacterAdded:Connect(function(model)
                print("character", model.Name, Players:GetPlayerFromCharacter(model) == player)
                model.Humanoid.Died:Connect(function() print("died") end)
                model.Humanoid:TakeDamage(150)
                print("health", model.Humanoid.Health)
            end)
        end)
        print(pcall(Instance.new, "Player"))
    "#,
    );
    assert!(printed(&rt)[0].starts_with("false"), "scripts can't make players");

    let (player, model, humanoid) = {
        let mut tree = rt.tree().borrow_mut();
        let players = tree.service(ClassKind::Players).unwrap();
        let player = tree.create(ClassKind::Player, "Alex").unwrap();
        tree.set_parent(player, Some(players)).unwrap();
        let model = tree.create(ClassKind::Model, "Alex").unwrap();
        let ws = tree.workspace();
        tree.set_parent(model, Some(ws)).unwrap();
        let humanoid = tree.create(ClassKind::Humanoid, "Humanoid").unwrap();
        tree.set_parent(humanoid, Some(model)).unwrap();
        tree.set(player, Property::Character, PropertyValue::Ref(Some(model))).unwrap();
        (player, model, humanoid)
    };
    assert!(rt.player_added(player).is_empty());
    assert!(rt.character_added(player, model).is_empty());
    assert!(rt.humanoid_died(humanoid).is_empty());
    assert_eq!(printed(&rt), vec!["joined Alex 1 10", "character Alex true", "health 0", "died"]);
}

#[test]
fn cframes_move_and_turn_parts() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local part = Instance.new("Part", workspace)
        part.CFrame = CFrame.new(1, 2, 3) * CFrame.Angles(0, math.rad(90), 0)
        print(part.Position, part.Orientation)
        local look = part.CFrame.LookVector
        print(math.round(look.X) + 0, math.round(look.Y) + 0, math.round(look.Z) + 0)
        local cf = CFrame.lookAt(Vector3.new(0, 0, 0), Vector3.new(0, 0, -10))
        print(cf.LookVector == Vector3.new(0, 0, -1), typeof(cf))
        local moved = (cf * CFrame.new(0, 0, -5)).Position
        print(moved.Z, (cf:Inverse() * cf).Position.Magnitude)
        print(CFrame.new(1, 0, 0):PointToWorldSpace(Vector3.new(1, 0, 0)))

        local model = Instance.new("Model", workspace)
        for i = 1, 2 do
            local p = Instance.new("Part", model)
            p.Position = Vector3.new(i * 10, 0, 0)
        end
        model:PivotTo(CFrame.new(0, 50, 0))
        local a, b = model:GetChildren()[1], model:GetChildren()[2]
        print(a.Position, b.Position)
    "#,
    );
    let out = printed(&rt);
    assert_eq!(out[0], "1, 2, 3 0, 90, 0");
    assert_eq!(out[1], "-1 0 0", "turned 90 degrees, forward is now -X");
    assert_eq!(out[2], "true CFrame");
    assert_eq!(out[3], "-5 0");
    assert_eq!(out[4], "2, 0, 0");
    assert_eq!(out[5], "0, 50, 0 10, 50, 0", "the model moved as one piece");
}

#[test]
fn tweens_animate_properties_and_signal_completion() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local TweenService = game:GetService("TweenService")
        local part = Instance.new("Part", workspace)
        part.Name = "Door"
        local info = TweenInfo.new(1, Enum.EasingStyle.Linear)
        local tween = TweenService:Create(part, info, { Position = Vector3.new(10, 0, 0), Transparency = 1 })
        tween:Play()
        tween.Completed:Wait()
        print("done", part.Position, part.Transparency)
    "#,
    );
    rt.step(0.5);
    {
        let tree = rt.tree().borrow();
        let door = tree.find_first_child(tree.workspace(), "Door").unwrap().unwrap();
        let x = tree.get_vec3(door, Property::Position).unwrap().x;
        assert!((x - 5.0).abs() < 0.2, "halfway after half the time: {x}");
    }
    assert!(printed(&rt).is_empty());
    rt.step(0.6);
    rt.step(0.1);
    assert_eq!(printed(&rt), vec!["done 10, 0, 0 1"]);
}

#[test]
fn change_events_and_signal_helpers() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local value = Instance.new("IntValue", workspace)
        value.Name = "Score"
        value.Changed:Connect(function(property) print("changed", property, value.Value) end)
        value:GetPropertyChangedSignal("Name"):Connect(function() print("renamed", value.Name) end)
        workspace.ChildAdded:Once(function(child) print("added", child.Name) end)
        task.spawn(function()
            local child = workspace.ChildAdded:Wait()
            print("waited for", child.Name)
        end)
    "#,
    );
    rt.tree().borrow_mut().drain_changes();
    run(
        &rt,
        r#"
        workspace.Score.Value = 5
        workspace.Score.Name = "Points"
        Instance.new("Folder", workspace).Name = "First"
    "#,
    );
    let changes = rt.tree().borrow_mut().drain_changes();
    assert!(rt.fire_changes(&changes).is_empty());
    rt.step(0.1);
    let out = printed(&rt);
    // A value object's Changed passes the new value, and only for its Value.
    assert!(out.contains(&"changed 5 5".to_string()), "{out:?}");
    assert_eq!(out.iter().filter(|l| l.starts_with("changed")).count(), 1, "{out:?}");
    assert!(out.contains(&"renamed Points".to_string()), "{out:?}");
    assert!(out.contains(&"added Folder".to_string()) || out.contains(&"added First".to_string()), "{out:?}");
    assert!(out.iter().any(|l| l.starts_with("waited for")), "{out:?}");

    // Once really is once.
    run(&rt, r#"Instance.new("Folder", workspace)"#);
    let changes = rt.tree().borrow_mut().drain_changes();
    rt.fire_changes(&changes);
    rt.step(0.1);
    assert!(printed(&rt).iter().all(|l| !l.starts_with("added")));
}

#[test]
fn guis_buttons_and_click_detectors() {
    let rt = runtime();
    let (player, button, detector) = {
        let mut tree = rt.tree().borrow_mut();
        let players = tree.service(ClassKind::Players).unwrap();
        let player = tree.create(ClassKind::Player, "Sam").unwrap();
        tree.set_parent(player, Some(players)).unwrap();
        let gui = tree.create(ClassKind::PlayerGui, "PlayerGui").unwrap();
        tree.set_parent(gui, Some(player)).unwrap();
        let screen = tree.create(ClassKind::ScreenGui, "Hud").unwrap();
        tree.set_parent(screen, Some(gui)).unwrap();
        let button = tree.create(ClassKind::TextButton, "Buy").unwrap();
        tree.set_parent(button, Some(screen)).unwrap();
        let part = tree.create(ClassKind::Part, "Lever").unwrap();
        let ws = tree.workspace();
        tree.set_parent(part, Some(ws)).unwrap();
        let detector = tree.create(ClassKind::ClickDetector, "ClickDetector").unwrap();
        tree.set_parent(detector, Some(part)).unwrap();
        (player, button, detector)
    };
    run(
        &rt,
        r#"
        local player = game:GetService("Players").LocalPlayer
        local button = player.PlayerGui.Hud.Buy
        button.Size = UDim2.new(0.5, 10, 0, 40)
        button.AnchorPoint = Vector2.new(0.5, 0.5)
        print(button.Size, button.Size.X.Scale, button.AnchorPoint, button:IsA("GuiObject"))
        button.MouseButton1Click:Connect(function() button.Text = "Bought!" end)
        workspace.Lever.ClickDetector.MouseClick:Connect(function(who) print("pulled by", who.Name) end)
    "#,
    );
    assert_eq!(printed(&rt), vec!["{0.5, 10}, {0, 40} 0.5 0.5, 0.5 true"]);
    assert!(rt.click_button(button).is_empty());
    assert!(rt.click_detector(detector, player).is_empty());
    assert_eq!(printed(&rt), vec!["pulled by Sam"]);
    let tree = rt.tree().borrow();
    assert_eq!(tree.get_string(button, Property::Text).as_deref(), Some("Bought!"));
}

#[test]
fn only_world_and_server_scripts_start() {
    let rt = runtime();
    {
        let mut tree = rt.tree().borrow_mut();
        for (service, name) in [
            (ClassKind::Workspace, "InWorld"),
            (ClassKind::ServerScriptService, "OnServer"),
            (ClassKind::ReplicatedStorage, "InStorage"),
            (ClassKind::StarterGui, "InStarterGui"),
        ] {
            let parent = tree.service(service).unwrap();
            let script = tree.create(ClassKind::Script, name).unwrap();
            tree.set_parent(script, Some(parent)).unwrap();
            tree.set(script, Property::Source, PropertyValue::String("print(script.Name)".into())).unwrap();
        }
    }
    assert!(rt.run_tree_scripts().is_empty());
    assert_eq!(printed(&rt), vec!["InWorld", "OnServer"]);
}

fn add_player(rt: &ScriptRuntime, name: &str) -> playlibre_instance::InstanceId {
    let mut tree = rt.tree().borrow_mut();
    let players = tree.service(ClassKind::Players).unwrap();
    let player = tree.create(ClassKind::Player, name).unwrap();
    tree.set_parent(player, Some(players)).unwrap();
    player
}

#[test]
fn raycasts_find_parts_and_respect_filters() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local floor = Instance.new("Part", workspace)
        floor.Name = "Floor"
        floor.Size = Vector3.new(20, 2, 20)
        floor.Material = Enum.Material.Grass
        local box = Instance.new("Part", workspace)
        box.Name = "Box"
        box.Position = Vector3.new(0, 4, 0)
        box.Size = Vector3.new(2, 2, 2)

        local down = Vector3.new(0, -100, 0)
        local hit = workspace:Raycast(Vector3.new(0, 20, 0), down)
        print(hit.Instance.Name, hit.Position, hit.Normal, hit.Distance)

        local params = RaycastParams.new()
        params.FilterDescendantsInstances = { box }
        print(params.FilterType)
        hit = workspace:Raycast(Vector3.new(0, 20, 0), down, params)
        print(hit.Instance.Name, hit.Material, hit.Position.Y)

        params.FilterType = Enum.RaycastFilterType.Include
        print(workspace:Raycast(Vector3.new(5, 20, 0), down, params))
        print(workspace:Raycast(Vector3.new(0, 20, 0), Vector3.new(0, -5, 0)))
    "#,
    );
    assert_eq!(
        printed(&rt),
        vec![
            "Box 0, 5, 0 0, 1, 0 15",
            "Enum.RaycastFilterType.Exclude",
            "Floor Enum.Material.Grass 1",
            "nil",
            "nil",
        ]
    );
}

#[test]
fn user_input_service_reports_keys_and_clicks() {
    use playlibre_instance::KeyCode;
    use playlibre_script::UserInput;
    let rt = runtime();
    run(
        &rt,
        r#"
        local UserInputService = game:GetService("UserInputService")
        UserInputService.InputBegan:Connect(function(input, processed)
            print("began", input.KeyCode, input.UserInputType, processed)
            print("down?", UserInputService:IsKeyDown(Enum.KeyCode.F))
        end)
        UserInputService.InputEnded:Connect(function(input)
            print("ended", input.KeyCode.Name, UserInputService:IsKeyDown(Enum.KeyCode.F))
        end)
    "#,
    );
    assert!(rt.input_began(UserInput::Key(KeyCode::F), false).is_empty());
    assert!(rt.input_ended(UserInput::Key(KeyCode::F), false).is_empty());
    assert!(rt.input_began(UserInput::MouseButton1, true).is_empty());
    assert_eq!(
        printed(&rt),
        vec![
            "began Enum.KeyCode.F Enum.UserInputType.Keyboard false",
            "down? true",
            "ended F false",
            "began Enum.KeyCode.Unknown Enum.UserInputType.MouseButton1 true",
            "down? false",
        ]
    );
}

#[test]
fn remote_and_bindable_events_carry_values() {
    let rt = runtime();
    add_player(&rt, "Sam");
    run(
        &rt,
        r##"
        local remote = Instance.new("RemoteEvent", game:GetService("ReplicatedStorage"))
        local bindable = Instance.new("BindableEvent", workspace)
        remote.OnServerEvent:Connect(function(player, a, b)
            print("server got", player.Name, a, b.x)
            remote:FireClient(player, "thanks")
        end)
        remote.OnClientEvent:Connect(function(message) print("client got", message) end)
        bindable.Event:Connect(function(...) print("bindable", select("#", ...), ...) end)

        remote:FireServer("hello", { x = 5 })
        remote:FireAllClients("everyone")
        bindable:Fire(1, nil, "three")
        print(pcall(function() bindable:FireServer() end))
    "##,
    );
    let out = printed(&rt);
    assert_eq!(
        &out[..4],
        ["server got Sam hello 5", "client got thanks", "client got everyone", "bindable 3 1 nil three"]
    );
    assert!(out[4].starts_with("false") && out[4].contains("FireServer is not a valid member"), "{out:?}");
}

#[test]
fn data_stores_save_and_load() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local store = game:GetService("DataStoreService"):GetDataStore("PlayerData")
        print(store:GetAsync("coins_1"))
        store:SetAsync("coins_1", 12)
        store:SetAsync("profile_1", { name = "Sam", badges = { "first", "fast" } })
        print(store:IncrementAsync("coins_1", 3), store:IncrementAsync("visits"))
        print(store:UpdateAsync("coins_1", function(old) return old * 2 end))
        print(store:UpdateAsync("coins_1", function() return nil end))
        print(store:RemoveAsync("visits"), store:GetAsync("visits"))
        print(pcall(function() store:SetAsync("bad", workspace) end))
    "#,
    );
    let out = printed(&rt);
    // GetAsync, UpdateAsync and RemoveAsync also give the key's info.
    assert_eq!(&out[..4], ["nil nil", "15 1", "30 DataStoreKeyInfo", "30 DataStoreKeyInfo"]);
    assert_eq!(out[4], "1 nil nil");
    assert!(out[5].starts_with("false") && out[5].contains("can't be saved"), "{out:?}");

    // A new runtime sees what the first one saved.
    let saved = rt.save_data();
    let rt2 = runtime();
    rt2.load_data(&saved).unwrap();
    run(
        &rt2,
        r#"
        local store = game:GetService("DataStoreService"):GetDataStore("PlayerData")
        local profile = store:GetAsync("profile_1")
        print(store:GetAsync("coins_1"), profile.name, profile.badges[2], #profile.badges)
    "#,
    );
    assert_eq!(printed(&rt2), vec!["30 Sam fast 2"]);
}

#[test]
fn debris_and_prompts() {
    let rt = runtime();
    let player = add_player(&rt, "Sam");
    let prompt = {
        let mut tree = rt.tree().borrow_mut();
        let part = tree.create(ClassKind::Part, "Chest").unwrap();
        let ws = tree.workspace();
        tree.set_parent(part, Some(ws)).unwrap();
        let prompt = tree.create(ClassKind::ProximityPrompt, "ProximityPrompt").unwrap();
        tree.set_parent(prompt, Some(part)).unwrap();
        prompt
    };
    run(
        &rt,
        r#"
        local prompt = workspace.Chest.ProximityPrompt
        print(prompt.ActionText, prompt.KeyboardKeyCode, prompt.MaxActivationDistance, prompt.HoldDuration)
        prompt.ActionText = "Open"
        prompt.Triggered:Connect(function(player) print(player.Name, "opened it") end)
        prompt.TriggerEnded:Connect(function(player) print(player.Name, "let go") end)

        local spark = Instance.new("Part", workspace)
        spark.Name = "Spark"
        game:GetService("Debris"):AddItem(spark, 0.5)
    "#,
    );
    assert!(rt.prompt_triggered(prompt, player).is_empty());
    assert!(rt.prompt_trigger_ended(prompt, player).is_empty());
    assert_eq!(printed(&rt), vec!["Interact Enum.KeyCode.E 10 0", "Sam opened it", "Sam let go"]);
    rt.step(0.25);
    let spark = |rt: &ScriptRuntime| {
        let tree = rt.tree().borrow();
        tree.find_first_child(tree.workspace(), "Spark").unwrap().is_some()
    };
    assert!(spark(&rt));
    rt.step(0.3);
    assert!(!spark(&rt));
}

fn flush(rt: &ScriptRuntime) {
    let changes = rt.tree().borrow_mut().drain_changes();
    let errors = rt.fire_changes(&changes);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn attributes_hold_values_and_signal_changes() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local part = Instance.new("Part", workspace)
        part:SetAttribute("Points", 5)
        part:SetAttribute("Owner", "Sam")
        part:SetAttribute("Tint", Color3.new(1, 0, 0))
        print(part:GetAttribute("Points"), part:GetAttribute("Owner"), part:GetAttribute("Tint"), part:GetAttribute("Nope"))
        local names = {}
        for name in part:GetAttributes() do table.insert(names, name) end
        table.sort(names)
        print(table.concat(names, ","))
        part.AttributeChanged:Connect(function(name) print("changed", name) end)
        part:GetAttributeChangedSignal("Points"):Connect(function() print("points now", part:GetAttribute("Points")) end)
        print(pcall(function() part:SetAttribute("bad name", 1) end))
        print(pcall(function() part:SetAttribute("Where", workspace) end))
    "#,
    );
    let out = printed(&rt);
    assert_eq!(&out[..2], ["5 Sam 1, 0, 0 nil", "Owner,Points,Tint"]);
    assert!(out[2].starts_with("false") && out[3].starts_with("false"), "{out:?}");
    // Changes are delivered once a frame, so this frame's setup shows up too.
    flush(&rt);
    printed(&rt);
    run(&rt, r#"workspace.Part:SetAttribute("Points", 6) workspace.Part:SetAttribute("Owner", nil)"#);
    flush(&rt);
    assert_eq!(printed(&rt), vec!["changed Points", "points now 6", "changed Owner"]);
}

#[test]
fn collection_service_tags() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local CollectionService = game:GetService("CollectionService")
        CollectionService:GetInstanceAddedSignal("Lava"):Connect(function(part) print("new lava", part.Name) end)
        CollectionService:GetInstanceRemovedSignal("Lava"):Connect(function(part) print("lava gone", part.Name) end)
        for i = 1, 2 do
            local part = Instance.new("Part", workspace)
            part.Name = "Lava" .. i
            CollectionService:AddTag(part, "Lava")
        end
        local other = Instance.new("Part", workspace)
        other:AddTag("Safe")
        print(#CollectionService:GetTagged("Lava"), other:HasTag("Safe"), CollectionService:HasTag(other, "Lava"))
        print(table.concat(CollectionService:GetTags(workspace.Lava1), ","))
    "#,
    );
    assert_eq!(printed(&rt), vec!["2 true false", "Lava"]);
    flush(&rt);
    assert_eq!(printed(&rt), vec!["new lava Lava1", "new lava Lava2"]);
    run(&rt, r#"workspace.Lava1:Destroy() workspace.Lava2:RemoveTag("Lava")"#);
    flush(&rt);
    assert_eq!(printed(&rt), vec!["lava gone Lava1", "lava gone Lava2"]);
    run(&rt, r#"print(#game:GetService("CollectionService"):GetTagged("Lava"))"#);
    assert_eq!(printed(&rt), vec!["0"]);
}

#[test]
fn tools_equip_activate_and_read_the_mouse() {
    use playlibre_script::{MouseRay, UserInput};
    let rt = runtime();
    let player = add_player(&rt, "Sam");
    {
        let mut tree = rt.tree().borrow_mut();
        let backpack = tree.create(ClassKind::Backpack, "Backpack").unwrap();
        tree.set_parent(backpack, Some(player)).unwrap();
        let character = tree.create(ClassKind::Model, "Sam").unwrap();
        let ws = tree.workspace();
        tree.set_parent(character, Some(ws)).unwrap();
        let humanoid = tree.create(ClassKind::Humanoid, "Humanoid").unwrap();
        tree.set_parent(humanoid, Some(character)).unwrap();
        tree.set(player, Property::Character, PropertyValue::Ref(Some(character))).unwrap();
        let tool = tree.create(ClassKind::Tool, "Wand").unwrap();
        tree.set_parent(tool, Some(backpack)).unwrap();
    }
    run(
        &rt,
        r#"
        local floor = Instance.new("Part", workspace)
        floor.Name = "Floor"
        floor.Size = Vector3.new(50, 1, 50)
        local player = game:GetService("Players").LocalPlayer
        local wand = player.Backpack.Wand
        wand.Equipped:Connect(function(mouse)
            print("equipped", mouse.Target and mouse.Target.Name, mouse.Hit.Position)
        end)
        wand.Unequipped:Connect(function() print("unequipped", wand.Parent.Name) end)
        wand.Activated:Connect(function() print("zap") end)
        wand.Deactivated:Connect(function() print("done") end)
        player:GetMouse().Button1Down:Connect(function() print("mouse down") end)
        workspace.Sam.Humanoid:EquipTool(wand)
    "#,
    );
    rt.set_mouse(Some(MouseRay {
        origin: Vec3::new(3.0, 10.0, 0.0),
        direction: Vec3::new(0.0, -1.0, 0.0),
        x: 100.0,
        y: 50.0,
    }));
    flush(&rt);
    assert!(rt.input_began(UserInput::MouseButton1, false).is_empty());
    assert!(rt.input_ended(UserInput::MouseButton1, false).is_empty());
    // A click the GUI used doesn't reach the tool.
    assert!(rt.input_began(UserInput::MouseButton1, true).is_empty());
    run(&rt, "workspace.Sam.Humanoid:UnequipTools()");
    flush(&rt);
    assert_eq!(
        printed(&rt),
        vec!["equipped Floor 3, 0.5, 0", "mouse down", "zap", "done", "unequipped Backpack"]
    );
}

#[test]
fn the_mouse_tells_older_scripts_which_keys_go_down() {
    use playlibre_instance::KeyCode;
    use playlibre_script::UserInput;
    let rt = runtime();
    add_player(&rt, "Sam");
    run(
        &rt,
        r#"
        local mouse = game:GetService("Players").LocalPlayer:GetMouse()
        mouse.KeyDown:Connect(function(key) print("down", key) end)
        mouse.keyDown:connect(function(key) print("old down", key) end)
        mouse.KeyUp:Connect(function(key) print("up", string.byte(key)) end)
    "#,
    );
    assert!(rt.input_began(UserInput::Key(KeyCode::Q), false).is_empty());
    assert!(rt.input_ended(UserInput::Key(KeyCode::Space), false).is_empty());
    // Keys a text box took, and keys that don't type, don't count.
    assert!(rt.input_began(UserInput::Key(KeyCode::E), true).is_empty());
    assert!(rt.input_began(UserInput::Key(KeyCode::LeftShift), false).is_empty());
    assert_eq!(printed(&rt), vec!["down q", "old down q", "up 32"]);
}

#[test]
fn camera_velocity_text_boxes_and_explosions() {
    use playlibre_script::VelocityRequest;
    let rt = runtime();
    rt.set_viewport(800.0, 600.0);
    run(
        &rt,
        r#"
        local cam = workspace.CurrentCamera
        print(cam.ClassName, cam == workspace.CurrentCamera, cam.FieldOfView, cam.CameraType)
        cam.CameraType = Enum.CameraType.Scriptable
        cam.CFrame = CFrame.new(0, 0, 10)
        local p, visible = cam:WorldToViewportPoint(Vector3.new(0, 0, 0))
        print(p.X, p.Y, p.Z, visible, cam.ViewportSize.X)
        local _, behind = cam:WorldToViewportPoint(Vector3.new(0, 0, 20))
        print(behind)

        local ball = Instance.new("Part", workspace)
        ball.Size = Vector3.new(1, 1, 1)
        print(math.floor(ball:GetMass() * 100 + 0.5) / 100)
        ball.AssemblyLinearVelocity = Vector3.new(0, 10, 0)
        ball:ApplyImpulse(Vector3.new(0.7, 0, 0))
        print(ball.AssemblyLinearVelocity)

        local box = Instance.new("TextBox")
        box.Text = "old"
        box.Focused:Connect(function() print("focused", box.Text == "") end)
        box.FocusLost:Connect(function(enter) print("lost", enter, box.Text) end)
        box:CaptureFocus()
        print(box:IsFocused())

        local corner = Instance.new("UICorner")
        corner.CornerRadius = UDim.new(0.5, 4)
        print(corner.CornerRadius.Scale, corner.CornerRadius.Offset, typeof(corner.CornerRadius))
        local list = Instance.new("UIListLayout")
        list.FillDirection = "Horizontal"
        list.Padding = UDim.new(0, 6)
        print(list.FillDirection, list.Padding)

    "#,
    );
    assert_eq!(
        printed(&rt),
        vec![
            "Camera true 70 Enum.CameraType.Custom",
            "400 300 10 true 800",
            "false",
            "0.7",
            "1, 10, 0",
            "focused true",
            "true",
            "0.5 4 UDim",
            "Enum.FillDirection.Horizontal 0, 6",
        ]
    );
    let requests = rt.take_velocity_requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].1, VelocityRequest::Set(Vec3::new(0.0, 10.0, 0.0)));
    assert_eq!(requests[1].1, VelocityRequest::Add(Vec3::new(1.0, 0.0, 0.0)));

    let focused = rt.focused_text_box().expect("the box has focus");
    rt.tree().borrow_mut().set(focused, Property::Text, PropertyValue::String("typed".into())).unwrap();
    assert!(rt.focus_text_box(None, true).is_empty());
    assert_eq!(rt.focused_text_box(), None);

    assert_eq!(printed(&rt), vec!["lost true typed"]);
}

#[test]
fn random_run_service_and_teams() {
    let rt = runtime();
    let player = add_player(&rt, "Sam");
    run(
        &rt,
        r#"
        local a, b = Random.new(42), Random.new(42)
        local same, inside = true, true
        for _ = 1, 50 do
            local x, y = a:NextInteger(1, 6), b:NextInteger(1, 6)
            same = same and x == y
            inside = inside and x >= 1 and x <= 6
            local n = a:NextNumber(5, 10)
            b:NextNumber(5, 10)
            inside = inside and n >= 5 and n < 10
        end
        print(same, inside, typeof(a))

        local RunService = game:GetService("RunService")
        print(RunService:IsClient(), RunService:IsServer())
        local order = {}
        RunService.Stepped:Connect(function(t, dt) table.insert(order, "stepped") end)
        RunService.Heartbeat:Connect(function() table.insert(order, "heartbeat") end)
        RunService.RenderStepped:Connect(function() table.insert(order, "render") print(table.concat(order, " ")) end)

        local Teams = game:GetService("Teams")
        local red = Instance.new("Team", Teams)
        red.Name = "Red"
        red.TeamColor = Color3.new(1, 0, 0)
        local blue = Instance.new("Team", Teams)
        blue.Name = "Blue"
        local sam = game:GetService("Players").Sam
        sam.Team = red
        print(#Teams:GetTeams(), sam.Neutral, #red:GetPlayers(), #blue:GetPlayers(), #game:GetService("Players"):GetPlayers())
        sam.Team = nil
        print(sam.Neutral)
        local h = Instance.new("Humanoid")
        h.Jump = true
        print(h.Jump)
    "#,
    );
    let _ = player;
    rt.step(0.1);
    assert_eq!(
        printed(&rt),
        vec!["true true Random", "true true", "2 false 1 0 1", "true", "true", "stepped heartbeat render"]
    );
}

#[test]
fn scripts_cannot_use_all_the_memory() {
    let rt = runtime();
    let err = rt
        .run(
            "hog",
            r#"
            local hoard = {}
            local chunk = string.rep("x", 1000000)
            for i = 1, 100000 do
                hoard[i] = chunk .. i
            end
        "#,
        )
        .unwrap_err();
    assert!(err.message.contains("memory"), "{}", err.message);
    // The memory comes back and the runtime still works.
    run(&rt, "print('alive')");
    assert_eq!(printed(&rt), vec!["alive"]);
}

#[test]
fn finds_parts_in_boxes_balls_and_other_parts() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local function part(name, position, size, shape)
            local p = Instance.new("Part")
            p.Name = name
            p.Position = position
            p.Size = size
            if shape then p.Shape = shape end
            p.Parent = workspace
            return p
        end
        local floor = part("Floor", Vector3.new(0, -0.5, 0), Vector3.new(50, 1, 50))
        local crate = part("Crate", Vector3.new(0, 1, 0), Vector3.new(2, 2, 2))
        local far = part("Far", Vector3.new(20, 1, 20), Vector3.new(2, 2, 2))
        local ball = part("Ball", Vector3.new(4, 1, 0), Vector3.new(2, 2, 2), Enum.PartType.Ball)

        local function names(parts)
            local list = {}
            for _, p in parts do table.insert(list, p.Name) end
            table.sort(list)
            return table.concat(list, ",")
        end

        print(names(workspace:GetPartBoundsInBox(CFrame.new(0, 1, 0), Vector3.new(1, 1, 1))))
        print(names(workspace:GetPartBoundsInRadius(Vector3.new(4, 2, 0), 1.5)))
        print(names(workspace:GetPartsInPart(crate)))
        print(names(crate:GetTouchingParts()))
        local params = OverlapParams.new()
        params.FilterDescendantsInstances = { floor }
        print(names(workspace:GetPartBoundsInRadius(Vector3.new(0, 0, 0), 40, params)))
        params.FilterType = Enum.RaycastFilterType.Include
        params.MaxParts = 1
        print(names(workspace:GetPartBoundsInRadius(Vector3.new(0, 0, 0), 40, params)), typeof(params))
        -- Only one part, and only the included one.
        local long = CFrame.new(0, 0, 0) * CFrame.Angles(0, math.rad(45), 0)
        print(#workspace:GetPartBoundsInBox(long, Vector3.new(29, 1, 1), params) )
        print(names(workspace:GetPartBoundsInBox(CFrame.new(10, 1, 10) * CFrame.Angles(0, math.rad(45), 0), Vector3.new(0.5, 0.5, 30))))
    "#,
    );
    assert_eq!(
        printed(&rt),
        vec!["Crate", "Ball", "Floor", "Floor", "Ball,Crate,Far", "Floor OverlapParams", "1", "Crate,Far"]
    );
}

#[test]
fn scripts_see_what_the_player_plays_with() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local uis = game:GetService("UserInputService")
        print(uis.TouchEnabled, uis.GamepadEnabled, uis:GetLastInputType().Name)
    "#,
    );
    rt.set_devices(playlibre_script::Devices {
        touch: true,
        gamepad: true,
        last: playlibre_instance::UserInputType::Gamepad1,
        ..Default::default()
    });
    run(
        &rt,
        r#"
        local uis = game:GetService("UserInputService")
        print(uis.TouchEnabled, uis.GamepadEnabled, uis:GetLastInputType() == Enum.UserInputType.Gamepad1)
    "#,
    );
    assert_eq!(printed(&rt), vec!["false false Keyboard", "true true true"]);
}

#[test]
fn cframes_from_two_directions_and_from_angles_in_any_order() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local function r(v) return math.round(v.X) + 0, math.round(v.Y) + 0, math.round(v.Z) + 0 end
        local turn = CFrame.fromRotationBetweenVectors(Vector3.new(0, -1, 0), Vector3.new(1, 0, 0))
        print(r(turn:VectorToWorldSpace(Vector3.new(0, -1, 0))))
        local flip = CFrame.fromRotationBetweenVectors(Vector3.new(0, 1, 0), Vector3.new(0, -3, 0))
        print(r(flip:VectorToWorldSpace(Vector3.new(0, 1, 0))))
        local a = CFrame.fromEulerAngles(0.3, 0.5, 0.7, Enum.RotationOrder.YXZ)
        local b = CFrame.fromEulerAnglesYXZ(0.3, 0.5, 0.7)
        print((a.LookVector - b.LookVector).Magnitude < 1e-5)
        print((CFrame.fromEulerAngles(0.3, 0.5, 0.7).UpVector - CFrame.Angles(0.3, 0.5, 0.7).UpVector).Magnitude < 1e-5)
    "#,
    );
    assert_eq!(printed(&rt), vec!["1 0 0", "0 -1 0", "true", "true"]);
}

#[test]
fn destroying_disconnects() {
    let rt = runtime();
    run(
        &rt,
        r#"
        for i = 1, 50 do
            local p = Instance.new("Part", workspace)
            p.Touched:Connect(function() end)
            p.Changed:Connect(function() end)
            p.Destroying:Connect(function() print("gone") end)
            p:Destroy()
        end
    "#,
    );
    let changes = rt.tree().borrow_mut().drain_changes();
    rt.fire_changes(&changes);
    assert_eq!(printed(&rt).len(), 50);
    assert_eq!(rt.connection_count(), 0);
}

#[test]
fn touches_reach_scripts_as_the_docs_describe() {
    use playlibre_script::TouchPhase::{Began, Ended, Moved};
    let rt = runtime();
    run(
        &rt,
        r#"
        local uis = game:GetService("UserInputService")
        uis.TouchStarted:Connect(function(t, p)
            print("started", t.UserInputType.Name, t.Position.X, t.Position.Y, p)
        end)
        uis.TouchMoved:Connect(function(t)
            print("moved", t.Delta.X, t.UserInputState.Name)
        end)
        uis.TouchEnded:Connect(function(t)
            print("ended", t.UserInputState.Name)
        end)
        uis.InputBegan:Connect(function(input)
            if input.UserInputType == Enum.UserInputType.Touch then
                print("input began")
            end
        end)
        uis.TouchTap:Connect(function(at, p)
            print("tap", #at, at[1].X, p)
        end)
        uis.TouchLongPress:Connect(function(at, state)
            print("long", state.Name)
        end)
        uis.TouchPan:Connect(function(at, total, speed, state)
            print("pan", state.Name, total.X)
        end)
        uis.TouchPinch:Connect(function(at, scale, speed, state)
            print("pinch", state.Name, string.format("%.1f", scale))
        end)
        uis.TouchSwipe:Connect(function(direction, count)
            print("swipe", direction.Name, count)
        end)
        "#,
    );
    // A tap.
    rt.touch(1, Began, 100.0, 50.0, false, 0.0);
    rt.touch(1, Ended, 102.0, 50.0, false, 0.1);
    assert_eq!(
        printed(&rt),
        vec!["input began", "started Touch 100 50 false", "tap 1 102 false", "ended End"]
    );
    // A finger held still is a long press.
    rt.touch(2, Began, 10.0, 10.0, true, 1.0);
    rt.touch_tick(1.2);
    rt.touch_tick(1.6);
    rt.touch(2, Ended, 10.0, 10.0, true, 2.0);
    let lines = printed(&rt);
    assert!(lines.contains(&"long Begin".to_owned()) && lines.contains(&"long End".to_owned()), "{lines:?}");
    assert!(!lines.iter().any(|l| l.starts_with("tap")), "a long press isn't a tap: {lines:?}");
    // A quick flick to the right is a pan and a swipe.
    rt.touch(3, Began, 0.0, 100.0, false, 3.0);
    rt.touch(3, Moved, 40.0, 100.0, false, 3.05);
    rt.touch(3, Moved, 120.0, 100.0, false, 3.1);
    rt.touch(3, Ended, 120.0, 100.0, false, 3.15);
    let lines = printed(&rt);
    for want in ["moved 40 Change", "pan Begin 40", "pan Change 120", "swipe Right 1", "pan End 120"] {
        assert!(lines.contains(&want.to_owned()), "{want} in {lines:?}");
    }
    // Two fingers spreading apart pinch.
    rt.touch(4, Began, 100.0, 100.0, false, 5.0);
    rt.touch(5, Began, 200.0, 100.0, false, 5.0);
    rt.touch(5, Moved, 300.0, 100.0, false, 5.1);
    rt.touch(5, Ended, 300.0, 100.0, false, 5.2);
    rt.touch(4, Ended, 100.0, 100.0, false, 5.2);
    let lines = printed(&rt);
    for want in ["pinch Begin 2.0", "pinch End 2.0"] {
        assert!(lines.contains(&want.to_owned()), "{want} in {lines:?}");
    }
}

#[test]
fn games_can_hide_the_touch_controls_and_see_what_the_player_has() {
    let rt = runtime();
    assert!(rt.touch_controls_shown());
    run(
        &rt,
        r#"
        local uis = game:GetService("UserInputService")
        uis.LastInputTypeChanged:Connect(function(kind)
            print("now", kind.Name)
        end)
        print(uis.KeyboardEnabled, uis.MouseEnabled)
        uis.ModalEnabled = true
        "#,
    );
    assert!(!rt.touch_controls_shown());
    run(&rt, r#"game:GetService("UserInputService").ModalEnabled = false"#);
    assert!(rt.touch_controls_shown());
    run(&rt, r#"game:GetService("GuiService").TouchControlsEnabled = false"#);
    assert!(!rt.touch_controls_shown());
    run(&rt, r#"game:GetService("GuiService").TouchControlsEnabled = true"#);
    rt.set_devices(playlibre_script::Devices {
        touch: true,
        last: playlibre_instance::UserInputType::Touch,
        keyboard: false,
        mouse: false,
        ..Default::default()
    });
    run(
        &rt,
        r#"local uis = game:GetService("UserInputService") print(uis.KeyboardEnabled, uis.TouchEnabled)"#,
    );
    assert_eq!(printed(&rt), vec!["true true", "now Touch", "false true"]);
}

#[test]
fn joints_worlds_terrain_copies_and_older_names() {
    let rt = runtime();
    run(
        &rt,
        r##"
        local cf = CFrame.new(1, 2, 3) * CFrame.Angles(0, math.pi / 2, 0)
        local c = { cf:components() }
        print(#c, math.round(cf.R02), math.round(cf.R20), cf.R11)
        local a = Instance.new("Part", workspace)
        a.Anchored = true
        local b = Instance.new("Part", workspace)
        b.Size = Vector3.new(4, 4, 4)
        local d = Instance.new("Part", workspace)
        local w = Instance.new("WeldConstraint", a)
        w.Part0, w.Part1 = a, b
        local m = Instance.new("Motor6D", b)
        m.Part0, m.Part1 = b, d
        print(#a:GetJoints(), #b:GetJoints(), #a:GetConnectedParts(true), #d:GetConnectedParts(false))
        print(d.AssemblyRootPart == a, d:IsGrounded(), #Instance.new("Part"):GetConnectedParts(true))
        local p = Instance.new("Part")
        p.Size = Vector3.new(2, 2, 2)
        print(p:Resize(Enum.NormalId.Top, 2), p.Size, p.Position)
        local ps = game:GetService("PhysicsService")
        ps:RegisterCollisionGroup("Ghosts")
        ps:CollisionGroupSetCollidable("Ghosts", "Default", false)
        b.CollisionGroup = "Ghosts"
        print(a:CanCollideWith(d), a:CanCollideWith(b))
        ps:RenameCollisionGroup("Ghosts", "Spirits")
        print(b.CollisionGroup, ps:CollisionGroupsAreCollidable("Spirits", "Default"), ps:IsCollisionGroupRegistered("Ghosts"))

        local world = Instance.new("WorldModel")
        local inside = Instance.new("Part", world)
        inside.Position = Vector3.new(50, 0, 0)
        local hit = world:Raycast(Vector3.new(50, 10, 0), Vector3.new(0, -20, 0))
        print(hit and hit.Instance == inside, hit and hit.Position.Y, workspace:Raycast(Vector3.new(50, 10, 0), Vector3.new(0, -20, 0)))
        print(#world:GetPartBoundsInRadius(Vector3.new(50, 0, 0), 2))

        local t = workspace.Terrain
        t:FillBlock(CFrame.new(2, 2, 2), Vector3.new(4, 4, 4), Enum.Material.Grass)
        local copy = t:CopyRegion(Region3int16.new(Vector3int16.new(0, 0, 0), Vector3int16.new(0, 0, 0)))
        print(copy.SizeInCells)
        t:PasteRegion(copy, Vector3int16.new(5, 0, 0), true)
        local mats = t:ReadVoxels(Region3.new(Vector3.new(20, 0, 0), Vector3.new(24, 4, 4)), 4)
        print(mats[1][1][1].Name)
        local ch = t:ReadVoxelChannels(Region3.new(Vector3.new(20, 0, 0), Vector3.new(24, 4, 4)), 4, { "SolidMaterial", "LiquidOccupancy" })
        print(ch.SolidMaterial[1][1][1].Name, ch.LiquidOccupancy[1][1][1])

        local folder = Instance.new("Folder")
        local kid = Instance.new("Part", folder)
        kid.Name = "Kid"
        print(#folder:children(), folder:findFirstChild("Kid") == kid, kid:isA("BasePart"), folder:clone().Name)
        kid:Remove()
        print(kid.Parent)
        print(select("#", Color3.toHSV(Color3.new(1, 0, 0))), PathWaypoint.new(Vector3.one).Action.Name, type(ElapsedTime()))
        print(pcall(function() return game:GetService("HttpService"):GetAsync("https://example.com") end))
        local path = game:GetService("PathfindingService"):FindPathAsync(Vector3.new(0, 3, 0), Vector3.new(10, 3, 0))
        print(typeof(path))
        local fired = false
        Instance.new("BindableEvent").Event:ConnectParallel(function() end)
        local rocket = Instance.new("RocketPropulsion")
        rocket:Fire()
        local att = Instance.new("Attachment")
        att.Axis = Vector3.new(0, 0, 1)
        print(att.Axis, att.SecondaryAxis.Y)
    "##,
    );
    assert_eq!(
        printed(&rt),
        vec![
            "12 1 -1 1",
            "1 2 2 2",
            "true true 0",
            "true 2, 4, 2 0, 1, 0",
            "true false",
            "Spirits false false",
            "true 0.5 nil",
            "1",
            "1, 1, 1",
            "Grass",
            "Grass 0",
            "1 true true Folder",
            "nil",
            "3 Walk number",
            "false Http requests are not enabled",
            "Path",
            "0, 0, 1 1",
        ]
    );
}

#[test]
fn actions_can_ask_for_touch_buttons() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local cas = game:GetService("ContextActionService")
        cas:BindAction("Sprint", function(name, state, input)
            print(name, state.Name, input.UserInputType.Name)
        end, true, Enum.KeyCode.LeftShift)
        cas:SetTitle("Sprint", "Run")
        cas:SetPosition("Sprint", UDim2.new(1, -90, 0, 20))
        cas:BindAction("Wave", function() end, false, Enum.KeyCode.G)
        cas:BindAction("Attack", function() end, true, Enum.KeyCode.F)
        print(cas:GetButton("Sprint"))
        "#,
    );
    let buttons = rt.touch_buttons();
    assert_eq!(
        buttons.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(),
        ["Sprint", "Attack"],
        "only actions that asked for one, in the order bound"
    );
    assert_eq!(buttons[0].title.as_deref(), Some("Run"));
    assert_eq!(buttons[0].position, Some([1.0, -90.0, 0.0, 20.0]));
    assert_eq!(buttons[1].title, None);
    assert!(rt.press_touch_button("Sprint", true).is_empty());
    assert!(rt.press_touch_button("Sprint", false).is_empty());
    assert_eq!(printed(&rt), vec!["nil", "Sprint Begin Touch", "Sprint End Touch"]);
    run(&rt, r#"game:GetService("ContextActionService"):UnbindAction("Sprint")"#);
    assert_eq!(rt.touch_buttons().len(), 1);
}

#[test]
fn phones_vibrate_through_the_haptic_service() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local haptics = game:GetService("HapticService")
        print(haptics:IsVibrationSupported(Enum.UserInputType.Touch))
        "#,
    );
    rt.set_devices(playlibre_script::Devices { vibration: true, ..Default::default() });
    run(
        &rt,
        r#"
        local haptics = game:GetService("HapticService")
        local touch, large = Enum.UserInputType.Touch, Enum.VibrationMotor.Large
        print(haptics:IsVibrationSupported(touch), haptics:IsMotorSupported(touch, large))
        haptics:SetMotor(touch, large, 0.6)
        print(haptics:GetMotor(touch, large))
        "#,
    );
    assert_eq!(printed(&rt), vec!["false", "true true", "0.6"]);
    let strength = rt.take_vibration().unwrap();
    assert!((strength - 0.6).abs() < 1e-6);
    assert_eq!(rt.take_vibration(), None, "taken once");
    run(
        &rt,
        r#"game:GetService("HapticService"):SetMotor(Enum.UserInputType.Touch, Enum.VibrationMotor.Large, 0)"#,
    );
    assert_eq!(rt.take_vibration(), Some(0.0));
}

#[test]
fn tapping_a_gui_object_fires_its_touch_tap() {
    use playlibre_script::TouchPhase::{Began, Ended};
    let rt = runtime();
    run(
        &rt,
        r#"
        local gui = Instance.new("ScreenGui")
        gui.Parent = workspace
        local button = Instance.new("TextButton")
        button.Name = "Fire"
        button.Parent = gui
        button.TouchTap:Connect(function(at)
            print("tapped", at[1].X, at[1].Y)
        end)
        button.TouchLongPress:Connect(function(at, state)
            print("held", state.Name)
        end)
        "#,
    );
    let id = {
        let tree = rt.tree().borrow();
        tree.descendants(tree.workspace())
            .unwrap()
            .into_iter()
            .find(|id| tree.name(*id).ok() == Some("Fire"))
            .unwrap()
    };
    rt.set_gui_rects(vec![(id, [0.0, 0.0, 100.0, 50.0])]);
    // A tap on it, then one beside it.
    rt.touch(1, Began, 10.0, 20.0, true, 0.0);
    rt.touch(1, Ended, 10.0, 20.0, true, 0.1);
    rt.touch(2, Began, 300.0, 20.0, false, 1.0);
    rt.touch(2, Ended, 300.0, 20.0, false, 1.1);
    // A long press on it.
    rt.touch(3, Began, 50.0, 25.0, true, 2.0);
    rt.touch_tick(2.6);
    rt.touch(3, Ended, 50.0, 25.0, true, 2.7);
    assert_eq!(printed(&rt), vec!["tapped 10 20", "held Begin", "held End"]);
}

#[test]
fn player_gui_finds_the_objects_at_a_point() {
    let rt = runtime();
    let ids = {
        let mut tree = rt.tree().borrow_mut();
        let players = tree.service(ClassKind::Players).unwrap();
        let player = tree.create(ClassKind::Player, "Sam").unwrap();
        tree.set_parent(player, Some(players)).unwrap();
        let gui = tree.create(ClassKind::PlayerGui, "PlayerGui").unwrap();
        tree.set_parent(gui, Some(player)).unwrap();
        let screen = tree.create(ClassKind::ScreenGui, "Hud").unwrap();
        tree.set_parent(screen, Some(gui)).unwrap();
        let panel = tree.create(ClassKind::Frame, "Panel").unwrap();
        tree.set_parent(panel, Some(screen)).unwrap();
        tree.set(panel, Property::ClipsDescendants, PropertyValue::Bool(true)).unwrap();
        let slot = tree.create(ClassKind::TextButton, "Slot").unwrap();
        tree.set_parent(slot, Some(panel)).unwrap();
        let tip = tree.create(ClassKind::TextLabel, "Tip").unwrap();
        tree.set_parent(tip, Some(panel)).unwrap();
        [panel, slot, tip]
    };
    rt.set_gui_rects(vec![
        (ids[0], [0.0, 0.0, 100.0, 100.0]),
        (ids[1], [10.0, 10.0, 50.0, 50.0]),
        (ids[2], [80.0, 10.0, 50.0, 50.0]),
    ]);
    run(
        &rt,
        r#"
        local gui = game:GetService("Players").LocalPlayer.PlayerGui
        local function names(x, y)
            local out = {}
            for _, o in gui:GetGuiObjectsAtPosition(x, y) do
                table.insert(out, o.Name)
            end
            return table.concat(out, ",")
        end
        print(names(20, 20), names(90, 20), names(120, 20), names(300, 300))
        "#,
    );
    assert_eq!(printed(&rt), vec!["Slot,Panel Tip,Panel  "]);
}

#[test]
fn older_collision_group_calls_still_work() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local ps = game:GetService("PhysicsService")
        ps:RegisterCollisionGroup("TreeLeaves")
        ps:CollisionGroupSetCollidable("TreeLeaves", "Default", false)
        local groups = ps:GetCollisionGroups()
        print(#groups, groups[2].name, groups[2].id, groups[2].mask, groups[1].mask)
        print(ps:GetCollisionGroupId("TreeLeaves"), ps:GetCollisionGroupName(0))
        print(pcall(ps.GetCollisionGroupId, ps, "Nope"))
        "#,
    );
    let lines = printed(&rt);
    assert_eq!(lines[..2], ["2 TreeLeaves 1 2 1", "1 Default"]);
    assert!(lines[2].starts_with("false"));
}

#[test]
fn rich_attributes_save_and_reach_other_computers() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local part = Instance.new("Part")
        part.Name = "Holder"
        part.Parent = workspace
        part:SetAttribute("Range", NumberRange.new(2, 8))
        part:SetAttribute("Fade", NumberSequence.new({ NumberSequenceKeypoint.new(0, 0), NumberSequenceKeypoint.new(1, 0.5) }))
        part:SetAttribute("Tint", ColorSequence.new(Color3.new(1, 0, 0), Color3.new(0, 0, 1)))
        part:SetAttribute("Where", CFrame.new(1, 2, 3))
        "#,
    );
    // Another computer, with only the saved place: no memory of the values.
    let text = playlibre_instance::place::save(&rt.tree().borrow()).unwrap();
    let (tree, _) = playlibre_instance::place::load(&text).unwrap();
    let other = playlibre_script::ScriptRuntime::new_with_tree(tree).unwrap();
    run(
        &other,
        r#"
        local part = workspace.Holder
        local r = part:GetAttribute("Range")
        print(typeof(r), r.Min, r.Max)
        local f = part:GetAttribute("Fade")
        print(typeof(f), #f.Keypoints, f.Keypoints[2].Value)
        local t = part:GetAttribute("Tint")
        print(typeof(t), t.Keypoints[1].Value.R, t.Keypoints[2].Value.B)
        print(typeof(part:GetAttribute("Where")), part:GetAttribute("Where").Position.Y)
        "#,
    );
    assert_eq!(
        printed(&other),
        vec!["NumberRange 2 8", "NumberSequence 2 0.5", "ColorSequence 1 1", "CFrame 2"]
    );
}

#[test]
fn parts_are_made_of_their_material_or_custom_physical_properties() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local part = Instance.new("Part")
        part.Size = Vector3.new(2, 1, 2)
        part.Parent = workspace
        print(part.CustomPhysicalProperties, string.format("%.2f", part:GetMass()))
        part.Material = Enum.Material.Metal
        print(string.format("%.2f", part:GetMass()), part.CurrentPhysicalProperties.Density)
        part.CustomPhysicalProperties = PhysicalProperties.new(1, 1.5, 0.9)
        local p = part.CustomPhysicalProperties
        print(p.Density, p.Friction, p.Elasticity, string.format("%.2f", part:GetMass()))
        print(PhysicalProperties.new(Enum.Material.Ice).Friction)
        part.CustomPhysicalProperties = nil
        print(part.CustomPhysicalProperties)
        "#,
    );
    assert_eq!(
        printed(&rt),
        vec![
            "nil 2.80",
            "31.40 7.849999904632568",
            "1 1.5 0.8999999761581421 4.00",
            "0.019999999552965164",
            "nil"
        ]
    );
}

#[test]
fn a_bare_yield_waits_until_the_thread_is_resumed() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local waiting
        task.spawn(function()
            waiting = coroutine.running()
            local got = coroutine.yield()
            print("resumed with", got)
        end)
        task.delay(0.5, function()
            print("spawning")
            task.spawn(waiting, "hello")
        end)
    "#,
    );
    rt.step(0.1);
    rt.step(0.1);
    assert!(printed(&rt).is_empty(), "a bare yield isn't resumed by the scheduler");
    rt.step(0.4);
    assert_eq!(printed(&rt), vec!["spawning", "resumed with hello"]);
}

#[test]
fn games_award_badges() {
    let rt = runtime();
    rt.set_badge_info(vec![playlibre_script::BadgeInfo {
        id: "top".into(),
        name: "Top".into(),
        description: "Got to the top".into(),
    }]);
    rt.add_owned_badges(2, ["top".to_owned()]);
    run(
        &rt,
        r#"
        local badges = game:GetService("BadgeService")
        print(badges:UserHasBadgeAsync(1, "top"), badges:UserHasBadgeAsync(2, "top"))
        print(badges:AwardBadge(1, "top"), badges:AwardBadge(1, "top"), badges:AwardBadge(2, "top"))
        print(badges:UserHasBadgeAsync(1, "top"), #badges:CheckUserBadgesAsync(1, { "top", "other" }))
        local info = badges:GetBadgeInfoAsync("top")
        print(info.Name, info.Description, info.IsEnabled, badges:GetBadgeInfoAsync("other").IsEnabled)
        "#,
    );
    assert_eq!(
        printed(&rt),
        vec!["false true", "true false false", "true 1", "Top Got to the top true true"]
    );
    assert_eq!(rt.take_badge_awards(), vec![(1, "top".to_owned())]);
    assert!(rt.take_badge_awards().is_empty());
}

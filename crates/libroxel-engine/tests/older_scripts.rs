//! Older and less-used parts of the scripting API, so scripts written for
//! other engines run.

use libroxel_engine::CharacterInput;
use libroxel_engine::session::start_on;
use libroxel_instance::{ClassKind, InstanceTree, Property, PropertyValue, Vec3};

/// Runs `source` in a world with a floor and a few parts, and returns what
/// it printed. Fails on any script error.
fn run(source: &str) -> Vec<String> {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    for (name, pos, size) in [
        ("Floor", Vec3::new(0.0, -0.5, 0.0), Vec3::new(100.0, 1.0, 100.0)),
        ("Crate", Vec3::new(0.0, 5.0, 0.0), Vec3::new(2.0, 2.0, 2.0)),
    ] {
        let p = tree.create(ClassKind::Part, name).unwrap();
        tree.set(p, Property::Position, PropertyValue::Vec3(pos)).unwrap();
        tree.set(p, Property::Size, PropertyValue::Vec3(size)).unwrap();
        tree.set(p, Property::Anchored, PropertyValue::Bool(true)).unwrap();
        tree.set_parent(p, Some(ws)).unwrap();
    }
    let script = tree.create(ClassKind::Script, "Test").unwrap();
    tree.set(script, Property::Source, PropertyValue::String(source.into())).unwrap();
    tree.set_parent(script, Some(ws)).unwrap();
    let mut session = start_on(tree, false).unwrap();
    for _ in 0..10 {
        session.step(1.0 / 60.0, CharacterInput::default());
    }
    let log = session.take_log();
    assert!(log.iter().all(|l| l.level != libroxel_engine::LogLevel::Error), "{log:?}");
    log.into_iter().map(|l| l.text).collect()
}

#[test]
fn cframes_take_apart_and_build_every_way() {
    let printed = run(r#"
        local cf = CFrame.new(1, 2, 3) * CFrame.Angles(0.3, -0.5, 0.2)
        local x, y, z = cf:ToEulerAnglesXYZ()
        print(string.format("%.3f %.3f %.3f", x, y, z))
        local again = CFrame.new(cf.Position) * CFrame.fromEulerAnglesXYZ(x, y, z)
        print((again.LookVector - cf.LookVector).Magnitude < 1e-4)
        local parts = { cf:GetComponents() }
        print(#parts)
        local rebuilt = CFrame.new(table.unpack(parts))
        print((rebuilt.RightVector - cf.RightVector).Magnitude < 1e-4, rebuilt.X)
        print(CFrame.new(0, 0, 0, 0, 0, 0, 1).LookVector.Z)
        local m = CFrame.fromMatrix(Vector3.new(5, 0, 0), Vector3.new(0, 0, -1), Vector3.new(0, 1, 0))
        print(m.RightVector.Z, m.XVector.X)
        local axis, angle = CFrame.Angles(0, math.pi / 2, 0):ToAxisAngle()
        print(string.format("%.2f %.2f", axis.Y, angle))
    "#);
    assert_eq!(printed, ["0.300 -0.500 0.200", "true", "12", "true 1", "-1", "-1 0", "1.00 1.57"]);
}

#[test]
fn brick_colours_name_and_paint_parts() {
    let printed = run(r#"
        local red = BrickColor.new("Bright red")
        print(red.Name, red.Number, math.floor(red.Color.R * 255))
        print(BrickColor.new(21) == red, BrickColor.Red() == red)
        print(BrickColor.new(Color3.new(1, 0, 0)).Name)
        workspace.Crate.BrickColor = BrickColor.new("Bright blue")
        print(workspace.Crate.BrickColor.Name, math.floor(workspace.Crate.Color.B * 255))
        print(typeof(BrickColor.random()), tostring(BrickColor.new("nonsense")))
    "#);
    assert_eq!(
        printed,
        ["Bright red 21 196", "true true", "Really red", "Bright blue 172", "BrickColor Medium stone grey"]
    );
}

#[test]
fn older_ray_searches_find_parts() {
    let printed = run(r#"
        local ray = Ray.new(Vector3.new(0, 20, 0), Vector3.new(0, -50, 0))
        local part, at, normal = workspace:FindPartOnRay(ray)
        print(part.Name, at.Y, normal.Y)
        part = workspace:FindPartOnRayWithIgnoreList(ray, { workspace.Crate })
        print(part.Name)
        part = workspace:FindPartOnRayWithWhitelist(ray, { workspace.Floor })
        print(part.Name)
        local missed, finish = workspace:FindPartOnRay(Ray.new(Vector3.new(0, 20, 0), Vector3.new(0, 5, 0)))
        print(missed, finish.Y)
        print(ray.Unit.Direction.Y, ray:Distance(Vector3.new(3, 0, 0)))
    "#);
    assert_eq!(printed, ["Crate 6 1", "Floor", "Floor", "nil 25", "-1 3"]);
}

#[test]
fn models_move_and_measure() {
    let printed = run(r#"
        local m = Instance.new("Model")
        workspace.Crate.Parent = m
        m.Parent = workspace
        local cf, size = m:GetBoundingBox()
        print(cf.Y, size.X, m:GetExtentsSize().Y)
        m:MoveTo(Vector3.new(10, 1, 0))
        print(workspace.Model.Crate.Position.X)
        m:TranslateBy(Vector3.new(0, 2, 0))
        print(m:GetPrimaryPartCFrame().Y)
        m:SetPrimaryPartCFrame(CFrame.new(0, 7, 0))
        print(workspace.Model.Crate.Position.Y)
    "#);
    assert_eq!(printed, ["5 2 2", "10", "3", "7"]);
}

#[test]
fn enumerations_list_their_items_and_tweens_ease() {
    let printed = run(r#"
        local items = Enum.UserInputState:GetEnumItems()
        print(#items, items[1].Name)
        local ts = game:GetService("TweenService")
        print(ts:GetValue(0.5, Enum.EasingStyle.Linear, Enum.EasingDirection.In))
        print(ts:GetValue(0.5, Enum.EasingStyle.Quad, Enum.EasingDirection.In))
    "#);
    assert_eq!(printed, ["5 Begin", "0.5", "0.25"]);
}

#[test]
fn services_without_much_behind_them_answer_harmlessly() {
    let printed = run(r#"
        local market = game:GetService("MarketplaceService")
        print(market == game:GetService("MarketplaceService"))
        print(market:UserOwnsGamePassAsync(1, 2), game:GetService("BadgeService"):AwardBadge(1, 2))
        game:GetService("PhysicsService"):RegisterCollisionGroup("Things")
        print(game:GetService("TextService"):GetTextSize("hello", 10).X)
        print(game:GetService("VRService").VREnabled, game:IsLoaded())
        game:BindToClose(function() end)
        local messages = game:GetService("MessagingService")
        local got
        local c = messages:SubscribeAsync("news", function(m) got = m.Data end)
        messages:PublishAsync("news", "hi")
        print(got)
        c:Disconnect()
        messages:PublishAsync("news", "again")
        print(got)
        local now = DateTime.now()
        print(now.UnixTimestamp > 1700000000, #DateTime.fromUnixTimestamp(0):ToIsoDate())
        print(DateTime.fromUnixTimestamp(86400 * 365):ToUniversalTime().Year)
        local lighting = game:GetService("Lighting")
        lighting:SetMinutesAfterMidnight(6 * 60)
        print(lighting.ClockTime, lighting:GetMinutesAfterMidnight())
        print(PhysicalProperties.new(2, 0.5, 0.1).Friction)
        print(workspace.Crate.AssemblyMass > 0, #workspace.Crate:GetConnectedParts())
        print(workspace:GetServerTimeNow() > 0, workspace.DistributedGameTime >= 0)
    "#);
    assert_eq!(
        printed,
        [
            "true",
            // Awarding a badge works (it's kept with the game's saved data).
            "false true",
            "25",
            "false true",
            "hi",
            "hi",
            "true 20",
            "1971",
            "6 360",
            "0.5",
            "true 0",
            "true true"
        ]
    );
}

#[test]
fn bound_actions_run_when_their_keys_go_down_and_up() {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let script = tree.create(ClassKind::Script, "Keys").unwrap();
    let source = r#"
        local actions = game:GetService("ContextActionService")
        actions:BindAction("Wave", function(name, state, input)
            print(name, state == Enum.UserInputState.Begin, state.Name, input.KeyCode.Name)
        end, false, Enum.KeyCode.F, Enum.KeyCode.G)
        actions:BindAction("Gone", function() print("never") end, false, Enum.KeyCode.F)
        actions:UnbindAction("Gone")
    "#;
    tree.set(script, Property::Source, PropertyValue::String(source.into())).unwrap();
    tree.set_parent(script, Some(ws)).unwrap();
    let mut session = start_on(tree, false).unwrap();
    session.step(1.0 / 60.0, CharacterInput::default());
    let f = libroxel_script::UserInput::Key(libroxel_instance::KeyCode::F);
    session.input_began(f, false);
    session.input_ended(f, false);
    session.input_began(libroxel_script::UserInput::Key(libroxel_instance::KeyCode::H), false);
    session.step(1.0 / 60.0, CharacterInput::default());
    let printed: Vec<String> = session.take_log().into_iter().map(|l| l.text).collect();
    assert_eq!(printed, ["Wave true Begin F", "Wave false End F"]);
}

#[test]
fn part_settings_are_kept_and_searches_skip_unqueryable_parts() {
    let printed = run(r#"
        local crate = workspace.Crate
        print(crate.CanTouch, crate.CanQuery, crate.CastShadow, crate.Massless, crate.Locked, crate.Reflectance)
        print(crate.CollisionGroup)
        crate.CastShadow = false
        crate.Reflectance = 2
        print(crate.CastShadow, crate.Reflectance)
        local down = Vector3.new(0, -50, 0)
        print(workspace:Raycast(Vector3.new(0, 20, 0), down).Instance.Name)
        crate.CanQuery = false
        print(workspace:Raycast(Vector3.new(0, 20, 0), down).Instance.Name)
        print(#workspace:GetPartBoundsInRadius(Vector3.new(0, 5, 0), 1))
    "#);
    assert_eq!(printed, ["true true true false false 0", "Default", "false 1", "Crate", "Floor", "0"]);
}

#[test]
fn parts_in_groups_that_dont_collide_fall_through_each_other() {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let floor = tree.create(ClassKind::Part, "Floor").unwrap();
    tree.set(floor, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, -0.5, 0.0))).unwrap();
    tree.set(floor, Property::Size, PropertyValue::Vec3(Vec3::new(100.0, 1.0, 100.0))).unwrap();
    tree.set(floor, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    tree.set_parent(floor, Some(ws)).unwrap();
    let mut boxes = Vec::new();
    for (name, x) in [("Ghost", 0.0), ("Solid", 6.0)] {
        let p = tree.create(ClassKind::Part, name).unwrap();
        tree.set(p, Property::Position, PropertyValue::Vec3(Vec3::new(x, 3.0, 0.0))).unwrap();
        tree.set(p, Property::Size, PropertyValue::Vec3(Vec3::new(2.0, 2.0, 2.0))).unwrap();
        tree.set(p, Property::Anchored, PropertyValue::Bool(false)).unwrap();
        tree.set_parent(p, Some(ws)).unwrap();
        boxes.push(p);
    }
    let script = tree.create(ClassKind::Script, "Groups").unwrap();
    let source = r#"
        local physics = game:GetService("PhysicsService")
        physics:RegisterCollisionGroup("Ghosts")
        physics:CollisionGroupSetCollidable("Ghosts", "Default", false)
        workspace.Ghost.CollisionGroup = "Ghosts"
        print(physics:CollisionGroupsAreCollidable("Default", "Ghosts"), #physics:GetRegisteredCollisionGroups())
    "#;
    tree.set(script, Property::Source, PropertyValue::String(source.into())).unwrap();
    tree.set_parent(script, Some(ws)).unwrap();
    let mut session = start_on(tree, false).unwrap();
    for _ in 0..60 {
        session.step(1.0 / 60.0, CharacterInput::default());
    }
    let printed: Vec<String> = session.take_log().into_iter().map(|l| l.text).collect();
    assert_eq!(printed, ["false 2"]);
    let t = session.runtime().tree().clone();
    let y = |id| t.borrow().get_vec3(id, Property::Position).unwrap().y;
    assert!(y(boxes[0]) < -5.0, "the ghost fell through: {}", y(boxes[0]));
    assert!((y(boxes[1]) - 1.0).abs() < 0.1, "the solid one landed: {}", y(boxes[1]));
}

#[test]
fn parts_that_cant_touch_dont_fire_touched() {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    for (name, y, anchored) in [("Floor", -0.5, true), ("Quiet", 3.0, false), ("Loud", 3.0, false)] {
        let p = tree.create(ClassKind::Part, name).unwrap();
        let x = if name == "Loud" { 6.0 } else { 0.0 };
        let size = if name == "Floor" { Vec3::new(100.0, 1.0, 100.0) } else { Vec3::new(2.0, 2.0, 2.0) };
        tree.set(p, Property::Position, PropertyValue::Vec3(Vec3::new(x, y, 0.0))).unwrap();
        tree.set(p, Property::Size, PropertyValue::Vec3(size)).unwrap();
        tree.set(p, Property::Anchored, PropertyValue::Bool(anchored)).unwrap();
        tree.set_parent(p, Some(ws)).unwrap();
    }
    let script = tree.create(ClassKind::Script, "Touches").unwrap();
    let source = r#"
        workspace.Quiet.CanTouch = false
        for _, name in { "Quiet", "Loud" } do
            workspace[name].Touched:Connect(function(other)
                if other.Name == "Floor" then print(name) end
            end)
        end
    "#;
    tree.set(script, Property::Source, PropertyValue::String(source.into())).unwrap();
    tree.set_parent(script, Some(ws)).unwrap();
    let mut session = start_on(tree, false).unwrap();
    for _ in 0..60 {
        session.step(1.0 / 60.0, CharacterInput::default());
    }
    let printed: Vec<String> = session.take_log().into_iter().map(|l| l.text).collect();
    assert_eq!(printed, ["Loud"]);
}

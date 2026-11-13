//! More of the API that scripts written for other engines use: welds and
//! motors, cancelling threads, ancestry events, class families, humanoid
//! states and the lenient layer's stand-ins.

use libroxel_engine::CharacterInput;
use libroxel_engine::session::start_on;
use libroxel_instance::{ClassKind, InstanceTree, Property, PropertyValue, Vec3};

/// Runs each source as its own script in a world with a floor and a crate,
/// steps `frames` frames, and returns what they printed. Fails on any
/// script error.
fn run_for(sources: &[&str], frames: usize) -> Vec<String> {
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
    for (i, source) in sources.iter().enumerate() {
        let script = tree.create(ClassKind::Script, format!("Test{i}")).unwrap();
        tree.set(script, Property::Source, PropertyValue::String((*source).into())).unwrap();
        tree.set_parent(script, Some(ws)).unwrap();
    }
    let mut session = start_on(tree, false).unwrap();
    for _ in 0..frames {
        session.step(1.0 / 60.0, CharacterInput::default());
    }
    let log = session.take_log();
    assert!(log.iter().all(|l| l.level != libroxel_engine::LogLevel::Error), "{log:?}");
    log.into_iter().map(|l| l.text).collect()
}

fn run(source: &str) -> Vec<String> {
    run_for(&[source], 10)
}

#[test]
fn a_weld_holds_its_part_at_c0_and_moves_it_when_c0_changes() {
    let printed = run_for(
        &[r#"
        local base = workspace.Crate
        local lid = Instance.new("Part")
        lid.Size = Vector3.new(2, 0.2, 2)
        lid.Anchored = false
        lid.Position = Vector3.new(30, 30, 30)
        lid.Parent = workspace
        local weld = Instance.new("Weld")
        weld.Part0 = base
        weld.Part1 = lid
        weld.C0 = CFrame.new(0, 1.1, 0)
        weld.Parent = base
        task.wait(0.2)
        print(string.format("%.2f %.2f %.2f", lid.Position.X, lid.Position.Y, lid.Position.Z))
        -- Swing it open: C0 turns it about the crate's back edge.
        weld.C0 = CFrame.new(0, 1, -1) * CFrame.Angles(math.rad(90), 0, 0) * CFrame.new(0, 0.1, 1)
        task.wait(0.2)
        print(string.format("%.2f %.2f %.2f", lid.Position.X, lid.Position.Y, lid.Position.Z))
        print(weld:IsA("JointInstance"), weld.C1 == CFrame.new())
    "#],
        40,
    );
    assert_eq!(printed, vec!["0.00 6.10 0.00", "0.00 5.00 -0.90", "true true"]);
}

#[test]
fn a_motor_turns_towards_its_desired_angle() {
    let printed = run_for(
        &[r#"
        local hub = workspace.Crate
        local arm = Instance.new("Part")
        arm.Size = Vector3.new(4, 0.5, 0.5)
        arm.Anchored = false
        arm.Parent = workspace
        local motor = Instance.new("Motor6D")
        motor.Part0 = hub
        motor.Part1 = arm
        motor.C1 = CFrame.new(-2, 0, 0) -- the arm's end sits on the hub
        motor.MaxVelocity = 0.1
        motor.Parent = hub
        task.wait(0.1)
        print(string.format("%.2f %.2f", arm.Position.X, arm.Position.Y))
        motor.DesiredAngle = math.pi / 2
        task.wait(1)
        print(string.format("%.2f %.2f", motor.CurrentAngle, arm.Position.Y), math.abs(arm.Position.X) < 0.01)
    "#],
        90,
    );
    assert_eq!(printed, vec!["2.00 5.00", "1.57 7.00 true"]);
}

#[test]
fn cancelled_threads_never_run_and_spawn_hands_back_the_thread() {
    let printed = run_for(
        &[r#"
        local later = task.delay(0.05, function() print("too late") end)
        task.cancel(later)
        local waiting = task.spawn(function()
            task.wait(0.05)
            print("woke up")
        end)
        print(typeof(later), typeof(waiting))
        task.cancel(waiting)
        task.delay(0.05, print, "still here")
    "#],
        20,
    );
    assert_eq!(printed, vec!["thread thread", "still here"]);
}

#[test]
fn ancestry_changed_fires_for_the_moved_thing_and_what_is_inside_it() {
    let printed = run(r#"
        local box = Instance.new("Model")
        local inside = Instance.new("Part", box)
        task.wait()
        box.AncestryChanged:Connect(function(child, parent)
            print("box", child == box, parent and parent.Name)
        end)
        inside.AncestryChanged:Connect(function(child, parent)
            print("inside", child == box, parent and parent.Name)
        end)
        box.Parent = workspace
        task.wait()
        box.Parent = nil
    "#);
    assert_eq!(
        printed,
        vec!["box true Workspace", "inside true Workspace", "box true nil", "inside true nil"]
    );
}

#[test]
fn families_of_classes_and_parts_that_stand_in_for_others() {
    let printed = run(r#"
        local wedge = Instance.new("WedgePart")
        local mesh = Instance.new("MeshPart")
        print(wedge.ClassName, wedge.Shape == Enum.PartType.Wedge, mesh:IsA("BasePart"))
        print(workspace.Crate:IsA("BasePart"), workspace.Crate:IsA("PVInstance"), workspace:IsA("Instance"))
        print(Instance.new("TextButton"):IsA("GuiButton"), Instance.new("IntValue"):IsA("ValueBase"))
        print(Instance.new("Folder"):IsA("BasePart"), Instance.new("Sky").ClassName)
        local model = Instance.new("Model", workspace)
        local a = Instance.new("Part", model)
        local b = Instance.new("Part", model)
        b.Position = Vector3.new(10, 0, 0)
        model.PrimaryPart = b
        print(model.PrimaryPart == b, model:GetPivot().Position.X)
    "#);
    assert_eq!(printed, vec!["Part true true", "true true true", "true true", "false Sky", "true 10"]);
}

#[test]
fn humanoid_states_change_and_tell_whoever_listens() {
    let printed = run(r#"
        local h = Instance.new("Humanoid")
        print(h:GetState() == Enum.HumanoidStateType.Running, Enum.HumanoidStateType.Dead.Value)
        h.StateChanged:Connect(function(old, new)
            print("from", old.Name, "to", new.Name)
        end)
        h:SetStateEnabled(Enum.HumanoidStateType.Climbing, false)
        print(h:GetStateEnabled(Enum.HumanoidStateType.Climbing), h:GetStateEnabled(Enum.HumanoidStateType.Jumping))
        h:ChangeState(Enum.HumanoidStateType.Jumping)
        task.wait()
        print(h.HipHeight, h.AutoRotate, h.RootPart)
    "#);
    assert_eq!(printed, vec!["true 15", "false true", "from Running to Jumping", "2 true nil"]);
}

#[test]
fn members_libroxel_lacks_remember_what_scripts_set() {
    let printed = run(r#"
        local label = Instance.new("TextLabel")
        print(label.RichText, label.Font == Enum.Font.SourceSans, label.TextStrokeTransparency)
        label.RichText = true
        label.Font = Enum.Font.GothamBold
        print(label.RichText, label.Font.Name, tostring(label.Font))
        local part = workspace.Crate
        part.TopSurface = Enum.SurfaceType.Studs
        print(part.TopSurface.Name, part.BottomSurface.Name, part.Archivable)
        part.Rotation = Vector3.new(0, 90, 0)
        print(part.Orientation.Y)
        local ok, err = pcall(function() return part.Bogus end)
        print(ok, string.match(tostring(err), "Bogus is not a valid member of Crate") ~= nil)
        ok, err = pcall(function() part.Bogus = 1 end)
        print(ok, string.match(tostring(err), "Bogus is not a valid member of Crate") ~= nil)
    "#);
    assert_eq!(
        printed,
        vec![
            "false true 1",
            "true GothamBold Enum.Font.GothamBold",
            "Studs Smooth true",
            "90",
            "false true",
            "false true",
        ]
    );
}

#[test]
fn services_libroxel_stands_in_for() {
    let printed = run(r#"
        print(game:GetService("ReplicatedFirst") == game:GetService("ReplicatedStorage"))
        print(game:FindService("NoSuchService"), game:FindService("Players") == game:GetService("Players"))
        local map = game:GetService("MemoryStoreService"):GetSortedMap("scores")
        map:SetAsync("b", 2, 60)
        map:SetAsync("a", 1, 60)
        local rows = map:GetRangeAsync(Enum.SortDirection.Ascending, 10)
        print(#rows, rows[1].key, rows[2].value)
        local board = game:GetService("DataStoreService"):GetOrderedDataStore("Wins")
        board:SetAsync("ana", 5)
        board:SetAsync("sam", 9)
        board:IncrementAsync("ana", 10)
        local top = board:GetSortedAsync(false, 10):GetCurrentPage()
        print(top[1].key, top[1].value, top[2].key)
        local gui = game:GetService("StarterGui")
        gui:SetCoreGuiEnabled(Enum.CoreGuiType.Backpack, false)
        print(gui:GetCoreGuiEnabled(Enum.CoreGuiType.Backpack), gui:GetCoreGuiEnabled(Enum.CoreGuiType.Chat))
        local tr = game:GetService("LocalizationService"):GetTranslatorForPlayerAsync(nil)
        print(tr:Translate(nil, "Hello"), game:GetService("TextChatService"):CanUserChatAsync(1))
        print(pcall(function() return game:GetService("GroupService").Nope end))
        local ran = 0
        game:GetService("RunService"):BindToRenderStep("count", Enum.RenderPriority.Camera.Value, function()
            ran += 1
        end)
        task.wait(0.05)
        game:GetService("RunService"):UnbindFromRenderStep("count")
        local before = ran
        task.wait(0.05)
        print(before > 0, ran == before)
        local uis = game:GetService("UserInputService")
        print(uis.MouseBehavior.Name, uis.TouchEnabled, typeof(uis:GetMouseLocation()))
        uis.MouseBehavior = Enum.MouseBehavior.LockCenter
        print(uis.MouseBehavior.Name)
    "#);
    assert_eq!(
        printed,
        vec![
            "true",
            "nil true",
            "2 a 2",
            "ana 15 sam",
            "false true",
            "Hello true",
            "false Nope is not a valid member of GroupService",
            "true true",
            "Default false Vector2",
            "LockCenter",
        ]
    );
}

#[test]
fn regions_shared_tables_and_old_globals() {
    let printed = run_for(
        &[
            r#"
        shared.score = 42
        _G.greeting = "hi"
        local region = Region3.new(Vector3.new(-2, 3, -2), Vector3.new(2, 7, 2))
        print(region.Size, region.CFrame.Position)
        local found = workspace:FindPartsInRegion3(region, nil, 10)
        print(#found, found[1] and found[1].Name)
        print(Game == game, Workspace == workspace, Vector3int16.new(1.7, 2.2, -0.5))
        local r = Rect.new(0, 0, 10, 5)
        print(r.Width, r.Height)
    "#,
            r#"
        task.wait(0.05)
        print(shared.score, _G.greeting)
    "#,
        ],
        20,
    );
    assert_eq!(printed, vec!["4, 4, 4 0, 5, 0", "1 Crate", "true true 1, 2, -1", "10 5", "42 hi"]);
}

/// A loose part held up in the air at `at` by an AlignPosition, for turning tests.
const HELD: &str = r#"
    local function held(at)
        local p = Instance.new("Part")
        p.Size = Vector3.new(4, 1, 2)
        p.Anchored = false
        p.Position = at
        p.Parent = workspace
        local a = Instance.new("Attachment", p)
        local hold = Instance.new("AlignPosition")
        hold.Attachment0 = a
        hold.Position = at
        hold.MaxForce = 1e6
        hold.Responsiveness = 50
        hold.Parent = p
        return p, a
    end
"#;

#[test]
fn align_orientation_and_body_gyro_turn_a_part_to_face_a_way() {
    let source = format!(
        r#"{HELD}
        local p, a = held(Vector3.new(20, 10, 0))
        local align = Instance.new("AlignOrientation")
        align.Attachment0 = a
        align.CFrame = CFrame.Angles(0, math.rad(90), 0)
        align.Responsiveness = 20
        align.Parent = p
        local q = held(Vector3.new(-20, 10, 0))
        local gyro = Instance.new("BodyGyro")
        gyro.MaxTorque = Vector3.new(1e6, 1e6, 1e6)
        gyro.CFrame = CFrame.Angles(0, math.rad(-45), 0)
        gyro.Parent = q
        task.wait(2)
        print(math.floor(p.Orientation.Y + 0.5), math.floor(q.Orientation.Y + 0.5))
        print(math.abs(p.Orientation.X) < 2, math.abs(q.Orientation.Z) < 2)
    "#
    );
    let printed = run_for(&[&source], 150);
    assert_eq!(printed, vec!["90 -45", "true true"]);
}

#[test]
fn a_torque_spins_a_part_up() {
    let source = format!(
        r#"{HELD}
        local p, a = held(Vector3.new(0, 20, 0))
        local t = Instance.new("Torque")
        t.Attachment0 = a
        t.Torque = Vector3.new(0, 50, 0)
        t.Parent = p
        local before = p.Orientation.Y
        task.wait(1)
        print(math.abs(p.Orientation.Y - before) > 5, typeof(t.Torque))
    "#
    );
    assert_eq!(run_for(&[&source], 75), vec!["true Vector3"]);
}

#[test]
fn a_rod_keeps_its_length_and_no_collision_lets_parts_through() {
    let printed = run_for(
        &[r#"
        local hub = workspace.Crate
        local ball = Instance.new("Part")
        ball.Shape = Enum.PartType.Ball
        ball.Size = Vector3.new(1, 1, 1)
        ball.Anchored = false
        ball.Position = Vector3.new(4, 5, 0)
        ball.Parent = workspace
        local a0 = Instance.new("Attachment", hub)
        local a1 = Instance.new("Attachment", ball)
        local rod = Instance.new("RodConstraint")
        rod.Attachment0 = a0
        rod.Attachment1 = a1
        rod.Length = 4
        rod.Parent = hub
        -- A brick dropped on the crate falls straight through it.
        local brick = Instance.new("Part")
        brick.Size = Vector3.new(1, 1, 1)
        brick.Anchored = false
        brick.Position = Vector3.new(0, 9, 0.5)
        brick.Parent = workspace
        local nc = Instance.new("NoCollisionConstraint")
        nc.Part0 = brick
        nc.Part1 = hub
        nc.Parent = brick
        task.wait(1.5)
        print(string.format("%.1f", (ball.Position - hub.Position).Magnitude), ball.Position.Y < 4)
        print(brick.Position.Y < 2)
    "#],
        110,
    );
    assert_eq!(printed, vec!["4.0 true", "true"]);
}

#[test]
fn gradients_keep_both_ends_of_their_sequences() {
    let printed = run(r#"
        local frame = Instance.new("Frame")
        local g = Instance.new("UIGradient", frame)
        g.Color = ColorSequence.new(Color3.new(1, 0, 0), Color3.new(0, 0, 1))
        g.Transparency = NumberSequence.new(0, 0.5)
        g.Rotation = 90
        local k = g.Color.Keypoints
        print(#k, k[1].Value.R, k[2].Value.B, g.Transparency.Keypoints[2].Value, g.Rotation)
        g.Color = Color3.new(0, 1, 0)
        print(g.Color.Keypoints[2].Value.G, typeof(g.Offset))
    "#);
    assert_eq!(printed, vec!["2 1 1 0.5 90", "1 Vector2"]);
}

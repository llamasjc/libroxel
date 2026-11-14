//! Roblox classes, properties and enumerations libroxel has no engine side
//! for still work in scripts, and the helpers games lean on (require from
//! several scripts at once, dialogs, terrain voxels) behave as on Roblox.

use libroxel_instance::{ClassKind, InstanceTree, Property, PropertyValue};
use libroxel_script::ScriptRuntime;

fn tree_with_player() -> (InstanceTree, libroxel_instance::InstanceId) {
    let mut tree = InstanceTree::new();
    let players = tree.service(ClassKind::Players).unwrap();
    let player = tree.create(ClassKind::Player, "Player1").unwrap();
    tree.set(player, Property::UserId, PropertyValue::Number(7.0)).unwrap();
    tree.set_parent(player, Some(players)).unwrap();
    let gui = tree.create(ClassKind::PlayerGui, "PlayerGui").unwrap();
    tree.set_parent(gui, Some(player)).unwrap();
    (tree, player)
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
fn any_roblox_class_can_be_made_and_keeps_its_properties() {
    let rt = ScriptRuntime::new_with_tree(tree_with_player().0).unwrap();
    run(
        &rt,
        r#"
        local d = Instance.new("Dialog")
        print(d.ClassName, d.Name, d:IsA("Dialog"), d:IsA("Instance"), d:IsA("Part"))
        print(d.ConversationDistance, d.InitialPrompt == "", d.Purpose == Enum.DialogPurpose.Help)
        d.InitialPrompt = "Hello there"
        d.Purpose = Enum.DialogPurpose.Shop
        print(d.InitialPrompt, d.Purpose.Name)
        local h = Instance.new("HumanoidDescription")
        h.HeadColor = Color3.new(1, 0, 0)
        print(h.HeadColor.R, h.ClassName)
        local m = Instance.new("Model")
        d.Parent = m
        print(m:FindFirstChildOfClass("Dialog") == d, m:FindFirstChildWhichIsA("Instance") == d)
        print(game.Lighting.TimeOfDay ~= nil, Enum.RunContext.Server.Name, Enum.KeyCode:FromName("E").Name)
        local v = Instance.new("ViewportFrame")
        local cam = Instance.new("Camera")
        v.CurrentCamera = cam
        print(v.ClassName, v.CurrentCamera == cam, v:IsA("GuiObject"), v:IsA("Frame"))
        "#,
    );
    assert_eq!(
        printed(&rt),
        [
            "Dialog Dialog true true false",
            "25 true true",
            "Hello there Shop",
            "1 HumanoidDescription",
            "true true",
            "true Server E",
            "ViewportFrame true true false",
        ]
    );
}

#[test]
fn kept_properties_are_saved_with_the_place() {
    let rt = ScriptRuntime::new_with_tree(tree_with_player().0).unwrap();
    run(
        &rt,
        r#"
        local d = Instance.new("Dialog")
        d.InitialPrompt = "Buy something?"
        d.Parent = workspace
        "#,
    );
    let text = libroxel_instance::place::save(&rt.tree().borrow()).unwrap();
    let (loaded, _) = libroxel_instance::place::load(&text).unwrap();
    let rt = ScriptRuntime::new_with_tree(loaded).unwrap();
    run(&rt, r#"print(workspace.Dialog.ClassName, workspace.Dialog.InitialPrompt)"#);
    assert_eq!(printed(&rt), ["Dialog Buy something?"]);
}

#[test]
fn a_module_still_loading_is_waited_for_not_a_loop() {
    let rt = ScriptRuntime::new_with_tree(tree_with_player().0).unwrap();
    run(
        &rt,
        r#"
        local m = Instance.new("ModuleScript")
        m.Name = "Slow"
        m.Source = "task.wait(0.2) return { value = 5 }"
        m.Parent = game:GetService("ReplicatedStorage")
        "#,
    );
    for name in ["a", "b"] {
        let src = format!(
            r#"local v = require(game:GetService("ReplicatedStorage").Slow) print("{name}", v.value)"#
        );
        run(&rt, &src);
    }
    for _ in 0..10 {
        let errors = rt.step(0.05);
        assert!(errors.is_empty(), "{errors:?}");
    }
    let mut out = printed(&rt);
    out.sort();
    assert_eq!(out, ["a 5", "b 5"]);
}

#[test]
fn a_real_require_loop_is_still_reported() {
    let rt = ScriptRuntime::new_with_tree(tree_with_player().0).unwrap();
    run(
        &rt,
        r#"
        local a = Instance.new("ModuleScript")
        a.Name = "A"
        a.Source = "return require(script.Parent.B)"
        a.Parent = workspace
        local b = Instance.new("ModuleScript")
        b.Name = "B"
        b.Source = "return require(script.Parent.A)"
        b.Parent = workspace
        local ok, err = pcall(require, a)
        print(ok, string.find(tostring(err), "require loop") ~= nil)
        "#,
    );
    assert_eq!(printed(&rt), ["false true"]);
}

#[test]
fn scripts_and_local_scripts_know_their_side() {
    let (mut tree, _) = tree_with_player();
    let workspace = tree.workspace();
    let server = tree.create(ClassKind::Script, "Server").unwrap();
    let src =
        r#"local rs = game:GetService("RunService") print(script.ClassName, rs:IsServer(), rs:IsClient())"#;
    tree.set(server, Property::Source, PropertyValue::String(src.into())).unwrap();
    tree.set_parent(server, Some(workspace)).unwrap();
    let rt = ScriptRuntime::new_with_tree(tree).unwrap();
    assert!(rt.run_tree_scripts().is_empty());
    assert_eq!(printed(&rt), ["Script true false"]);
}

#[test]
fn talking_to_a_dialog_shows_its_prompt_and_choices() {
    let (mut tree, player) = tree_with_player();
    let workspace = tree.workspace();
    let head = tree.create(ClassKind::Part, "Head").unwrap();
    tree.set_parent(head, Some(workspace)).unwrap();
    let dialog = tree.create(ClassKind::Other, "Dialog").unwrap();
    tree.set(dialog, Property::OtherClass, PropertyValue::String("Dialog".into())).unwrap();
    tree.set_extra(dialog, "InitialPrompt", Some(PropertyValue::String("Need a sword?".into()))).unwrap();
    tree.set_parent(dialog, Some(head)).unwrap();
    let choice = tree.create(ClassKind::Other, "Yes").unwrap();
    tree.set(choice, Property::OtherClass, PropertyValue::String("DialogChoice".into())).unwrap();
    tree.set_extra(choice, "UserDialog", Some(PropertyValue::String("Yes please".into()))).unwrap();
    tree.set_extra(choice, "ResponseDialog", Some(PropertyValue::String("Here you go".into()))).unwrap();
    tree.set_parent(choice, Some(dialog)).unwrap();
    let rt = ScriptRuntime::new_with_tree(tree).unwrap();
    assert!(rt.run_tree_scripts().is_empty());
    run(
        &rt,
        r#"workspace.Head.Dialog.DialogChoiceSelected:Connect(function(p, c) print("picked", p.Name, c.Name) end)"#,
    );
    let prompt = rt.tree().borrow().find_first_child(head, "DialogPrompt").unwrap().expect("a Talk prompt");
    assert!(rt.prompt_triggered(prompt, player).is_empty());
    run(
        &rt,
        r#"
        local gui = game.Players.Player1.PlayerGui:FindFirstChild("Dialog")
        local frame = gui:FindFirstChildOfClass("Frame")
        local texts = {}
        for _, c in ipairs(frame:GetChildren()) do table.insert(texts, c.Text) end
        print(table.concat(texts, " | "))
        "#,
    );
    assert_eq!(printed(&rt), ["Need a sword? | Yes please | Goodbye!"]);
}

#[test]
fn terrain_voxels_can_be_written_and_read_back() {
    let rt = ScriptRuntime::new_with_tree(tree_with_player().0).unwrap();
    run(
        &rt,
        r#"
        local t = workspace.Terrain
        local r = Region3.new(Vector3.new(0, 0, 0), Vector3.new(8, 4, 4))
        t:WriteVoxels(r, 4, { { { Enum.Material.Grass } }, { { Enum.Material.Sand } } }, { { { 1 } }, { { 0.5 } } })
        local m, o = t:ReadVoxels(r, 4)
        print(m.Size, m[1][1][1].Name, o[1][1][1], m[2][1][1].Name, o[2][1][1])
        t:ReplaceMaterial(r, 4, Enum.Material.Grass, Enum.Material.Rock)
        m = t:ReadVoxels(r, 4)
        print(m[1][1][1].Name, t:WorldToCell(Vector3.new(5, 1, -1)))
        t:SetMaterialColor(Enum.Material.Grass, Color3.new(1, 0, 0))
        print(t:GetMaterialColor(Enum.Material.Grass).R, t:GetMaterialColor(Enum.Material.Mud).R > 0)
        "#,
    );
    assert_eq!(printed(&rt), ["2, 1, 1 Grass 1 Sand 0.5", "Rock 1, 0, -1", "1 true"]);
}

#[test]
fn tweens_move_members_libroxel_keeps_by_name() {
    let rt = ScriptRuntime::new_with_tree(tree_with_player().0).unwrap();
    run(
        &rt,
        r#"
        local f = Instance.new("Frame")
        shared.f = f
        game:GetService("TweenService"):Create(f, TweenInfo.new(1, Enum.EasingStyle.Linear), { Rotation = 90 }):Play()
        "#,
    );
    for _ in 0..10 {
        assert!(rt.step(0.05).is_empty());
    }
    run(&rt, "print(math.floor(shared.f.Rotation + 0.5))");
    assert_eq!(printed(&rt), ["45"]);
}

#[test]
fn raycast_params_have_every_setting() {
    let rt = ScriptRuntime::new_with_tree(tree_with_player().0).unwrap();
    run(
        &rt,
        r#"
        local p = RaycastParams.new()
        p.IgnoreWater = true
        p.RespectCanCollide = true
        p.CollisionGroup = "Bullets"
        p:AddToFilter(workspace)
        print(p.IgnoreWater, p.RespectCanCollide, p.CollisionGroup, #p.FilterDescendantsInstances)
        "#,
    );
    assert_eq!(printed(&rt), ["true true Bullets 1"]);
}

#[test]
fn members_beyond_saved_properties_have_change_events() {
    let rt = ScriptRuntime::new_with_tree(tree_with_player().0).unwrap();
    run(
        &rt,
        r#"
        workspace:GetPropertyChangedSignal("CurrentCamera"):Connect(function() end)
        workspace.CurrentCamera:GetPropertyChangedSignal("ViewportSize"):Connect(function()
            print("viewport", workspace.CurrentCamera.ViewportSize.X)
        end)
        local d = Instance.new("Dialog")
        d:GetPropertyChangedSignal("InitialPrompt"):Connect(function() print("prompt", d.InitialPrompt) end)
        d.InitialPrompt = "Hi"
        local ok, err = pcall(function() d:GetPropertyChangedSignal("Bogus") end)
        print(ok, tostring(err):find("Bogus is not a valid member of Dialog") ~= nil)
        "#,
    );
    rt.set_viewport(800.0, 600.0);
    for _ in 0..2 {
        assert!(rt.step(0.05).is_empty());
    }
    let mut out = printed(&rt);
    out.sort();
    assert_eq!(out, ["false true", "prompt Hi", "viewport 800"]);
}

#[test]
fn settings_and_services_act_like_instances() {
    let rt = ScriptRuntime::new_with_tree(tree_with_player().0).unwrap();
    run(
        &rt,
        r#"
        local gs = UserSettings().GameSettings
        gs:GetPropertyChangedSignal("SavedQualityLevel"):Connect(function() print("quality", gs.SavedQualityLevel.Name) end)
        gs.SavedQualityLevel = Enum.SavedQualitySetting.QualityLevel5
        settings().Rendering:GetPropertyChangedSignal("QualityLevel"):Connect(function() end)
        local gui = game:GetService("GuiService")
        gui.MenuOpened:Connect(function() end)
        game:GetService("ContextActionService"):SetTitle("x", "y")
        local chat = game:GetService("TextChatService")
        chat:GetPropertyChangedSignal("ChatVersion"):Connect(function() end)
        chat:SetAttribute("Muted", true)
        print(chat:GetAttribute("Muted"), chat:IsA("Instance"), gui:IsA("GuiService"))
        local h = Instance.new("Highlight")
        h.Parent = chat
        print(h.Parent == chat)
        local ok, err = pcall(function() return gui.NotAThing end)
        print(ok, tostring(err):find("NotAThing is not a valid member of GuiService") ~= nil)
        "#,
    );
    assert!(rt.step(0.05).is_empty());
    let mut out = printed(&rt);
    out.sort();
    assert_eq!(out, ["false true", "quality QualityLevel5", "true", "true true true"]);
}

#[test]
fn more_of_the_script_library_is_there() {
    let rt = ScriptRuntime::new_with_tree(tree_with_player().0).unwrap();
    run(
        &rt,
        r#"
        debug.profilebegin("tick") debug.profileend()
        task.desynchronize() task.synchronize()
        local a, b = Vector2.new(3, 4), Vector2.new(1, 0)
        print(a:Dot(b), a:Cross(b), a.Unit.X, a:Max(Vector2.new(5, 0)).X, a:Lerp(b, 0.5).X)
        print(Vector3.FromNormalId(Enum.NormalId.Front).Z, Vector3.FromAxis(Enum.Axis.Y).Y)
        local v = Random.new(3):NextUnitVector()
        print(math.abs(v.Magnitude - 1) < 1e-4)
        local d = DateTime.fromUniversalTime(2024, 3, 5, 14, 7, 9)
        print(d:FormatUniversalTime("dddd, MMMM D YYYY HH:mm:ss", "en-us"), d:FormatLocalTime("LT", "en-us"))
        local p = Instance.new("Part", workspace)
        p:PivotTo(CFrame.new(1, 2, 3))
        print(p:GetPivot().Position.Y, p.Position.X)
        local actor = Instance.new("Actor")
        print(actor.Name, actor.ClassName)
        actor:BindToMessage("go", function(n) print("message", n) end)
        actor:SendMessage("go", 5)
        "#,
    );
    assert!(rt.step(0.05).is_empty());
    assert_eq!(
        printed(&rt),
        [
            "3 -4 0.6000000238418579 5 2",
            "-1 1",
            "true",
            "Tuesday, March 5 2024 14:07:09 2:07 PM",
            "2 1",
            "Actor Actor",
            "message 5",
        ]
    );
}

#[test]
fn attributes_hold_any_roblox_value() {
    let rt = ScriptRuntime::new_with_tree(tree_with_player().0).unwrap();
    run(
        &rt,
        r#"
        local p = Instance.new("Part")
        p:SetAttribute("Spot", CFrame.new(1, 2, 3))
        p:SetAttribute("Team", BrickColor.new("Bright red"))
        p:SetAttribute("Range", NumberRange.new(2, 5))
        p:SetAttribute("Kind", Enum.Material.Wood)
        print(p:GetAttribute("Spot").Y, p:GetAttribute("Team").Name, p:GetAttribute("Range").Max, p:GetAttribute("Kind").Name)
        local c = p:Clone()
        print(c:GetAttribute("Spot").Z, p:GetAttributes().Range.Min)
        "#,
    );
    assert_eq!(printed(&rt), ["2 Bright red 5 Wood", "3 2"]);
}

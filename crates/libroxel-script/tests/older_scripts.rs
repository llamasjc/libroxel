//! Spellings and members that older scripts and free models lean on, and
//! value-type corners that scripts written for other engines expect.

use libroxel_script::ScriptRuntime;

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
fn vector_components_by_a_key_worked_out_while_running() {
    let rt = runtime();
    run(
        &rt,
        r##"
        local v = Vector3.new(1, 2, 3)
        local out = {}
        for _, axis in { "X", "Y", "Z", "x", "y", "z" } do
            table.insert(out, v[axis])
        end
        print(table.concat(out, " "))
        local key = "magnitude"
        print(v[key] == v.Magnitude, v.unit == v.Unit)
        print(pcall(function() return v[("W")] end))
    "##,
    );
    let out = printed(&rt);
    assert_eq!(out[0], "1 2 3 1 2 3");
    assert_eq!(out[1], "true true");
    assert!(out[2].starts_with("false"), "{}", out[2]);
}

#[test]
fn cframe_keeps_its_older_spellings() {
    let rt = runtime();
    run(
        &rt,
        r##"
        local cf = CFrame.new(1, 2, 3) * CFrame.Angles(0, math.pi / 2, 0)
        print(cf.p == cf.Position, cf.lookVector == cf.LookVector, cf.rightVector == cf.RightVector, cf.upVector == cf.UpVector)
        print(cf:inverse() == cf:Inverse(), cf:lerp(CFrame.new(), 0.5) == cf:Lerp(CFrame.new(), 0.5))
        local other = CFrame.new(4, 5, 6)
        print(cf:toWorldSpace(other) == cf:ToWorldSpace(other), cf:toObjectSpace(other) == cf:ToObjectSpace(other))
        local p = Vector3.new(1, 0, 0)
        print(cf:pointToWorldSpace(p) == cf:PointToWorldSpace(p), cf:pointToObjectSpace(p) == cf:PointToObjectSpace(p))
        print(cf:vectorToWorldSpace(p) == cf:VectorToWorldSpace(p), cf:vectorToObjectSpace(p) == cf:VectorToObjectSpace(p))
        local a, b, c = cf:ToEulerAngles()
        local x, y, z = cf:ToEulerAnglesXYZ()
        print(a == x and b == y and c == z, select("#", cf:toAxisAngle()))
    "##,
    );
    assert_eq!(
        printed(&rt),
        ["true true true true", "true true", "true true", "true true", "true true", "true 2"]
    );
}

#[test]
fn time_of_day_is_another_way_to_set_the_clock() {
    let rt = runtime();
    run(
        &rt,
        r##"
        local lighting = game:GetService("Lighting")
        lighting.TimeOfDay = "06:30:00"
        print(lighting.ClockTime, lighting.TimeOfDay)
        lighting.ClockTime = 18.25
        print(lighting.TimeOfDay)
        lighting.TimeOfDay = "25:00:00"
        print(lighting.ClockTime)
    "##,
    );
    assert_eq!(printed(&rt), ["6.5 06:30:00", "18:15:00", "1"]);
}

#[test]
fn enumerations_list_and_name_themselves() {
    let rt = runtime();
    run(
        &rt,
        r##"
        local all = Enum:GetEnums()
        local found = false
        for _, e in all do
            if e == Enum.Material then found = true end
        end
        print(#all > 100, found)
        local item = Enum.Material.Plastic
        print(item:IsA("Material"), item:IsA("PartType"), item.EnumType == Enum.Material, tostring(item.EnumType))
        print(tostring(Enum.Material), Enum.Material:FromName("Plastic") == item)
        print(Enum.DialogPurpose.Help.EnumType == Enum.DialogPurpose, tostring(Enum.DialogPurpose))
    "##,
    );
    assert_eq!(
        printed(&rt),
        ["true true", "true false true Material", "Material true", "true DialogPurpose"]
    );
}

#[test]
fn newer_constructor_spellings() {
    let rt = runtime();
    run(
        &rt,
        r##"
        print(Vector3.fromNormalId(Enum.NormalId.Top), Vector3.fromAxis(Enum.Axis.X))
        local p = Instance.new("Part")
        p.Name = "Source"
        p.Transparency = 0.5
        Instance.new("Decal").Parent = p
        local copy = Instance.fromExisting(p)
        print(copy.ClassName, copy.Name, copy.Transparency, #copy:GetChildren(), #p:GetChildren(), copy ~= p)
    "##,
    );
    assert_eq!(printed(&rt), ["0, 1, 0 1, 0, 0", "Part Source 0.5 0 1 true"]);
}

#[test]
fn older_lower_case_member_names() {
    let rt = runtime();
    run(
        &rt,
        r##"
        local part = Instance.new("Part")
        part.Name = "Brick"
        part.Parent = workspace
        local fired = 0
        local conn = part.Changed:connect(function() fired += 1 end)
        part.Transparency = 0.5
        print(conn.connected, typeof(fired))
        conn:disconnect()
        print(conn.connected, conn.Connected)
        print(workspace:findFirstChild("Brick") == part, #workspace:getChildren() > 0, #workspace:children() > 0)
        print(part:isA("BasePart"), part:isDescendantOf(game), part:clone().Name)
        print(game:service("Players") == game:GetService("Players"), game:getService("Workspace") == workspace)
        print(#game.Players:getPlayers(), game.Players:playerFromCharacter(part))
        game:GetService("Debris"):addItem(Instance.new("Part"), 1)
        local h = Instance.new("Humanoid")
        h:takeDamage(10)
        print(h.Health)
        local s = Instance.new("Sound")
        s:play()
        print(s.Playing)
        s:stop()
        print(s.Playing)
        game.Lighting:setMinutesAfterMidnight(9 * 60)
        print(game.Lighting:getMinutesAfterMidnight())
        local hit = workspace:findPartOnRay(Ray.new(Vector3.new(0, 50, 0), Vector3.new(0, -100, 0)))
        print(hit == part)
        local value = Instance.new("IntValue")
        local seen
        value.changed:connect(function(v) seen = v end)
        value.Value = 5
        Instance.new("Part"):remove()
        print(pcall(function() return part.definitelyNotAMember end))
    "##,
    );
    let _ = rt.step(0.1);
    let out = printed(&rt);
    assert_eq!(out[0], "true number");
    assert_eq!(out[1], "false false");
    assert_eq!(out[2], "true true true");
    assert_eq!(out[3], "true true Brick");
    assert_eq!(out[4], "true true");
    assert_eq!(out[5], "0 nil");
    assert_eq!(out[6], "90");
    assert_eq!(out[7], "true");
    assert_eq!(out[8], "false");
    assert_eq!(out[9], "540");
    assert_eq!(out[10], "true");
    assert!(out[11].starts_with("false"), "{}", out[11]);
}

#[test]
fn a_scrolling_frames_canvas_grows_with_what_is_in_it() {
    let rt = ScriptRuntime::new_with_tree(Default::default()).unwrap();
    run(
        &rt,
        r##"
        local gui = Instance.new("ScreenGui")
        local list = Instance.new("ScrollingFrame")
        list.Size = UDim2.fromOffset(200, 100)
        list.CanvasSize = UDim2.new()
        list.Parent = gui
        Instance.new("UIListLayout").Parent = list
        for i = 1, 6 do
            local row = Instance.new("Frame")
            row.Size = UDim2.new(1, 0, 0, 40)
            row.Parent = list
        end
        print(list.AbsoluteCanvasSize)
        list.AutomaticCanvasSize = Enum.AutomaticSize.Y
        print(list.AbsoluteCanvasSize)
    "##,
    );
    assert_eq!(printed(&rt), ["200, 100", "200, 240"]);
}

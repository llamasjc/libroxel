//! Remote and bindable functions, the web helper service, and the smaller
//! helpers scripts expect: older globals, colour and vector maths, lookups.

use libroxel_instance::{ClassKind, InstanceTree, Property, PropertyValue};
use libroxel_script::ScriptRuntime;

fn runtime_with_player() -> ScriptRuntime {
    let mut tree = InstanceTree::new();
    let players = tree.service(ClassKind::Players).unwrap();
    let player = tree.create(ClassKind::Player, "Player1").unwrap();
    tree.set(player, Property::UserId, PropertyValue::Number(7.0)).unwrap();
    tree.set_parent(player, Some(players)).unwrap();
    ScriptRuntime::new_with_tree(tree).expect("runtime starts")
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
fn remote_functions_answer_the_caller() {
    let rt = runtime_with_player();
    run(
        &rt,
        r#"
        local rf = Instance.new("RemoteFunction")
        rf.Name = "Price"
        rf.Parent = game:GetService("ReplicatedStorage")
        rf.OnServerInvoke = function(player, item)
            task.wait(0.1) -- answering may take a while
            return player.Name .. " wants " .. item, 50
        end
        "#,
    );
    run(
        &rt,
        r#"
        local rf = game:GetService("ReplicatedStorage"):WaitForChild("Price")
        local text, cost = rf:InvokeServer("sword")
        print(text, cost)
        "#,
    );
    assert!(printed(&rt).is_empty(), "the caller waits for the answer");
    for _ in 0..10 {
        assert!(rt.step(0.05).is_empty());
    }
    assert_eq!(printed(&rt), ["Player1 wants sword 50"]);
}

#[test]
fn invoking_waits_until_someone_answers() {
    let rt = runtime_with_player();
    run(
        &rt,
        r#"
        local bf = Instance.new("BindableFunction", workspace)
        task.spawn(function()
            print("got", bf:Invoke(2, 3))
        end)
        task.wait(0.5)
        bf.OnInvoke = function(a, b) return a * b end
        "#,
    );
    assert!(printed(&rt).is_empty());
    for _ in 0..20 {
        assert!(rt.step(0.05).is_empty());
    }
    assert_eq!(printed(&rt), ["got 6"]);
}

#[test]
fn callbacks_can_be_set_but_not_read() {
    let rt = runtime_with_player();
    let err = rt
        .run(
            "test",
            r#"
            local rf = Instance.new("RemoteFunction")
            rf.OnClientInvoke = function() return 1 end
            local _ = rf.OnClientInvoke
            "#,
        )
        .unwrap_err();
    assert!(err.message.contains("can be set but not read"), "{}", err.message);
    let err = rt.run("test", "Instance.new('RemoteFunction').OnServerInvoke = 5").unwrap_err();
    assert!(err.message.contains("expects a function"), "{}", err.message);
    let err = rt.run("test", "Instance.new('BindableFunction'):InvokeServer()").unwrap_err();
    assert!(err.message.contains("not a valid member"), "{}", err.message);
}

#[test]
fn errors_in_an_answer_reach_the_caller() {
    let rt = runtime_with_player();
    run(
        &rt,
        r#"
        local bf = Instance.new("BindableFunction")
        bf.OnInvoke = function() error("out of stock") end
        local ok, err = pcall(function() return bf:Invoke() end)
        print(ok, string.find(err, "out of stock") ~= nil)
        "#,
    );
    assert_eq!(printed(&rt), ["false true"]);
}

#[test]
fn json_round_trips_and_guids_look_right() {
    let rt = runtime_with_player();
    run(
        &rt,
        r#"
        local http = game:GetService("HttpService")
        local text = http:JSONEncode({ name = "Ava", coins = 12, items = { "sword", "shield" } })
        local back = http:JSONDecode(text)
        print(back.name, back.coins, back.items[2], #back.items)
        print(http:JSONEncode({1, 2, 3}))
        local id = http:GenerateGUID(false)
        print(#id, string.match(id, "^%x+%-%x+%-4%x+%-[89AB]%x+%-%x+$") ~= nil)
        print(string.sub(http:GenerateGUID(), 1, 1), http:GenerateGUID() ~= http:GenerateGUID())
        print(http:UrlEncode("a b&c"))
        print(pcall(http.JSONDecode, http, "{nope"))
        "#,
    );
    let out = printed(&rt);
    assert_eq!(out[0], "Ava 12 shield 2");
    assert_eq!(out[1], "[1,2,3]");
    assert_eq!(out[2], "36 true");
    assert_eq!(out[3], "{ true");
    assert_eq!(out[4], "a%20b%26c");
    assert!(out[5].starts_with("false"), "{}", out[5]);
}

#[test]
fn older_globals_and_defer_still_work() {
    let rt = runtime_with_player();
    run(
        &rt,
        r#"
        print(tick() > 1e9, elapsedTime() == time())
        spawn(function() print("spawned") end)
        delay(0.2, function() print("delayed") end)
        task.defer(function(x) print("deferred", x) end, 4)
        print("first")
        "#,
    );
    assert_eq!(printed(&rt), ["true true", "spawned", "first"]);
    rt.step(0.05);
    assert_eq!(printed(&rt), ["deferred 4"]);
    for _ in 0..5 {
        rt.step(0.05);
    }
    assert_eq!(printed(&rt), ["delayed"]);
}

#[test]
fn vector_and_colour_helpers() {
    let rt = runtime_with_player();
    run(
        &rt,
        r##"
        local v = Vector3.new(-1.5, 2.5, 3)
        print(v:Abs(), v:Floor(), v:Ceil(), v:Sign())
        print(v:Min(Vector3.zero), v:Max(Vector3.zero, Vector3.new(0, 5, 0)))
        print(Vector3.new(1, 0, 0):FuzzyEq(Vector3.new(1.000001, 0, 0)), Vector3.zero:FuzzyEq(Vector3.one))
        print(math.round(math.deg(Vector3.xAxis:Angle(Vector3.zAxis))), Vector3.yAxis)
        print(math.round(math.deg(Vector3.xAxis:Angle(Vector3.zAxis, Vector3.yAxis))))
        local red = Color3.fromRGB(255, 0, 0)
        print(red:ToHex(), Color3.fromHex("#00ff00"):ToHex(), Color3.fromHex("fff"):ToHex())
        local h, s, val = Color3.fromHSV(2/3, 1, 1):ToHSV()
        print(math.round(h * 3), s, val, Color3.fromHSV(2/3, 1, 1):ToHex())
        print(red:Lerp(Color3.new(0, 0, 1), 0.5))
        "##,
    );
    assert_eq!(
        printed(&rt),
        [
            "1.5, 2.5, 3 -2, 2, 3 -1, 3, 3 -1, 1, 1",
            "-1.5, 0, 0 0, 5, 3",
            "true false",
            "90 0, 1, 0",
            "-90",
            "ff0000 00ff00 ffffff",
            "2 1 1 0000ff",
            "0.5, 0, 0.5",
        ]
    );
}

#[test]
fn more_ways_to_find_instances() {
    let rt = runtime_with_player();
    run(
        &rt,
        r#"
        local model = Instance.new("Model", workspace)
        local seat = Instance.new("Seat", model)
        local folder = Instance.new("Folder", seat)
        print(model:FindFirstChildWhichIsA("Part") == seat, model:FindFirstChildOfClass("Part"))
        print(folder:FindFirstAncestorOfClass("Model") == model, folder:FindFirstAncestorWhichIsA("Part") == seat)
        print(game:IsAncestorOf(folder), folder:IsAncestorOf(model), model:IsAncestorOf(model))
        local players = game:GetService("Players")
        print(players:GetPlayerByUserId(7), players:GetPlayerByUserId(8))
        print(game.PlaceId, game.JobId == "")
        print(game:GetService("ServerStorage").Name, game:GetService("StarterPlayer").CharacterWalkSpeed)
        "#,
    );
    assert_eq!(
        printed(&rt),
        ["true Seat", "true true", "true false false", "Player1 nil", "0 true", "ServerStorage 16"]
    );
}

#[test]
fn value_objects_for_links_vectors_and_colours() {
    let rt = runtime_with_player();
    run(
        &rt,
        r#"
        local target = Instance.new("ObjectValue", workspace)
        target.Value = workspace
        local spot = Instance.new("Vector3Value")
        spot.Value = Vector3.new(1, 2, 3)
        local tint = Instance.new("Color3Value")
        tint.Value = Color3.new(1, 0, 0)
        local changes = 0
        spot.Changed:Connect(function() changes += 1 end)
        spot.Value += Vector3.one
        print(target.Value == workspace, spot.Value, tint.Value)
        target.Value = nil
        print(target.Value)
        print(pcall(function() spot.Value = 5 end))
        "#,
    );
    rt.step(0.0);
    let out = printed(&rt);
    assert_eq!(out[0], "true 2, 3, 4 1, 0, 0");
    assert_eq!(out[1], "nil");
    assert!(out[2].starts_with("false"), "{}", out[2]);
}

#[test]
fn lighting_has_fog_and_ambient() {
    let rt = runtime_with_player();
    run(
        &rt,
        r#"
        local lighting = game:GetService("Lighting")
        lighting.FogEnd = 300
        lighting.FogStart = -5
        lighting.FogColor = Color3.new(1, 1, 1)
        print(lighting.FogEnd, lighting.FogStart, lighting.Ambient)
        print(workspace.FallenPartsDestroyHeight)
        "#,
    );
    assert_eq!(printed(&rt), ["300 0 0.5, 0.5, 0.5", "-100"]);
}

#[test]
fn particle_values_ranges_and_sequences() {
    let rt = runtime_with_player();
    run(
        &rt,
        r#"
        local e = Instance.new("ParticleEmitter", workspace)
        print(e.Lifetime, e.Lifetime.Min, e.Speed.Max, typeof(e.Lifetime))
        e.Lifetime = NumberRange.new(1, 3)
        e.Speed = 7
        e.Color = ColorSequence.new(Color3.new(1, 0, 0), Color3.new(0, 0, 1))
        e.Transparency = NumberSequence.new({
            NumberSequenceKeypoint.new(0, 0.25), NumberSequenceKeypoint.new(1, 1),
        })
        e.Size = NumberSequence.new(2)
        print(e.Lifetime, e.Speed, e.Color, e.Transparency, e.Size)
        local seq = ColorSequence.new({
            ColorSequenceKeypoint.new(0, Color3.new(0, 1, 0)), ColorSequenceKeypoint.new(1, Color3.new(1, 1, 1)),
        })
        print(#seq.Keypoints, seq.Keypoints[2].Time, seq.Keypoints[1].Value)
        print(pcall(NumberRange.new, 5, 1))
        e:Emit(12)
        e:Emit()
        print(pcall(function() workspace:Emit(3) end))
        local f = Instance.new("Fire", workspace)
        print(f.Size, f.Heat, f.Color, f.SecondaryColor)
        local h = Instance.new("Highlight", workspace)
        h.Adornee = e
        print(h.Adornee == e, h.FillTransparency, h.OutlineColor)
        "#,
    );
    let out = printed(&rt);
    assert_eq!(out[0], "5 10 5 5 NumberRange");
    assert_eq!(out[1], "1 3 7 7 1, 0, 0 0.25 2");
    assert_eq!(out[2], "2 1 0, 1, 0");
    assert!(out[3].starts_with("false"), "{}", out[3]);
    assert!(out[4].starts_with("false"), "{}", out[4]);
    assert!(out[5].starts_with("5 9 0.9254"), "{}", out[5]);
    assert_eq!(out[6], "true 0.5 1, 1, 1");
    let emits = rt.take_emit_requests();
    assert_eq!(emits.len(), 2);
    assert_eq!((emits[0].1, emits[1].1), (12, 16));
}

//! Members that behave as the reference platform's API reference says.

use playlibre_instance::{ClassKind, InstanceTree, Property, PropertyValue};
use playlibre_script::ScriptRuntime;

fn runtime() -> ScriptRuntime {
    let mut tree = InstanceTree::new();
    let players = tree.service(ClassKind::Players).unwrap();
    let player = tree.create(ClassKind::Player, "Player1").unwrap();
    tree.set(player, Property::UserId, PropertyValue::Number(7.0)).unwrap();
    tree.set_parent(player, Some(players)).unwrap();
    ScriptRuntime::new_with_tree(tree).unwrap()
}

fn printed(src: &str) -> Vec<String> {
    let rt = runtime();
    if let Err(err) = rt.run("test", src) {
        panic!("script failed: {err}");
    }
    rt.take_output().into_iter().map(|l| l.text).collect()
}

#[test]
fn find_first_child_can_search_every_descendant() {
    let out = printed(
        r##"
        local m = Instance.new("Model")
        local f = Instance.new("Folder", m)
        local p = Instance.new("Part", f)
        p.Name = "Deep"
        print(m:FindFirstChild("Deep"), m:FindFirstChild("Deep", true))
        print(m:FindFirstChildWhichIsA("BasePart"), m:FindFirstChildWhichIsA("BasePart", true))
        "##,
    );
    assert_eq!(out, ["nil Deep", "nil Deep"]);
}

#[test]
fn enum_items_have_the_reference_numbers() {
    let out = printed(
        r##"
        print(Enum.KeyCode.E.Value, Enum.KeyCode.Space.Value, Enum.Material.Plastic.Value, Enum.Material.Neon.Value)
        print(Enum.UserInputType.MouseButton1.Value, Enum.PartType.Ball.Value, Enum.EasingStyle.Quad.Value)
        print(Enum.Material:FromValue(288), #Enum.KeyCode:GetEnumItems() > 200)
        local items = Enum.EasingStyle:GetEnumItems()
        print(items[1].Name, items[1].Value)
        local p = Instance.new("Part")
        p.Material = 288
        print(p.Material)
        "##,
    );
    assert_eq!(out, ["101 32 256 288", "0 0 3", "Enum.Material.Neon true", "Linear 0", "Enum.Material.Neon"]);
}

#[test]
fn clone_leaves_out_what_isnt_archivable() {
    let out = printed(
        r##"
        local p = Instance.new("Part")
        p.Archivable = false
        print(p:Clone())
        local m = Instance.new("Model")
        Instance.new("Part", m).Archivable = false
        Instance.new("Folder", m)
        print(#m:Clone():GetChildren())
        "##,
    );
    assert_eq!(out, ["nil", "1"]);
}

#[test]
fn health_stays_between_zero_and_max_health() {
    let out = printed(
        r##"
        local h = Instance.new("Humanoid")
        h.Health = 500
        print(h.Health)
        h.Health = -5
        print(h.Health)
        h.Health = 100
        h.MaxHealth = 40
        print(h.Health)
        "##,
    );
    assert_eq!(out, ["100", "0", "40"]);
}

#[test]
fn team_and_team_colour_follow_each_other() {
    let out = printed(
        r##"
        local red = Instance.new("Team", game:GetService("Teams"))
        red.TeamColor = BrickColor.new("Bright red")
        local p = game:GetService("Players").Player1
        print(p.TeamColor)
        p.Team = red
        print(p.TeamColor, p.Neutral)
        p.Team = nil
        p.TeamColor = BrickColor.new("Bright red")
        print(p.Team == red, p.Neutral)
        "##,
    );
    assert_eq!(out, ["White", "Bright red false", "true false"]);
}

#[test]
fn cframe_space_methods_take_several_values() {
    let out = printed(
        r##"
        local cf = CFrame.new(1, 0, 0)
        local a, b = cf:ToWorldSpace(CFrame.new(1, 0, 0), CFrame.new(2, 0, 0))
        print(a.X, b.X, select("#", cf:ToObjectSpace(a, b)))
        print(cf:PointToWorldSpace(Vector3.zero, Vector3.one))
        print(cf:VectorToObjectSpace(Vector3.xAxis, Vector3.yAxis))
        "##,
    );
    assert_eq!(out, ["2 3 2", "1, 0, 0 2, 1, 1", "1, 0, 0 0, 1, 0"]);
}

#[test]
fn attachments_know_their_world_frame() {
    let out = printed(
        r##"
        local part = Instance.new("Part", workspace)
        part.CFrame = CFrame.new(0, 10, 0) * CFrame.Angles(0, math.pi / 2, 0)
        local a = Instance.new("Attachment", part)
        a.Position = Vector3.new(1, 0, 0)
        local world = a.WorldCFrame
        print(math.abs(math.round(world.X)), math.round(world.Y), math.round(world.Z), math.round(a.WorldOrientation.Y))
        "##,
    );
    assert_eq!(out, ["0 10 -1 90"]);
}

#[test]
fn signals_and_connections_have_their_types() {
    let out = printed(
        r##"
        local s = workspace.ChildAdded
        print(typeof(s), typeof(s:Connect(function() end)))
        print(game:GetService("MaterialService").Use2022Materials)
        "##,
    );
    assert_eq!(out, ["RBXScriptSignal RBXScriptConnection", "true"]);
}

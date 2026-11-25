//! Sounds that play on as they're destroyed, and how sounds say they fall
//! off with distance.

use libroxel_instance::{ClassKind, Property, PropertyValue};
use libroxel_script::ScriptRuntime;

fn run(rt: &ScriptRuntime, src: &str) {
    if let Err(err) = rt.run("test", src) {
        panic!("script failed: {err}");
    }
}

fn printed(rt: &ScriptRuntime) -> Vec<String> {
    rt.take_output().into_iter().map(|l| l.text).collect()
}

#[test]
fn a_sound_set_to_play_on_remove_plays_where_it_was_and_then_goes() {
    let rt = ScriptRuntime::new_with_tree(Default::default()).unwrap();
    run(
        &rt,
        r##"
        local coin = Instance.new("Part")
        coin.Position = Vector3.new(5, 3, 1)
        coin.Parent = workspace
        local ding = Instance.new("Sound")
        ding.Name = "Ding"
        ding.SoundId = "rbxassetid://1"
        ding.PlayOnRemove = true
        ding.Parent = coin
        local quiet = Instance.new("Sound")
        quiet.Parent = coin
        coin:Destroy()
        local holder = workspace:FindFirstChild("PlayOnRemove")
        local copy = holder and holder:FindFirstChild("Ding")
        print(holder ~= nil, holder and holder.Position, copy and copy.IsPlaying, copy and copy.PlayOnRemove)
        print(#holder:GetChildren(), holder.Transparency, holder.CanCollide)
        -- Not in the game: nothing to hear, so only the holder is there.
        local away = Instance.new("Sound")
        away.PlayOnRemove = true
        away:Destroy()
        print(#workspace:GetChildren())
    "##,
    );
    assert_eq!(printed(&rt), ["true 5, 3, 1 true false", "1 1 false", "1"]);
    let restarts = rt.take_sound_restarts();
    assert_eq!(restarts.len(), 1);
    let copy = restarts[0];
    assert_eq!(rt.tree().borrow().get(copy, Property::Playing).unwrap(), PropertyValue::Bool(true));
    // Played to its end, it and its holder go.
    let holder = rt.tree().borrow().parent(copy).unwrap().unwrap();
    rt.sound_ended(copy);
    assert!(rt.tree().borrow().is_destroyed(holder).unwrap());
}

#[test]
fn clearing_children_and_debris_play_removed_sounds_too() {
    let rt = ScriptRuntime::new_with_tree(Default::default()).unwrap();
    run(
        &rt,
        r##"
        local folder = Instance.new("Folder")
        folder.Parent = workspace
        local a = Instance.new("Sound")
        a.Name = "A"
        a.PlayOnRemove = true
        a.Parent = folder
        folder:ClearAllChildren()
        print(workspace:FindFirstChild("A") ~= nil)
        local b = Instance.new("Sound")
        b.Name = "B"
        b.PlayOnRemove = true
        b.Parent = workspace
        game:GetService("Debris"):AddItem(b, 0.1)
    "##,
    );
    let _ = rt.step(0.2);
    run(&rt, "print(#workspace:GetChildren())");
    // A's copy, B's copy (B itself is gone), the folder.
    assert_eq!(printed(&rt), ["true", "3"]);
    let tree = rt.tree().borrow();
    let ws = tree.workspace();
    assert_eq!(tree.descendants_of_class(ws, ClassKind::Sound).len(), 2);
}

#[test]
fn roll_off_settings_are_kept_as_numbers_the_engine_reads() {
    let rt = ScriptRuntime::new_with_tree(Default::default()).unwrap();
    run(
        &rt,
        r##"
        local s = Instance.new("Sound")
        s.Name = "S"
        s.RollOffMode = Enum.RollOffMode.Linear
        s.RollOffMinDistance = 4
        s.RollOffMaxDistance = 60
        s.Parent = workspace
        print(s.RollOffMode == Enum.RollOffMode.Linear, s.RollOffMaxDistance)
    "##,
    );
    assert_eq!(printed(&rt), ["true 60"]);
    let tree = rt.tree().borrow();
    let s = tree.find_first_child(tree.workspace(), "S").unwrap().unwrap();
    assert_eq!(tree.get_extra(s, "RollOffMode").unwrap(), Some(PropertyValue::Number(1.0)));
    assert_eq!(tree.get_extra(s, "RollOffMinDistance").unwrap(), Some(PropertyValue::Number(4.0)));
}

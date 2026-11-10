//! End-to-end tests of the script-facing API.

use libroxel_instance::{ClassKind, Property, PropertyValue, Vec3};
use libroxel_script::{OutputLevel, OutputLine, ScriptRuntime};

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
    use libroxel_instance::InstanceTree;
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

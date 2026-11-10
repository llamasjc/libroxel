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

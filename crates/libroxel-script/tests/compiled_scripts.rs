//! Copies of one script share their compiled code but nothing else.

use libroxel_script::ScriptRuntime;

#[test]
fn copies_of_a_script_each_have_their_own_globals() {
    let rt = ScriptRuntime::new_with_tree(Default::default()).expect("runtime starts");
    let source = r#"
        count = (count or 0) + 1
        local name = script and script.Name or "none"
        print(count, name)
    "#;
    for _ in 0..3 {
        rt.run("Copy", source).expect("runs");
    }
    // A different script with the same text is told apart by its name.
    rt.run("Other", source).expect("runs");
    let lines: Vec<String> = rt.take_output().into_iter().map(|l| l.text).collect();
    assert_eq!(lines, vec!["1 none", "1 none", "1 none", "1 none"]);
}

#[test]
fn copies_report_errors_with_their_own_name() {
    let rt = ScriptRuntime::new_with_tree(Default::default()).expect("runtime starts");
    for _ in 0..2 {
        let err = rt.run("Broken", "local x = nil; x.y = 1").unwrap_err();
        assert!(err.message.contains("Broken"), "{}", err.message);
    }
}

#[test]
fn enum_defaults_stay_enum_items() {
    let rt = ScriptRuntime::new_with_tree(Default::default()).expect("runtime starts");
    rt.run(
        "test",
        r#"
        local p = Instance.new("Part")
        for _ = 1, 3 do
            print(typeof(p.TopSurface) == typeof(Enum.SurfaceType.Smooth), p.TopSurface == Enum.SurfaceType.Smooth)
        end
    "#,
    )
    .expect("runs");
    let lines: Vec<String> = rt.take_output().into_iter().map(|l| l.text).collect();
    assert_eq!(lines, vec!["true true"; 3]);
}

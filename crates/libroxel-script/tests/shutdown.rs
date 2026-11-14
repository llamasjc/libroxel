//! The game shutting down, as when you stop playing.

use libroxel_script::ScriptRuntime;

#[test]
fn bind_to_close_runs_until_it_finishes() {
    let rt = ScriptRuntime::new_with_tree(libroxel_instance::InstanceTree::new()).unwrap();
    rt.run(
        "Saver",
        r#"
        game:BindToClose(function()
            print("closing")
            task.wait(1)
            print("saved")
        end)
        "#,
    )
    .unwrap();
    assert!(rt.step(0.1).is_empty());
    assert!(rt.take_output().is_empty(), "nothing runs before the game closes");
    assert!(rt.close().is_empty());
    let out: Vec<String> = rt.take_output().into_iter().map(|l| l.text).collect();
    assert_eq!(out, ["closing", "saved"]);
}

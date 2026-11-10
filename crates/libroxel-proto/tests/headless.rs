//! Runs the prototype's plugin without a window and checks that the demo
//! script really moves the part through the ECS.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use libroxel_proto::{LibroxelPlugin, PartLink, PartName, StartupScript};

fn app_with(script: &str) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)))
        .add_plugins(LibroxelPlugin {
            scripts: vec![StartupScript { name: "test.luau".into(), source: script.into() }],
        });
    app
}

fn position_of(app: &mut App, name: &str) -> Option<Vec3> {
    let mut query = app.world_mut().query_filtered::<(&PartName, &Transform), With<PartLink>>();
    query.iter(app.world()).find(|(n, _)| n.0 == name).map(|(_, t)| t.translation)
}

#[test]
fn demo_script_moves_the_part() {
    let mut app = app_with(include_str!("../scripts/move_part.luau"));
    app.update();
    app.update();
    let start = position_of(&mut app, "Mover").expect("Mover is mirrored into the ECS");
    for _ in 0..60 {
        app.update();
    }
    let end = position_of(&mut app, "Mover").unwrap();
    assert!(start.distance(end) > 1.0, "part should move: {start} -> {end}");

    // The baseplate is mirrored with its size as scale, and stays put.
    let mut query = app.world_mut().query::<(&PartName, &Transform)>();
    let (_, base) = query.iter(app.world()).find(|(n, _)| n.0 == "Baseplate").unwrap();
    assert_eq!(base.scale, Vec3::new(32.0, 1.0, 32.0));
    assert_eq!(base.translation, Vec3::new(0.0, -0.5, 0.0));
}

#[test]
fn destroyed_parts_are_despawned() {
    let mut app = app_with(
        r#"
        local p = Instance.new("Part", workspace)
        p.Name = "Doomed"
        local frames = 0
        game:GetService("RunService").Heartbeat:Connect(function()
            frames += 1
            if frames == 5 then p:Destroy() end
        end)
        "#,
    );
    app.update();
    app.update();
    assert!(position_of(&mut app, "Doomed").is_some());
    for _ in 0..10 {
        app.update();
    }
    assert!(position_of(&mut app, "Doomed").is_none());
}

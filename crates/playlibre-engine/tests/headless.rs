//! Runs the Bevy side without a window and checks parts are mirrored.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use playlibre_engine::PlaySession;
use playlibre_engine::view::{EnginePlugin, EngineState, PartLink, PartName};
use playlibre_instance::{ClassKind, InstanceTree, Property, PropertyValue};

const DEMO: &str = include_str!("../../playlibre-proto/scripts/move_part.luau");

fn app_with(source: &str) -> App {
    let mut tree = InstanceTree::new();
    let script = tree.create(ClassKind::Script, "test").unwrap();
    tree.set_parent(script, Some(tree.workspace())).unwrap();
    tree.set(script, Property::Source, PropertyValue::String(source.into())).unwrap();
    let mut engine = EngineState::new(tree);
    engine.session = Some(PlaySession::start(engine.tree.clone(), false).unwrap());

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)))
        .insert_non_send(engine)
        .add_plugins(EnginePlugin);
    app
}

fn position_of(app: &mut App, name: &str) -> Option<Vec3> {
    let mut query = app.world_mut().query_filtered::<(&PartName, &Transform), With<PartLink>>();
    query.iter(app.world()).find(|(n, _)| n.0 == name).map(|(_, t)| t.translation)
}

#[test]
fn demo_script_moves_the_part() {
    let mut app = app_with(DEMO);
    app.update();
    app.update();
    let start = position_of(&mut app, "Mover").expect("Mover is mirrored into the ECS");
    for _ in 0..60 {
        app.update();
    }
    let end = position_of(&mut app, "Mover").unwrap();
    assert!(start.distance(end) > 1.0, "part should move: {start} -> {end}");

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
        p.Anchored = true
        task.wait(0.1)
        p:Destroy()
        "#,
    );
    app.update();
    app.update();
    assert!(position_of(&mut app, "Doomed").is_some());
    for _ in 0..12 {
        app.update();
    }
    assert!(position_of(&mut app, "Doomed").is_none());
}

#[test]
fn particle_emitters_send_out_particles_and_bursts() {
    let mut app = app_with(
        r#"
        local p = Instance.new("Part", workspace)
        p.Anchored = true
        p.Position = Vector3.new(0, 5, 0)
        local e = Instance.new("ParticleEmitter", p)
        e.Name = "Steady"
        e.Rate = 60
        e.Lifetime = NumberRange.new(10)
        local q = Instance.new("Part", workspace)
        q.Anchored = true
        local burst = Instance.new("ParticleEmitter", q)
        burst.Enabled = false
        task.wait(0.5)
        burst:Emit(25)
        "#,
    );
    app.add_plugins(bevy::asset::AssetPlugin::default());
    app.init_asset::<Mesh>().init_asset::<StandardMaterial>();
    app.add_plugins(playlibre_engine::effects::EffectsPlugin);
    for _ in 0..20 {
        app.update();
    }
    let steady = playlibre_engine::effects::particle_count(app.world());
    assert!((15..=25).contains(&steady), "about 60 a second for a third of a second: {steady}");
    for _ in 0..30 {
        app.update();
    }
    let after = playlibre_engine::effects::particle_count(app.world());
    assert!(after >= steady + 25 + 20, "the burst of 25 came out too: {steady} -> {after}");
}

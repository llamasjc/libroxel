//! Joining a heavy game with the 3D view running, as the app does: how long
//! until every part is in and drawn, and how long the slowest frame took.
//! `cargo test -p libroxel-engine --test heavy_join_view -- --nocapture`
//! prints the numbers.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use libroxel_engine::view::{EnginePlugin, EngineState, PartLink, VIEW_FILLING};
use libroxel_engine::{CharacterInput, PlaySession};
use libroxel_instance::{ClassKind, InstanceTree, Property, PropertyValue};

const DT: f32 = 1.0 / 60.0;
const MODELS: usize = 3000;
const PARTS_EACH: usize = 20;

/// 60,001 parts in 3,000 models over a 2,000-stud floor.
fn heavy() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let floor = tree.create(ClassKind::Part, "Floor").unwrap();
    tree.set_parent(floor, Some(ws)).unwrap();
    let v = |x, y, z| PropertyValue::Vec3(libroxel_instance::Vec3::new(x, y, z));
    tree.set(floor, Property::Size, v(2000.0, 1.0, 2000.0)).unwrap();
    tree.set(floor, Property::Position, v(0.0, -0.5, 0.0)).unwrap();
    tree.set(floor, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    for m in 0..MODELS {
        let model = tree.create(ClassKind::Model, format!("Model{m}")).unwrap();
        tree.set_parent(model, Some(ws)).unwrap();
        for p in 0..PARTS_EACH {
            let part = tree.create(ClassKind::Part, format!("Part{p}")).unwrap();
            tree.set_parent(part, Some(model)).unwrap();
            let x = (m % 60) as f32 * 30.0 - 900.0;
            let z = (m / 60) as f32 * 30.0 - 900.0;
            tree.set(part, Property::Position, v(x, 1.0 + p as f32, z)).unwrap();
            tree.set(part, Property::Size, v(2.0, 1.0, 2.0)).unwrap();
            tree.set(part, Property::Anchored, PropertyValue::Bool(true)).unwrap();
        }
    }
    tree
}

#[test]
fn a_heavy_join_fills_the_view_without_long_frames() {
    let mut host = PlaySession::host(Rc::new(RefCell::new(heavy())), 0, None, true).unwrap();
    let port = host.host_address().unwrap().port();
    for _ in 0..3 {
        host.step(DT, CharacterInput::default());
    }
    let mut engine = EngineState::new(InstanceTree::new());
    engine.session =
        Some(PlaySession::join(engine.tree.clone(), &format!("127.0.0.1:{port}"), "Bea").unwrap());
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(DT as f64)))
        .insert_non_send(engine)
        .add_plugins(EnginePlugin);

    let began = Instant::now();
    let mut frames = 0u32;
    let mut slowest = Duration::ZERO;
    let mut guest_time = Duration::ZERO;
    let mut joined_at = None;
    let mut settled = 0;
    let mut drawn = 0;
    let mut filled_at = None;
    let mut step_time = 0.0;
    let mut parts = libroxel_engine::FrameTimes::default();
    let mut sync_time = 0.0;
    libroxel_engine::view::take_sync_seconds();
    loop {
        host.step(DT, CharacterInput::default());
        let t = Instant::now();
        app.update();
        let took = t.elapsed();
        guest_time += took;
        slowest = slowest.max(took);
        frames += 1;
        let engine = app.world_mut().non_send_mut::<EngineState>();
        let session = engine.into_inner().session.as_mut().expect("still joined");
        if filled_at.is_none() {
            let t = session.take_frame_times();
            step_time += t.step_seconds();
            parts.add(&t);
            sync_time += libroxel_engine::view::take_sync_seconds();
        }
        let engine = app.world().non_send::<EngineState>();
        let session = engine.session.as_ref().expect("still joined");
        assert!(!session.disconnected(), "lost the connection after {frames} frames");
        if joined_at.is_none() && session.joining().is_none() {
            joined_at = Some((frames, began.elapsed()));
        }
        // Done once the world is in and no more parts have come into the
        // view for a while (the host keeps streaming after the first batch).
        let now_drawn = {
            let world = app.world_mut();
            world.query_filtered::<(), With<PartLink>>().iter(world).count()
        };
        if now_drawn > drawn || VIEW_FILLING.load(Ordering::Relaxed) || joined_at.is_none() {
            drawn = now_drawn;
            settled = 0;
            filled_at = None;
        } else {
            if settled == 0 {
                filled_at = Some((frames, guest_time, slowest));
            }
            settled += 1;
        }
        if settled >= 120 {
            break;
        }
        assert!(began.elapsed() < Duration::from_secs(180), "still filling after {frames} frames");
        std::thread::sleep(Duration::from_millis(1));
    }
    let (joined_frames, joined_after) = joined_at.unwrap();
    let (filled_frames, filling_time, filling_slowest) = filled_at.unwrap();
    eprintln!(
        "heavy join: first batch in after {joined_frames} frames ({:.2} s); view filled after {filled_frames} frames, {:.0} ms of guest time ({:.0} ms game step, {:.0} ms view sync), {:.1} ms a frame, slowest {:.1} ms; {drawn} parts drawn",
        joined_after.as_secs_f64(),
        filling_time.as_secs_f64() * 1000.0,
        step_time * 1000.0,
        sync_time * 1000.0,
        filling_time.as_secs_f64() * 1000.0 / f64::from(filled_frames),
        filling_slowest.as_secs_f64() * 1000.0,
    );
    eprintln!("  game step: {parts:?}");
    assert!(drawn > 50_000, "only {drawn} parts drawn");
    assert!(filling_slowest < Duration::from_millis(100), "a guest frame took {filling_slowest:?}");
}

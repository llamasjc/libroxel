//! Movers and constraints pushing loose parts around.

use libroxel_engine::CharacterInput;
use libroxel_engine::session::start_on;
use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue, Vec3};

const DT: f32 = 1.0 / 60.0;

fn part(tree: &mut InstanceTree, name: &str, pos: Vec3, size: Vec3, anchored: bool) -> InstanceId {
    let id = tree.create(ClassKind::Part, name).unwrap();
    tree.set_parent(id, Some(tree.workspace())).unwrap();
    tree.set(id, Property::Position, PropertyValue::Vec3(pos)).unwrap();
    tree.set(id, Property::Size, PropertyValue::Vec3(size)).unwrap();
    tree.set(id, Property::Anchored, PropertyValue::Bool(anchored)).unwrap();
    id
}

fn attach(tree: &mut InstanceTree, part: InstanceId, offset: Vec3) -> InstanceId {
    let a = tree.create(ClassKind::Attachment, "Attachment").unwrap();
    tree.set(a, Property::Position, PropertyValue::Vec3(offset)).unwrap();
    tree.set_parent(a, Some(part)).unwrap();
    a
}

fn add(
    tree: &mut InstanceTree,
    class: ClassKind,
    parent: InstanceId,
    props: &[(Property, PropertyValue)],
) -> InstanceId {
    let id = tree.create(class, "Mover").unwrap();
    for (p, v) in props {
        tree.set(id, *p, v.clone()).unwrap();
    }
    tree.set_parent(id, Some(parent)).unwrap();
    id
}

fn world() -> InstanceTree {
    let mut tree = InstanceTree::new();
    part(&mut tree, "Floor", Vec3::new(0.0, -0.5, 0.0), Vec3::new(400.0, 1.0, 400.0), true);
    tree
}

/// Runs the world without a player for `seconds`.
fn run(tree: InstanceTree, seconds: f32) -> libroxel_script::SharedTree {
    let mut session = start_on(tree, false).unwrap();
    let shared = session.runtime().tree().clone();
    for _ in 0..(seconds / DT) as usize {
        session.step(DT, CharacterInput::default());
    }
    let log = session.take_log();
    assert!(log.iter().all(|l| l.level != libroxel_engine::LogLevel::Error), "{log:?}");
    shared
}

fn pos(tree: &libroxel_script::SharedTree, id: InstanceId) -> Vec3 {
    tree.borrow().get_vec3(id, Property::Position).unwrap()
}

#[test]
fn linear_velocity_drives_a_part_along() {
    let mut tree = world();
    let block = part(&mut tree, "Block", Vec3::new(0.0, 0.5, 0.0), Vec3::new(2.0, 1.0, 2.0), false);
    let a = attach(&mut tree, block, Vec3::ZERO);
    add(
        &mut tree,
        ClassKind::LinearVelocity,
        block,
        &[
            (Property::Attachment0, PropertyValue::Ref(Some(a))),
            (Property::VectorVelocity, PropertyValue::Vec3(Vec3::new(10.0, 0.0, 0.0))),
            (Property::MaxForce, PropertyValue::Number(1e6)),
        ],
    );
    let t = run(tree, 1.0);
    let p = pos(&t, block);
    assert!((p.x - 10.0).abs() < 0.5, "moved 10 studs in a second: {p:?}");
    assert!((p.y - 0.5).abs() < 0.1, "stayed on the floor: {p:?}");
}

#[test]
fn a_weak_linear_velocity_cant_lift_a_heavy_part() {
    let mut tree = world();
    let block = part(&mut tree, "Block", Vec3::new(0.0, 2.0, 0.0), Vec3::new(4.0, 4.0, 4.0), false);
    let a = attach(&mut tree, block, Vec3::ZERO);
    add(
        &mut tree,
        ClassKind::LinearVelocity,
        block,
        &[
            (Property::Attachment0, PropertyValue::Ref(Some(a))),
            (Property::VectorVelocity, PropertyValue::Vec3(Vec3::new(0.0, 10.0, 0.0))),
        ],
    );
    let t = run(tree, 1.0);
    assert!(pos(&t, block).y < 2.2, "1000 force is less than its weight: {:?}", pos(&t, block));
}

#[test]
fn a_vector_force_can_cancel_gravity() {
    let mut tree = world();
    let block = part(&mut tree, "Block", Vec3::new(0.0, 20.0, 0.0), Vec3::new(2.0, 2.0, 2.0), false);
    let a = attach(&mut tree, block, Vec3::ZERO);
    let weight = 8.0 * 0.7 * 196.2;
    add(
        &mut tree,
        ClassKind::VectorForce,
        block,
        &[
            (Property::Attachment0, PropertyValue::Ref(Some(a))),
            (Property::Force, PropertyValue::Vec3(Vec3::new(weight * 0.1, weight, 0.0))),
        ],
    );
    let t = run(tree, 1.0);
    let p = pos(&t, block);
    assert!((p.y - 20.0).abs() < 0.2, "floats: {p:?}");
    // 10% of its weight sideways: a = 19.62, so x = a t² / 2.
    assert!((p.x - 9.81).abs() < 0.6, "drifts sideways: {p:?}");
}

#[test]
fn align_position_pulls_a_part_to_a_point_and_holds_it() {
    let mut tree = world();
    let block = part(&mut tree, "Block", Vec3::new(0.0, 5.0, 10.0), Vec3::new(2.0, 2.0, 2.0), false);
    let a = attach(&mut tree, block, Vec3::ZERO);
    let target = part(&mut tree, "Target", Vec3::new(5.0, 20.0, 0.0), Vec3::ONE, true);
    tree.set(target, Property::CanCollide, PropertyValue::Bool(false)).unwrap();
    let b = attach(&mut tree, target, Vec3::new(0.0, 2.0, 0.0));
    add(
        &mut tree,
        ClassKind::AlignPosition,
        block,
        &[
            (Property::Attachment0, PropertyValue::Ref(Some(a))),
            (Property::Attachment1, PropertyValue::Ref(Some(b))),
        ],
    );
    let t = run(tree, 3.0);
    let p = pos(&t, block);
    assert!((p - Vec3::new(5.0, 22.0, 0.0)).length() < 0.3, "at the target attachment: {p:?}");
}

#[test]
fn a_rope_swings_a_ball_without_stretching() {
    let mut tree = world();
    let hook = part(&mut tree, "Hook", Vec3::new(0.0, 30.0, 0.0), Vec3::ONE, true);
    let ball = part(&mut tree, "Ball", Vec3::new(8.0, 30.0, 0.0), Vec3::ONE, false);
    let a = attach(&mut tree, hook, Vec3::ZERO);
    let b = attach(&mut tree, ball, Vec3::ZERO);
    add(
        &mut tree,
        ClassKind::RopeConstraint,
        hook,
        &[
            (Property::Attachment0, PropertyValue::Ref(Some(a))),
            (Property::Attachment1, PropertyValue::Ref(Some(b))),
            (Property::Length, PropertyValue::Number(8.0)),
        ],
    );
    let mut session = start_on(tree, false).unwrap();
    let t = session.runtime().tree().clone();
    let mut lowest = f32::MAX;
    let mut longest: f32 = 0.0;
    for _ in 0..120 {
        session.step(DT, CharacterInput::default());
        let p = pos(&t, ball);
        lowest = lowest.min(p.y);
        longest = longest.max((p - Vec3::new(0.0, 30.0, 0.0)).length());
    }
    assert!(lowest < 23.0, "swung down near the bottom of the arc: {lowest}");
    assert!(longest < 8.4, "the rope barely stretches: {longest}");
}

#[test]
fn a_spring_hangs_a_weight_where_its_pull_matches_gravity() {
    let mut tree = world();
    let hook = part(&mut tree, "Hook", Vec3::new(0.0, 40.0, 0.0), Vec3::ONE, true);
    let weight = part(&mut tree, "Weight", Vec3::new(0.0, 36.0, 0.0), Vec3::ONE, false);
    let a = attach(&mut tree, hook, Vec3::ZERO);
    let b = attach(&mut tree, weight, Vec3::ZERO);
    add(
        &mut tree,
        ClassKind::SpringConstraint,
        hook,
        &[
            (Property::Attachment0, PropertyValue::Ref(Some(a))),
            (Property::Attachment1, PropertyValue::Ref(Some(b))),
            (Property::FreeLength, PropertyValue::Number(4.0)),
            (Property::Stiffness, PropertyValue::Number(500.0)),
            (Property::Damping, PropertyValue::Number(20.0)),
        ],
    );
    let t = run(tree, 6.0);
    // Mass 0.7 pulls 137 down; the spring stretches 137 / 500 past 4.
    let stretch = 40.0 - pos(&t, weight).y;
    assert!((stretch - (4.0 + 0.7 * 196.2 / 500.0)).abs() < 0.1, "stretch {stretch}");
}

#[test]
fn body_movers_hover_and_push() {
    let mut tree = world();
    let hover = part(&mut tree, "Hover", Vec3::new(0.0, 3.0, 0.0), Vec3::new(2.0, 1.0, 2.0), false);
    add(
        &mut tree,
        ClassKind::BodyPosition,
        hover,
        &[(Property::Position, PropertyValue::Vec3(Vec3::new(0.0, 12.0, 0.0)))],
    );
    let lift = part(&mut tree, "Lift", Vec3::new(20.0, 0.5, 0.0), Vec3::new(2.0, 1.0, 2.0), false);
    add(&mut tree, ClassKind::BodyVelocity, lift, &[]);
    let t = run(tree, 3.0);
    assert!((pos(&t, hover).y - 12.0).abs() < 0.3, "{:?}", pos(&t, hover));
    // Rises at the default 2 studs per second.
    assert!((pos(&t, lift).y - 6.5).abs() < 0.4, "{:?}", pos(&t, lift));
}

#[test]
fn a_mover_on_a_welded_part_moves_the_whole_assembly() {
    let mut tree = world();
    let base = part(&mut tree, "Base", Vec3::new(0.0, 0.5, 0.0), Vec3::new(4.0, 1.0, 4.0), false);
    let top = part(&mut tree, "Top", Vec3::new(0.0, 1.5, 0.0), Vec3::new(1.0, 1.0, 1.0), false);
    add(
        &mut tree,
        ClassKind::WeldConstraint,
        base,
        &[
            (Property::Part0, PropertyValue::Ref(Some(base))),
            (Property::Part1, PropertyValue::Ref(Some(top))),
        ],
    );
    let a = attach(&mut tree, top, Vec3::ZERO);
    add(
        &mut tree,
        ClassKind::LinearVelocity,
        top,
        &[
            (Property::Attachment0, PropertyValue::Ref(Some(a))),
            (Property::VectorVelocity, PropertyValue::Vec3(Vec3::new(0.0, 0.0, -6.0))),
            (Property::MaxForce, PropertyValue::Number(1e6)),
        ],
    );
    let t = run(tree, 1.0);
    assert!((pos(&t, base).z + 6.0).abs() < 0.5, "{:?}", pos(&t, base));
    assert!((pos(&t, top) - pos(&t, base) - Vec3::new(0.0, 1.0, 0.0)).length() < 0.05);
}

#[test]
fn scripts_can_build_movers_and_read_world_positions() {
    let mut tree = world();
    let script = tree.create(ClassKind::Script, "Launcher").unwrap();
    tree.set_parent(script, Some(tree.workspace())).unwrap();
    let source = r#"
        local p = Instance.new("Part")
        p.Name = "Rocket"
        p.Size = Vector3.new(1, 4, 1)
        p.Position = Vector3.new(0, 2, 30)
        p.Parent = workspace
        local a = Instance.new("Attachment", p)
        a.Position = Vector3.new(0, 2, 0)
        print(a.WorldPosition)
        local v = Instance.new("LinearVelocity", p)
        v.Attachment0 = a
        v.MaxForce = math.huge
        v.VectorVelocity = Vector3.new(0, 20, 0)
    "#;
    tree.set(script, Property::Source, PropertyValue::String(source.into())).unwrap();
    let mut session = start_on(tree, false).unwrap();
    for _ in 0..60 {
        session.step(DT, CharacterInput::default());
    }
    let printed: Vec<_> = session.take_log().iter().map(|l| l.text.clone()).collect();
    assert!(printed.iter().any(|t| t.contains("0, 4, 30")), "{printed:?}");
    let t = session.runtime().tree().clone();
    let rocket = t.borrow().find_first_child(t.borrow().workspace(), "Rocket").unwrap().unwrap();
    assert!((pos(&t, rocket).y - 22.0).abs() < 1.0, "{:?}", pos(&t, rocket));
}

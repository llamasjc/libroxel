//! Humanoid rules scripts count on: a broken neck kills (unless
//! RequiresNeck is off), a body stays whole on death with
//! BreakJointsOnDeath off, the camera's focus moves by CameraOffset, and
//! `MoveTo(point, part)` follows the part as it moves.

use libroxel_engine::CharacterInput;
use libroxel_engine::session::{PlaySession, start_on};
use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue, Vec3};

const DT: f32 = 1.0 / 60.0;

fn part(tree: &mut InstanceTree, class: ClassKind, name: &str, pos: Vec3, size: Vec3) -> InstanceId {
    let id = tree.create(class, name).unwrap();
    tree.set_parent(id, Some(tree.workspace())).unwrap();
    tree.set(id, Property::Position, PropertyValue::Vec3(pos)).unwrap();
    tree.set(id, Property::Size, PropertyValue::Vec3(size)).unwrap();
    tree.set(id, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    id
}

fn session() -> PlaySession {
    let mut tree = InstanceTree::new();
    part(&mut tree, ClassKind::Part, "Floor", Vec3::new(0.0, -0.5, 0.0), Vec3::new(200.0, 1.0, 200.0));
    let spawn = part(
        &mut tree,
        ClassKind::SpawnLocation,
        "Spawn",
        Vec3::new(0.0, 0.5, 0.0),
        Vec3::new(6.0, 1.0, 6.0),
    );
    tree.set(spawn, Property::SpawnShield, PropertyValue::Number(0.0)).unwrap();
    let mut session = start_on(tree, true).unwrap();
    for _ in 0..30 {
        session.step(DT, CharacterInput::default());
    }
    session
}

fn script(session: &PlaySession, body: &str) {
    let src = format!(
        "local c = game:GetService('Players'):GetPlayers()[1].Character\nlocal h = c:FindFirstChildOfClass('Humanoid')\n{body}"
    );
    session.runtime().run("setup", &src).expect("script runs");
}

fn steps(session: &mut PlaySession, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    for _ in 0..n {
        session.step(DT, CharacterInput::default());
        out.extend(session.take_log().into_iter().map(|l| l.text));
    }
    out
}

fn health(session: &PlaySession) -> f64 {
    let c = session.character().unwrap();
    session.runtime().tree().borrow().get_number(c.humanoid, Property::Health).unwrap()
}

#[test]
fn a_humanoid_dies_when_its_neck_breaks() {
    let mut session = session();
    script(&session, "h.Died:Connect(function() print('died') end)\nc.Torso.Neck:Destroy()");
    let out = steps(&mut session, 5);
    assert_eq!(health(&session), 0.0);
    assert!(out.iter().any(|l| l == "died"), "{out:?}");
}

#[test]
fn a_humanoid_that_does_not_require_its_neck_lives_on() {
    let mut session = session();
    script(&session, "h.RequiresNeck = false\nprint('requires', h.RequiresNeck)\nc.Torso.Neck:Destroy()");
    let out = steps(&mut session, 5);
    assert!(out.iter().any(|l| l == "requires false"), "{out:?}");
    assert!(health(&session) > 0.0);
}

#[test]
fn losing_the_head_kills_too() {
    let mut session = session();
    script(&session, "c.Head:Destroy()");
    steps(&mut session, 5);
    assert_eq!(health(&session), 0.0);
}

/// Whether the dead body's Neck is still in its torso.
fn neck_kept_after_death(break_joints: Option<bool>) -> bool {
    let mut session = session();
    let c = session.character().unwrap();
    if let Some(b) = break_joints {
        script(&session, &format!("h.BreakJointsOnDeath = {b}"));
    }
    script(&session, "h.Health = 0");
    steps(&mut session, 20);
    let tree = session.runtime().tree().borrow();
    tree.find_first_child(c.limbs.torso, "Neck").ok().flatten().is_some()
}

#[test]
fn a_body_comes_apart_on_death_unless_break_joints_on_death_is_off() {
    assert!(!neck_kept_after_death(None), "comes apart by default");
    assert!(!neck_kept_after_death(Some(true)));
    assert!(neck_kept_after_death(Some(false)), "stays whole");
}

#[test]
fn a_whole_dead_body_falls_together() {
    let mut session = session();
    let c = session.character().unwrap();
    script(&session, "h.BreakJointsOnDeath = false\nh.Health = 0");
    steps(&mut session, 90);
    let tree = session.runtime().tree().borrow();
    let at = |id| tree.get_vec3(id, Property::Position).unwrap();
    let apart = (at(c.head) - at(c.limbs.torso)).length();
    assert!(apart < 2.5, "the head stays on the body: {apart}");
}

#[test]
fn camera_offset_moves_the_focus_with_the_root() {
    let session = session();
    let c = session.character().unwrap();
    script(&session, "h.CameraOffset = Vector3.new(1, 2, 0)\nprint(h.CameraOffset)");
    let tree = session.runtime().tree().borrow();
    let facing = tree.get_vec3(c.root, Property::Orientation).unwrap();
    let offset = libroxel_engine::character::camera_offset(&tree, c.humanoid, c.root);
    assert!((offset.y - 2.0).abs() < 1e-4, "{offset:?}");
    assert!(
        (offset.length() - 5f32.sqrt()).abs() < 1e-3,
        "turned, not stretched: {offset:?} facing {facing:?}"
    );
}

#[test]
fn move_to_a_part_follows_it_when_it_moves() {
    let mut session = session();
    script(
        &session,
        r#"
        local goal = Instance.new("Part")
        goal.Name = "Goal"
        goal.Anchored = true
        goal.CanCollide = false
        goal.Size = Vector3.new(1, 1, 1)
        goal.Position = Vector3.new(15, 0.5, 0)
        goal.Parent = workspace
        h:MoveTo(goal.Position, goal)
        print("walking to", h.WalkToPart.Name)
        h.MoveToFinished:Connect(function(reached) print("finished", reached) end)
        task.delay(0.5, function() goal.Position = Vector3.new(0, 0.5, 15) end)
    "#,
    );
    let out = steps(&mut session, 600);
    assert!(out.iter().any(|l| l == "walking to Goal"), "{out:?}");
    assert!(out.iter().any(|l| l == "finished true"), "{out:?}");
    let root = session.character().unwrap().root;
    let at = session.runtime().tree().borrow().get_vec3(root, Property::Position).unwrap();
    assert!(at.x.abs() < 2.0 && (at.z - 15.0).abs() < 2.0, "went where the part went: {at:?}");
}

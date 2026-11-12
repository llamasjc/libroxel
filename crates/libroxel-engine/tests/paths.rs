//! Characters walking paths that the pathfinding service found.

use libroxel_engine::session::start_on;
use libroxel_engine::{Character, CharacterInput, LogLevel};
use libroxel_instance::{ClassKind, InstanceTree, Property, PropertyValue, Vec3};

fn block(tree: &mut InstanceTree, name: &str, pos: Vec3, size: Vec3) {
    let id = tree.create(ClassKind::Part, name).unwrap();
    tree.set_parent(id, Some(tree.workspace())).unwrap();
    tree.set(id, Property::Position, PropertyValue::Vec3(pos)).unwrap();
    tree.set(id, Property::Size, PropertyValue::Vec3(size)).unwrap();
    tree.set(id, Property::Anchored, PropertyValue::Bool(true)).unwrap();
}

const WALK: &str = r#"
local PathfindingService = game:GetService("PathfindingService")
local walker = workspace:WaitForChild("Walker")
walker.HumanoidRootPart.Position = Vector3.new(0, 3, 0)
task.wait(0.5)
local path = PathfindingService:CreatePath({ AgentRadius = 1, AgentHeight = 5 })
path:ComputeAsync(walker.HumanoidRootPart.Position, workspace.Goal.Position)
print("status", path.Status == Enum.PathStatus.Success, typeof(path))
local jumps = 0
for _, waypoint in path:GetWaypoints() do
	if waypoint.Action == Enum.PathWaypointAction.Jump then
		jumps += 1
		walker.Humanoid.Jump = true
	end
	walker.Humanoid:MoveTo(waypoint.Position)
	walker.Humanoid.MoveToFinished:Wait()
end
print("arrived", jumps)
"#;

#[test]
fn a_character_walks_round_a_wall_and_jumps_up_a_ledge_to_its_goal() {
    let mut tree = InstanceTree::new();
    block(&mut tree, "Floor", Vec3::new(0.0, -0.5, 0.0), Vec3::new(120.0, 1.0, 120.0));
    // A wall in the way, open at its +Z end.
    block(&mut tree, "Wall", Vec3::new(10.0, 5.0, -6.0), Vec3::new(1.0, 10.0, 36.0));
    // The goal stands on a 2.5-stud ledge beyond it.
    block(&mut tree, "Ledge", Vec3::new(25.0, 1.25, 0.0), Vec3::new(10.0, 2.5, 60.0));
    let goal = tree.create(ClassKind::Part, "Goal").unwrap();
    tree.set_parent(goal, Some(tree.workspace())).unwrap();
    tree.set(goal, Property::Position, PropertyValue::Vec3(Vec3::new(25.0, 3.0, 0.0))).unwrap();
    tree.set(goal, Property::CanCollide, PropertyValue::Bool(false)).unwrap();
    tree.set(goal, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    Character::spawn_named(&mut tree, "Walker");
    let server = tree.service(ClassKind::ServerScriptService).unwrap();
    let script = tree.create(ClassKind::Script, "Walk").unwrap();
    tree.set(script, Property::Source, PropertyValue::String(WALK.into())).unwrap();
    tree.set_parent(script, Some(server)).unwrap();

    let mut session = start_on(tree, false).unwrap();
    let mut log = Vec::new();
    for _ in 0..60 * 30 {
        session.step(1.0 / 60.0, CharacterInput::default());
        log.extend(session.take_log());
        if log.iter().any(|l| l.text.starts_with("arrived")) {
            break;
        }
    }
    let errors: Vec<_> = log.iter().filter(|l| l.level == LogLevel::Error).collect();
    assert!(errors.is_empty(), "{errors:?}");
    let text: Vec<_> = log.iter().map(|l| l.text.as_str()).collect();
    assert!(text.contains(&"status true Path"), "{text:?}");
    assert!(text.iter().any(|t| t.starts_with("arrived") && !t.ends_with(" 0")), "{text:?}");
    let tree = session.runtime().tree().borrow();
    let walker = tree.find_first_child(tree.workspace(), "Walker").unwrap().unwrap();
    let root = tree.find_first_child(walker, "HumanoidRootPart").unwrap().unwrap();
    let at = tree.get_vec3(root, Property::Position).unwrap();
    assert!((at - Vec3::new(25.0, at.y, 0.0)).length() < 2.0 && at.y > 3.0, "{at:?}");
}

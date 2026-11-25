//! Humanoid settings scripts reach for: standing on a platform, not
//! turning to face the walk, jumping to a height, and the camera following
//! something other than the player's character.

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

fn humanoid_script(session: &PlaySession, body: &str) {
    let src = format!(
        "local h = game:GetService('Players'):GetPlayers()[1].Character:FindFirstChildOfClass('Humanoid')\n{body}"
    );
    session.runtime().run("setup", &src).expect("script runs");
}

fn root_position(session: &PlaySession) -> Vec3 {
    let root = session.character().unwrap().root;
    session.runtime().tree().borrow().get_vec3(root, Property::Position).unwrap()
}

#[test]
fn platform_standing_stops_walking_and_jumping() {
    let mut session = session();
    humanoid_script(&session, "h.PlatformStand = true");
    let start = root_position(&session);
    let walk_and_jump =
        CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), jump: true, ..Default::default() };
    for _ in 0..60 {
        session.step(DT, walk_and_jump);
    }
    let end = root_position(&session);
    assert!((end.x - start.x).abs() < 0.5, "didn't walk: {start:?} -> {end:?}");
    assert!(end.y < start.y + 0.5, "didn't jump: {start:?} -> {end:?}");

    humanoid_script(&session, "h.PlatformStand = false");
    for _ in 0..60 {
        session.step(DT, CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), ..Default::default() });
    }
    assert!(root_position(&session).x > end.x + 10.0, "walks again");
}

#[test]
fn a_humanoid_that_does_not_auto_rotate_keeps_facing_the_same_way() {
    let mut session = session();
    let root = session.character().unwrap().root;
    let yaw = |s: &PlaySession| s.runtime().tree().borrow().get_vec3(root, Property::Orientation).unwrap().y;
    let before = yaw(&session);
    humanoid_script(&session, "h.AutoRotate = false");
    let sideways = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), ..Default::default() };
    for _ in 0..60 {
        session.step(DT, sideways);
    }
    assert!((yaw(&session) - before).abs() < 0.01, "kept facing {before}, now {}", yaw(&session));
    humanoid_script(&session, "h.AutoRotate = true");
    for _ in 0..60 {
        session.step(DT, sideways);
    }
    assert!((yaw(&session) - before).abs() > 45.0, "turned to face the walk");
}

#[test]
fn jump_height_sets_how_high_a_jump_goes_once_jump_power_is_off() {
    let peak_of = |setup: &str| {
        let mut session = session();
        humanoid_script(&session, setup);
        let ground = root_position(&session).y;
        session.step(DT, CharacterInput { jump: true, ..Default::default() });
        let mut peak: f32 = 0.0;
        for _ in 0..90 {
            session.step(DT, CharacterInput::default());
            peak = peak.max(root_position(&session).y - ground);
        }
        peak
    };
    let low = peak_of("h.UseJumpPower = false; h.JumpHeight = 2");
    let high = peak_of("h.UseJumpPower = false; h.JumpHeight = 12");
    assert!(low > 1.0 && low < 3.5, "about 2 studs: {low}");
    assert!(high > 9.0 && high < 14.0, "about 12 studs: {high}");
}

#[test]
fn characters_climb_trusses_hang_on_and_leap_off() {
    let mut session = session();
    // A tall truss just ahead (+X) of the spawn.
    session
        .runtime()
        .run(
            "truss",
            r#"
            local t = Instance.new("TrussPart")
            t.Anchored = true
            t.Size = Vector3.new(2, 20, 2)
            t.Position = Vector3.new(4, 10, 0)
            t.Parent = workspace
            print(t.ClassName)
        "#,
        )
        .unwrap();
    let start = root_position(&session);
    let into = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), ..Default::default() };
    for _ in 0..60 {
        session.step(DT, into);
    }
    let climbed = root_position(&session);
    assert!(climbed.y > start.y + 5.0, "climbed: {start:?} -> {climbed:?}");
    assert!(climbed.x < 3.0, "stayed in front of the truss: {climbed:?}");

    // Let go of the keys: hang on.
    for _ in 0..30 {
        session.step(DT, CharacterInput::default());
    }
    let hanging = root_position(&session);
    assert!((hanging.y - climbed.y).abs() < 1.0, "hung on: {climbed:?} -> {hanging:?}");

    // Space leaps off, and the character falls back to the floor.
    session.step(DT, CharacterInput { jump: true, ..Default::default() });
    for _ in 0..180 {
        session.step(DT, CharacterInput::default());
    }
    let landed = root_position(&session);
    assert!(landed.y < start.y + 1.0 && landed.x < climbed.x, "leapt off and landed: {landed:?}");
}

#[test]
fn climbing_to_the_top_of_a_truss_steps_onto_it() {
    let mut session = session();
    session
        .runtime()
        .run(
            "truss",
            r#"
            local t = Instance.new("TrussPart")
            t.Anchored = true
            t.Size = Vector3.new(2, 8, 6)
            t.Position = Vector3.new(4, 4, 0)
            t.Parent = workspace
        "#,
        )
        .unwrap();
    let into = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), ..Default::default() };
    let mut stood_on_top = false;
    for _ in 0..240 {
        session.step(DT, into);
        let p = root_position(&session);
        // Over the truss (x 3..5) with the feet on its top (y 8).
        stood_on_top |= p.x > 3.2 && p.x < 4.8 && p.y > 10.0 && p.y < 12.0;
    }
    assert!(stood_on_top, "walked over the top; ended at {:?}", root_position(&session));
}

#[test]
fn a_climbing_humanoid_says_so_and_its_climb_animation_plays() {
    let mut session = session();
    humanoid_script(
        &session,
        r#"
        local t = Instance.new("TrussPart")
        t.Anchored = true
        t.Size = Vector3.new(2, 20, 2)
        t.Position = Vector3.new(4, 10, 0)
        t.Parent = workspace
        -- As a character's animate script keeps it.
        local holder = Instance.new("StringValue")
        holder.Name = "climb"
        local animation = Instance.new("Animation")
        animation.Name = "ClimbAnim"
        animation.AnimationId = "rbxassetid://5"
        animation.Parent = holder
        local track = h:LoadAnimation(animation)
        local fastest = 0
        h.Climbing:Connect(function(speed)
            fastest = math.max(fastest, speed)
            if not track.IsPlaying then track:Play() end
        end)
        h.StateChanged:Connect(function(_, new)
            if new == Enum.HumanoidStateType.Climbing then print("climbing", h:GetState() == new) end
        end)
        task.delay(0.8, function() print(fastest > 5, track.IsPlaying, track.Looped) end)
    "#,
    );
    let into = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), ..Default::default() };
    let mut out = Vec::new();
    for _ in 0..60 {
        session.step(DT, into);
        out.extend(session.take_log().into_iter().map(|l| l.text).filter(|t| !t.is_empty()));
    }
    assert_eq!(out, ["climbing true", "true true true"]);
    let humanoid = session.character().unwrap().humanoid;
    let (_, legs) = session.runtime().animation_pose(humanoid).expect("climbing animation");
    assert!(legs);
}

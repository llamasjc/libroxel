//! How characters look: body colours and hats.

use playlibre_engine::character::{Hat, Look};
use playlibre_engine::session::start_on;
use playlibre_engine::{CharacterInput, LogLevel, PlaySession};
use playlibre_instance::{ClassKind, InstanceTree, Property};

fn step(session: &mut PlaySession, frames: usize) {
    for _ in 0..frames {
        session.step(1.0 / 60.0, CharacterInput::default());
    }
    let errors: Vec<_> = session.take_log().into_iter().filter(|l| l.level == LogLevel::Error).collect();
    assert!(errors.is_empty(), "{errors:?}");
}

/// Steps, keeping what scripts printed.
fn run_for(session: &mut PlaySession, frames: usize) -> Vec<String> {
    let mut printed = session.runtime().take_output().into_iter().map(|l| l.text).collect::<Vec<_>>();
    for _ in 0..frames {
        session.step(1.0 / 60.0, CharacterInput::default());
        for line in session.take_log() {
            assert!(line.level != LogLevel::Error, "{}", line.text);
            printed.push(line.text);
        }
    }
    printed
}

/// A world with just a floor to stand on.
fn floored() -> InstanceTree {
    use playlibre_instance::{PropertyValue, Vec3};
    let mut tree = InstanceTree::new();
    let floor = tree.create(ClassKind::Part, "Floor").unwrap();
    tree.set_parent(floor, Some(tree.workspace())).unwrap();
    tree.set(floor, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, -0.5, 0.0))).unwrap();
    tree.set(floor, Property::Size, PropertyValue::Vec3(Vec3::new(100.0, 1.0, 100.0))).unwrap();
    tree.set(floor, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    tree
}

#[test]
fn scripts_recolour_a_character_through_its_body_colours() {
    let mut session = start_on(InstanceTree::new(), true).unwrap();
    step(&mut session, 1);
    session
        .runtime()
        .run(
            "paint",
            r#"
            local colors = workspace.Player1:FindFirstChildOfClass("BodyColors")
            colors.TorsoColor3 = Color3.fromRGB(10, 20, 250)
            colors.HeadColor3 = Color3.fromRGB(250, 250, 0)
        "#,
        )
        .unwrap();
    step(&mut session, 2);
    let tree = session.runtime().tree().borrow();
    let model = tree.find_first_child(tree.workspace(), "Player1").unwrap().unwrap();
    let torso = tree.find_first_child(model, "Torso").unwrap().unwrap();
    let head = tree.find_first_child(model, "Head").unwrap().unwrap();
    assert!(tree.get_color(torso, Property::Color).unwrap().b > 0.95);
    assert!(tree.get_color(head, Property::Color).unwrap().g > 0.95);
}

#[test]
fn hats_sit_on_the_head_and_follow_it() {
    let mut session = start_on(floored(), true).unwrap();
    step(&mut session, 1);
    let look = Look { hat: Hat::TallHat, ..Look::for_player(0) };
    session.set_local_look(look);
    let head_and_hat = |session: &PlaySession| {
        let tree = session.runtime().tree().borrow();
        let model = tree.find_first_child(tree.workspace(), "Player1").unwrap().unwrap();
        let head = tree.find_first_child(model, "Head").unwrap().unwrap();
        let hat = tree.find_first_child(model, "Tall hat").unwrap().unwrap();
        assert_eq!(tree.class(hat).unwrap(), ClassKind::Accessory);
        let tower = tree.find_first_child(hat, "Tower").unwrap().unwrap();
        (tree.get_vec3(head, Property::Position).unwrap(), tree.get_vec3(tower, Property::Position).unwrap())
    };
    let (head, tower) = head_and_hat(&session);
    assert!((tower.y - head.y - 1.5).abs() < 0.01, "{head:?} {tower:?}");
    session
        .runtime()
        .run("move", "workspace.Player1.HumanoidRootPart.Position += Vector3.new(10, 0, 0)")
        .unwrap();
    step(&mut session, 2);
    let (head, tower) = head_and_hat(&session);
    assert!((tower.x - head.x).abs() < 0.01 && (tower.y - head.y - 1.5).abs() < 0.1, "{head:?} {tower:?}");
    // Respawning keeps the hat.
    session.runtime().run("die", "workspace.Player1.Humanoid.Health = 0").unwrap();
    step(&mut session, 60 * 7);
    head_and_hat(&session);
}

#[test]
fn looks_survive_being_saved_and_read_back() {
    let look = Look { hat: Hat::Halo, ..Look::for_player(3) };
    assert_eq!(Look::from_json(&look.to_json(), Look::for_player(0)), look);
    // Rubbish falls back.
    let rubbish = playlibre_instance::serde_json::json!({ "hat": "Space helmet", "skin": [1, 2] });
    assert_eq!(Look::from_json(&rubbish, look.clone()).colors, look.colors);
    assert_eq!(Look::from_json(&rubbish, look).hat, Hat::None);
}

#[test]
fn animations_move_the_limbs_and_stop() {
    let mut world = InstanceTree::new();
    let floor = world.create(ClassKind::Part, "Floor").unwrap();
    world.set_parent(floor, Some(world.workspace())).unwrap();
    world
        .set(
            floor,
            Property::Size,
            playlibre_instance::PropertyValue::Vec3(playlibre_instance::Vec3::new(50.0, 1.0, 50.0)),
        )
        .unwrap();
    world.set(floor, Property::Anchored, playlibre_instance::PropertyValue::Bool(true)).unwrap();
    let mut session = start_on(world, true).unwrap();
    step(&mut session, 1);
    session
        .runtime()
        .run(
            "wave",
            r#"
            local animator = workspace.Player1.Humanoid:WaitForChild("Animator")
            local animation = Instance.new("Animation")
            animation.AnimationId = "playlibre://wave"
            track = animator:LoadAnimation(animation)
            print(typeof(track), track.Length, track.IsPlaying, track.Looped)
            track.Stopped:Connect(function() print("stopped", track.IsPlaying) end)
            track:Play()
            print(#animator:GetPlayingAnimationTracks())
        "#,
        )
        .unwrap();
    let arm_height = |session: &PlaySession| {
        let tree = session.runtime().tree().borrow();
        let model = tree.find_first_child(tree.workspace(), "Player1").unwrap().unwrap();
        let arm = tree.find_first_child(model, "Right Arm").unwrap().unwrap();
        let root = tree.find_first_child(model, "HumanoidRootPart").unwrap().unwrap();
        tree.get_vec3(arm, Property::Position).unwrap().y - tree.get_vec3(root, Property::Position).unwrap().y
    };
    let arm_out = |session: &PlaySession| {
        let tree = session.runtime().tree().borrow();
        let model = tree.find_first_child(tree.workspace(), "Player1").unwrap().unwrap();
        let arm = tree.find_first_child(model, "Right Arm").unwrap().unwrap();
        let root = tree.find_first_child(model, "HumanoidRootPart").unwrap().unwrap();
        tree.get_vec3(arm, Property::Position).unwrap().x - tree.get_vec3(root, Property::Position).unwrap().x
    };
    let resting = arm_height(&session);
    let mut printed = run_for(&mut session, 20);
    // A third of a second in, the arm is up in the air.
    assert!(arm_height(&session) > resting + 1.0, "{} {}", arm_height(&session), resting);
    // Out to the right side, not across the body.
    assert!(arm_out(&session) > 1.5, "{}", arm_out(&session));
    printed.extend(run_for(&mut session, 120));
    assert!((arm_height(&session) - resting).abs() < 0.05);
    let out: Vec<String> = printed.into_iter().filter(|t| !t.is_empty()).collect();
    assert_eq!(out, vec!["AnimationTrack 2 false false", "1", "stopped false"]);
    // A missing animation is a clear error.
    let err = session
        .runtime()
        .run(
            "bad",
            r#"
            local a = Instance.new("Animation")
            a.AnimationId = "playlibre://moonwalk"
            workspace.Player1.Humanoid:LoadAnimation(a)
        "#,
        )
        .unwrap_err();
    assert!(err.message.contains("built-in ones are"), "{}", err.message);
}

#[test]
fn a_keyframe_sequence_turns_the_characters_joints() {
    let mut world = InstanceTree::new();
    let floor = world.create(ClassKind::Part, "Floor").unwrap();
    world.set_parent(floor, Some(world.workspace())).unwrap();
    world
        .set(
            floor,
            Property::Size,
            playlibre_instance::PropertyValue::Vec3(playlibre_instance::Vec3::new(50.0, 1.0, 50.0)),
        )
        .unwrap();
    world.set(floor, Property::Anchored, playlibre_instance::PropertyValue::Bool(true)).unwrap();
    let mut session = start_on(world, true).unwrap();
    step(&mut session, 1);
    session
        .runtime()
        .run(
            "reach",
            r#"
            -- The right shoulder turned a quarter about its own Z, and the
            -- left hip a little, held: the right arm points ahead.
            local sequence = Instance.new("KeyframeSequence")
            local frame = Instance.new("Keyframe")
            local root = Instance.new("Pose")
            root.Name = "HumanoidRootPart"
            root.Weight = 0
            root.Parent = frame
            local torso = Instance.new("Pose")
            torso.Name = "Torso"
            torso.Weight = 0
            torso.Parent = root
            local arm = Instance.new("Pose")
            arm.Name = "Right Arm"
            arm.CFrame = CFrame.Angles(0, 0, math.pi / 2)
            arm.Parent = torso
            frame.Parent = sequence
            local animation = Instance.new("Animation")
            animation.AnimationId = game:GetService("KeyframeSequenceProvider"):RegisterKeyframeSequence(sequence)
            workspace.Player1.Humanoid.Animator:LoadAnimation(animation):Play(0)
        "#,
        )
        .unwrap();
    run_for(&mut session, 10);
    let tree = session.runtime().tree().borrow();
    let model = tree.find_first_child(tree.workspace(), "Player1").unwrap().unwrap();
    let arm = tree.find_first_child(model, "Right Arm").unwrap().unwrap();
    let torso = tree.find_first_child(model, "Torso").unwrap().unwrap();
    let arm_at = tree.get_vec3(arm, Property::Position).unwrap();
    let torso_at = tree.get_vec3(torso, Property::Position).unwrap();
    let yaw = tree.get_vec3(torso, Property::Orientation).unwrap().y.to_radians();
    // Ahead of the torso is -Z turned by its yaw.
    let ahead = playlibre_instance::Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let d = arm_at - torso_at;
    let forward = d.x * ahead.x + d.z * ahead.z;
    assert!(forward > 0.7, "the arm is {forward} ahead ({d:?})");
    // Raised to shoulder height, not hanging.
    assert!(d.y > 0.2, "{d:?}");
}

#[test]
fn a_character_that_dies_falls_apart_and_its_hat_stays_on_its_head() {
    let mut session = start_on(floored(), true).unwrap();
    session.set_local_look(Look { hat: Hat::Crown, ..Look::for_player(0) });
    step(&mut session, 30);
    session.runtime().run("kill", "workspace.Player1.Humanoid.Health = 0").unwrap();
    // Not long enough to respawn.
    step(&mut session, 120);
    let tree = session.runtime().tree().borrow();
    let model = tree.find_first_child(tree.workspace(), "Player1").unwrap().unwrap();
    let find = |name: &str| tree.find_first_child(model, name).unwrap().unwrap();
    let (head, torso, left_leg) = (find("Head"), find("Torso"), find("Left Leg"));
    for piece in [head, torso, left_leg] {
        assert_eq!(tree.get_bool(piece, Property::Anchored), Some(false));
        let y = tree.get_vec3(piece, Property::Position).unwrap().y;
        // Lying on the floor, not standing up (the head was about 4.5 up).
        assert!(y > 0.0 && y < 2.0, "{} at {y}", tree.name(piece).unwrap());
    }
    // The crown came down with the head and is still on it.
    let crown = tree.find_first_child_of_class(model, ClassKind::Accessory).unwrap().unwrap();
    let jewel = tree.descendants_of_class(crown, ClassKind::Part)[0];
    let gap =
        tree.get_vec3(jewel, Property::Position).unwrap() - tree.get_vec3(head, Property::Position).unwrap();
    assert!(gap.length() < 2.5, "{gap:?}");
}

#[test]
fn a_hat_made_from_parts_sits_on_the_head_and_survives_being_sent() {
    use playlibre_engine::character::{Character, CustomHat};
    use playlibre_instance::{Color3, PropertyValue, Vec3};
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let model = tree.create(ClassKind::Model, "Top hat").unwrap();
    tree.set_parent(model, Some(ws)).unwrap();
    // Built anywhere: a wide brim with a tall tower on it.
    for (name, at, size) in [
        ("Brim", Vec3::new(50.0, 20.0, 10.0), Vec3::new(3.0, 0.2, 3.0)),
        ("Tower", Vec3::new(50.0, 21.1, 10.0), Vec3::new(1.6, 2.0, 1.6)),
    ] {
        let part = tree.create(ClassKind::Part, name).unwrap();
        tree.set_parent(part, Some(model)).unwrap();
        tree.set(part, Property::Position, PropertyValue::Vec3(at)).unwrap();
        tree.set(part, Property::Size, PropertyValue::Vec3(size)).unwrap();
        tree.set(part, Property::Color, PropertyValue::Color3(Color3::from_rgb8(20, 20, 30))).unwrap();
    }
    let hat = CustomHat::from_parts(&tree, model).unwrap();
    assert_eq!(hat.name, "Top hat");
    assert_eq!(hat.parts.len(), 2);
    // The brim's bottom rests on the top of the head (0.7 above its middle), centred.
    let brim = &hat.parts[0];
    assert!((brim.offset - Vec3::new(0.0, 0.8, 0.0)).length() < 1e-4, "{:?}", brim.offset);
    assert!((hat.parts[1].offset - Vec3::new(0.0, 1.9, 0.0)).length() < 1e-4);

    // Sent as JSON (to a host, or the catalog) it comes back the same.
    let look = Look { item: Some(std::sync::Arc::new(hat.clone())), ..Look::for_player(0) };
    let back = Look::from_json(&look.to_json(), Look::for_player(0));
    assert_eq!(back, look);

    // A character wearing it has its parts, placed by the head.
    let character = Character::spawn_wearing(&mut tree, "Ada", None, look);
    let accessory = tree.find_first_child_of_class(character.model, ClassKind::Accessory).unwrap().unwrap();
    assert_eq!(tree.name(accessory).unwrap(), "Top hat");
    assert_eq!(tree.children(accessory).unwrap().len(), 2);

    // Hats too big, or reaching too far, are turned away.
    let mut huge = hat.clone();
    huge.parts[0].size = Vec3::new(40.0, 1.0, 1.0);
    assert!(huge.check().is_err());
    assert!(CustomHat::from_json(&huge.to_json()).is_none());
    let mut far = hat;
    far.parts[1].offset = Vec3::new(0.0, 30.0, 0.0);
    assert!(far.check().is_err());
}

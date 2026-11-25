//! Animations kept as keyframe sequences, and the stock animate script's
//! animations that libroxel can't fetch.

use libroxel_instance::InstanceId;
use libroxel_script::ScriptRuntime;

fn run(rt: &ScriptRuntime, src: &str) {
    if let Err(err) = rt.run("test", src) {
        panic!("script failed: {err}");
    }
}

/// Runs `src` with `sequence` made first.
fn run_with_sequence(rt: &ScriptRuntime, src: &str) {
    run(rt, &format!("{SEQUENCE}\n{src}"));
}

fn printed(rt: &ScriptRuntime) -> Vec<String> {
    rt.take_output().into_iter().map(|l| l.text).collect()
}

fn named(rt: &ScriptRuntime, name: &str) -> InstanceId {
    let tree = rt.tree().borrow();
    tree.find_first_child(tree.workspace(), name).unwrap().unwrap()
}

/// A sequence that lifts the classic body's right arm straight out in
/// front by half a second, with a named keyframe and a marker there.
const SEQUENCE: &str = r##"
    local function pose(name, parent, cf)
        local p = Instance.new("Pose")
        p.Name = name
        p.CFrame = cf or CFrame.new()
        p.Weight = cf and 1 or 0
        p.Parent = parent
        return p
    end
    local function keyframe(sequence, time, name, angle)
        local k = Instance.new("Keyframe")
        k.Time = time
        if name then k.Name = name end
        local torso = pose("Torso", pose("HumanoidRootPart", k))
        pose("Right Arm", torso, CFrame.Angles(0, 0, angle))
        k.Parent = sequence
        return k
    end
    local sequence = Instance.new("KeyframeSequence")
    sequence.Name = "Reach"
    sequence.Loop = false
    sequence.Priority = Enum.AnimationPriority.Action
    keyframe(sequence, 0, nil, 0)
    local up = keyframe(sequence, 0.5, "Up", math.pi / 2)
    local marker = Instance.new("KeyframeMarker")
    marker.Name = "Hit"
    marker.Value = "boom"
    marker.Parent = up
    keyframe(sequence, 1, nil, 0)
"##;

#[test]
fn a_registered_sequence_plays_with_its_keyframes_and_markers() {
    let rt = ScriptRuntime::new_with_tree(Default::default()).unwrap();
    run_with_sequence(
        &rt,
        r##"
        local model = Instance.new("Model")
        model.Name = "Rig"
        local humanoid = Instance.new("Humanoid")
        humanoid.Parent = model
        local animator = Instance.new("Animator")
        animator.Parent = humanoid
        model.Parent = workspace
        local provider = game:GetService("KeyframeSequenceProvider")
        local id = provider:RegisterKeyframeSequence(sequence)
        print(id:sub(1, 10), provider:GetKeyframeSequenceAsync(id).Name)
        local animation = Instance.new("Animation")
        animation.AnimationId = id
        local track = animator:LoadAnimation(animation)
        print(track.Length, track.Name, track.Priority == Enum.AnimationPriority.Action, track:GetTimeOfKeyframe("Up"))
        track.KeyframeReached:Connect(function(name) print("keyframe", name) end)
        track:GetMarkerReachedSignal("Hit"):Connect(function(value) print("marker", value) end)
        track.Stopped:Connect(function() print("stopped") end)
        track:Play(0)
    "##,
    );
    assert_eq!(printed(&rt), ["rbxtemp:// Reach", "1 Reach true 0.5"].map(String::from));
    let humanoid = {
        let tree = rt.tree().borrow();
        tree.find_first_child(named(&rt, "Rig"), "Humanoid").unwrap().unwrap()
    };
    for _ in 0..5 {
        let _ = rt.step(0.1);
    }
    let (pose, legs) = rt.animation_pose(humanoid).expect("playing");
    assert!(!legs);
    // Half a second in, the arm points straight ahead.
    assert!((pose.right_arm.swing - 90.0).abs() < 15.0, "{pose:?}");
    assert!(pose.left_arm.swing.abs() < 1e-3, "{pose:?}");
    for _ in 0..7 {
        let _ = rt.step(0.1);
    }
    assert_eq!(printed(&rt), ["keyframe Up", "marker boom", "stopped"].map(String::from));
}

#[test]
fn the_engine_can_supply_a_sequence_for_an_asset_id() {
    let rt = ScriptRuntime::new_with_tree(Default::default()).unwrap();
    run_with_sequence(&rt, "sequence.Parent = workspace");
    rt.register_animation("rbxassetid://4242", named(&rt, "Reach")).unwrap();
    run(
        &rt,
        r##"
        local humanoid = Instance.new("Humanoid")
        local animation = Instance.new("Animation")
        animation.AnimationId = "http://assets.example/asset/?id=4242"
        local track = humanoid:LoadAnimation(animation)
        print(track.Length, track.Name)
        local unknown = Instance.new("Animation")
        unknown.AnimationId = "rbxassetid://1"
        print(humanoid:LoadAnimation(unknown).Length)
    "##,
    );
    assert_eq!(printed(&rt), ["1 Reach", "1"].map(String::from));
}

#[test]
fn a_looped_sequence_says_each_time_it_goes_round() {
    let rt = ScriptRuntime::new_with_tree(Default::default()).unwrap();
    run_with_sequence(
        &rt,
        r##"
        sequence.Loop = true
        local humanoid = Instance.new("Humanoid")
        humanoid.Parent = workspace
        local animation = Instance.new("Animation")
        animation.AnimationId = game:GetService("KeyframeSequenceProvider"):RegisterKeyframeSequence(sequence)
        local track = humanoid:LoadAnimation(animation)
        local loops = 0
        track.DidLoop:Connect(function() loops += 1; print(loops) end)
        track:Play()
    "##,
    );
    for _ in 0..25 {
        let _ = rt.step(0.1);
    }
    assert_eq!(printed(&rt), ["1", "2"]);
}

#[test]
fn higher_priorities_play_over_lower_ones_only_where_they_move() {
    let rt = ScriptRuntime::new_with_tree(Default::default()).unwrap();
    run_with_sequence(
        &rt,
        r##"
        local humanoid = Instance.new("Humanoid")
        humanoid.Name = "H"
        humanoid.Parent = workspace
        local id = game:GetService("KeyframeSequenceProvider"):RegisterKeyframeSequence(sequence)
        local reach = Instance.new("Animation")
        reach.AnimationId = id
        local dance = Instance.new("Animation")
        dance.AnimationId = "libroxel://dance"
        local under = humanoid:LoadAnimation(dance)
        under.Priority = Enum.AnimationPriority.Idle
        local over = humanoid:LoadAnimation(reach)
        -- Loaded first, but the dance is under it.
        over:Play(0)
        under:Play(0)
    "##,
    );
    for _ in 0..5 {
        let _ = rt.step(0.1);
    }
    let humanoid = named(&rt, "H");
    let (pose, legs) = rt.animation_pose(humanoid).unwrap();
    // The reach holds the right arm; the dance still moves the rest.
    assert!((pose.right_arm.swing - 90.0).abs() < 15.0, "{pose:?}");
    assert!(pose.left_arm.out.abs() > 10.0 && legs, "{pose:?}");
}

#[test]
fn the_stock_animate_scripts_animations_play_by_their_names() {
    let rt = ScriptRuntime::new_with_tree(Default::default()).unwrap();
    run(
        &rt,
        r##"
        local humanoid = Instance.new("Humanoid")
        humanoid.Name = "H"
        humanoid.Parent = workspace
        local folder = Instance.new("StringValue")
        folder.Name = "wave"
        local animation = Instance.new("Animation")
        animation.Name = "WaveAnim"
        animation.AnimationId = "rbxassetid://99999"
        animation.Parent = folder
        local track = humanoid:LoadAnimation(animation)
        local walk = Instance.new("Animation")
        walk.Name = "WalkAnim"
        walk.AnimationId = "rbxassetid://99998"
        print(track.Length, humanoid:LoadAnimation(walk).Length)
        track:Play(0)
    "##,
    );
    assert_eq!(printed(&rt), ["2 1"]);
    let _ = rt.step(0.3);
    let (pose, _) = rt.animation_pose(named(&rt, "H")).unwrap();
    assert!(pose.right_arm.out > 90.0, "{pose:?}");
}

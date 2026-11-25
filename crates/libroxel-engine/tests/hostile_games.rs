//! Games that do strange things on purpose (NaN and endless positions and
//! sizes, huge forces, odd joints, deep trees, terrain everywhere) must keep
//! running: a game is downloaded from strangers, so one that misbehaves may
//! stop its own scripts but never the app, the physics or the host.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use libroxel_engine::CharacterInput;
use libroxel_engine::session::start_on;
use libroxel_instance::{ClassKind, InstanceTree, Property, PropertyValue, Vec3};

const DT: f32 = 1.0 / 60.0;

fn world_with(source: &str) -> InstanceTree {
    let mut tree = InstanceTree::new();
    let floor = tree.create(ClassKind::Part, "Floor").unwrap();
    tree.set_parent(floor, Some(tree.workspace())).unwrap();
    tree.set(floor, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, -0.5, 0.0))).unwrap();
    tree.set(floor, Property::Size, PropertyValue::Vec3(Vec3::new(100.0, 1.0, 100.0))).unwrap();
    tree.set(floor, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    let script = tree.create(ClassKind::Script, "Hostile").unwrap();
    tree.set_parent(script, Some(tree.workspace())).unwrap();
    tree.set(script, Property::Source, PropertyValue::String(source.into())).unwrap();
    tree
}

/// Runs a game with `source` as its script for a few seconds; says what
/// went wrong if a step panicked or took far too long.
fn survives(name: &str, source: &str) -> Option<String> {
    let ran = catch_unwind(AssertUnwindSafe(|| {
        let mut session = start_on(world_with(source), true).expect("the game starts");
        let mut slowest = Duration::ZERO;
        for frame in 0..120 {
            let input = CharacterInput {
                move_dir: Vec3::new(1.0, 0.0, (frame % 7) as f32 - 3.0),
                jump: frame % 20 == 0,
                ..Default::default()
            };
            let began = Instant::now();
            session.step(DT, input);
            slowest = slowest.max(began.elapsed());
        }
        slowest
    }));
    match ran {
        Err(why) => Some(format!(
            "{name}: panicked: {}",
            why.downcast_ref::<String>()
                .cloned()
                .or_else(|| why.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default()
        )),
        Ok(slowest) if slowest > Duration::from_secs(2) => {
            Some(format!("{name}: one step took {:.1}s", slowest.as_secs_f32()))
        }
        Ok(_) => None,
    }
}

/// Values a script might put anywhere.
const ODD_VECTORS: &[&str] = &[
    "Vector3.new(0/0, 0/0, 0/0)",
    "Vector3.new(math.huge, 0, 0)",
    "Vector3.new(-math.huge, math.huge, 0/0)",
    "Vector3.new(1e30, 1e30, 1e30)",
    "Vector3.new(0, 0, 0)",
    "Vector3.new(-4, -4, -4)",
    "Vector3.new(1e-30, 1e-30, 1e-30)",
];

const SHAPED: &str = r#"
    local function parts()
        local made = {}
        for _, shape in {"Block", "Ball", "Cylinder", "Wedge", "CornerWedge"} do
            for _, anchored in {true, false} do
                local p = Instance.new("Part")
                pcall(function() p.Shape = Enum.PartType[shape] end)
                p.Anchored = anchored
                p.Position = Vector3.new(#made * 3, 5, 0)
                p.Parent = workspace
                table.insert(made, p)
            end
        end
        local w = Instance.new("WedgePart") w.Parent = workspace table.insert(made, w)
        local t = Instance.new("TrussPart") t.Parent = workspace table.insert(made, t)
        local m = Instance.new("MeshPart") m.Parent = workspace table.insert(made, m)
        return made
    end
"#;

fn cases() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for v in ODD_VECTORS {
        for property in ["Position", "Size", "Orientation", "AssemblyLinearVelocity", "AssemblyAngularVelocity", "Velocity", "RotVelocity", "PivotOffset"] {
            out.push((
                format!("{property} = {v}"),
                format!(
                    "{SHAPED}\nfor _, p in parts() do pcall(function() p.{property} = {v} end) end\n\
                     task.wait(0.2)\nfor _, p in parts() do pcall(function() p.{property} = {v} end) end"
                ),
            ));
        }
        out.push((
            format!("CFrame at {v}"),
            format!(
                "{SHAPED}\nfor _, p in parts() do pcall(function() p.CFrame = CFrame.new({v}) end) \
                 pcall(function() p.CFrame = CFrame.new(Vector3.new(), {v}) end) \
                 pcall(function() p.CFrame = CFrame.fromAxisAngle({v}, 1e30) end) end"
            ),
        ));
        out.push((
            format!("forces of {v}"),
            format!(
                "{SHAPED}\nfor _, p in parts() do
                    pcall(function() p:ApplyImpulse({v}) end)
                    pcall(function() p:ApplyAngularImpulse({v}) end)
                    for _, class in {{'BodyVelocity', 'BodyPosition', 'BodyGyro', 'BodyForce', 'BodyAngularVelocity',
                                      'LinearVelocity', 'AngularVelocity', 'VectorForce', 'AlignPosition',
                                      'AlignOrientation', 'Attachment'}} do
                        local f = Instance.new(class)
                        for _, prop in {{'Velocity', 'Position', 'Force', 'MaxForce', 'VectorVelocity',
                                         'AngularVelocity', 'MaxTorque', 'P', 'D', 'Responsiveness'}} do
                            pcall(function() f[prop] = {v} end)
                            pcall(function() f[prop] = 0/0 end)
                            pcall(function() f[prop] = math.huge end)
                        end
                        f.Parent = p
                    end
                end"
            ),
        ));
        out.push((
            format!("character moved to {v}"),
            format!(
                "game.Players.PlayerAdded:Connect(function(pl) pl.CharacterAdded:Connect(function(c)
                    task.wait(0.1)
                    pcall(function() c:PivotTo(CFrame.new({v})) end)
                    pcall(function() c:MoveTo({v}) end)
                    local h = c:FindFirstChildOfClass('Humanoid')
                    if h then
                        for _, prop in {{'WalkSpeed', 'JumpPower', 'JumpHeight', 'HipHeight', 'Health', 'MaxHealth', 'MaxSlopeAngle'}} do
                            pcall(function() h[prop] = 0/0 end)
                            pcall(function() h[prop] = math.huge end)
                            pcall(function() h[prop] = -1e30 end)
                        end
                        pcall(function() h:MoveTo({v}) end)
                        pcall(function() h.CameraOffset = {v} end)
                    end
                    pcall(function() workspace.Gravity = 0/0 end)
                    task.wait(0.3)
                    pcall(function() workspace.Gravity = math.huge end)
                end) end)
                for _, p in game.Players:GetPlayers() do pcall(function() p.Character:PivotTo(CFrame.new({v})) end) end"
            ),
        ));
        out.push((
            format!("joints at {v}"),
            format!(
                "{SHAPED}\nlocal ps = parts()
                for i = 1, #ps - 1 do
                    for _, class in {{'Weld', 'WeldConstraint', 'Motor6D', 'HingeConstraint', 'RopeConstraint',
                                      'SpringConstraint', 'BallSocketConstraint', 'PrismaticConstraint', 'RodConstraint'}} do
                        local j = Instance.new(class)
                        pcall(function() j.Part0 = ps[i] j.Part1 = ps[i + 1] end)
                        pcall(function() j.C0 = CFrame.new({v}) end)
                        local a0, a1 = Instance.new('Attachment'), Instance.new('Attachment')
                        pcall(function() a0.Position = {v} end)
                        a0.Parent = ps[i] a1.Parent = ps[i + 1]
                        pcall(function() j.Attachment0 = a0 j.Attachment1 = a1 end)
                        for _, prop in {{'Length', 'Stiffness', 'Damping', 'MaxForce', 'AngularVelocity', 'MotorMaxTorque',
                                         'TargetAngle', 'Restitution', 'FreeLength'}} do
                            pcall(function() j[prop] = 0/0 end)
                            pcall(function() j[prop] = math.huge end)
                        end
                        j.Parent = ps[i]
                    end
                end
                -- A weld to itself and a loop of welds.
                local self_weld = Instance.new('Weld') self_weld.Part0 = ps[1] self_weld.Part1 = ps[1] self_weld.Parent = ps[1]
                local loop = Instance.new('WeldConstraint') loop.Part0 = ps[#ps] loop.Part1 = ps[1] loop.Parent = ps[1]"
            ),
        ));
    }
    out.push((
        "terrain everywhere".into(),
        "local t = workspace.Terrain
         for _, size in {Vector3.new(1e9, 1e9, 1e9), Vector3.new(math.huge, 4, 4), Vector3.new(0/0, 4, 4)} do
             pcall(function() t:FillBlock(CFrame.new(), size, Enum.Material.Grass) end)
             pcall(function() t:FillWedge(CFrame.new(), size, Enum.Material.Grass) end)
             pcall(function() t:FillRegion(Region3.new(-size, size), 4, Enum.Material.Grass) end)
             pcall(function() t:ReadVoxels(Region3.new(-size, size), 1e9) end)
             pcall(function() t:ReplaceMaterial(Region3.new(-size, size), 4, Enum.Material.Grass, Enum.Material.Rock) end)
         end
         pcall(function() t:FillBall(Vector3.new(0/0, 0, 0), math.huge, Enum.Material.Grass) end)
         pcall(function() t:FillCylinder(CFrame.new(), math.huge, math.huge, Enum.Material.Grass) end)
         pcall(function() t:WriteVoxels(Region3.new(Vector3.new(), Vector3.new(4, 4, 4)), 1e9, {{{Enum.Material.Grass}}}, {{{1}}}) end)
         t:FillBlock(CFrame.new(0, -10, 0), Vector3.new(64, 4, 64), Enum.Material.Grass)
         pcall(function() workspace:Raycast(Vector3.new(0, 100, 0), Vector3.new(1e30, 0, 0)) end)
         pcall(function() workspace:Raycast(Vector3.new(0, 100, 0), Vector3.new(0/0, 0, 0)) end)
         pcall(function() workspace:Raycast(Vector3.new(0/0, 100, 0), Vector3.new(0, -1e30, 0)) end)
         pcall(function() workspace:FindPartOnRay(Ray.new(Vector3.new(0, 100, 0), Vector3.new(math.huge, 0, 0))) end)"
            .into(),
    ));
    out.push((
        "a very deep tree".into(),
        "local at = workspace
         for i = 1, 3000 do local f = Instance.new('Part') f.Anchored = true f.Parent = at at = f end
         at:Destroy()"
            .into(),
    ));
    out.push((
        "many parts in one place".into(),
        "for i = 1, 400 do local p = Instance.new('Part') p.Position = Vector3.new(0, 5, 0) p.Parent = workspace end".into(),
    ));
    out.push((
        "gui with odd sizes".into(),
        "game.Players.PlayerAdded:Connect(function(pl)
            local g = Instance.new('ScreenGui') g.Parent = pl:WaitForChild('PlayerGui')
            for _, class in {'Frame', 'TextLabel', 'TextButton', 'ImageLabel', 'ScrollingFrame', 'TextBox', 'ViewportFrame'} do
                local f = Instance.new(class)
                pcall(function() f.Size = UDim2.new(0/0, 1e30, math.huge, -1e30) end)
                pcall(function() f.Position = UDim2.new(1e30, 0/0, -math.huge, 1e9) end)
                pcall(function() f.Rotation = 0/0 end)
                pcall(function() f.TextSize = 1e9 end)
                pcall(function() f.Text = string.rep('W', 100000) end)
                pcall(function() f.CanvasSize = UDim2.new(1e9, 1e9, 1e9, 1e9) end)
                pcall(function() f.ImageRectSize = Vector2.new(0/0, 1e30) end)
                f.Parent = g
            end
        end)"
            .into(),
    ));
    out
}

#[test]
fn hostile_games_keep_running() {
    let quiet = std::panic::take_hook();
    if std::env::var_os("HOSTILE_LOUD").is_none() {
        std::panic::set_hook(Box::new(|_| {}));
    }
    let only = std::env::var("HOSTILE_ONLY").ok();
    let mut problems = Vec::new();
    for (name, source) in cases() {
        if only.as_deref().is_some_and(|o| !name.contains(o)) {
            continue;
        }
        eprintln!("{name}");
        problems.extend(survives(&name, &source));
    }
    std::panic::set_hook(quiet);
    assert!(problems.is_empty(), "{} problems:\n{}", problems.len(), problems.join("\n"));
}

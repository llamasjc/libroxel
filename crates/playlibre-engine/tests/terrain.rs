//! Terrain: scripts build it, things land on it, characters climb it,
//! rays hit it, and joined computers get it.

use std::cell::RefCell;
use std::rc::Rc;

use playlibre_engine::session::start_on;
use playlibre_engine::{CharacterInput, LogLevel, PlaySession};
use playlibre_instance::{ClassKind, InstanceId, InstanceTree, Material, Property, PropertyValue, Vec3};

const DT: f32 = 1.0 / 60.0;

fn script(tree: &mut InstanceTree, source: &str) {
    let id = tree.create(ClassKind::Script, "Build").unwrap();
    tree.set(id, Property::Source, PropertyValue::String(source.into())).unwrap();
    let sss = tree.service(ClassKind::ServerScriptService).unwrap();
    tree.set_parent(id, Some(sss)).unwrap();
}

fn printed(session: &mut PlaySession) -> Vec<String> {
    let log = session.take_log();
    assert!(log.iter().all(|l| l.level != LogLevel::Error), "{log:?}");
    log.into_iter().map(|l| l.text).collect()
}

/// A grass field 60 studs across with its top at y = 0, and three one-stud
/// steps of rock up towards +X.
const FIELD: &str = r#"
    local t = workspace.Terrain
    t:FillBlock(CFrame.new(0, -4, 0), Vector3.new(60, 8, 60), Enum.Material.Grass)
    for i = 1, 3 do
        t:FillBlock(CFrame.new(10 + i * 3, i / 2, 0), Vector3.new(3, i, 8), Enum.Material.Rock)
    end
"#;

#[test]
fn parts_land_on_terrain_and_fall_into_holes_dug_in_it() {
    let mut tree = InstanceTree::new();
    script(&mut tree, FIELD);
    let ws = tree.workspace();
    let drop = |tree: &mut InstanceTree, name: &str, x: f32| -> InstanceId {
        let p = tree.create(ClassKind::Part, name).unwrap();
        tree.set(p, Property::Position, PropertyValue::Vec3(Vec3::new(x, 10.0, -20.0))).unwrap();
        tree.set(p, Property::Size, PropertyValue::Vec3(Vec3::new(2.0, 2.0, 2.0))).unwrap();
        tree.set_parent(p, Some(ws)).unwrap();
        p
    };
    let crate_ = drop(&mut tree, "Crate", -20.0);
    let ball = drop(&mut tree, "Ball", 20.0);
    let mut session = start_on(tree, false).unwrap();
    for _ in 0..90 {
        session.step(DT, CharacterInput::default());
    }
    let t = session.runtime().tree().clone();
    let y = |id| t.borrow().get_vec3(id, Property::Position).unwrap().y;
    assert!((y(crate_) - 1.0).abs() < 0.05, "rests on the grass: {}", y(crate_));
    assert!((y(ball) - 1.0).abs() < 0.05);
    // Dig a pit under the ball.
    session
        .runtime()
        .run("dig", "workspace.Terrain:FillBall(Vector3.new(20, 0, -20), 4, Enum.Material.Air)")
        .unwrap();
    for _ in 0..60 {
        session.step(DT, CharacterInput::default());
    }
    assert!(y(ball) < -1.5, "fell in: {}", y(ball));
    assert!((y(crate_) - 1.0).abs() < 0.05);
    printed(&mut session);
}

#[test]
fn characters_walk_up_terrain_steps() {
    let mut tree = InstanceTree::new();
    script(&mut tree, FIELD);
    let mut session = start_on(tree, true).unwrap();
    let c = session.character().unwrap();
    let t = session.runtime().tree().clone();
    let at = || t.borrow().get_vec3(c.root, Property::Position).unwrap();
    // Stand at the bottom of the steps and walk up them.
    session.step(DT, CharacterInput::default());
    t.borrow_mut().set(c.root, Property::Position, PropertyValue::Vec3(Vec3::new(4.0, 3.0, 0.0))).unwrap();
    for _ in 0..60 {
        session.step(DT, CharacterInput::default());
    }
    let ground = at().y;
    let walk = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), ..Default::default() };
    let mut highest = ground;
    for _ in 0..90 {
        session.step(DT, walk);
        highest = highest.max(at().y);
    }
    let end = at();
    assert!(end.x > 22.0, "got over the steps: {end:?}");
    assert!(highest > ground + 2.5, "climbed three studs: {ground} -> {highest}");
    assert!((end.y - ground).abs() < 0.1, "and down the far side: {end:?}");
    printed(&mut session);
}

#[test]
fn rays_hit_terrain_and_say_what_it_is_made_of() {
    let mut tree = InstanceTree::new();
    script(
        &mut tree,
        &format!(
            "{FIELD}\n{}",
            r#"
            local hit = workspace:Raycast(Vector3.new(-5, 20, 3), Vector3.new(0, -50, 0))
            print(hit.Instance.Name, hit.Position.Y, hit.Material, hit.Normal.Y)
            local step = workspace:Raycast(Vector3.new(16, 20, 0), Vector3.new(0, -50, 0))
            print(step.Position.Y, step.Material)
            workspace.Terrain:Clear()
            print(workspace:Raycast(Vector3.new(-5, 20, 3), Vector3.new(0, -50, 0)))
            "#
        ),
    );
    let mut session = start_on(tree, false).unwrap();
    session.step(DT, CharacterInput::default());
    let out = printed(&mut session);
    assert_eq!(out, ["Terrain 0 Enum.Material.Grass 1", "2 Enum.Material.Rock", "nil"]);
}

#[test]
fn terrain_is_saved_with_the_game_and_reaches_joined_players() {
    let mut tree = InstanceTree::new();
    let terrain = tree.terrain_or_make().unwrap();
    let axes = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];
    tree.edit_voxels(terrain, |v| {
        v.fill_block(Vec3::new(0.0, -2.0, 0.0), axes, Vec3::new(64.0, 4.0, 64.0), Some(Material::Sand))
    })
    .unwrap();
    let mut host = PlaySession::host(Rc::new(RefCell::new(tree)), 0, None, false).unwrap();
    let port = host.host_address().unwrap().port();
    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Ann")
            .unwrap();
    let has_sand = |s: &PlaySession, x: i32| {
        let tree = s.runtime().tree().borrow();
        tree.terrain().and_then(|t| tree.voxels(t).ok()?.get(x, -1, 0)) == Some(Material::Sand)
    };
    let mut ok = false;
    for _ in 0..600 {
        host.step(DT, CharacterInput::default());
        guest.step(DT, CharacterInput::default());
        if has_sand(&guest, 31) && guest.character().is_some() {
            ok = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(ok, "the guest got the terrain");
    // Digging on the host reaches the guest too.
    host.runtime()
        .run(
            "dig",
            "workspace.Terrain:FillBlock(CFrame.new(31, -2, 0), Vector3.new(2, 4, 2), Enum.Material.Air)",
        )
        .unwrap();
    let mut dug = false;
    for _ in 0..600 {
        host.step(DT, CharacterInput::default());
        guest.step(DT, CharacterInput::default());
        if !has_sand(&guest, 31) {
            dug = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(dug, "the guest saw the hole");
    assert!(has_sand(&guest, 0));
}

#[test]
fn things_float_and_characters_swim_in_water() {
    let mut tree = InstanceTree::new();
    // A rock basin with water 8 deep inside, its top at y = 8.
    script(
        &mut tree,
        r#"
        local t = workspace.Terrain
        t:FillBlock(CFrame.new(0, 4, 0), Vector3.new(40, 10, 40), Enum.Material.Rock)
        t:FillBlock(CFrame.new(0, 5, 0), Vector3.new(30, 8, 30), Enum.Material.Water)
        local hit = workspace:Raycast(Vector3.new(0, 30, 0), Vector3.new(0, -50, 0))
        print(hit.Material, hit.Position.Y)
        "#,
    );
    let ws = tree.workspace();
    let float = tree.create(ClassKind::Part, "Float").unwrap();
    tree.set(float, Property::Position, PropertyValue::Vec3(Vec3::new(8.0, 3.0, 8.0))).unwrap();
    tree.set(float, Property::Size, PropertyValue::Vec3(Vec3::new(1.0, 1.0, 1.0))).unwrap();
    tree.set_parent(float, Some(ws)).unwrap();
    let mut session = start_on(tree, true).unwrap();
    // Rays go through water to the bottom.
    assert_eq!(printed(&mut session), ["Enum.Material.Rock 1"]);
    let c = session.character().unwrap();
    let t = session.runtime().tree().clone();
    let at = |id| t.borrow().get_vec3(id, Property::Position).unwrap();
    t.borrow_mut().set(c.root, Property::Position, PropertyValue::Vec3(Vec3::new(-5.0, 4.0, 0.0))).unwrap();
    for _ in 0..240 {
        session.step(DT, CharacterInput::default());
    }
    // The block bobbed up to the top; the character drifted up and treads water.
    assert!((at(float).y - 8.0).abs() < 1.0, "floats: {:?}", at(float));
    let treading = at(c.root).y;
    assert!(treading > 5.5 && treading < 9.0, "{treading}");
    // Swimming along.
    let swim = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), ..Default::default() };
    for _ in 0..30 {
        session.step(DT, swim);
    }
    let x = at(c.root).x;
    assert!(x > 0.0 && x < 7.0, "swims slower than walking: {x}");
    // Jumping at the top leaps out of the water.
    let jump = CharacterInput { jump: true, ..Default::default() };
    let mut highest = 0.0f32;
    for _ in 0..30 {
        session.step(DT, jump);
        highest = highest.max(at(c.root).y);
    }
    assert!(highest > 10.0, "leapt out: {highest}");
    printed(&mut session);
}

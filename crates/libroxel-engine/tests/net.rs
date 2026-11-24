//! A hosted game and a computer joining it, over a real connection on this
//! machine.

use std::cell::RefCell;
use std::rc::Rc;

use libroxel_engine::{CharacterInput, PlaySession};
use libroxel_instance::{
    ClassKind, EnumValue, InstanceId, InstanceTree, ModelStreamingMode, Property, PropertyValue,
    StreamOutBehavior, Vec3,
};

const DT: f32 = 1.0 / 60.0;

fn add(tree: &mut InstanceTree, class: ClassKind, parent: InstanceId, name: &str) -> InstanceId {
    let id = tree.create(class, name).unwrap();
    tree.set_parent(id, Some(parent)).unwrap();
    id
}

fn code(tree: &mut InstanceTree, class: ClassKind, parent: InstanceId, name: &str, source: &str) {
    let id = add(tree, class, parent, name);
    tree.set(id, Property::Source, PropertyValue::String(source.into())).unwrap();
}

fn block(tree: &mut InstanceTree, name: &str, pos: Vec3, size: Vec3) -> InstanceId {
    let ws = tree.workspace();
    let id = add(tree, ClassKind::Part, ws, name);
    tree.set(id, Property::Position, PropertyValue::Vec3(pos)).unwrap();
    tree.set(id, Property::Size, PropertyValue::Vec3(size)).unwrap();
    tree.set(id, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    id
}

/// A floor, a spawn, a remote event, a game script and a GUI with a local script.
fn game() -> InstanceTree {
    let mut tree = InstanceTree::new();
    block(&mut tree, "Floor", Vec3::new(0.0, -0.5, 0.0), Vec3::new(200.0, 1.0, 200.0));
    let ws = tree.workspace();
    let spawn = add(&mut tree, ClassKind::SpawnLocation, ws, "Spawn");
    tree.set(spawn, Property::Size, PropertyValue::Vec3(Vec3::new(6.0, 1.0, 6.0))).unwrap();
    tree.set(spawn, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, 0.5, 0.0))).unwrap();
    let lamp = block(&mut tree, "Lamp", Vec3::new(10.0, 2.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
    tree.set_attribute(lamp, "Lit", Some(PropertyValue::Bool(false))).unwrap();
    let storage = tree.service(ClassKind::ReplicatedStorage).unwrap();
    add(&mut tree, ClassKind::RemoteEvent, storage, "Wave");
    let sss = tree.service(ClassKind::ServerScriptService).unwrap();
    code(
        &mut tree,
        ClassKind::Script,
        sss,
        "Game",
        r#"
        local wave = game.ReplicatedStorage.Wave
        game.Players.PlayerAdded:Connect(function(player)
            print("hello " .. player.Name)
        end)
        game.Players.PlayerRemoving:Connect(function(player)
            print("bye " .. player.Name)
        end)
        wave.OnServerEvent:Connect(function(player, how)
            print(player.Name .. " waves " .. how)
            workspace.Lamp.Color = Color3.new(1, 0, 0)
            workspace.Lamp:SetAttribute("Lit", true)
            local ball = Instance.new("Part")
            ball.Name = "Ball"
            ball.Position = Vector3.new(0, 20, 0)
            ball.Parent = workspace
            wave:FireClient(player, "back at you", ball)
        end)
        "#,
    );
    let starter = tree.service(ClassKind::StarterGui).unwrap();
    let screen = add(&mut tree, ClassKind::ScreenGui, starter, "Hud");
    code(
        &mut tree,
        ClassKind::LocalScript,
        screen,
        "Waver",
        r#"
        local wave = game.ReplicatedStorage.Wave
        print("local script runs for " .. game.Players.LocalPlayer.Name)
        wave.OnClientEvent:Connect(function(text, ball)
            print(text .. " " .. ball.Name)
        end)
        wave:FireServer("hi")
        "#,
    );
    tree
}

fn printed(session: &mut PlaySession) -> Vec<String> {
    session.take_log().into_iter().map(|l| l.text).collect()
}

/// Steps both until `done` says so, or gives up.
fn until(
    host: &mut PlaySession,
    guest: &mut PlaySession,
    input: CharacterInput,
    mut done: impl FnMut(&PlaySession, &PlaySession) -> bool,
) {
    for _ in 0..600 {
        host.step(DT, CharacterInput::default());
        guest.step(DT, input);
        if done(host, guest) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let c = guest.character();
    let gp = c.and_then(|c| guest.runtime().tree().borrow().get_vec3(c.root, Property::Position));
    let hp = host.players().get(1).and_then(|p| {
        let t = host.runtime().tree().borrow();
        let m = t.get_ref(*p, Property::Character)?;
        let r = t.find_first_child(m, "HumanoidRootPart").ok()??;
        t.get_vec3(r, Property::Position)
    });
    panic!("gave up waiting: guest at {gp:?}, host has them at {hp:?}");
}

fn find(
    session: &PlaySession,
    parent: impl Fn(&InstanceTree) -> InstanceId,
    name: &str,
) -> Option<InstanceId> {
    let tree = session.runtime().tree().borrow();
    tree.find_first_child(parent(&tree), name).ok().flatten()
}

#[test]
fn a_joined_player_sees_the_world_and_plays_in_it() {
    let tree = Rc::new(RefCell::new(game()));
    let mut host = PlaySession::host(tree, 0, None, true).unwrap();
    let port = host.host_address().unwrap().port();
    let mut host_log = printed(&mut host);
    // The host plays too: its own GUI's local script runs and waves.
    assert!(host_log.contains(&"hello Player1".to_owned()), "{host_log:?}");
    assert!(host_log.contains(&"Player1 waves hi".to_owned()), "{host_log:?}");

    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Bea")
            .unwrap();
    assert!(guest.is_joined());
    // The world arrives, the guest gets a player and a character, and the
    // local script in their GUI runs and waves.
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| g.character().is_some());
    assert_eq!(host.joined_count(), 1);
    assert_eq!(host.players().len(), 2);
    assert!(find(&guest, |t| t.workspace(), "Floor").is_some());
    assert!(find(&guest, |t| t.service(ClassKind::Players).unwrap(), "Player1").is_some());
    // Server code stays on the host.
    assert!(find(&guest, |t| t.service(ClassKind::ServerScriptService).unwrap(), "Game").is_none());
    let guest_player = guest.player().unwrap();
    assert_eq!(guest.runtime().tree().borrow().name(guest_player).unwrap(), "Bea");

    until(&mut host, &mut guest, CharacterInput::default(), |h, g| {
        find(g, |t| t.workspace(), "Ball").is_some()
            && h.runtime()
                .tree()
                .borrow()
                .get_vec3(find(h, |t| t.workspace(), "Ball").unwrap(), Property::Position)
                .is_some()
    });
    host_log.extend(printed(&mut host));
    assert!(host_log.contains(&"Bea joined".to_owned()), "{host_log:?}");
    assert!(host_log.contains(&"hello Bea".to_owned()), "{host_log:?}");
    assert!(host_log.contains(&"Bea waves hi".to_owned()), "{host_log:?}");

    // What the host's script changed shows up on the guest.
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| {
        let lamp = find(g, |t| t.workspace(), "Lamp").unwrap();
        let tree = g.runtime().tree().borrow();
        tree.get_attribute(lamp, "Lit").unwrap() == Some(PropertyValue::Bool(true))
    });
    let lamp = find(&guest, |t| t.workspace(), "Lamp").unwrap();
    assert_eq!(
        guest.runtime().tree().borrow().get_color(lamp, Property::Color).map(|c| (c.r, c.g)),
        Some((1.0, 0.0))
    );
    let guest_log = printed(&mut guest);
    assert!(guest_log.contains(&"local script runs for Bea".to_owned()), "{guest_log:?}");
    assert!(guest_log.contains(&"back at you Ball".to_owned()), "{guest_log:?}");

    // The guest walks: their character moves on their computer straight
    // away, without waiting for the host.
    let root_at = |s: &PlaySession, c: libroxel_engine::Character| {
        s.runtime().tree().borrow().get_vec3(c.root, Property::Position).unwrap()
    };
    let mine = guest.character().unwrap();
    // Let them land first.
    for _ in 0..60 {
        host.step(DT, CharacterInput::default());
        guest.step(DT, CharacterInput::default());
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let start = root_at(&guest, mine);
    let walk = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), ..Default::default() };
    guest.step(DT, walk);
    guest.step(DT, walk);
    assert!(root_at(&guest, mine).x > start.x + 0.2, "moved before hearing from the host");
    // The host follows where the guest says it is.
    until(&mut host, &mut guest, walk, |h, g| {
        let theirs = h.runtime().tree().borrow().get_ref(h.players()[1], Property::Character).unwrap();
        let on_host = h
            .runtime()
            .tree()
            .borrow()
            .find_first_child(theirs, "HumanoidRootPart")
            .unwrap()
            .and_then(|r| h.runtime().tree().borrow().get_vec3(r, Property::Position))
            .unwrap();
        root_at(g, mine).x > start.x + 5.0 && on_host.x > start.x + 4.0
    });
    // A host script teleporting them wins over where the guest thought it was.
    host.runtime().run("tp", "workspace.Bea.HumanoidRootPart.Position = Vector3.new(-30, 3, 20)").unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| {
        let at = root_at(g, mine);
        (at.x + 30.0).abs() < 1.0 && (at.z - 20.0).abs() < 1.0
    });
    for _ in 0..30 {
        host.step(DT, CharacterInput::default());
        guest.step(DT, CharacterInput::default());
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let at = root_at(&guest, mine);
    assert!((at.x + 30.0).abs() < 1.0 && (at.z - 20.0).abs() < 1.0, "stayed teleported: {at:?}");

    // Leaving takes their player out of the host's game.
    drop(guest);
    let mut log = Vec::new();
    for _ in 0..600 {
        host.step(DT, CharacterInput::default());
        log.extend(printed(&mut host));
        if host.players().len() == 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(log.contains(&"bye Bea".to_owned()), "{log:?}");
    assert!(log.contains(&"Bea left".to_owned()), "{log:?}");
    assert_eq!(host.joined_count(), 0);
    // Someone else can join with the same name afterwards.
    let mut again =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Bea")
            .unwrap();
    until(&mut host, &mut again, CharacterInput::default(), |_, g| g.character().is_some());
    let names: Vec<String> = {
        let tree = host.runtime().tree().borrow();
        host.players().iter().map(|p| tree.name(*p).unwrap().to_owned()).collect()
    };
    assert_eq!(names, ["Player1", "Bea"]);
}

#[test]
fn everyone_sees_how_a_joined_player_chose_to_look() {
    use libroxel_engine::character::{BodyColors, Hat, Look};
    use libroxel_instance::Color3;
    let tree = Rc::new(RefCell::new(game()));
    let mut host = PlaySession::host(tree, 0, None, true).unwrap();
    let port = host.host_address().unwrap().port();
    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Bea")
            .unwrap();
    let green = Color3::from_rgb8(40, 200, 90);
    let look = Look {
        colors: BodyColors { shirt: green, ..BodyColors::for_player(0) },
        hat: Hat::Crown,
        hat_color: Color3::from_rgb8(250, 210, 60),
        item: None,
    };
    guest.set_local_look(look);
    let dressed = |session: &PlaySession| {
        let tree = session.runtime().tree().borrow();
        let ws = tree.workspace();
        let Some(bea) = tree.find_first_child(ws, "Bea").ok().flatten() else { return false };
        let crown = tree.find_first_child(bea, "Crown").ok().flatten();
        let torso = tree.find_first_child(bea, "Torso").ok().flatten();
        crown.is_some_and(|c| tree.class(c).unwrap() == ClassKind::Accessory)
            && torso
                .and_then(|t| tree.get_color(t, Property::Color))
                .is_some_and(|c| (c.g - green.g).abs() < 0.01)
    };
    until(&mut host, &mut guest, CharacterInput::default(), |h, g| dressed(h) && dressed(g));
}

#[test]
fn the_host_removes_a_player_who_then_cannot_come_back() {
    let tree = Rc::new(RefCell::new(game()));
    let mut host = PlaySession::host(tree, 0, None, true).unwrap();
    let port = host.host_address().unwrap().port();
    let join = || {
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Griefer")
            .unwrap()
    };
    let mut guest = join();
    until(&mut host, &mut guest, CharacterInput::default(), |h, _| h.joined_players().len() == 1);
    let (player, name) = host.joined_players()[0].clone();
    assert_eq!(name, "Griefer");
    host.kick(player, "no griefing");
    until(&mut host, &mut guest, CharacterInput::default(), |h, g| {
        g.disconnected() && h.joined_players().is_empty()
    });
    let said = printed(&mut guest);
    assert!(said.iter().any(|l| l.contains("The host removed you: no griefing")), "{said:?}");

    // Joining again under the same name is refused.
    let mut again = join();
    until(&mut host, &mut again, CharacterInput::default(), |_, g| g.disconnected());
    assert!(host.joined_players().is_empty());
}

#[test]
fn bad_words_are_not_allowed_in_names() {
    let tree = Rc::new(RefCell::new(game()));
    let mut host = PlaySession::host(tree, 0, None, true).unwrap();
    let port = host.host_address().unwrap().port();
    let mut guest = PlaySession::join(
        Rc::new(RefCell::new(InstanceTree::new())),
        &format!("127.0.0.1:{port}"),
        "shithead",
    )
    .unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |h, _| h.joined_players().len() == 1);
    assert!(host.joined_players()[0].1.starts_with("Player"), "{:?}", host.joined_players());
}

#[test]
fn a_joined_player_in_a_seat_faces_the_seat_way() {
    let mut tree = game();
    let ws = tree.workspace();
    let seat = add(&mut tree, ClassKind::Seat, ws, "Bench");
    tree.set(seat, Property::Position, PropertyValue::Vec3(Vec3::new(-15.0, 0.5, 0.0))).unwrap();
    tree.set(seat, Property::Size, PropertyValue::Vec3(Vec3::new(2.0, 1.0, 2.0))).unwrap();
    tree.set(seat, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    let mut host = PlaySession::host(Rc::new(RefCell::new(tree)), 0, None, true).unwrap();
    let port = host.host_address().unwrap().port();
    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Bea")
            .unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| g.character().is_some());
    host.runtime().run("sit", "workspace.Bench:Sit(workspace.Bea.Humanoid)").unwrap();
    let seated = |g: &PlaySession| {
        let c = g.character().unwrap();
        g.runtime().tree().borrow().get_ref(c.humanoid, Property::SeatPart).is_some()
    };
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| seated(g));
    let look = CharacterInput { face: Some(90.0), ..Default::default() };
    for _ in 0..30 {
        host.step(DT, CharacterInput::default());
        guest.step(DT, look);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let c = guest.character().unwrap();
    let yaw = guest.runtime().tree().borrow().get_vec3(c.root, Property::Orientation).unwrap().y;
    assert!(yaw.abs() < 1.0, "turned in the seat: {yaw}");
}

#[test]
fn eyes_stay_on_the_head_of_someone_turning() {
    use libroxel_instance::Rotation;
    let tree = Rc::new(RefCell::new(game()));
    let mut host = PlaySession::host(tree, 0, None, true).unwrap();
    let port = host.host_address().unwrap().port();
    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Bea")
            .unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |h, g| {
        h.character().is_some() && g.character().is_some()
    });
    let name = {
        let t = host.runtime().tree().borrow();
        t.name(host.character().unwrap().model).unwrap().to_owned()
    };
    let other = |g: &PlaySession| {
        let t = g.runtime().tree().borrow();
        t.find_first_child(t.workspace(), &name).ok().flatten()
    };
    let rigged = |g: &PlaySession| {
        other(g).is_some_and(|m| {
            let t = g.runtime().tree().borrow();
            ["Head", "LeftEye", "RightEye"].iter().all(|n| t.find_first_child(m, n).ok().flatten().is_some())
        })
    };
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| rigged(g));
    let model = other(&guest).unwrap();
    let mut worst = 0.0f32;
    for i in 0..240 {
        let a = i as f32 * 0.15;
        let walk = CharacterInput { move_dir: Vec3::new(a.cos(), 0.0, a.sin()), ..Default::default() };
        host.step(DT, walk);
        guest.step(DT, CharacterInput::default());
        std::thread::sleep(std::time::Duration::from_millis(1));
        let t = guest.runtime().tree().borrow();
        let part = |n: &str| t.find_first_child(model, n).ok().flatten().unwrap();
        let head = part("Head");
        let at = t.get_vec3(head, Property::Position).unwrap();
        let turn = Rotation::from_orientation(t.get_vec3(head, Property::Orientation).unwrap()).transpose();
        for (name, x) in [("LeftEye", -0.35), ("RightEye", 0.35)] {
            let eye = turn.apply(t.get_vec3(part(name), Property::Position).unwrap() - at);
            worst = worst.max((eye - Vec3::new(x, 0.1, -0.71)).length());
        }
    }
    assert!(worst < 0.01, "an eye came off the head by {worst}");
}

/// Where the host has the guest's character.
fn host_sees(host: &PlaySession) -> Option<Vec3> {
    let t = host.runtime().tree().borrow();
    let m = t.get_ref(*host.players().get(1)?, Property::Character)?;
    let r = t.find_first_child(m, "HumanoidRootPart").ok()??;
    t.get_vec3(r, Property::Position)
}

#[test]
fn a_joined_computer_cannot_teleport_its_character_across_the_map() {
    let tree = Rc::new(RefCell::new(game()));
    let mut host = PlaySession::host(tree, 0, None, true).unwrap();
    let port = host.host_address().unwrap().port();
    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Bea")
            .unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |h, g| {
        g.character().is_some() && host_sees(h).is_some()
    });
    // Let the character land.
    for _ in 0..60 {
        host.step(DT, CharacterInput::default());
        guest.step(DT, CharacterInput::default());
    }
    let start = host_sees(&host).unwrap();

    // A changed copy of the app moves its character 5000 studs in one go.
    let root = guest.character().unwrap().root;
    let far = Vec3::new(start.x + 5000.0, start.y, start.z);
    guest.runtime().tree().borrow_mut().set(root, Property::Position, PropertyValue::Vec3(far)).unwrap();
    for _ in 0..30 {
        host.step(DT, CharacterInput::default());
        guest.step(DT, CharacterInput::default());
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let on_host = host_sees(&host).unwrap();
    assert!((on_host - start).length() < 20.0, "the host kept them where they were: {on_host:?}");
    // And their own computer is put back too.
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| {
        let at = g.runtime().tree().borrow().get_vec3(root, Property::Position).unwrap();
        (at - start).length() < 20.0
    });

    // Walking normally still works afterwards.
    let walk = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), ..CharacterInput::default() };
    until(&mut host, &mut guest, walk, |h, _| host_sees(h).is_some_and(|p| p.x > start.x + 5.0));
}

#[test]
fn two_players_on_one_spawn_stay_on_the_ground() {
    // Before, the joined player stood on the host's player, the host's
    // player was pushed up out of them, and the two climbed into the sky.
    let tree = Rc::new(RefCell::new(game()));
    let mut host = PlaySession::host(tree, 0, None, true).unwrap();
    let port = host.host_address().unwrap().port();
    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Bea")
            .unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |h, g| {
        g.character().is_some() && host_sees(h).is_some()
    });
    for _ in 0..180 {
        host.step(DT, CharacterInput::default());
        guest.step(DT, CharacterInput::default());
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let root = guest.character().unwrap().root;
    let guest_at = guest.runtime().tree().borrow().get_vec3(root, Property::Position).unwrap();
    let host_at = host_sees(&host).unwrap();
    assert!(guest_at.y < 12.0, "the joined player stayed down: {guest_at:?}");
    assert!(host_at.y < 12.0, "and the host has them down too: {host_at:?}");
}

/// A streamed world, as a place saved with StreamingEnabled: a floor and
/// spawn at the middle, a part near it, a part far away, a far persistent
/// model, an atomic model with one part near and one far, and a far folder
/// of values.
fn host_streamed(behavior: StreamOutBehavior) -> PlaySession {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    tree.set(ws, Property::StreamingEnabled, PropertyValue::Bool(true)).unwrap();
    tree.set(ws, Property::StreamingTargetRadius, PropertyValue::Number(128.0)).unwrap();
    tree.set(ws, Property::StreamOutBehavior, PropertyValue::Enum(EnumValue::StreamOutBehavior(behavior)))
        .unwrap();
    block(&mut tree, "Floor", Vec3::new(0.0, -0.5, 0.0), Vec3::new(60.0, 1.0, 60.0));
    let spawn = add(&mut tree, ClassKind::SpawnLocation, ws, "Spawn");
    tree.set(spawn, Property::Size, PropertyValue::Vec3(Vec3::new(6.0, 1.0, 6.0))).unwrap();
    tree.set(spawn, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, 0.5, 0.0))).unwrap();
    block(&mut tree, "Near", Vec3::new(20.0, 2.0, 0.0), Vec3::new(2.0, 2.0, 2.0));
    block(&mut tree, "Far", Vec3::new(900.0, 2.0, 0.0), Vec3::new(2.0, 2.0, 2.0));
    let model = |tree: &mut InstanceTree, name: &str, mode: ModelStreamingMode, parts: &[(&str, f32)]| {
        let ws = tree.workspace();
        let m = add(tree, ClassKind::Model, ws, name);
        tree.set(m, Property::ModelStreamingMode, PropertyValue::Enum(EnumValue::ModelStreamingMode(mode)))
            .unwrap();
        for (part, x) in parts {
            let id = block(tree, part, Vec3::new(*x, 2.0, 40.0), Vec3::new(2.0, 1.0, 2.0));
            tree.set_parent(id, Some(m)).unwrap();
        }
    };
    model(&mut tree, "Statue", ModelStreamingMode::Persistent, &[("Stone", -900.0)]);
    model(&mut tree, "Bridge", ModelStreamingMode::Atomic, &[("Plank1", 30.0), ("Plank2", 700.0)]);
    model(&mut tree, "Vault", ModelStreamingMode::PersistentPerPlayer, &[("Gold", -600.0)]);
    let settings = add(&mut tree, ClassKind::Folder, ws, "Settings");
    add(&mut tree, ClassKind::NumberValue, settings, "Speed");
    PlaySession::host(Rc::new(RefCell::new(tree)), 0, None, true).unwrap()
}

fn has(session: &PlaySession, path: &[&str]) -> bool {
    let t = session.runtime().tree().borrow();
    let mut at = t.workspace();
    for name in path {
        match t.find_first_child(at, name).ok().flatten() {
            Some(next) => at = next,
            None => return false,
        }
    }
    true
}

#[test]
fn a_streamed_game_sends_each_player_only_what_is_near_them() {
    let mut host = host_streamed(StreamOutBehavior::Opportunistic);
    let port = host.host_address().unwrap().port();
    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Bea")
            .unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| {
        g.character().is_some() && has(g, &["Near"])
    });
    assert!(has(&guest, &["Floor"]) && has(&guest, &["Spawn"]));
    assert!(!has(&guest, &["Far"]), "far parts aren't sent");
    // A persistent model always is, however far.
    assert!(has(&guest, &["Statue", "Stone"]));
    // An atomic model with a part near comes whole.
    assert!(has(&guest, &["Bridge", "Plank1"]) && has(&guest, &["Bridge", "Plank2"]));
    // Folders and what isn't in a part always come.
    assert!(has(&guest, &["Settings", "Speed"]));
    // Everything is still on the host.
    assert!(has(&host, &["Far"]));

    // The host moves them across the map: the far part streams in, and with
    // Opportunistic, the near one streams out.
    host.runtime().run("tp", "workspace.Bea.HumanoidRootPart.Position = Vector3.new(890, 5, 0)").unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| has(g, &["Far"]) && !has(g, &["Near"]));
    assert!(has(&guest, &["Statue", "Stone"]), "persistent stays");
}

#[test]
fn parts_stay_once_sent_unless_streaming_out_is_opportunistic() {
    let mut host = host_streamed(StreamOutBehavior::Default);
    let port = host.host_address().unwrap().port();
    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Bea")
            .unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| {
        g.character().is_some() && has(g, &["Near"])
    });
    host.runtime().run("tp", "workspace.Bea.HumanoidRootPart.Position = Vector3.new(890, 5, 0)").unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| has(g, &["Far"]));
    assert!(has(&guest, &["Near"]), "with Default, what was sent stays");
}

#[test]
fn a_server_script_can_stream_an_area_or_move_the_focus() {
    let mut host = host_streamed(StreamOutBehavior::Opportunistic);
    let port = host.host_address().unwrap().port();
    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Bea")
            .unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| {
        g.character().is_some() && has(g, &["Near"])
    });
    assert!(!has(&guest, &["Far"]));
    host.runtime().run("ask", "game.Players.Bea:RequestStreamAroundAsync(Vector3.new(900, 2, 0))").unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| has(g, &["Far"]));
    assert!(has(&guest, &["Near"]), "their own surroundings stay too");

    // A ReplicationFocus replaces the character as the centre.
    let mut host = host_streamed(StreamOutBehavior::Opportunistic);
    let port = host.host_address().unwrap().port();
    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Bea")
            .unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| {
        g.character().is_some() && has(g, &["Near"])
    });
    host.runtime().run("focus", "game.Players.Bea.ReplicationFocus = workspace.Far").unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| has(g, &["Far"]) && !has(g, &["Near"]));
}

#[test]
fn a_game_without_streaming_still_sends_everything() {
    let tree = Rc::new(RefCell::new(game()));
    block(&mut tree.borrow_mut(), "Far", Vec3::new(5000.0, 2.0, 0.0), Vec3::new(2.0, 2.0, 2.0));
    let mut host = PlaySession::host(tree, 0, None, true).unwrap();
    let port = host.host_address().unwrap().port();
    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Bea")
            .unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| {
        g.character().is_some() && has(g, &["Far"])
    });
}

#[test]
fn a_per_player_persistent_model_goes_to_the_players_added_to_it() {
    let mut host = host_streamed(StreamOutBehavior::Opportunistic);
    let port = host.host_address().unwrap().port();
    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Bea")
            .unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| {
        g.character().is_some() && has(g, &["Near"])
    });
    // The model itself comes (as Default), but not its far part.
    assert!(has(&guest, &["Vault"]) && !has(&guest, &["Vault", "Gold"]));
    host.runtime().run("add", "workspace.Vault:AddPersistentPlayer(game.Players.Bea)").unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| has(g, &["Vault", "Gold"]));
    host.runtime().run("remove", "workspace.Vault:RemovePersistentPlayer(game.Players.Bea)").unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |_, g| !has(g, &["Vault", "Gold"]));
    // Only a Model takes players, and only players.
    assert!(host.runtime().run("bad", "workspace.Near:AddPersistentPlayer(game.Players.Bea)").is_err());
    assert!(host.runtime().run("bad", "workspace.Vault:AddPersistentPlayer(workspace.Near)").is_err());
}

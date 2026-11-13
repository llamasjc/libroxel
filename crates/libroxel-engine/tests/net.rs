//! A hosted game and a computer joining it, over a real connection on this
//! machine.

use std::cell::RefCell;
use std::rc::Rc;

use libroxel_engine::{CharacterInput, PlaySession};
use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue, Vec3};

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

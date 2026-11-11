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
    panic!("gave up waiting");
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

    // The guest walks: the host moves their character, and the guest sees it.
    let start = {
        let c = guest.character().unwrap();
        guest.runtime().tree().borrow().get_vec3(c.root, Property::Position).unwrap()
    };
    let walk = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), ..Default::default() };
    until(&mut host, &mut guest, walk, |_, g| {
        let c = g.character().unwrap();
        let at = g.runtime().tree().borrow().get_vec3(c.root, Property::Position).unwrap();
        at.x > start.x + 5.0
    });

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

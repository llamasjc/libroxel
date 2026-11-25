//! A hosted game joined with a join code, through iroh on this computer.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use playlibre_engine::{CharacterInput, PlaySession};
use playlibre_instance::{ClassKind, InstanceTree, Property, PropertyValue, Vec3};
use playlibre_net::{HostTunnel, Identity, JoinTunnel, Network};

const DT: f32 = 1.0 / 60.0;

fn game() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let floor = tree.create(ClassKind::Part, "Floor").unwrap();
    tree.set_parent(floor, Some(ws)).unwrap();
    tree.set(floor, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, -0.5, 0.0))).unwrap();
    tree.set(floor, Property::Size, PropertyValue::Vec3(Vec3::new(200.0, 1.0, 200.0))).unwrap();
    tree.set(floor, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    let spawn = tree.create(ClassKind::SpawnLocation, "Spawn").unwrap();
    tree.set_parent(spawn, Some(ws)).unwrap();
    tree.set(spawn, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, 0.5, 0.0))).unwrap();
    tree
}

fn until(
    host: &mut PlaySession,
    guest: &mut PlaySession,
    input: CharacterInput,
    done: impl Fn(&PlaySession) -> bool,
) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        host.step(DT, CharacterInput::default());
        guest.step(DT, input);
        if done(guest) {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("gave up waiting");
}

#[test]
fn a_friend_joins_with_a_code_and_walks_around() {
    let mut host = PlaySession::host(Rc::new(RefCell::new(game())), 0, None, true).unwrap();
    let port = host.host_address().unwrap().port();
    let tunnel = HostTunnel::start(&Identity::generate(), Network::LocalOnly, port).unwrap();
    let start = Instant::now();
    let code = loop {
        host.step(DT, CharacterInput::default());
        if let Some(code) = tunnel.code() {
            break code;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "no join code: {:?}", tunnel.error());
        std::thread::sleep(Duration::from_millis(10));
    };

    let link = JoinTunnel::start(&Identity::generate(), Network::LocalOnly, &code).unwrap();
    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &link.local_addr().to_string(), "Bea")
            .unwrap();
    until(&mut host, &mut guest, CharacterInput::default(), |g| g.character().is_some());
    assert!(link.connected());
    assert_eq!(tunnel.connections(), 1);
    assert_eq!(host.joined_count(), 1);

    let from = {
        let c = guest.character().unwrap();
        guest.runtime().tree().borrow().get_vec3(c.root, Property::Position).unwrap()
    };
    let walk = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), ..Default::default() };
    until(&mut host, &mut guest, walk, |g| {
        let c = g.character().unwrap();
        g.runtime().tree().borrow().get_vec3(c.root, Property::Position).unwrap().x > from.x + 5.0
    });

    // Leaving through the tunnel takes the player out of the host's game.
    drop(guest);
    drop(link);
    let start = Instant::now();
    while host.joined_count() > 0 {
        assert!(start.elapsed() < Duration::from_secs(10), "the player never left");
        host.step(DT, CharacterInput::default());
        std::thread::sleep(Duration::from_millis(2));
    }
}

//! Games over bad networks: slow, uneven and lossy connections, lag
//! spikes, connections that go silent or hang up, hosts that leave, many
//! players at once, and long games. Each joined computer must end up
//! showing exactly the world the host has.
//!
//! The network is pretended by [`bad_network::Proxy`] on this machine.

mod bad_network;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use bad_network::{Proxy, Weather};
use playlibre_engine::{CharacterInput, PlaySession};
use playlibre_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue, Vec3};

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

/// Makes, removes, moves, renames, recolours and re-parents parts every
/// step while the workspace's `Churning` attribute is on, from a seeded
/// random sequence, so a run can be repeated.
const CHURN: &str = r#"
local seed = 20261125
local function rnd(n)
    seed = (seed * 1103515245 + 12345) % 2147483648
    return seed % n + 1
end
local folder = Instance.new("Folder")
folder.Name = "Churn"
folder.Parent = workspace
local parts = {}
local made = 0
local function live()
    local kept = {}
    for _, p in parts do
        if p:IsDescendantOf(folder) then
            table.insert(kept, p)
        end
    end
    parts = kept
end
game:GetService("RunService").Heartbeat:Connect(function()
    if not workspace:GetAttribute("Churning") then
        return
    end
    for _ = 1, 12 do
        local r = rnd(8)
        pcall(function()
            if r == 1 or #parts < 8 then
                made += 1
                local p = Instance.new("Part")
                p.Name = "P" .. made
                p.Anchored = true
                p.Size = Vector3.new(1, 1, 1)
                p.Position = Vector3.new(rnd(200) - 100, rnd(30), rnd(200) - 100)
                if rnd(3) == 1 and #parts > 0 then
                    p.Parent = parts[rnd(#parts)]
                else
                    p.Parent = folder
                end
                table.insert(parts, p)
            elseif r == 2 then
                parts[rnd(#parts)]:Destroy()
                live()
            elseif r == 3 then
                parts[rnd(#parts)].Position = Vector3.new(rnd(200) - 100, rnd(30), rnd(200) - 100)
            elseif r == 4 then
                parts[rnd(#parts)]:SetAttribute("N", rnd(1000))
            elseif r == 5 then
                local a, b = parts[rnd(#parts)], parts[rnd(#parts)]
                if a ~= b and not b:IsDescendantOf(a) then
                    a.Parent = b
                end
            elseif r == 6 then
                parts[rnd(#parts)].Name = "R" .. rnd(40)
            elseif r == 7 then
                local a = parts[rnd(#parts)]
                a.Parent = folder
            else
                parts[rnd(#parts)].Color = Color3.new(rnd(255) / 255, 0, 0)
            end
        end)
    end
end)
"#;

/// A floor, a spawn, a remote function and the churning script.
fn game() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let floor = add(&mut tree, ClassKind::Part, ws, "Floor");
    tree.set(floor, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, -0.5, 0.0))).unwrap();
    tree.set(floor, Property::Size, PropertyValue::Vec3(Vec3::new(400.0, 1.0, 400.0))).unwrap();
    tree.set(floor, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    let spawn = add(&mut tree, ClassKind::SpawnLocation, ws, "Spawn");
    tree.set(spawn, Property::Size, PropertyValue::Vec3(Vec3::new(6.0, 1.0, 6.0))).unwrap();
    tree.set(spawn, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, 0.5, 0.0))).unwrap();
    tree.set(spawn, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    let storage = tree.service(ClassKind::ReplicatedStorage).unwrap();
    add(&mut tree, ClassKind::RemoteFunction, storage, "Ask");
    let sss = tree.service(ClassKind::ServerScriptService).unwrap();
    code(&mut tree, ClassKind::Script, sss, "Churn", CHURN);
    code(
        &mut tree,
        ClassKind::Script,
        sss,
        "Answer",
        r#"
        game.ReplicatedStorage.Ask.OnServerInvoke = function(player, x)
            return x * 2
        end
        "#,
    );
    tree
}

fn host_game() -> PlaySession {
    PlaySession::host(Rc::new(RefCell::new(game())), 0, None, true).unwrap()
}

/// The game with `Workspace.StreamingEnabled`: each player is sent what's
/// near them, here everything (the churn stays within the radius).
fn host_streamed_game() -> PlaySession {
    let mut tree = game();
    let ws = tree.workspace();
    tree.set(ws, Property::StreamingEnabled, PropertyValue::Bool(true)).unwrap();
    PlaySession::host(Rc::new(RefCell::new(tree)), 0, None, true).unwrap()
}

fn host_addr(host: &PlaySession) -> std::net::SocketAddr {
    let port = host.host_address().unwrap().port();
    format!("127.0.0.1:{port}").parse().unwrap()
}

fn join(address: &str, name: &str) -> PlaySession {
    PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), address, name).unwrap()
}

fn churn(host: &PlaySession, on: bool) {
    host.runtime().run("churn", &format!("workspace:SetAttribute('Churning', {on})")).unwrap();
}

/// Everything a player sees of the shared world, in a form two computers
/// can compare: each instance's path, class, and what it looks like.
/// The characters of `players` (the host's players, by name) are left
/// out, since each computer moves its own; any other character, such as
/// one left behind by a player who has gone, counts.
fn picture(session: &PlaySession, players: &[String]) -> Vec<String> {
    let t = session.runtime().tree().borrow();
    let ws = t.workspace();
    let characters: Vec<InstanceId> = players
        .iter()
        .filter_map(|name| t.find_first_child(ws, name).ok().flatten())
        .filter(|m| t.class(*m) == Ok(ClassKind::Model))
        .collect();
    let path = |id: InstanceId| {
        let mut names = Vec::new();
        let mut at = Some(id);
        while let Some(i) = at {
            names.push(t.name(i).unwrap_or("?").to_owned());
            at = t.parent(i).ok().flatten();
        }
        names.reverse();
        names.join(".")
    };
    let mut out = Vec::new();
    for class in [ClassKind::Workspace, ClassKind::ReplicatedStorage, ClassKind::Lighting] {
        let Some(service) = t.service(class) else { continue };
        for id in t.descendants(service).unwrap_or_default() {
            let Ok(kind) = t.class(id) else { continue };
            if matches!(kind, ClassKind::Camera | ClassKind::Terrain)
                || characters.iter().any(|c| *c == id || t.is_descendant_of(id, *c).unwrap_or(false))
            {
                continue;
            }
            let pos = t.get_vec3(id, Property::Position).map(|p| format!("{:.1},{:.1},{:.1}", p.x, p.y, p.z));
            let colour = t.get_color(id, Property::Color).map(|c| format!("{:.2}", c.r));
            let attributes = t.attributes(id).map(|a| format!("{a:?}")).unwrap_or_default();
            out.push(format!("{} {} {pos:?} {colour:?} {attributes}", path(id), kind.file_name()));
        }
    }
    out.sort();
    out
}

/// Steps everyone, `guests` with no input, for `seconds` of wall time.
fn run(host: &mut PlaySession, guests: &mut [PlaySession], seconds: f32) {
    let until = Instant::now() + Duration::from_secs_f32(seconds);
    while Instant::now() < until {
        tick(host, guests);
    }
}

fn tick(host: &mut PlaySession, guests: &mut [PlaySession]) {
    host.step(DT, CharacterInput::default());
    for guest in guests.iter_mut() {
        guest.step(DT, CharacterInput::default());
    }
    std::thread::sleep(Duration::from_millis(2));
}

/// Steps everyone until `done`, or panics after `seconds`.
fn wait_for(
    host: &mut PlaySession,
    guests: &mut [PlaySession],
    seconds: f32,
    what: &str,
    mut done: impl FnMut(&PlaySession, &[PlaySession]) -> bool,
) {
    let until = Instant::now() + Duration::from_secs_f32(seconds);
    while Instant::now() < until {
        tick(host, guests);
        if done(host, guests) {
            return;
        }
    }
    panic!("gave up waiting for {what}");
}

/// Steps until every guest's picture of the world is the host's, or
/// panics with the first differences.
fn converge(host: &mut PlaySession, guests: &mut [PlaySession], seconds: f32) {
    let until = Instant::now() + Duration::from_secs_f32(seconds);
    loop {
        tick(host, guests);
        let names: Vec<String> = {
            let t = host.runtime().tree().borrow();
            host.players().iter().filter_map(|p| t.name(*p).ok().map(str::to_owned)).collect()
        };
        let want = picture(host, &names);
        let pictures: Vec<Vec<String>> = guests.iter().map(|g| picture(g, &names)).collect();
        if pictures.iter().all(|p| *p == want) {
            return;
        }
        if Instant::now() > until {
            for (i, got) in pictures.iter().enumerate() {
                let missing: Vec<&String> = want.iter().filter(|w| !got.contains(w)).take(8).collect();
                let extra: Vec<&String> = got.iter().filter(|g| !want.contains(g)).take(8).collect();
                if !missing.is_empty() || !extra.is_empty() {
                    panic!(
                        "guest {i} never caught up with the host ({} vs {} instances)\nmissing: {missing:#?}\nextra: {extra:#?}",
                        got.len(),
                        want.len()
                    );
                }
            }
            panic!("never caught up");
        }
    }
}

fn joined(guests: &[PlaySession]) -> bool {
    guests.iter().all(|g| g.character().is_some())
}

fn said(session: &mut PlaySession, log: &mut Vec<String>) {
    log.extend(session.take_log().into_iter().map(|l| l.text));
}

// ---------------------------------------------------------------------------

#[test]
fn a_busy_world_arrives_whole_over_a_stormy_connection() {
    let mut host = host_game();
    let proxy = Proxy::start(host_addr(&host), Weather::STORMY, 1);
    churn(&host, true);
    run(&mut host, &mut [], 0.5);
    let mut guests = vec![join(&proxy.address(), "Ana")];
    wait_for(&mut host, &mut guests, 30.0, "Ana to join", |_, g| joined(g));
    // Keep changing things for a while, then stop: Ana's copy must catch up exactly.
    run(&mut host, &mut guests, 4.0);
    churn(&host, false);
    converge(&mut host, &mut guests, 20.0);
}

#[test]
fn a_busy_streamed_world_arrives_whole_over_a_stormy_connection() {
    let mut host = host_streamed_game();
    let proxy = Proxy::start(host_addr(&host), Weather::STORMY, 2);
    churn(&host, true);
    run(&mut host, &mut [], 0.5);
    let mut guests = vec![join(&proxy.address(), "Ana")];
    wait_for(&mut host, &mut guests, 30.0, "Ana to join", |_, g| joined(g));
    run(&mut host, &mut guests, 4.0);
    churn(&host, false);
    converge(&mut host, &mut guests, 20.0);
}

#[test]
fn players_who_join_one_after_another_while_the_world_changes_all_agree() {
    let mut host = host_game();
    churn(&host, true);
    let proxies: Vec<Proxy> = (0..3)
        .map(|i| Proxy::start(host_addr(&host), if i == 1 { Weather::AWFUL } else { Weather::STORMY }, i))
        .collect();
    let mut guests = Vec::new();
    // Each joins a few steps after the last, while the first is still
    // being sent the world.
    for (i, proxy) in proxies.iter().enumerate() {
        guests.push(join(&proxy.address(), &format!("G{i}")));
        for _ in 0..3 {
            tick(&mut host, &mut guests);
        }
    }
    wait_for(&mut host, &mut guests, 40.0, "everyone to join", |_, g| joined(g));
    run(&mut host, &mut guests, 3.0);
    churn(&host, false);
    converge(&mut host, &mut guests, 30.0);
}

#[test]
fn two_players_joining_in_the_same_step_as_the_first_get_the_same_world() {
    let mut host = host_game();
    churn(&host, true);
    run(&mut host, &mut [], 0.3);
    let address = host_addr(&host).to_string();
    let mut guests = vec![join(&address, "A")];
    // The host is still writing out the world for A when B and C come.
    tick(&mut host, &mut guests);
    guests.push(join(&address, "B"));
    tick(&mut host, &mut guests);
    guests.push(join(&address, "C"));
    wait_for(&mut host, &mut guests, 30.0, "everyone to join", |_, g| joined(g));
    run(&mut host, &mut guests, 1.0);
    churn(&host, false);
    converge(&mut host, &mut guests, 20.0);
}

#[test]
fn a_lag_spike_does_not_throw_a_walking_player_back() {
    let mut host = host_game();
    let proxy = Proxy::start(host_addr(&host), Weather::STORMY, 7);
    let mut guests = vec![join(&proxy.address(), "Walker")];
    wait_for(&mut host, &mut guests, 30.0, "the walker to join", |_, g| joined(g));
    run(&mut host, &mut guests, 1.0);
    let walk = CharacterInput { move_dir: Vec3::new(1.0, 0.0, 0.0), ..Default::default() };
    let root = |s: &PlaySession| {
        let c = s.character().unwrap();
        s.runtime().tree().borrow().get_vec3(c.root, Property::Position).unwrap()
    };
    let start = root(&guests[0]);
    let mut log = Vec::new();
    let mut spiked = false;
    let began = Instant::now();
    let mut furthest = start.x;
    while began.elapsed() < Duration::from_secs(5) {
        if !spiked && began.elapsed() > Duration::from_secs(1) {
            // A second and a half of nothing either way, then it all arrives at once.
            proxy.hold(Duration::from_millis(1500));
            spiked = true;
        }
        host.step(DT, CharacterInput::default());
        guests[0].step(DT, walk);
        said(&mut guests[0], &mut log);
        let x = root(&guests[0]).x;
        // At the real pace, as a game runs: the host's limits are in real seconds.
        std::thread::sleep(Duration::from_millis(14));
        assert!(x > furthest - 1.0, "the walker was thrown back from {furthest} to {x}");
        furthest = furthest.max(x);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(furthest > start.x + 30.0, "walked only {}", furthest - start.x);
    // The host ends up where the walker is.
    wait_for(&mut host, &mut guests, 10.0, "the host to catch up", |h, g| {
        let theirs = {
            let t = h.runtime().tree().borrow();
            let model = t.get_ref(h.players()[1], Property::Character).unwrap();
            let r = t.find_first_child(model, "HumanoidRootPart").unwrap().unwrap();
            t.get_vec3(r, Property::Position).unwrap()
        };
        (theirs - root(&g[0])).length() < 2.0
    });
    assert!(!guests[0].disconnected(), "{log:?}");
}

#[test]
fn a_player_whose_network_goes_silent_is_let_go() {
    let mut host = host_game();
    host.set_silence_limit(Duration::from_secs(3));
    let proxy = Proxy::start(host_addr(&host), Weather::CLEAR, 3);
    let mut guests = vec![join(&proxy.address(), "Ghost")];
    guests[0].set_silence_limit(Duration::from_secs(3));
    wait_for(&mut host, &mut guests, 20.0, "the ghost to join", |_, g| joined(g));
    // A quiet game: nothing changes, and the player stands still, so
    // only the keep-alives say anyone is there.
    run(&mut host, &mut guests, 5.0);
    assert_eq!(host.joined_count(), 1, "a quiet player was let go");
    assert!(!guests[0].disconnected(), "a quiet host was given up on");
    proxy.vanish();
    let mut host_log = Vec::new();
    let mut guest_log = Vec::new();
    wait_for(&mut host, &mut guests, 15.0, "both sides to notice", |h, g| {
        h.joined_count() == 0 && g[0].disconnected()
    });
    said(&mut host, &mut host_log);
    said(&mut guests[0], &mut guest_log);
    assert!(host_log.iter().any(|l| l.contains("Ghost left")), "{host_log:?}");
    assert!(guest_log.iter().any(|l| l.contains("stopped answering")), "{guest_log:?}");
    assert_eq!(host.players().len(), 1);
}

#[test]
fn a_host_that_closes_the_game_says_so() {
    let mut host = host_game();
    let mut guests = vec![join(&host_addr(&host).to_string(), "Stayer")];
    wait_for(&mut host, &mut guests, 20.0, "the stayer to join", |_, g| joined(g));
    // Closing the game (as Stop does) tells everyone still there.
    host.stop();
    drop(host);
    let mut log = Vec::new();
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until && !guests[0].disconnected() {
        guests[0].step(DT, CharacterInput::default());
        said(&mut guests[0], &mut log);
        std::thread::sleep(Duration::from_millis(2));
    }
    said(&mut guests[0], &mut log);
    assert!(guests[0].disconnected());
    assert!(log.iter().any(|l| l.contains("The host ended the game")), "{log:?}");
    assert!(!guests[0].can_reconnect(), "nothing to reconnect to when the host ended it");
}

#[test]
fn a_call_to_the_server_fails_instead_of_waiting_forever_when_the_connection_drops() {
    let mut host = host_game();
    let proxy = Proxy::start(host_addr(&host), Weather::CLEAR, 5);
    let mut guests = vec![join(&proxy.address(), "Asker")];
    wait_for(&mut host, &mut guests, 20.0, "the asker to join", |_, g| joined(g));
    // An answer comes back while all is well.
    guests[0].runtime().run("ask", "print('answer', game.ReplicatedStorage.Ask:InvokeServer(21))").unwrap();
    let mut log = Vec::new();
    let until = Instant::now() + Duration::from_secs(10);
    while !log.iter().any(|l: &String| l.contains("answer") && l.contains("42")) {
        assert!(Instant::now() < until, "no answer: {log:?}");
        tick(&mut host, &mut guests);
        said(&mut guests[0], &mut log);
    }
    // A call made just as the network goes away fails once the
    // connection counts as lost, and the script goes on.
    proxy.cut();
    guests[0]
        .runtime()
        .run(
            "ask again",
            "local ok, why = pcall(function() return game.ReplicatedStorage.Ask:InvokeServer(1) end) print('after', ok, why)",
        )
        .unwrap();
    // A remote this computer made itself was never on the server.
    guests[0]
        .runtime()
        .run(
            "ask nobody",
            "local f = Instance.new('RemoteFunction') f.Parent = workspace local ok = pcall(function() return f:InvokeServer() end) print('nobody', ok)",
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let mut lines = Vec::new();
    while Instant::now() < until {
        guests[0].step(DT, CharacterInput::default());
        lines.extend(guests[0].take_log().into_iter().map(|l| l.text));
        if lines.iter().any(|l| l.starts_with("after")) && lines.iter().any(|l| l.starts_with("nobody")) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let after = lines.iter().find(|l| l.starts_with("after")).unwrap_or_else(|| panic!("{lines:?}"));
    assert!(after.contains("false") && after.contains("connection"), "{after}");
    let nobody = lines.iter().find(|l| l.starts_with("nobody")).unwrap_or_else(|| panic!("{lines:?}"));
    assert!(nobody.contains("false"), "{nobody}");
}

#[test]
fn a_player_who_drops_out_can_come_back_and_sees_the_world_as_it_is_now() {
    let mut host = host_game();
    churn(&host, true);
    let proxy = Proxy::start(host_addr(&host), Weather::STORMY, 9);
    let mut guests = vec![join(&proxy.address(), "Bounce")];
    wait_for(&mut host, &mut guests, 30.0, "Bounce to join", |_, g| joined(g));
    run(&mut host, &mut guests, 1.0);
    proxy.cut();
    wait_for(&mut host, &mut guests, 10.0, "the drop to be noticed", |h, g| {
        g[0].disconnected() && h.joined_count() == 0
    });
    assert!(guests[0].can_reconnect());
    // The world goes on changing while they're away.
    run(&mut host, &mut guests, 1.0);
    proxy.heal();
    guests[0].reconnect().unwrap();
    assert!(!guests[0].disconnected());
    wait_for(&mut host, &mut guests, 30.0, "Bounce to be back", |h, g| joined(g) && h.joined_count() == 1);
    let names: Vec<String> = {
        let t = host.runtime().tree().borrow();
        host.players().iter().map(|p| t.name(*p).unwrap().to_owned()).collect()
    };
    assert_eq!(names, ["Player1", "Bounce"]);
    run(&mut host, &mut guests, 1.0);
    churn(&host, false);
    converge(&mut host, &mut guests, 20.0);
}

#[test]
fn sixteen_players_join_at_once_and_half_drop_out() {
    let mut host = host_game();
    churn(&host, true);
    let proxies: Vec<Proxy> = (0..16)
        .map(|i| {
            Proxy::start(host_addr(&host), if i % 2 == 0 { Weather::STORMY } else { Weather::CLEAR }, 100 + i)
        })
        .collect();
    let mut guests: Vec<PlaySession> =
        proxies.iter().enumerate().map(|(i, p)| join(&p.address(), &format!("Crowd{i}"))).collect();
    wait_for(&mut host, &mut guests, 60.0, "the crowd to join", |_, g| joined(g));
    assert_eq!(host.joined_count(), 16);
    run(&mut host, &mut guests, 2.0);
    // Half lose their connection at once, some politely, some not.
    for (i, proxy) in proxies.iter().enumerate() {
        if i % 4 == 0 {
            proxy.cut();
        }
    }
    let leaving: Vec<usize> = (0..16).filter(|i| i % 4 == 2).collect();
    for i in leaving.iter().rev() {
        guests.remove(*i);
    }
    let mut kept = Vec::new();
    for (i, g) in guests.into_iter().enumerate() {
        let original = (0..16).filter(|j| j % 4 != 2).nth(i).unwrap();
        if original % 4 != 0 {
            kept.push(g);
        }
    }
    let mut guests = kept;
    wait_for(&mut host, &mut guests, 15.0, "the leavers to go", |h, _| h.joined_count() == 8);
    assert_eq!(host.players().len(), 9);
    run(&mut host, &mut guests, 1.0);
    churn(&host, false);
    converge(&mut host, &mut guests, 30.0);
}

#[test]
fn a_long_game_of_things_made_and_destroyed_does_not_keep_growing() {
    // Bullets: a part made and destroyed every step, for a long time.
    let mut tree = game();
    let sss = tree.service(ClassKind::ServerScriptService).unwrap();
    code(
        &mut tree,
        ClassKind::Script,
        sss,
        "Bullets",
        r#"
        game:GetService("RunService").Heartbeat:Connect(function()
            for i = 1, 10 do
                local b = Instance.new("Part")
                b.Name = "Bullet"
                b.Anchored = true
                b.Position = Vector3.new(i, 5, 0)
                b.Parent = workspace
                local e = Instance.new("Explosion")
                e.Position = b.Position
                e.BlastPressure = 0
                e.Parent = workspace
                task.delay(0.05, function() b:Destroy() e:Destroy() end)
            end
        end)
        "#,
    );
    let mut host = PlaySession::host(Rc::new(RefCell::new(tree)), 0, None, true).unwrap();
    let mut guests = vec![join(&host_addr(&host).to_string(), "Long")];
    wait_for(&mut host, &mut guests, 20.0, "Long to join", |_, g| joined(g));
    let sizes = |h: &PlaySession, g: &PlaySession| {
        (
            h.runtime().tree().borrow().len(),
            g.runtime().tree().borrow().len(),
            h.bookkeeping_len(),
            g.bookkeeping_len(),
        )
    };
    // The most each holds over a few seconds, since destroyed things are
    // freed every few seconds rather than straight away.
    let most = |host: &mut PlaySession, guests: &mut [PlaySession], seconds: f32| {
        let until = Instant::now() + Duration::from_secs_f32(seconds);
        let mut most = (0, 0, 0, 0);
        while Instant::now() < until {
            tick(host, guests);
            let s = sizes(host, &guests[0]);
            most = (most.0.max(s.0), most.1.max(s.1), most.2.max(s.2), most.3.max(s.3));
        }
        most
    };
    run(&mut host, &mut guests, 3.0);
    let early = most(&mut host, &mut guests, 5.0);
    run(&mut host, &mut guests, 10.0);
    let late = most(&mut host, &mut guests, 5.0);
    // Over the game, some 40,000 parts and explosions came and went.
    assert!(late.0 < early.0 + 2000, "the host's world keeps growing: {early:?} -> {late:?}");
    assert!(late.1 < early.1 + 2000, "the joined world keeps growing: {early:?} -> {late:?}");
    assert!(late.2 < early.2 + 2000, "the host's records keep growing: {early:?} -> {late:?}");
    assert!(late.3 < early.3 + 2000, "the joined records keep growing: {early:?} -> {late:?}");
}

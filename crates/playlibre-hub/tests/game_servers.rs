//! Play: the Hub finds one of its own game servers with room, or starts one.
//!
//! The "game server" here is a stand-in script that only notes how it was
//! started; the test announces it to the Hub the way `playlibre server` does.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use playlibre_hub::{
    DownloadProof, FleetOptions, Heartbeat, HostKind, HubClient, HubOptions, PlayRequest, Publish,
    host_message, new_secret, player_id_of, publish_message, sign, start_with,
};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("playlibre-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A program that writes its arguments to `calls` and then idles.
fn stand_in(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let program = dir.join("fake-playlibre");
    std::fs::write(
        &program,
        format!("#!/bin/sh\n(printf '%s\\t' \"$@\"; echo) >> {}/calls\nexec sleep 20\n", dir.display()),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    program
}

/// Each start so far: its arguments.
fn calls(dir: &Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(dir.join("calls"))
        .unwrap_or_default()
        .lines()
        .map(|l| l.split('\t').filter(|a| !a.is_empty()).map(str::to_owned).collect())
        .collect()
}

fn flag<'a>(call: &'a [String], name: &str) -> &'a str {
    let at = call.iter().position(|a| a == name).unwrap_or_else(|| panic!("no {name} in {call:?}"));
    &call[at + 1]
}

/// Announces the server a call started, as `playlibre server` would.
fn announce(hub: &HubClient, call: &[String], players: u32) -> String {
    let key: [u8; 32] = std::fs::read(flag(call, "--key")).unwrap().try_into().unwrap();
    let id = player_id_of(&key);
    let code = playlibre_hub::code_for(&id);
    let secret = format!("{id}-secret-for-the-test");
    hub.heartbeat(&Heartbeat {
        server_id: id.clone(),
        secret: secret.clone(),
        name: "Sample obby".into(),
        game: "Sample obby".into(),
        code: code.clone(),
        players,
        max_players: flag(call, "--max-players").parse().unwrap(),
        kind: HostKind::Official,
        game_id: Some(flag(call, "--game").into()),
        host_id: Some(id.clone()),
        host_signature: Some(sign(&key, &host_message(&id, &secret))),
    })
    .unwrap();
    code
}

fn wait_for_calls(dir: &Path, count: usize) -> Vec<Vec<String>> {
    for _ in 0..100 {
        let found = calls(dir);
        if found.len() >= count {
            return found;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("expected {count} servers started, saw {:?}", calls(dir));
}

#[test]
fn play_fills_the_hubs_servers_and_starts_more() {
    let dir = temp_dir("fleet");
    let fleet = FleetOptions {
        max_servers: 2,
        players_per_server: 2,
        ..FleetOptions::new(stand_in(&dir), dir.join("servers"))
    };
    let options = HubOptions { no_limits: true, fleet: Some(fleet), ..Default::default() };
    let running = start_with("127.0.0.1:0".parse().unwrap(), None, options).unwrap();
    let hub = HubClient::new(&running.url());
    let play = |ticket: &str| {
        hub.play(&PlayRequest {
            game_id: "starter:Sample obby".into(),
            ticket: ticket.into(),
            ..Default::default()
        })
        .unwrap()
        .unwrap()
    };

    // Nobody playing yet: a server starts, and the player waits for it.
    let first = play("ticketaaaa");
    assert!(first.wait && first.code.is_none());
    let started = wait_for_calls(&dir, 1);
    let call = &started[0];
    assert_eq!(call[0], "server");
    assert_eq!(flag(call, "--game"), "starter:Sample obby");
    assert_eq!(flag(call, "--max-players"), "2");
    assert_eq!(flag(call, "--udp-port"), "7500");
    assert!(call.contains(&"--official".to_owned()));
    // Asking again while it starts: still waiting, and no second server.
    assert!(play("ticketaaaa").wait);
    assert_eq!(calls(&dir).len(), 1);

    // It comes up: the waiting player gets its code.
    let first = announce(&hub, call, 0);
    let joined = play("ticketaaaa");
    assert_eq!(joined.code.as_deref(), Some(first.as_str()));
    let listed = hub.servers("").unwrap();
    assert!(listed[0].hub_server, "{listed:?}");
    assert_eq!(listed[0].kind, HostKind::Official);

    // A second player fills it, so the next server starts before anyone needs it.
    assert_eq!(play("ticketbbbb").code.as_deref(), Some(first.as_str()));
    let started = wait_for_calls(&dir, 2);
    assert_eq!(flag(&started[1], "--udp-port"), "7501");

    // A third goes to the new one, and waits for it.
    assert!(play("ticketcccc").wait);
    let second = announce(&hub, &started[1], 0);
    assert_eq!(play("ticketcccc").code.as_deref(), Some(second.as_str()));

    // Both full, and no room for a third server: the Hub says it's busy.
    assert_eq!(play("ticketdddd").code.as_deref(), Some(second.as_str()));
    let busy = hub.play(&PlayRequest {
        game_id: "starter:Sample obby".into(),
        ticket: "ticketeeee".into(),
        ..Default::default()
    });
    assert!(busy.unwrap_err().contains("busy"));

    // Games the Hub doesn't know, and nonsense tickets, are turned away.
    let unknown = hub.play(&PlayRequest {
        game_id: "no-such-game".into(),
        ticket: "ticketffff".into(),
        ..Default::default()
    });
    assert!(unknown.is_err());
    let bad = hub.play(&PlayRequest {
        game_id: "starter:Sample obby".into(),
        ticket: "x".into(),
        ..Default::default()
    });
    assert!(bad.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_hub_without_game_servers_says_so() {
    let running = playlibre_hub::start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let answer = hub.play(&PlayRequest {
        game_id: "starter:Sample obby".into(),
        ticket: new_secret(),
        ..Default::default()
    });
    assert_eq!(answer, Ok(None));
}

#[test]
fn a_server_that_fails_to_start_is_not_retried_at_once() {
    let dir = temp_dir("fleet-broken");
    let fleet = FleetOptions::new(dir.join("not-there"), dir.join("servers"));
    let options = HubOptions { no_limits: true, fleet: Some(fleet), ..Default::default() };
    let running = start_with("127.0.0.1:0".parse().unwrap(), None, options).unwrap();
    let hub = HubClient::new(&running.url());
    for ticket in ["ticketaaaa", "ticketbbbb"] {
        let answer = hub.play(&PlayRequest {
            game_id: "starter:Sample obby".into(),
            ticket: ticket.into(),
            ..Default::default()
        });
        assert!(answer.unwrap_err().contains("couldn't start"));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn place(seed: &str) -> String {
    let scripts: Vec<String> = (0..6)
        .map(|n| {
            format!(
                r#"{{"class":"script","name":"S{n}","properties":{{"source":"local {seed}{n} = {n}\n{}"}}}}"#,
                "print(value) ".repeat(30)
            )
        })
        .collect();
    format!(r#"{{"format":"libroxel-place","version":1,"children":[{}]}}"#, scripts.join(","))
}

fn publish(hub: &HubClient, key: &[u8; 32], place: String) -> Result<String, String> {
    let secret = new_secret();
    hub.publish(&Publish {
        game_id: None,
        author_signature: Some(sign(key, &publish_message(&secret))),
        secret,
        title: "Closed Tower".into(),
        author: "Alice".into(),
        description: String::new(),
        place,
        author_id: Some(player_id_of(key)),
    })
    .map(|p| p.game_id)
}

fn proof(key: &[u8; 32], game_id: &str) -> DownloadProof {
    DownloadProof::new(game_id, player_id_of(key), |m| sign(key, m))
}

#[test]
fn only_the_creator_and_the_hubs_servers_get_a_games_place() {
    let dir = temp_dir("fleet-closed");
    let options = HubOptions {
        no_limits: true,
        fleet: Some(FleetOptions::new(stand_in(&dir), dir.join("servers"))),
        ..Default::default()
    };
    let running = start_with("127.0.0.1:0".parse().unwrap(), None, options).unwrap();
    let hub = HubClient::new(&running.url());
    let (alice, bob) = ([1u8; 32], [2u8; 32]);
    let id = publish(&hub, &alice, place("a")).unwrap();
    assert!(hub.game(&id).unwrap().closed);

    assert!(hub.download(&id, None).unwrap_err().contains("only its creator"));
    assert!(hub.download(&id, Some(&proof(&bob, &id))).is_err());
    assert_eq!(hub.download(&id, Some(&proof(&alice, &id))).unwrap(), place("a"));
    let mut stale = proof(&alice, &id);
    stale.time -= 3600;
    stale.signature = sign(&alice, &playlibre_hub::download_message(&id, &stale.player_id, stale.time));
    assert!(hub.download(&id, Some(&stale)).is_err());

    hub.play(&PlayRequest { game_id: id.clone(), ticket: "ticketaaaa".into(), ..Default::default() })
        .unwrap();
    let call = &wait_for_calls(&dir, 1)[0];
    let server: [u8; 32] = std::fs::read(flag(call, "--key")).unwrap().try_into().unwrap();
    assert_eq!(hub.download(&id, Some(&proof(&server, &id))).unwrap(), place("a"));

    assert!(publish(&hub, &bob, place("a")).unwrap_err().contains("copied"));
    assert!(publish(&hub, &bob, place("b")).is_ok());
    assert!(publish(&hub, &alice, place("a")).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

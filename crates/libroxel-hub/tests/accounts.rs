//! Accounts no one else can take: passwords to sign in on another computer,
//! names kept back or held, signed hosts and rate limits.

use libroxel_hub::{
    AccountUpdate, Announcer, HostKind, HostSigner, HubClient, Listing, account_message, player_id_of,
    seal_password, seal_password_with, sign, start,
};
use std::sync::Arc;

fn time() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

fn claim(hub: &HubClient, key: &[u8; 32], name: &str, time: i64) -> Result<(), String> {
    let id = player_id_of(key);
    let look = serde_json::json!({});
    let message = account_message(&id, name, "", &look, time);
    hub.update_account(&AccountUpdate {
        player_id: id,
        name: name.into(),
        about: String::new(),
        look,
        time,
        signature: sign(key, &message),
    })
}

#[test]
fn a_password_signs_in_on_another_computer_and_nobody_else_gets_in() {
    let dir = std::env::temp_dir().join(format!("libroxel-accounts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let db = dir.join("hub.sqlite");
    let running = start("127.0.0.1:0".parse().unwrap(), Some(&db)).unwrap();
    let hub = HubClient::new(&running.url());
    let kai = [7u8; 32];
    let now = time();

    // A password needs an account first.
    let early = seal_password(&kai, "correct horse battery", now).unwrap();
    assert!(hub.set_password(&early).unwrap_err().contains("claim a name"));
    claim(&hub, &kai, "kai2", now).unwrap();

    // Someone else can't set a password on Kai's account: it's signed with Kai's key.
    let mut forged = seal_password(&[8u8; 32], "mallory's password", now).unwrap();
    forged.player_id = player_id_of(&kai);
    assert!(hub.set_password(&forged).unwrap_err().contains("signature"));

    let set = seal_password(&kai, "correct horse battery", now + 1).unwrap();
    // The password goes nowhere near the Hub.
    assert!(!serde_json::to_string(&set).unwrap().contains("correct horse"));
    hub.set_password(&set).unwrap();
    // An old signed change can't be sent again to roll it back.
    assert!(hub.set_password(&early).is_err());

    // On a new computer: the name and password give back Kai's own key, whatever the case.
    assert_eq!(hub.sign_in("KAI2", "correct horse battery").unwrap(), kai);
    assert!(hub.sign_in("kai2", "wrong horse battery").unwrap_err().contains("wrong name or password"));
    assert!(hub.sign_in("nobody_here", "correct horse battery").is_err());

    // Accounts and passwords outlast the Hub.
    drop(running);
    let running = start("127.0.0.1:0".parse().unwrap(), Some(&db)).unwrap();
    let hub = HubClient::new(&running.url());
    assert_eq!(hub.profile("kai2").unwrap().unwrap().player_id, player_id_of(&kai));
    assert_eq!(hub.sign_in("kai2", "correct horse battery").unwrap(), kai);

    // Guessing gets locked out after 10 wrong tries, even with the right one after.
    for _ in 0..10 {
        let _ = hub.sign_in("kai2", "guess guess guess");
    }
    assert!(hub.sign_in("kai2", "correct horse battery").unwrap_err().contains("too many wrong passwords"));
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_hub_cannot_ask_for_weakly_stretched_passwords() {
    // The floor is the weakest a client signs in with; below it is refused at setting time.
    let set = seal_password_with(&[9u8; 32], "a fine long password", time(), 8 * 1024, 1).unwrap();
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    claim(&hub, &[9u8; 32], "Sam", time()).unwrap();
    assert!(hub.set_password(&set).unwrap_err().contains("not safe"));
}

#[test]
fn official_looking_names_are_kept_and_given_up_names_are_held() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let (ana, eve) = ([1u8; 32], [2u8; 32]);
    let now = time();
    for name in ["Admin", "moderator_1", "LibroxelTeam", "official"] {
        assert!(claim(&hub, &eve, name, now).unwrap_err().contains("kept"), "{name}");
    }
    claim(&hub, &ana, "Ana", now).unwrap();
    assert!(claim(&hub, &eve, "ANA", now).unwrap_err().contains("already has"));
    // Ana renames; her old name stays hers for a while, so no one can pass for her.
    claim(&hub, &ana, "AnaB", now + 1).unwrap();
    assert!(claim(&hub, &eve, "ana", now + 2).unwrap_err().contains("30 days"));
    claim(&hub, &ana, "Ana", now + 3).unwrap();
    assert_eq!(hub.profile("ana").unwrap().unwrap().player_id, player_id_of(&ana));
}

#[test]
fn one_computer_can_only_make_a_few_accounts_an_hour() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    for i in 0..5u8 {
        claim(&hub, &[10 + i; 32], &format!("squat{i}"), time()).unwrap();
    }
    assert!(claim(&hub, &[20u8; 32], "squat5", time()).unwrap_err().contains("too many new accounts"));
    // Changing an account you already have still works.
    claim(&hub, &[10u8; 32], "squat0", time() + 1).unwrap();
}

#[test]
fn the_server_list_shows_who_really_hosts() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let kai = [7u8; 32];
    claim(&hub, &kai, "kai2", time()).unwrap();
    let listing = |name: &str, code: &str| Listing {
        name: name.into(),
        game: "Obby".into(),
        code: Some(code.into()),
        players: 1,
        kind: HostKind::Player,
        ..Listing::default()
    };
    let signer = HostSigner { player_id: player_id_of(&kai), sign: Arc::new(move |m: &[u8]| sign(&kai, m)) };
    let _real =
        Announcer::start_signed(hub.clone(), "real".into(), listing("kai2's game", "lrx1a"), Some(signer));
    // Someone names their server after Kai, and even says it's Kai's, signed with their own key.
    let eve = [3u8; 32];
    let fake = HostSigner { player_id: player_id_of(&kai), sign: Arc::new(move |m: &[u8]| sign(&eve, m)) };
    let forged =
        Announcer::start_signed(hub.clone(), "fake".into(), listing("kai2's game", "lrx1b"), Some(fake));
    let _unsigned = Announcer::start(hub.clone(), "plain".into(), listing("kai2's game", "lrx1c"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let servers = loop {
        let servers = hub.servers("").unwrap();
        if servers.len() >= 2 && forged.status().is_some() || std::time::Instant::now() > deadline {
            break servers;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert!(!forged.status().unwrap().starts_with("Listed"));
    assert_eq!(servers.len(), 2);
    let host = |id: &str| servers.iter().find(|s| s.server_id == id).unwrap().host.clone();
    assert_eq!(host("real").as_deref(), Some("kai2"));
    assert_eq!(host("plain"), None);
}

#[test]
fn whoever_runs_the_hub_bans_accounts_and_names_its_owners() {
    use libroxel_hub::{Heartbeat, add_owner, ban, open_db, unban};
    let dir = std::env::temp_dir().join(format!("libroxel-owners-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("hub.sqlite");
    let running = start("127.0.0.1:0".parse().unwrap(), Some(&path)).unwrap();
    let hub = HubClient::new(&running.url());
    let (kai, eve) = ([7u8; 32], [3u8; 32]);
    let now = time();
    claim(&hub, &kai, "kai2", now).unwrap();
    claim(&hub, &eve, "eve", now).unwrap();
    let db = open_db(Some(&path)).unwrap();
    add_owner(&db, "kai2").unwrap();
    // Owners can't be banned by mistake.
    assert!(ban(&db, "kai2", "oops").is_err());

    // Official servers only from owners; anyone else's are listed as Community.
    let official = |key: &[u8; 32], id: &str| {
        let secret = libroxel_hub::new_secret();
        Heartbeat {
            server_id: id.into(),
            name: id.into(),
            game: "Obby".into(),
            code: format!("lrx1{id}"),
            players: 0,
            max_players: 0,
            kind: HostKind::Official,
            game_id: None,
            host_id: Some(player_id_of(key)),
            host_signature: Some(sign(key, &libroxel_hub::host_message(id, &secret))),
            secret,
        }
    };
    hub.heartbeat(&official(&kai, "mine")).unwrap();
    hub.heartbeat(&official(&eve, "hers")).unwrap();
    let kind = |id: &str| hub.servers("").unwrap().into_iter().find(|s| s.server_id == id).map(|s| s.kind);
    assert_eq!(kind("mine"), Some(HostKind::Official));
    assert_eq!(kind("hers"), Some(HostKind::Community));

    ban(&db, "eve", "spam").unwrap();
    assert!(claim(&hub, &eve, "eve", now + 1).unwrap_err().contains("banned"));
    assert!(hub.profile("eve").unwrap().is_none());
    assert!(hub.people("eve").unwrap().is_empty());
    assert_eq!(kind("hers"), None);
    assert!(hub.heartbeat(&official(&eve, "again")).is_err());
    // The name stays taken while she's banned.
    assert!(claim(&hub, &[4u8; 32], "eve", now).unwrap_err().contains("already has"));
    unban(&db, "eve").unwrap();
    claim(&hub, &eve, "eve", now + 2).unwrap();
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

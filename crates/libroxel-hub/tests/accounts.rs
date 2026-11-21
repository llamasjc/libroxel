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
        work: Some(libroxel_hub::account_work(&id, name)),
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
    let alice = [7u8; 32];
    let now = time();

    // A password needs an account first.
    let early = seal_password(&alice, "correct horse battery", now).unwrap();
    assert!(hub.set_password(&early).unwrap_err().contains("claim a name"));
    claim(&hub, &alice, "alice2", now).unwrap();

    // Someone else can't set a password on Alice's account: it's signed with Alice's key.
    let mut forged = seal_password(&[8u8; 32], "mallory's password", now).unwrap();
    forged.player_id = player_id_of(&alice);
    assert!(hub.set_password(&forged).unwrap_err().contains("signature"));

    let set = seal_password(&alice, "correct horse battery", now + 1).unwrap();
    // The password goes nowhere near the Hub.
    assert!(!serde_json::to_string(&set).unwrap().contains("correct horse"));
    hub.set_password(&set).unwrap();
    // An old signed change can't be sent again to roll it back.
    assert!(hub.set_password(&early).is_err());

    // On a new computer: the name and password give back Alice's own key, whatever the case.
    assert_eq!(hub.sign_in("ALICE2", "correct horse battery", None).unwrap(), alice);
    assert!(
        hub.sign_in("alice2", "wrong horse battery", None).unwrap_err().contains("wrong name or password")
    );
    assert!(hub.sign_in("nobody_here", "correct horse battery", None).is_err());

    // Accounts and passwords outlast the Hub.
    drop(running);
    let running = start("127.0.0.1:0".parse().unwrap(), Some(&db)).unwrap();
    let hub = HubClient::new(&running.url());
    assert_eq!(hub.profile("alice2").unwrap().unwrap().player_id, player_id_of(&alice));
    assert_eq!(hub.sign_in("alice2", "correct horse battery", None).unwrap(), alice);

    // Guessing gets locked out after 10 wrong tries, even with the right one after.
    for _ in 0..10 {
        let _ = hub.sign_in("alice2", "guess guess guess", None);
    }
    assert!(
        hub.sign_in("alice2", "correct horse battery", None)
            .unwrap_err()
            .contains("too many wrong passwords")
    );
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
fn names_ignore_case_and_given_up_names_are_held() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let (ana, eve) = ([1u8; 32], [2u8; 32]);
    let now = time();
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
fn a_demo_hub_has_no_limits() {
    let options = libroxel_hub::HubOptions { no_limits: true, ..Default::default() };
    let running = libroxel_hub::start_with("127.0.0.1:0".parse().unwrap(), None, options).unwrap();
    let hub = HubClient::new(&running.url());
    for i in 0..12u8 {
        claim(&hub, &[40 + i; 32], &format!("pretend{i}"), time()).unwrap();
    }
}

#[test]
fn the_server_list_shows_who_really_hosts() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let alice = [7u8; 32];
    claim(&hub, &alice, "alice2", time()).unwrap();
    let listing = |name: &str, code: &str| Listing {
        name: name.into(),
        game: "Obby".into(),
        code: Some(code.into()),
        players: 1,
        kind: HostKind::Player,
        ..Listing::default()
    };
    let signer =
        HostSigner { player_id: player_id_of(&alice), sign: Arc::new(move |m: &[u8]| sign(&alice, m)) };
    let _real =
        Announcer::start_signed(hub.clone(), "real".into(), listing("alice2's game", "lrx1a"), Some(signer));
    // Someone names their server after Alice, and even says it's Alice's, signed with their own key.
    let eve = [3u8; 32];
    let fake = HostSigner { player_id: player_id_of(&alice), sign: Arc::new(move |m: &[u8]| sign(&eve, m)) };
    let forged =
        Announcer::start_signed(hub.clone(), "fake".into(), listing("alice2's game", "lrx1b"), Some(fake));
    let _unsigned = Announcer::start(hub.clone(), "plain".into(), listing("alice2's game", "lrx1c"));
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
    assert_eq!(host("real").as_deref(), Some("alice2"));
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
    let (alice, eve) = ([7u8; 32], [3u8; 32]);
    let now = time();
    claim(&hub, &alice, "alice2", now).unwrap();
    claim(&hub, &eve, "eve", now).unwrap();
    let db = open_db(Some(&path)).unwrap();
    // Any name is first come, first served, "admin" too, whatever the case.
    claim(&hub, &alice, "Admin", now + 1).unwrap();
    assert!(claim(&hub, &eve, "ADMIN", now + 1).unwrap_err().contains("already has"));
    // Owners are made on the Hub's computer, whatever their name.
    assert!(!hub.profile("admin").unwrap().unwrap().admin);
    add_owner(&db, "admin").unwrap();
    let page = hub.profile("admin").unwrap().unwrap();
    assert!(page.admin && page.player_id == player_id_of(&alice));
    assert!(!hub.profile("eve").unwrap().unwrap().admin);
    // Renaming from the Hub's computer.
    libroxel_hub::rename(&db, "admin", "alice2").unwrap();
    libroxel_hub::rename(&db, "alice2", "Admin").unwrap();
    assert!(libroxel_hub::rename(&db, "eve", "admin").is_err());
    // Owners can't be banned by mistake.
    assert!(ban(&db, "Admin", "oops").is_err());

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
    hub.heartbeat(&official(&alice, "mine")).unwrap();
    hub.heartbeat(&official(&eve, "hers")).unwrap();
    let kind = |id: &str| hub.servers("").unwrap().into_iter().find(|s| s.server_id == id).map(|s| s.kind);
    assert_eq!(kind("mine"), Some(HostKind::Official));
    let admin_host =
        |id: &str| hub.servers("").unwrap().into_iter().find(|s| s.server_id == id).unwrap().admin_host;
    assert!(admin_host("mine") && !admin_host("hers"));
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

#[test]
fn two_step_sign_in_needs_the_app_code_and_sign_ins_are_listed() {
    use libroxel_hub::{
        NEEDS_CODE, TwoStepSet, new_two_step_secret, security_message, two_step_code, two_step_message,
    };
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let alice = [7u8; 32];
    let id = player_id_of(&alice);
    claim(&hub, &alice, "alice2", time()).unwrap();
    hub.set_password(&seal_password_with(&alice, "correct horse battery", time(), 19 * 1024, 2).unwrap())
        .unwrap();

    // Turning it on needs a code from the app, to show the app has the secret.
    let secret = new_two_step_secret();
    let mut set = TwoStepSet {
        player_id: id.clone(),
        secret: Some(secret.clone()),
        code: "000000".into(),
        time: time(),
        signature: String::new(),
    };
    set.code =
        if two_step_code(&secret, time()).unwrap() == "000000" { "111111".into() } else { "000000".into() };
    set.signature = sign(&alice, &two_step_message(&set));
    assert!(hub.set_two_step(&set).unwrap_err().contains("does not match"));
    // Use the code from 30 seconds ago, so the sign-in below gets a newer one.
    set.code = two_step_code(&secret, time() - 30).unwrap();
    set.signature = sign(&alice, &two_step_message(&set));
    hub.set_two_step(&set).unwrap();

    // The password alone isn't enough now; a wrong code isn't either.
    assert_eq!(hub.sign_in("alice2", "correct horse battery", None).unwrap_err(), NEEDS_CODE);
    assert!(hub.sign_in("alice2", "correct horse battery", Some("12")).unwrap_err().contains("wrong code"));
    let code = two_step_code(&secret, time()).unwrap();
    assert_eq!(hub.sign_in("alice2", "correct horse battery", Some(&code)).unwrap(), alice);
    // A code works once.
    assert!(
        hub.sign_in("alice2", "correct horse battery", Some(&code)).unwrap_err().contains("already used")
    );

    // Only Alice sees how the account is kept safe, and the sign-in shows there.
    let now_ms = time() * 1000;
    let security = hub.security(&id, now_ms, &sign(&alice, &security_message(&id, now_ms))).unwrap();
    assert!(security.has_password && security.two_step);
    assert_eq!(security.sign_ins.len(), 1);
    assert_eq!(security.sign_ins[0].from, "127.0.0.1");
    assert!(hub.security(&id, now_ms, &sign(&[3u8; 32], &security_message(&id, now_ms))).is_err());
}

#[test]
fn new_accounts_need_the_bot_check_and_whoever_runs_the_hub_verifies_people() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let bot = [5u8; 32];
    let id = player_id_of(&bot);
    let look = serde_json::json!({});
    let mut account = AccountUpdate {
        player_id: id.clone(),
        name: "bot1".into(),
        about: String::new(),
        look: look.clone(),
        time: time(),
        signature: sign(&bot, &account_message(&id, "bot1", "", &look, time())),
        work: None,
    };
    assert!(hub.update_account(&account).unwrap_err().contains("bot check"));
    account.work = Some(libroxel_hub::account_work(&id, "someone_else"));
    assert!(hub.update_account(&account).is_err());
    account.work = Some(libroxel_hub::account_work(&id, "bot1"));
    hub.update_account(&account).unwrap();
    assert!(!hub.profile("bot1").unwrap().unwrap().verified);
    // Changing an account that exists needs no new puzzle.
    claim(&hub, &bot, "bot1", time() + 1).unwrap();

    let dir = std::env::temp_dir().join(format!("libroxel-verified-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("hub.sqlite");
    let running = start("127.0.0.1:0".parse().unwrap(), Some(&path)).unwrap();
    let hub = HubClient::new(&running.url());
    claim(&hub, &bot, "Famous", time()).unwrap();
    libroxel_hub::set_verified(&libroxel_hub::open_db(Some(&path)).unwrap(), "famous", true).unwrap();
    assert!(hub.profile("Famous").unwrap().unwrap().verified);
    libroxel_hub::set_verified(&libroxel_hub::open_db(Some(&path)).unwrap(), "famous", false).unwrap();
    assert!(!hub.profile("Famous").unwrap().unwrap().verified);
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn accounts_are_numbered_in_the_order_they_joined() {
    let dir = std::env::temp_dir().join(format!("libroxel-numbers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("hub.sqlite");
    let running = start("127.0.0.1:0".parse().unwrap(), Some(&path)).unwrap();
    let hub = HubClient::new(&running.url());
    let now = time();
    claim(&hub, &[1u8; 32], "Admin", now).unwrap();
    claim(&hub, &[2u8; 32], "llamas", now).unwrap();
    claim(&hub, &[3u8; 32], "eve", now).unwrap();
    // Renaming keeps the number.
    claim(&hub, &[1u8; 32], "Boss", now + 1).unwrap();
    let number = |who: &str| hub.profile(who).unwrap().unwrap().number;
    assert_eq!((number("Boss"), number("llamas"), number("eve")), (0, 1, 2));
    // Profiles open by number, as profile links do.
    assert_eq!(hub.profile("1").unwrap().unwrap().name, "llamas");
    assert_eq!(hub.profile("2").unwrap().unwrap().name, "eve");
    assert!(hub.profile("3").unwrap().is_none());
    assert_eq!(hub.people("lla").unwrap()[0].number, 1);
    // A bare number can't be a name.
    assert!(claim(&hub, &[4u8; 32], "123", now).is_err());
    drop(running);

    // A Hub from before numbers numbers its accounts in the order they joined.
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("DROP INDEX accounts_number; UPDATE accounts SET number = NULL;").unwrap();
    db.execute("UPDATE accounts SET created = created + 100 WHERE name = 'Boss'", []).unwrap();
    drop(db);
    let running = start("127.0.0.1:0".parse().unwrap(), Some(&path)).unwrap();
    let hub = HubClient::new(&running.url());
    let number = |who: &str| hub.profile(who).unwrap().unwrap().number;
    assert_eq!((number("llamas"), number("eve"), number("Boss")), (0, 1, 2));
    claim(&hub, &[5u8; 32], "newcomer", time()).unwrap();
    assert_eq!(number("newcomer"), 3);
}

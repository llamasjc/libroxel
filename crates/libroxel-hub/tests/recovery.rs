use std::sync::{Arc, Mutex};

use libroxel_hub::{
    AccountUpdate, EmailCode, HubClient, HubOptions, Mail, NEEDS_CODE, SIGNED_OUT, TwoStepSet,
    account_message, email_code_message, new_two_step_secret, player_id_of, seal_password_with,
    security_message, sign, start_with, two_step_code, two_step_message,
};

fn time() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

fn ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

type Outbox = Arc<Mutex<Vec<(String, String)>>>;

fn hub() -> (libroxel_hub::RunningHub, HubClient, Outbox) {
    let outbox: Outbox = Arc::default();
    let options = HubOptions { mail: Mail::Outbox(outbox.clone()), ..Default::default() };
    let running = start_with("127.0.0.1:0".parse().unwrap(), None, options).unwrap();
    let client = HubClient::new(&running.url());
    (running, client, outbox)
}

fn account(hub: &HubClient, outbox: &Outbox, key: &[u8; 32], name: &str, password: &str) {
    let id = player_id_of(key);
    let look = serde_json::json!({});
    let t = time();
    hub.update_account(&AccountUpdate {
        work: Some(libroxel_hub::account_work(&id, name)),
        email: Some(format!("{name}@example.com")),
        player_id: id.clone(),
        name: name.into(),
        about: String::new(),
        signature: sign(key, &account_message(&id, name, "", &look, t)),
        look,
        time: t,
    })
    .unwrap();
    let (_, code) = outbox.lock().unwrap().pop().unwrap();
    let t = ms();
    hub.verify_email(&EmailCode {
        player_id: id.clone(),
        signature: sign(key, &email_code_message(&id, &code, t)),
        code,
        time: t,
    })
    .unwrap();
    hub.set_password(&seal_password_with(key, password, time(), 19 * 1024, 2).unwrap()).unwrap();
}

fn security(
    hub: &HubClient,
    key: &[u8; 32],
    session: Option<&str>,
) -> Result<libroxel_hub::Security, String> {
    let id = player_id_of(key);
    let t = ms();
    hub.security_on(&id, t, &sign(key, &security_message(&id, t)), session)
}

fn two_step_on(hub: &HubClient, key: &[u8; 32]) -> (String, Vec<String>) {
    let secret = new_two_step_secret();
    let mut set = TwoStepSet {
        player_id: player_id_of(key),
        secret: Some(secret.clone()),
        code: two_step_code(&secret, time() - 30).unwrap(),
        time: time(),
        signature: String::new(),
    };
    set.signature = sign(key, &two_step_message(&set));
    let codes = hub.set_two_step(&set).unwrap();
    (secret, codes)
}

#[test]
fn recovery_codes_stand_in_for_the_app_once_each() {
    let (_running, hub, outbox) = hub();
    let alice = [21u8; 32];
    account(&hub, &outbox, &alice, "alice", "correct horse battery");
    let (_, codes) = two_step_on(&hub, &alice);
    assert_eq!(codes.len(), libroxel_hub::RECOVERY_CODES);
    assert_eq!(security(&hub, &alice, None).unwrap().recovery_codes_left, 10);

    assert_eq!(hub.sign_in("alice", "correct horse battery", None).unwrap_err(), NEEDS_CODE);
    assert!(
        hub.sign_in("alice", "correct horse battery", Some("aaaaa-bbbbb")).unwrap_err().contains("recovery")
    );
    let shouted = codes[0].to_uppercase();
    assert_eq!(hub.sign_in("alice", "correct horse battery", Some(&shouted)).unwrap(), alice);
    assert!(hub.sign_in("alice", "correct horse battery", Some(&codes[0])).is_err());
    assert_eq!(security(&hub, &alice, None).unwrap().recovery_codes_left, 9);

    let fresh = hub.recovery_codes(&alice).unwrap();
    assert_eq!(security(&hub, &alice, None).unwrap().recovery_codes_left, 10);
    assert!(hub.sign_in("alice", "correct horse battery", Some(&codes[1])).is_err());
    assert_eq!(hub.sign_in("alice", "correct horse battery", Some(&fresh[0])).unwrap(), alice);
    assert!(hub.recovery_codes(&[22u8; 32]).is_err());
}

#[test]
fn sessions_are_listed_and_signing_out_everywhere_drops_every_other_key() {
    let (_running, hub, outbox) = hub();
    let bob = [31u8; 32];
    account(&hub, &outbox, &bob, "bob", "blue kettle on mars");
    let home = hub.start_session(&bob, "Windows").unwrap();
    let (key, _) = hub.sign_in_on("bob", "blue kettle on mars", None, Some("Android")).unwrap();
    assert_eq!(key, bob);

    let seen = security(&hub, &bob, Some(&home)).unwrap();
    assert_eq!(seen.sessions.len(), 2);
    assert!(seen.sessions.iter().any(|s| s.device == "Windows" && s.this));
    assert!(seen.sessions.iter().any(|s| s.device == "Android" && !s.this));

    assert!(hub.sign_out_everywhere(&bob, "bob", "wrong kettle on mars", None, Some("Windows")).is_err());
    let (new, session) =
        hub.sign_out_everywhere(&bob, "bob", "blue kettle on mars", None, Some("Windows")).unwrap();
    assert_ne!(new, bob);
    assert_eq!(hub.profile("bob").unwrap().unwrap().player_id, player_id_of(&new));
    assert_eq!(security(&hub, &bob, None).unwrap_err(), SIGNED_OUT);
    assert_eq!(hub.profile(&player_id_of(&bob)).unwrap_err(), SIGNED_OUT);
    assert!(hub.start_session(&bob, "Android").is_err());
    let now = security(&hub, &new, Some(&session)).unwrap();
    assert_eq!(now.sessions.len(), 1);
    assert!(now.sessions[0].this && now.email_verified && now.has_password);
    assert_eq!(hub.sign_in("bob", "blue kettle on mars", None).unwrap(), new);
}

#[test]
fn a_forgotten_password_is_reset_by_email() {
    let (_running, hub, outbox) = hub();
    let cara = [41u8; 32];
    account(&hub, &outbox, &cara, "cara", "dragonfly sunset 42");
    let (secret, codes) = two_step_on(&hub, &cara);
    let friend = [42u8; 32];
    account(&hub, &outbox, &friend, "dave", "tq8#Lm2vZr!p");

    assert!(hub.finish_reset("cara", "123456", None, "a brand new phrase", None).is_err());
    hub.ask_reset("CARA").unwrap();
    let (to, code) = outbox.lock().unwrap().pop().unwrap();
    assert_eq!(to, "cara@example.com");
    assert!(hub.ask_reset("cara").unwrap_err().contains("just sent"));
    assert!(hub.ask_reset("nobody").is_err());

    let wrong = if code == "000000" { "111111" } else { "000000" };
    assert!(
        hub.finish_reset("cara", wrong, None, "a brand new phrase", None)
            .unwrap_err()
            .contains("not the code")
    );
    assert_eq!(hub.finish_reset("cara", &code, None, "a brand new phrase", None).unwrap_err(), NEEDS_CODE);
    let (key, _) =
        hub.finish_reset("cara", &code, Some(&codes[3]), "a brand new phrase", Some("Firefox")).unwrap();

    assert!(hub.sign_in("cara", "dragonfly sunset 42", Some(&codes[4])).is_err());
    let app = two_step_code(&secret, time()).unwrap();
    assert_eq!(hub.sign_in("cara", "a brand new phrase", Some(&app)).unwrap(), key);
    assert_eq!(security(&hub, &cara, None).unwrap_err(), SIGNED_OUT);
    let s = security(&hub, &key, None).unwrap();
    assert!(s.two_step && s.email_verified);
    assert_eq!(s.recovery_codes_left, 9);
    assert!(hub.finish_reset("cara", &code, Some(&codes[5]), "another new phrase", None).is_err());
}

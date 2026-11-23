use libroxel_hub::{Crash, HubClient, Ping, add_owner, admin_message, open_db, player_id_of, sign, start};

fn ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

#[test]
fn crashes_and_usage_reach_only_owners() {
    let dir = std::env::temp_dir().join(format!("libroxel-stats-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("hub.sqlite");
    let running = start("127.0.0.1:0".parse().unwrap(), Some(&path)).unwrap();
    let hub = HubClient::new(&running.url());
    let install = |n: u8| format!("{n:0>2}abcdefghijklmnopqrstuv");
    for (n, version, platform) in
        [(1, "0.4.0", "windows x86_64"), (2, "0.4.0", "linux x86_64"), (3, "0.3.9", "android aarch64")]
    {
        hub.ping(&Ping { install: install(n), version: version.into(), platform: platform.into() }).unwrap();
    }
    hub.ping(&Ping { install: install(1), version: "0.4.0".into(), platform: "windows x86_64".into() })
        .unwrap();
    assert!(hub.ping(&Ping { install: "short".into(), ..Ping::default() }).is_err());
    let text = "index out of bounds at crates/libroxel/src/ui.rs:10\nC:\\Users\\Bob Smith\\AppData\\x\nsent to bob@example.com\ntoken=abc";
    for n in [1, 2] {
        hub.send_crash(&Crash {
            install: install(n),
            version: "0.4.0".into(),
            platform: "linux x86_64".into(),
            text: text.into(),
        })
        .unwrap();
    }

    let (alice, eve) = ([7u8; 32], [3u8; 32]);
    let ask = |key: &[u8; 32]| {
        let id = player_id_of(key);
        let time = ms();
        hub.usage(&id, time, &sign(key, &admin_message(&id, time)), 7)
    };
    assert!(ask(&alice).is_err());
    add_owner(&open_db(Some(&path)).unwrap(), &player_id_of(&alice)).unwrap();
    assert!(ask(&eve).is_err());
    let usage = ask(&alice).unwrap();
    assert_eq!(usage.users, 3);
    assert_eq!(usage.online, 3);
    assert_eq!(usage.sessions, 3);
    assert_eq!(usage.days.len(), 7);
    assert_eq!(usage.days.last().unwrap().users, 3);
    assert_eq!(usage.days.last().unwrap().new_users, 3);
    assert_eq!(usage.versions[0].name, "0.4.0");
    assert_eq!(usage.versions[0].users, 2);
    assert_eq!(usage.platforms.len(), 3);
    assert_eq!(usage.crashes, 2);
    let crash = &usage.top_crashes[0];
    assert_eq!((crash.count, crash.users), (2, 2));
    assert!(crash.title.starts_with("index out of bounds"));
    assert!(
        !crash.text.contains("Bob")
            && !crash.text.contains("Smith")
            && !crash.text.contains("bob@")
            && !crash.text.contains("abc"),
        "{}",
        crash.text
    );
}

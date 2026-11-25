//! A cheat's computer that joins a game to take it: it keeps every message
//! the host sends it, then asks the game's own RemoteFunctions about every
//! instance on the host, one id at a time. None of the game's server code
//! or storage may reach it, and the code it does get (its own local
//! scripts) has no comments or local names left.

use super::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Words only in what must stay on the host.
const SERVER_ONLY: &[&str] = &[
    "SECRET_SERVER_CODE",
    "SECRET_WORKSPACE_SCRIPT",
    "SECRET_STORAGE_PART",
    "SECRET_STORAGE_MODULE",
    "SECRET_HIDDEN_MODULE",
    "SECRET_CHARACTER_SCRIPT",
    "SECRET_TOOL_SCRIPT",
    "SECRET_SERVER_FOLDER",
    "SECRET_STARTER_CHARACTER_FOLDER",
    "VaultOfSecrets",
];

/// Words in local scripts' comments and local names, which the shield takes out.
const WRITING: &[&str] = &["SECRET_COMMENT", "secretLocalName", "secretHelper", "secretArgument"];

fn add(tree: &mut InstanceTree, class: ClassKind, parent: InstanceId, name: &str) -> InstanceId {
    let id = tree.create(class, name).unwrap();
    tree.set_parent(id, Some(parent)).unwrap();
    id
}

fn code(
    tree: &mut InstanceTree,
    class: ClassKind,
    parent: InstanceId,
    name: &str,
    source: &str,
) -> InstanceId {
    let id = add(tree, class, parent, name);
    tree.set(id, Property::Source, PropertyValue::String(source.into())).unwrap();
    id
}

const LOCAL: &str = r#"
-- SECRET_COMMENT: how the shop works
local secretLocalName = game:GetService("ReplicatedStorage")
local function secretHelper(secretArgument)
    return secretArgument .. "!"
end
--[[ SECRET_COMMENT
     long notes ]]
print(secretHelper(secretLocalName.Name))
"#;

fn game() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let sss = tree.service(ClassKind::ServerScriptService).unwrap();
    code(
        &mut tree,
        ClassKind::Script,
        sss,
        "Main",
        r#"
        -- SECRET_SERVER_CODE
        local rs = game:GetService("ReplicatedStorage")
        local ask = Instance.new("RemoteFunction")
        ask.Name = "Ask"
        ask.Parent = rs
        ask.OnServerInvoke = function(player, thing)
            if typeof(thing) ~= "Instance" then
                return "none"
            end
            local copy = thing:Clone()
            if copy then
                copy.Parent = player.Backpack
            end
            local said = thing:GetFullName()
            if thing:IsA("LuaSourceContainer") then
                said ..= " " .. tostring(thing.Source)
            end
            return said
        end
        "#,
    );
    let folder = add(&mut tree, ClassKind::Folder, sss, "SECRET_SERVER_FOLDER");
    code(&mut tree, ClassKind::ModuleScript, folder, "Lib", "return 'SECRET_HIDDEN_MODULE'");
    let storage = tree.service(ClassKind::ServerStorage).unwrap();
    let vault = add(&mut tree, ClassKind::Folder, storage, "VaultOfSecrets");
    add(&mut tree, ClassKind::Part, vault, "SECRET_STORAGE_PART");
    code(&mut tree, ClassKind::ModuleScript, vault, "Prices", "return 'SECRET_STORAGE_MODULE'");
    let ws = tree.workspace();
    code(&mut tree, ClassKind::Script, ws, "Spinner", "-- SECRET_WORKSPACE_SCRIPT\nlocal x = 1");
    let starter = tree.service(ClassKind::StarterPlayer).unwrap();
    let character = add(&mut tree, ClassKind::StarterCharacterScripts, starter, "StarterCharacterScripts");
    code(&mut tree, ClassKind::Script, character, "Health", "-- SECRET_CHARACTER_SCRIPT\nlocal y = 2");
    let hidden = add(&mut tree, ClassKind::Folder, starter, "SECRET_STARTER_CHARACTER_FOLDER");
    add(&mut tree, ClassKind::Part, hidden, "Nothing");
    let player_scripts = add(&mut tree, ClassKind::StarterPlayerScripts, starter, "StarterPlayerScripts");
    code(&mut tree, ClassKind::LocalScript, player_scripts, "Shop", LOCAL);
    let gui = tree.service(ClassKind::StarterGui).unwrap();
    let screen = add(&mut tree, ClassKind::ScreenGui, gui, "Hud");
    code(&mut tree, ClassKind::LocalScript, screen, "HudScript", LOCAL);
    let rs = tree.service(ClassKind::ReplicatedStorage).unwrap();
    code(&mut tree, ClassKind::ModuleScript, rs, "Shared", LOCAL);
    let pack = tree.service(ClassKind::StarterPack).unwrap();
    let tool = add(&mut tree, ClassKind::Tool, pack, "Sword");
    add(&mut tree, ClassKind::Part, tool, "Handle");
    code(&mut tree, ClassKind::Script, tool, "Damage", "-- SECRET_TOOL_SCRIPT\nlocal z = 3");
    code(&mut tree, ClassKind::LocalScript, tool, "Swing", LOCAL);
    tree
}

/// Everything the cheat was sent, as text.
fn take(thief: &mut Conn, kept: &mut Vec<Json>) {
    thief.flush();
    kept.extend(thief.receive());
}

#[test]
fn a_cheat_that_joins_gets_no_server_code_or_storage_and_no_readable_local_code() {
    let tree = Rc::new(RefCell::new(game()));
    let mut host = PlaySession::host(tree.clone(), 0, None, false).unwrap();
    let port = host.host_address().unwrap().port();
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut thief = Conn::new(stream, Side::Joining).unwrap();
    thief.send(&json!({ "t": "hello", "name": "Thief", "version": PROTOCOL, "ka": true }));
    let mut kept = Vec::new();
    for _ in 0..240 {
        host.step(1.0 / 60.0, CharacterInput::default());
        take(&mut thief, &mut kept);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(kept.iter().any(|m| m["t"] == "rep"), "the world never came: {kept:?}");

    // Then it names every instance the host has to the game's own
    // RemoteFunction, which hands back whatever it is given.
    let ask = {
        let t = tree.borrow();
        let rs = t.service(ClassKind::ReplicatedStorage).unwrap();
        t.find_first_child(rs, "Ask").unwrap().unwrap()
    };
    let everything: Vec<InstanceId> = {
        let t = tree.borrow();
        t.descendants(t.root()).unwrap()
    };
    for (n, ids) in everything.chunks(8).enumerate() {
        for (i, id) in ids.iter().enumerate() {
            let call = (n * 8 + i) as u64 + 1;
            thief
                .send(&json!({ "t": "invoke", "call": call, "r": bits(ask), "a": [{ "$i": id.to_bits() }] }));
        }
        for _ in 0..3 {
            host.step(1.0 / 60.0, CharacterInput::default());
            take(&mut thief, &mut kept);
        }
    }
    for _ in 0..60 {
        host.step(1.0 / 60.0, CharacterInput::default());
        take(&mut thief, &mut kept);
    }
    let answers = kept.iter().filter(|m| m["t"] == "result").count();
    assert!(answers >= everything.len() / 2, "the host answered only {answers} questions");

    let dump: String = kept.iter().map(|m| m.to_string()).collect::<Vec<_>>().join("\n");
    for word in SERVER_ONLY {
        let at = dump.find(word).map(|i| &dump[i.saturating_sub(300)..(i + 100).min(dump.len())]);
        assert!(at.is_none(), "the cheat got {word}: {at:?}");
    }
    for word in WRITING {
        assert!(!dump.contains(word), "the cheat could read {word} in a local script");
    }
    // It still gets its local scripts, as code that runs.
    assert!(dump.contains("GetService") && dump.contains("ReplicatedStorage"), "no local scripts came");
    // And the host's tree is as it was: nothing from storage was copied out.
    let t = tree.borrow();
    let players = t.service(ClassKind::Players).unwrap();
    for id in t.descendants(players).unwrap() {
        let name = t.name(id).unwrap();
        assert!(!SERVER_ONLY.iter().any(|w| name.contains(w)), "{name} was copied to a player");
    }
}

#[test]
fn people_editing_together_still_get_everything() {
    let tree = game();
    let mut replicator = Replicator::edits();
    let dump = Json::Array(replicator.delta(&tree)).to_string();
    for word in ["SECRET_SERVER_CODE", "VaultOfSecrets", "SECRET_COMMENT", "secretLocalName"] {
        assert!(dump.contains(word), "an editor didn't get {word}");
    }
}

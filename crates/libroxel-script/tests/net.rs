//! Remote events and functions between a server and a player's computer,
//! passed by hand the way the engine passes them over the network.

use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue};
use libroxel_script::{NetMessage, NetRole, ScriptRuntime, map_instances};

/// A server and a client over copies of one tree, so ids match.
fn pair() -> (ScriptRuntime, ScriptRuntime, InstanceId, InstanceId) {
    let mut tree = InstanceTree::new();
    let players = tree.service(ClassKind::Players).unwrap();
    let player = tree.create(ClassKind::Player, "Ana").unwrap();
    tree.set(player, Property::UserId, PropertyValue::Number(2.0)).unwrap();
    tree.set_parent(player, Some(players)).unwrap();
    let storage = tree.service(ClassKind::ReplicatedStorage).unwrap();
    for (class, name) in [(ClassKind::RemoteEvent, "Say"), (ClassKind::RemoteFunction, "Ask")] {
        let id = tree.create(class, name).unwrap();
        tree.set_parent(id, Some(storage)).unwrap();
    }
    let part = tree.create(ClassKind::Part, "Target").unwrap();
    tree.set_parent(part, Some(tree.workspace())).unwrap();
    let server = ScriptRuntime::new_with_tree(tree.clone()).unwrap();
    server.set_net_role(NetRole::Server);
    let client = ScriptRuntime::new_with_tree(tree).unwrap();
    client.set_net_role(NetRole::Client);
    client.set_local_player(Some(player));
    (server, client, player, part)
}

fn run(rt: &ScriptRuntime, src: &str) {
    rt.run("test", src).unwrap_or_else(|e| panic!("script failed: {e}"));
}

fn printed(rt: &ScriptRuntime) -> Vec<String> {
    rt.take_output().into_iter().map(|l| l.text).collect()
}

/// Hands every message from one side to the other.
fn pass(from: &ScriptRuntime, to: &ScriptRuntime, player: InstanceId) {
    for message in from.take_net_messages() {
        let errors = match message {
            NetMessage::ServerEvent { remote, args } => to.deliver_server_event(remote, player, &args),
            NetMessage::ClientEvent { remote, args, .. } => to.deliver_client_event(remote, &args),
            NetMessage::Invoke { call, remote, args } => to.deliver_invoke(call, remote, player, &args),
            NetMessage::InvokeResult { call, ok, values, .. } => {
                to.deliver_invoke_result(call, ok, values);
                Vec::new()
            }
        };
        assert!(errors.is_empty(), "{errors:?}");
    }
}

#[test]
fn remote_events_cross_with_their_values() {
    let (server, client, player, _) = pair();
    run(
        &server,
        r#"
        local say = game.ReplicatedStorage.Say
        print(game:GetService("RunService"):IsServer(), game:GetService("RunService"):IsClient())
        say.OnServerEvent:Connect(function(player, text, at, part, list)
            print(player.Name, text, at.Y, part.Name, list[2].colour.G, list.n)
            say:FireAllClients("hello " .. player.Name, Enum.Material.Neon)
        end)
        "#,
    );
    assert_eq!(printed(&server), ["true false"]);
    run(
        &client,
        r#"
        local say = game.ReplicatedStorage.Say
        print(game.Players.LocalPlayer.Name, game:GetService("RunService"):IsServer())
        say.OnClientEvent:Connect(function(text, material)
            print(text, material == Enum.Material.Neon)
        end)
        say:FireServer("hi", Vector3.new(1, 2, 3), workspace.Target, { 1, { colour = Color3.new(0, 0.5, 1) } })
        "#,
    );
    assert_eq!(printed(&client), ["Ana false"]);
    pass(&client, &server, player);
    assert_eq!(printed(&server), ["Ana hi 2 Target 0.5 nil"]);
    // FireAllClients on a server with no player of its own fires nothing there.
    pass(&server, &client, player);
    assert_eq!(printed(&client), ["hello Ana true"]);
    assert!(client.run("bad", "game.ReplicatedStorage.Say:FireAllClients()").is_err());
}

#[test]
fn invoke_server_waits_for_the_answer() {
    let (server, client, player, _) = pair();
    run(
        &server,
        r#"
        game.ReplicatedStorage.Ask.OnServerInvoke = function(player, a, b)
            task.wait(0.1)
            if a == 0 then error("no zeros") end
            return player.Name, a + b
        end
        "#,
    );
    run(
        &client,
        r#"
        task.spawn(function()
            local name, sum = game.ReplicatedStorage.Ask:InvokeServer(2, 3)
            print(name, sum)
            local ok, err = pcall(function() return game.ReplicatedStorage.Ask:InvokeServer(0, 1) end)
            print(ok, string.find(err, "no zeros") ~= nil)
        end)
        "#,
    );
    for _ in 0..30 {
        assert!(client.step(1.0 / 30.0).is_empty());
        pass(&client, &server, player);
        assert!(server.step(1.0 / 30.0).is_empty());
        pass(&server, &client, player);
    }
    assert_eq!(printed(&client), ["Ana 5", "false true"]);
}

#[test]
fn instances_can_be_renumbered_for_the_other_side() {
    let (_, client, _, part) = pair();
    run(&client, "game.ReplicatedStorage.Say:FireServer(workspace.Target, { workspace.Target })");
    let Some(NetMessage::ServerEvent { mut args, .. }) = client.take_net_messages().pop() else { panic!() };
    let other = InstanceId::from_bits(999);
    map_instances(&mut args, &mut |id| (id == part).then_some(other));
    assert_eq!(args, libroxel_instance::serde_json::json!([{ "$i": 999 }, [{ "$i": 999 }]]));
}

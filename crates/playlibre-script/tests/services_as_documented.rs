//! Game services behaving as their documentation says: tweens, data
//! stores, tags, sounds, render steps, bound actions, text chat, text
//! sizes, the marketplace and badges.

use playlibre_instance::{ClassKind, KeyCode};
use playlibre_script::{ScriptRuntime, UserInput};

fn runtime() -> ScriptRuntime {
    ScriptRuntime::new_with_tree(Default::default()).expect("runtime starts")
}

fn run(rt: &ScriptRuntime, src: &str) {
    if let Err(err) = rt.run("test", src) {
        panic!("script failed: {err}");
    }
}

fn printed(rt: &ScriptRuntime) -> Vec<String> {
    rt.take_output().into_iter().map(|l| l.text).collect()
}

/// Runs `seconds` of frames, handing the tree's changes to scripts after each.
fn play(rt: &ScriptRuntime, seconds: f64) {
    let frames = (seconds * 30.0).ceil() as usize;
    for _ in 0..frames {
        let errors = rt.step(1.0 / 30.0);
        assert!(errors.is_empty(), "{errors:?}");
        let changes = rt.tree().borrow_mut().drain_changes();
        let errors = rt.fire_changes(&changes);
        assert!(errors.is_empty(), "{errors:?}");
    }
}

fn add_player(rt: &ScriptRuntime, name: &str) {
    let mut tree = rt.tree().borrow_mut();
    let players = tree.service(ClassKind::Players).unwrap();
    let player = tree.create(ClassKind::Player, name).unwrap();
    tree.set_parent(player, Some(players)).unwrap();
}

#[test]
fn tweens_know_their_info_and_a_newer_tween_cancels_an_older_one() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local TweenService = game:GetService("TweenService")
        local part = Instance.new("Part", workspace)
        local slow = TweenService:Create(part, TweenInfo.new(1), { Transparency = 1 })
        print(slow.TweenInfo.Time, slow.ClassName, slow:IsA("TweenBase"), slow.PlaybackState.Name)
        local fast = TweenService:Create(part, TweenInfo.new(0.2), { Transparency = 0.5 })
        slow.Completed:Connect(function(state) print("slow", state.Name) end)
        fast.Completed:Connect(function(state) print("fast", state.Name) end)
        slow:Play()
        task.delay(0.1, function() fast:Play() end)
        task.delay(0.6, function()
            print(part.Transparency)
            -- Played again, the overridden tween has its goal back.
            slow:Play()
            slow.Completed:Wait()
            print(part.Transparency)
        end)

        local flag = Instance.new("BoolValue")
        local flip = TweenService:Create(flag, TweenInfo.new(0.3), { Value = true })
        flip:Play()
        task.delay(0.15, function() print("halfway", flag.Value) end)
        flip.Completed:Connect(function() print("done", flag.Value) end)
    "#,
    );
    play(&rt, 2.5);
    assert_eq!(
        printed(&rt),
        [
            "1 Tween true Begin",
            "slow Cancelled",
            "halfway false",
            "fast Completed",
            "done true",
            "0.5",
            "slow Completed",
            "1"
        ]
    );
}

#[test]
fn data_stores_keep_versions_scopes_user_ids_and_metadata() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local DataStoreService = game:GetService("DataStoreService")
        local store = DataStoreService:GetDataStore("Profiles")
        local options = Instance.new("DataStoreSetOptions")
        options:SetMetadata({ source = "shop" })
        local first = store:SetAsync("p1", { coins = 1 }, { 101 }, options)
        local second = store:SetAsync("p1", { coins = 2 })
        print(type(first), first ~= second)

        local value, info = store:GetAsync("p1")
        print(value.coins, info.ClassName, info.Version == second, info:GetUserIds()[1], info:GetMetadata().source)
        print(info.CreatedTime <= info.UpdatedTime, info.CreatedTime > 1e12)
        print(store:GetVersionAsync("p1", first).coins)

        local versions = store:ListVersionsAsync("p1"):GetCurrentPage()
        print(#versions, versions[1].Version == first, versions[2].IsDeleted)

        -- The callback is handed the key's info, and can set user ids.
        print(store:UpdateAsync("p1", function(old, keyInfo)
            return { coins = old.coins + keyInfo:GetUserIds()[1] }, { 202 }
        end).coins)
        print(select(2, store:GetAsync("p1")):GetUserIds()[1])

        -- Each scope is a store of its own.
        DataStoreService:GetDataStore("Profiles", "test"):SetAsync("p1", "other")
        print(store:GetAsync("p1").coins, DataStoreService:GetDataStore("Profiles", "test"):GetAsync("p1"))

        -- What GetAsync hands back is a copy.
        value.coins = 999
        print(store:GetAsync("p1").coins)
        local removed, removedInfo = store:RemoveAsync("p1")
        print(removed.coins, removedInfo ~= nil, store:GetAsync("p1"))
        print(pcall(store.SetAsync, store, string.rep("k", 51), 1))
    "#,
    );
    let out = printed(&rt);
    assert_eq!(out[0], "string true");
    assert_eq!(out[1], "2 DataStoreKeyInfo true 101 shop");
    assert_eq!(out[2], "true true");
    assert_eq!(out[3], "1");
    assert_eq!(out[4], "2 true false");
    assert_eq!(out[5], "103");
    assert_eq!(out[6], "202");
    assert_eq!(out[7], "103 other DataStoreKeyInfo");
    assert_eq!(out[8], "103");
    assert_eq!(out[9], "103 true nil nil");
    assert!(out[10].starts_with("false"), "{out:?}");

    // Values (not their history) are what's saved.
    let saved = rt.save_data();
    let rt2 = runtime();
    rt2.load_data(&saved).unwrap();
    run(
        &rt2,
        r#"print(game:GetService("DataStoreService"):GetDataStore("Profiles", "test"):GetAsync("p1"))"#,
    );
    assert_eq!(printed(&rt2), ["other DataStoreKeyInfo"]);
}

#[test]
fn ordered_data_stores_hold_whole_numbers_and_page_by_value() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local board = game:GetService("DataStoreService"):GetOrderedDataStore("Wins")
        for name, wins in { ann = 5, bo = 10, cy = 1, di = 7 } do
            board:SetAsync(name, wins)
        end
        local pages = board:GetSortedAsync(false, 2)
        local function names(page)
            local out = {}
            for _, row in page do
                table.insert(out, row.key .. "=" .. row.value)
            end
            return table.concat(out, " ")
        end
        print(names(pages:GetCurrentPage()), pages.IsFinished)
        pages:AdvanceToNextPageAsync()
        print(names(pages:GetCurrentPage()), pages.IsFinished)
        print(names(board:GetSortedAsync(true, 10, 2, 8):GetCurrentPage()))
        print(pcall(board.SetAsync, board, "ed", 1.5))
        print(board:IncrementAsync("ann", 2))
    "#,
    );
    let out = printed(&rt);
    assert_eq!(out[0], "bo=10 di=7 false");
    assert_eq!(out[1], "ann=5 cy=1 true");
    assert_eq!(out[2], "ann=5 di=7");
    assert!(out[3].starts_with("false") && out[3].contains("whole number"), "{out:?}");
    assert_eq!(out[4], "7");
}

#[test]
fn collection_service_says_when_a_tag_is_first_used_and_last_dropped() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local CollectionService = game:GetService("CollectionService")
        CollectionService.TagAdded:Connect(function(tag) print("added", tag) end)
        CollectionService.TagRemoved:Connect(function(tag) print("removed", tag) end)
        local a = Instance.new("Part", workspace)
        local b = Instance.new("Part", workspace)
        a:AddTag("Lava")
        task.delay(0.1, function()
            b:AddTag("Lava")
            task.wait(0.1)
            a:RemoveTag("Lava")
            task.wait(0.1)
            b:Destroy()
        end)
    "#,
    );
    play(&rt, 0.6);
    assert_eq!(printed(&rt), ["added Lava", "removed Lava"]);
}

#[test]
fn sounds_fire_played_paused_resumed_and_stopped() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local sound = Instance.new("Sound", workspace)
        sound.SoundId = "rbxassetid://123"
        for _, name in { "Played", "Paused", "Resumed", "Stopped" } do
            sound[name]:Connect(function(id) print(name, id) end)
        end
        sound:Play()
        sound:Pause()
        sound:Pause()
        sound:Resume()
        sound.TimePosition = 3
        sound:Stop()
        print(sound.TimePosition, sound.IsPlaying)
    "#,
    );
    play(&rt, 0.1);
    assert_eq!(
        printed(&rt),
        [
            "Played rbxassetid://123",
            "Paused rbxassetid://123",
            "Resumed rbxassetid://123",
            "Stopped rbxassetid://123",
            "0 false"
        ]
    );
}

#[test]
fn render_steps_run_lowest_priority_first() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local RunService = game:GetService("RunService")
        local seen = {}
        RunService:BindToRenderStep("last", Enum.RenderPriority.Last.Value, function() table.insert(seen, "last") end)
        RunService:BindToRenderStep("camera", Enum.RenderPriority.Camera.Value, function() table.insert(seen, "camera") end)
        RunService:BindToRenderStep("first", Enum.RenderPriority.First.Value, function(dt)
            table.insert(seen, "first")
        end)
        task.delay(0.05, function()
            RunService:UnbindFromRenderStep("first")
            RunService:UnbindFromRenderStep("camera")
            RunService:UnbindFromRenderStep("last")
            print(table.concat(seen, " ", 1, 3))
            local count = #seen
            task.wait(0.1)
            print(#seen == count)
        end)
    "#,
    );
    play(&rt, 0.3);
    assert_eq!(printed(&rt), ["first camera last", "true"]);
}

#[test]
fn bound_actions_go_highest_priority_first_and_sink_unless_passed() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local ContextActionService = game:GetService("ContextActionService")
        ContextActionService:BindAction("Low", function(_, state)
            print("low", state.Name)
            return Enum.ContextActionResult.Pass
        end, false, Enum.KeyCode.G)
        ContextActionService:BindActionAtPriority("High", function(_, state, input)
            print("high", state.Name, input.KeyCode.Name)
            return Enum.ContextActionResult.Pass
        end, false, Enum.ContextActionPriority.High.Value, Enum.KeyCode.G)
        ContextActionService:BindAction("Older", function(_, state) print("older", state.Name) end, false, Enum.KeyCode.H)
        -- Bound later at the same priority: goes first, and sinks the key.
        ContextActionService:BindAction("Newer", function(_, state) print("newer", state.Name) end, false, Enum.KeyCode.H)
        local info = ContextActionService:GetBoundActionInfo("High")
        print(info.inputTypes[1] == Enum.KeyCode.G, info.priorityLevel, ContextActionService:GetAllBoundActionInfo().Low ~= nil)
    "#,
    );
    rt.input_began(UserInput::Key(KeyCode::G), false);
    rt.input_ended(UserInput::Key(KeyCode::G), false);
    rt.input_began(UserInput::Key(KeyCode::H), false);
    // Unbinding while the key is held cancels it.
    run(&rt, r#"game:GetService("ContextActionService"):UnbindAction("Newer")"#);
    rt.input_ended(UserInput::Key(KeyCode::H), false);
    assert_eq!(
        printed(&rt),
        [
            "true 3000 true",
            "high Begin G",
            "low Begin",
            "high End G",
            "low End",
            "newer Begin",
            "newer Cancel",
            "older End"
        ]
    );
}

#[test]
fn text_chat_delivers_messages_through_its_channels_and_commands() {
    let rt = runtime();
    add_player(&rt, "Sam");
    run(
        &rt,
        r#"
        local TextChatService = game:GetService("TextChatService")
        local general = TextChatService:WaitForChild("TextChannels"):WaitForChild("RBXGeneral")
        print(TextChatService.TextChannels == TextChatService:FindFirstChild("TextChannels"), general.ClassName)
        TextChatService.OnIncomingMessage = function(message)
            local props = Instance.new("TextChatMessageProperties")
            if message.TextSource then
                props.PrefixText = "[VIP] Sam:"
            end
            return props
        end
        general.MessageReceived:Connect(function(message)
            print("got", message.PrefixText, message.Text, message.TextSource and message.TextSource.Name)
        end)
        local players = game:GetService("Players")
        players.LocalPlayer.Chatted:Connect(function(text) print("chatted", text) end)

        local command = Instance.new("TextChatCommand")
        command.PrimaryAlias = "/dance"
        command.Parent = TextChatService:FindFirstChild("TextChatCommands")
        command.Triggered:Connect(function(source, text) print("command", source.Name, text) end)

        local sent = general:SendAsync("hello")
        print(sent.Text, sent.Status.Name)
        general:SendAsync("/DANCE now")
        general:DisplaySystemMessage("Welcome!")
    "#,
    );
    play(&rt, 0.1);
    assert_eq!(
        printed(&rt),
        [
            "true TextChannel",
            "chatted hello",
            "got [VIP] Sam: hello Sam",
            "hello Success",
            "command Sam /DANCE now",
            "Welcome!",
            "got  Welcome! nil",
        ]
    );
}

#[test]
fn text_sizes_wrap_at_the_frame() {
    let rt = runtime();
    run(
        &rt,
        r#"
        local TextService = game:GetService("TextService")
        local one = TextService:GetTextSize("Hello", 14, Enum.Font.SourceSans, Vector2.new(1000, 1000))
        local wrapped = TextService:GetTextSize(string.rep("word ", 40), 14, Enum.Font.SourceSans, Vector2.new(100, 1000))
        print(one.Y, one.X > 0, wrapped.X <= 100, wrapped.Y > 28)
        print(TextService:FilterStringAsync("hi", 1).ClassName)
    "#,
    );
    assert_eq!(printed(&rt), ["14 true true true", "TextFilterResult"]);
}

#[test]
fn purchases_close_unbought_and_badges_are_remembered() {
    let rt = runtime();
    add_player(&rt, "Sam");
    run(
        &rt,
        r#"
        local MarketplaceService = game:GetService("MarketplaceService")
        local player = game:GetService("Players").LocalPlayer
        MarketplaceService.PromptGamePassPurchaseFinished:Connect(function(who, id, bought)
            print("pass", who == player, id, bought)
        end)
        MarketplaceService.PromptProductPurchaseFinished:Connect(function(userId, id, bought)
            print("product", userId == player.UserId, id, bought)
        end)
        MarketplaceService:PromptGamePassPurchase(player, 55)
        MarketplaceService:PromptProductPurchase(player, 66)
        local info = MarketplaceService:GetProductInfoAsync(55, Enum.InfoType.GamePass)
        print(info.PriceInRobux, info.IsForSale, info.TargetId, MarketplaceService:PlayerOwnsAssetAsync(player, 1))

        local BadgeService = game:GetService("BadgeService")
        print(BadgeService:GetBadgeInfoAsync(7).IsEnabled, BadgeService:UserHasBadgeAsync(42, 7))
        print(BadgeService:AwardBadge(42, 7), BadgeService:AwardBadge(42, 7), BadgeService:UserHasBadgeAsync(42, 7))
        print(#BadgeService:CheckUserBadgesAsync(42, { 7, 8 }))
    "#,
    );
    play(&rt, 0.1);
    let out: Vec<String> = printed(&rt).into_iter().filter(|l| !l.contains("isn't available")).collect();
    assert_eq!(
        out,
        [
            "0 false 55 false",
            "true false",
            "true false true",
            "1",
            "pass true 55 false",
            "product true 66 false"
        ]
    );

    let saved = rt.save_data();
    let rt2 = runtime();
    rt2.load_data(&saved).unwrap();
    run(&rt2, r#"print(game:GetService("BadgeService"):UserHasBadgeAsync(42, 7))"#);
    assert_eq!(printed(&rt2), ["true"]);
}

#[test]
fn game_passes_come_from_the_host_and_prompts_wait_for_the_player() {
    use playlibre_instance::{Property, PropertyValue};
    let rt = runtime();
    {
        let mut tree = rt.tree().borrow_mut();
        let players = tree.service(ClassKind::Players).unwrap();
        let player = tree.create(ClassKind::Player, "Sam").unwrap();
        tree.set(player, Property::UserId, PropertyValue::Number(7.0)).unwrap();
        tree.set_parent(player, Some(players)).unwrap();
    }
    rt.set_pass_info(vec![playlibre_script::PassInfo {
        id: 12,
        name: "VIP".into(),
        description: "The VIP room".into(),
        price: 40,
    }]);
    run(
        &rt,
        r#"
        local MarketplaceService = game:GetService("MarketplaceService")
        local player = game:GetService("Players"):GetPlayers()[1]
        MarketplaceService.PromptGamePassPurchaseFinished:Connect(function(who, id, bought)
            print("finished", who == player, id, bought, MarketplaceService:UserOwnsGamePassAsync(who.UserId, id))
        end)
        local info = MarketplaceService:GetProductInfo(12, Enum.InfoType.GamePass)
        print(info.Name, info.PriceInRobux, info.IsForSale)
        task.spawn(function()
            print("owns", MarketplaceService:UserOwnsGamePassAsync(7, 12))
            MarketplaceService:PromptGamePassPurchase(player, 12)
        end)
    "#,
    );
    play(&rt, 0.3);
    // Waits until the host has looked the player up.
    assert_eq!(printed(&rt), ["VIP 40 true"]);
    rt.add_owned_passes(7, []);
    play(&rt, 0.3);
    assert_eq!(printed(&rt), ["owns false"]);
    assert_eq!(rt.take_pass_prompts(), [(7, 12)]);
    let errors = rt.pass_prompt_finished(7, 12, true);
    assert!(errors.is_empty(), "{errors:?}");
    play(&rt, 0.1);
    assert_eq!(printed(&rt), ["finished true 12 true true"]);
}

#[test]
fn teleports_wait_for_the_host_and_places_know_who_they_are() {
    use playlibre_instance::{Property, PropertyValue};
    let rt = runtime();
    {
        let mut tree = rt.tree().borrow_mut();
        let players = tree.service(ClassKind::Players).unwrap();
        let player = tree.create(ClassKind::Player, "Sam").unwrap();
        tree.set(player, Property::UserId, PropertyValue::Number(7.0)).unwrap();
        tree.set_parent(player, Some(players)).unwrap();
    }
    rt.set_game_identity(playlibre_script::GameIdentity {
        game_id: 3,
        place_id: 40,
        job_id: "job".into(),
        private_server_id: "abc".into(),
        private_server_owner_id: 9,
    });
    rt.set_join_data(
        7,
        playlibre_script::JoinData { source_place_id: 41, data: Some(serde_json::json!({ "coins": 5 })) },
    );
    run(
        &rt,
        r#"
        local TeleportService = game:GetService("TeleportService")
        local player = game:GetService("Players"):GetPlayers()[1]
        print(game.GameId, game.PlaceId, game.JobId, game.PrivateServerId, game.PrivateServerOwnerId)
        local join = player:GetJoinData()
        print(join.SourcePlaceId, join.TeleportData.coins)
        local code, id = TeleportService:ReserveServer(41)
        print(#code, code == id)
        TeleportService:TeleportToPrivateServer(41, code, { player }, nil, { level = 2 })
        TeleportService:Teleport(40, player)
    "#,
    );
    assert_eq!(printed(&rt)[..2], ["3 40 job abc 9".to_owned(), "41 5".to_owned()]);
    let sent = rt.take_teleports();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].user_id, 7);
    assert_eq!(sent[0].place_id, 41);
    assert_eq!(sent[0].access_code.as_ref().map(String::len), Some(32));
    assert_eq!(sent[0].data, Some(serde_json::json!({ "level": 2 })));
    assert_eq!((sent[1].place_id, sent[1].access_code.clone()), (40, None));
}

#[test]
fn products_prompt_the_host_and_receipts_go_to_process_receipt() {
    use playlibre_instance::{Property, PropertyValue};
    let rt = runtime();
    {
        let mut tree = rt.tree().borrow_mut();
        let players = tree.service(ClassKind::Players).unwrap();
        let player = tree.create(ClassKind::Player, "Sam").unwrap();
        tree.set(player, Property::UserId, PropertyValue::Number(7.0)).unwrap();
        tree.set_parent(player, Some(players)).unwrap();
    }
    rt.set_product_info(vec![playlibre_script::ProductInfo {
        id: 5,
        name: "Coins".into(),
        description: "100 coins".into(),
        price: 10,
    }]);
    let receipt = |id| playlibre_script::ReceiptInfo { id, user_id: 7, product_id: 5, price: 10 };
    let errors = rt.process_receipt(&receipt(1));
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(rt.take_receipt_results(), [(1, false)]);
    run(
        &rt,
        r#"
        local MarketplaceService = game:GetService("MarketplaceService")
        local seen = {}
        MarketplaceService.ProcessReceipt = function(info)
            task.wait(0.1)
            if seen[info.PurchaseId] then
                return Enum.ProductPurchaseDecision.NotProcessedYet
            end
            seen[info.PurchaseId] = true
            print("granted", info.PlayerId, info.ProductId, info.CurrencySpent, info.CurrencyType == Enum.CurrencyType.Robux)
            return Enum.ProductPurchaseDecision.PurchaseGranted
        end
        MarketplaceService.PromptProductPurchaseFinished:Connect(function(userId, id, bought)
            print("finished", userId, id, bought)
        end)
        local info = MarketplaceService:GetProductInfo(5, Enum.InfoType.Product)
        print(info.Name, info.PriceInRobux)
        MarketplaceService:PromptProductPurchase(game:GetService("Players"):GetPlayers()[1], 5)
    "#,
    );
    assert_eq!(rt.take_product_prompts(), [(7, 5)]);
    assert!(rt.product_prompt_finished(7, 5, true).is_empty());
    assert!(rt.process_receipt(&receipt(2)).is_empty());
    assert!(rt.process_receipt(&receipt(2)).is_empty());
    play(&rt, 0.3);
    assert_eq!(printed(&rt), ["Coins 10", "finished 7 5 true", "granted 7 5 10 true"]);
    assert_eq!(rt.take_receipt_results(), [(2, true)]);
    rt.process_receipt(&receipt(2));
    play(&rt, 0.3);
    assert_eq!(rt.take_receipt_results(), [(2, false)]);
}

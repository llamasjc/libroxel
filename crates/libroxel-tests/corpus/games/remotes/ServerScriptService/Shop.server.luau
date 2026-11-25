-- Server side of a shop: validates every request, rate limits, replies.
local Net = require(game:GetService("ReplicatedStorage"):WaitForChild("Net"))

local buy = Net.func("BuyItem")
local notify = Net.event("Notify")
local click = Net.event("Click")

local PRICES = { Sword = 50, Shield = 30 }
local wallets = {}
local lastClick = {}

buy.OnServerInvoke = function(player, item)
	if type(item) ~= "string" or not PRICES[item] then
		return false, "no such item"
	end
	local money = wallets[player] or 100
	if money < PRICES[item] then
		return false, "too poor"
	end
	wallets[player] = money - PRICES[item]
	notify:FireClient(player, "You bought " .. item)
	return true, wallets[player]
end

click.OnServerEvent:Connect(function(player, payload)
	local now = os.clock()
	if lastClick[player] and now - lastClick[player] < 0.05 then
		return
	end
	lastClick[player] = now
	if typeof(payload) == "table" and typeof(payload.position) == "Vector3" then
		print(string.format("shop: click from %s at %.0f", player.Name, payload.position.Y))
		notify:FireAllClients("clicked")
	end
end)
print("shop: server ready")

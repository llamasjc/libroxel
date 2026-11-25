-- Leaderstats saved with DataStoreService, with pcall and retries the way
-- most games guard their data.
local Players = game:GetService("Players")
local DataStoreService = game:GetService("DataStoreService")
local HttpService = game:GetService("HttpService")

local store = DataStoreService:GetDataStore("PlayerData_v2")
local DEFAULT = { Coins = 0, Gems = 5, Inventory = {}, Settings = { Music = true } }

local function deepCopy(t)
	local copy = {}
	for k, v in pairs(t) do
		copy[k] = type(v) == "table" and deepCopy(v) or v
	end
	return copy
end

local function load(player)
	local key = "player_" .. player.UserId
	local ok, data
	for attempt = 1, 3 do
		ok, data = pcall(function()
			return store:GetAsync(key)
		end)
		if ok then break end
		task.wait(0.1 * attempt)
	end
	if not ok then
		warn("could not load data for " .. player.Name)
	end
	if type(data) == "string" then
		data = HttpService:JSONDecode(data)
	end
	data = data or deepCopy(DEFAULT)
	for k, v in pairs(DEFAULT) do
		if data[k] == nil then
			data[k] = type(v) == "table" and deepCopy(v) or v
		end
	end
	return data
end

local sessionData = {}

Players.PlayerAdded:Connect(function(player)
	local data = load(player)
	sessionData[player] = data
	local stats = Instance.new("Folder")
	stats.Name = "leaderstats"
	local coins = Instance.new("IntValue")
	coins.Name = "Coins"
	coins.Value = data.Coins
	coins.Parent = stats
	local gems = Instance.new("NumberValue")
	gems.Name = "Gems"
	gems.Value = data.Gems
	gems.Parent = stats
	stats.Parent = player
	coins.Changed:Connect(function(value)
		data.Coins = value
	end)
	print(string.format("data: loaded %s with %d coins and %d gems", player.Name, coins.Value, gems.Value))

	task.spawn(function()
		for _ = 1, 3 do
			task.wait(0.5)
			coins.Value += 10
		end
		table.insert(data.Inventory, "Sword")
		local ok, err = pcall(function()
			store:SetAsync("player_" .. player.UserId, HttpService:JSONEncode(data))
		end)
		print("data: saved", ok, err)
		local again = HttpService:JSONDecode(store:GetAsync("player_" .. player.UserId))
		print("data: stored coins " .. again.Coins .. ", first item " .. tostring(again.Inventory[1]))
		local ok2, result = pcall(function()
			return store:UpdateAsync("player_" .. player.UserId, function(old)
				local t = HttpService:JSONDecode(old)
				t.Gems += 1
				return HttpService:JSONEncode(t)
			end)
		end)
		if ok2 then
			print("data: gems now " .. HttpService:JSONDecode(result).Gems)
		else
			print("data: update failed " .. tostring(result))
		end
	end)
end)

Players.PlayerRemoving:Connect(function(player)
	sessionData[player] = nil
end)

game:BindToClose(function()
	print("data: closing")
end)

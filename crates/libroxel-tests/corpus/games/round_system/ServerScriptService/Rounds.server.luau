-- Intermission, map vote, round, repeat: the loop behind most minigame games.
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Teams = game:GetService("Teams")

local Config = require(ReplicatedStorage:WaitForChild("Config"))
local Signal = require(ReplicatedStorage.Signal)

local status = Instance.new("StringValue")
status.Name = "Status"
status.Parent = ReplicatedStorage

local roundEnded = Signal.new()

local red = Instance.new("Team")
red.Name = "Red"
red.TeamColor = BrickColor.new("Bright red")
red.Parent = Teams
local blue = Instance.new("Team")
blue.Name = "Blue"
blue.TeamColor = BrickColor.new("Bright blue")
blue.Parent = Teams

local function countdown(label, seconds)
	for t = seconds, 1, -1 do
		status.Value = label .. " " .. t
		task.wait(1)
	end
end

local rounds = 0
local function round()
	repeat
		task.wait(0.5)
	until #Players:GetPlayers() >= Config.MinPlayers
	countdown("Intermission", Config.IntermissionTime)
	local map = Config.Maps[rounds % #Config.Maps + 1]
	print("rounds: playing on " .. map)
	for i, player in ipairs(Players:GetPlayers()) do
		player.Team = (i % 2 == 0) and blue or red
	end
	task.delay(Config.RoundTime, function()
		roundEnded:Fire(map, red)
	end)
	local endedMap, winner = roundEnded:Wait()
	rounds += 1
	print(string.format("rounds: %s ended, %s team wins (round %d)", endedMap, winner.Name, rounds))
end

task.spawn(function()
	while rounds < 2 do
		round()
	end
	status.Value = "Game over"
	print("rounds: done after " .. rounds)
end)

status.Changed:Connect(function(value)
	if value:match("^Intermission") then
		print("rounds: " .. value)
	end
end)

local ok = pcall(function()
	Config.RoundTime = 99
end)
print("rounds: config frozen " .. tostring(not ok))

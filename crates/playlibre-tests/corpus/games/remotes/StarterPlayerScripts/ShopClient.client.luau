-- Client side: buys things, reacts to notifications.
local Players = game:GetService("Players")
local Net = require(game:GetService("ReplicatedStorage"):WaitForChild("Net"))

local player = Players.LocalPlayer
local notify = Net.event("Notify")
local seen = 0
notify.OnClientEvent:Connect(function(text)
	seen += 1
	print("client: " .. text)
end)

task.wait(0.5)
local buy = Net.func("BuyItem")
print("client: bought sword", buy:InvokeServer("Sword"))
print("client: bought shield", buy:InvokeServer("Shield"))
local ok, why = buy:InvokeServer("Sword")
print("client: third purchase", ok, why)
local bad = buy:InvokeServer({ "not a string" })
print("client: bad request refused", bad == false)
Net.event("Click"):FireServer({ position = Vector3.new(1, 7, 3) })
task.wait(0.5)
print("client: " .. player.Name .. " saw " .. seen .. " notes")

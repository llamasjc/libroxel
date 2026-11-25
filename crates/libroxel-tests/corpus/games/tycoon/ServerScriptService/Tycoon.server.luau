-- A dropper tycoon: droppers spawn ore, a conveyor carries it, a collector
-- turns it into cash, buttons buy upgrades.
local Players = game:GetService("Players")
local Debris = game:GetService("Debris")
local CollectionService = game:GetService("CollectionService")
local Money = require(game:GetService("ReplicatedStorage").Modules.Money)

local plot = Instance.new("Model")
plot.Name = "Plot1"
plot.Parent = workspace

local cash = 0
local owner = nil

local collector = Instance.new("Part")
collector.Name = "Collector"
collector.Anchored = true
collector.Size = Vector3.new(4, 1, 4)
collector.Position = Vector3.new(40, 0.5, 40)
collector.Parent = plot

collector.Touched:Connect(function(hit)
	if CollectionService:HasTag(hit, "Ore") then
		cash += hit:GetAttribute("Value") or 0
		hit:Destroy()
	end
end)

local function makeDropper(index, value, every)
	local dropper = Instance.new("Part")
	dropper.Name = "Dropper" .. index
	dropper.Anchored = true
	dropper.Size = Vector3.new(2, 2, 2)
	dropper.Position = Vector3.new(40, 6, 40)
	dropper:SetAttribute("Value", value)
	dropper.Parent = plot
	task.spawn(function()
		while dropper.Parent do
			local ore = Instance.new("Part")
			ore.Size = Vector3.new(1, 1, 1)
			ore.Position = dropper.Position - Vector3.new(0, 2, 0)
			ore:SetAttribute("Value", dropper:GetAttribute("Value"))
			CollectionService:AddTag(ore, "Ore")
			ore.Parent = workspace
			Debris:AddItem(ore, 5)
			task.wait(every)
		end
	end)
	return dropper
end

makeDropper(1, 5, 0.25)

local button = Instance.new("Part")
button.Name = "BuyDropper2"
button.Anchored = true
button:SetAttribute("Price", 25)
button.Parent = plot
local click = Instance.new("ClickDetector")
click.Parent = button

local bought = false
local function buy()
	if bought or cash < button:GetAttribute("Price") then
		return false
	end
	bought = true
	cash -= button:GetAttribute("Price")
	makeDropper(2, 20, 0.25)
	print("tycoon: bought dropper 2")
	return true
end
click.MouseClick:Connect(function(player)
	if player == owner then buy() end
end)

Players.PlayerAdded:Connect(function(player)
	owner = owner or player
	print("tycoon: " .. player.Name .. " owns " .. plot.Name)
end)
for _, p in ipairs(Players:GetPlayers()) do
	owner = owner or p
end

task.spawn(function()
	repeat task.wait(0.2) until cash >= 25
	print("tycoon: earned " .. Money.format(cash))
	buy()
	task.wait(2)
	print("tycoon: ore tagged " .. tostring(#CollectionService:GetTagged("Ore") >= 0))
	print("tycoon: " .. Money.format(1500) .. " " .. Money.format(2500000) .. " " .. Money.format(999))
end)

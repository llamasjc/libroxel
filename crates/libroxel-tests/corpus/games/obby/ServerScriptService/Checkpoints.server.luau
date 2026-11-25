-- A typical obby: numbered checkpoint pads, kill bricks and a stage counter.
local Players = game:GetService("Players")

local checkpoints = Instance.new("Folder")
checkpoints.Name = "Checkpoints"
checkpoints.Parent = workspace

for i = 1, 5 do
	local pad = Instance.new("Part")
	pad.Name = tostring(i)
	pad.Anchored = true
	pad.Size = Vector3.new(6, 1, 6)
	pad.Position = Vector3.new(i * 12, 0.5 + i * 2, 0)
	pad.BrickColor = BrickColor.new("Bright green")
	pad.Parent = checkpoints
end

local lava = Instance.new("Part")
lava.Name = "KillBrick"
lava.Anchored = true
lava.Size = Vector3.new(4, 1, 4)
lava.Position = Vector3.new(6, 0.5, 20)
lava.Color = Color3.fromRGB(255, 0, 0)
lava.Material = Enum.Material.Neon
lava.Parent = workspace

lava.Touched:Connect(function(hit)
	local humanoid = hit.Parent and hit.Parent:FindFirstChildOfClass("Humanoid")
	if humanoid then
		humanoid.Health = 0
	end
end)

local function onPlayerAdded(player)
	local stats = Instance.new("Folder")
	stats.Name = "leaderstats"
	stats.Parent = player
	local stage = Instance.new("IntValue")
	stage.Name = "Stage"
	stage.Value = 1
	stage.Parent = stats

	player.CharacterAdded:Connect(function(character)
		local root = character:WaitForChild("HumanoidRootPart")
		local pad = checkpoints:FindFirstChild(tostring(stage.Value))
		if pad then
			root.CFrame = pad.CFrame + Vector3.new(0, 4, 0)
		end
		print("obby: spawned at stage " .. stage.Value)
	end)
end

for _, player in ipairs(Players:GetPlayers()) do
	onPlayerAdded(player)
end
Players.PlayerAdded:Connect(onPlayerAdded)

for _, pad in ipairs(checkpoints:GetChildren()) do
	pad.Touched:Connect(function(hit)
		local player = Players:GetPlayerFromCharacter(hit.Parent)
		if not player then return end
		local stage = player.leaderstats.Stage
		local n = tonumber(pad.Name)
		if n and n == stage.Value + 1 then
			stage.Value = n
			print("obby: reached stage " .. n)
		end
	end)
end

print("obby: " .. #checkpoints:GetChildren() .. " checkpoints ready")

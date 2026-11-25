-- A classic sword tool: handle, Activated swing, damage on touch with a
-- debounce, and a dummy to hit.
local Players = game:GetService("Players")

local dummy = Instance.new("Model")
dummy.Name = "Dummy"
local torso = Instance.new("Part")
torso.Name = "HumanoidRootPart"
torso.Anchored = true
torso.Size = Vector3.new(2, 2, 1)
torso.Position = Vector3.new(0, 3, 8)
torso.Parent = dummy
local dummyHumanoid = Instance.new("Humanoid")
dummyHumanoid.Parent = dummy
dummy.Parent = workspace

local tool = Instance.new("Tool")
tool.Name = "Sword"
tool.RequiresHandle = true
tool.CanBeDropped = false
local handle = Instance.new("Part")
handle.Name = "Handle"
handle.Size = Vector3.new(1, 0.8, 4)
handle.Parent = tool

local DAMAGE = 25
local swinging = false
local hitList = {}

tool.Activated:Connect(function()
	if swinging then return end
	swinging = true
	hitList = {}
	tool.Grip = CFrame.Angles(math.rad(-90), 0, 0)
	task.wait(0.3)
	tool.Grip = CFrame.new()
	swinging = false
end)

local function hit(part)
	local humanoid = part.Parent and part.Parent:FindFirstChildOfClass("Humanoid")
	if not humanoid or hitList[humanoid] then return end
	local character = tool.Parent
	if humanoid.Parent == character then return end
	hitList[humanoid] = true
	humanoid:TakeDamage(DAMAGE)
	print("sword: hit " .. humanoid.Parent.Name .. ", health " .. humanoid.Health)
end
handle.Touched:Connect(hit)

tool.Equipped:Connect(function()
	print("sword: equipped by " .. tostring(tool.Parent))
end)

local function give(player)
	local backpack = player:WaitForChild("Backpack")
	tool:Clone().Parent = backpack
	print("sword: given to " .. player.Name .. ", backpack has " .. #backpack:GetChildren())
end
Players.PlayerAdded:Connect(give)
for _, p in ipairs(Players:GetPlayers()) do
	give(p)
end

-- The swing's damage path, run directly so the result doesn't depend on
-- where the character happens to stand.
tool.Parent = workspace
hit(torso)
hit(torso)
print("sword: debounce kept health at " .. dummyHumanoid.Health)

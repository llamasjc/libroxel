-- Sliding doors opened with a ProximityPrompt and TweenService.
local TweenService = game:GetService("TweenService")

local door = Instance.new("Part")
door.Name = "Door"
door.Anchored = true
door.Size = Vector3.new(6, 8, 1)
door.Position = Vector3.new(0, 4, -20)
door.Parent = workspace

local prompt = Instance.new("ProximityPrompt")
prompt.ActionText = "Open"
prompt.ObjectText = "Door"
prompt.HoldDuration = 0
prompt.MaxActivationDistance = 10
prompt.Parent = door

local closed = door.CFrame
local open = closed * CFrame.new(6, 0, 0)
local info = TweenInfo.new(0.5, Enum.EasingStyle.Quad, Enum.EasingDirection.Out)
local isOpen = false

local function toggle()
	isOpen = not isOpen
	local tween = TweenService:Create(door, info, { CFrame = isOpen and open or closed })
	tween:Play()
	tween.Completed:Wait()
	prompt.ActionText = isOpen and "Close" or "Open"
	print(string.format("doors: %s at x=%.1f", isOpen and "open" or "closed", door.Position.X))
end
prompt.Triggered:Connect(toggle)

-- A light that fades on a loop, with a NumberValue tween as well.
local glow = Instance.new("NumberValue")
glow.Value = 0
local light = Instance.new("PointLight")
light.Parent = door
glow.Changed:Connect(function(v)
	light.Brightness = v
end)
local pulse = TweenService:Create(glow, TweenInfo.new(0.4), { Value = 2 })
pulse:Play()
pulse.Completed:Connect(function(state)
	print("doors: light tween " .. tostring(state) .. " brightness " .. light.Brightness)
end)

task.wait(0.2)
toggle()
task.wait(0.2)
toggle()

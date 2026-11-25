-- A sprint script in StarterCharacterScripts: shift to run, stamina drain.
local UserInputService = game:GetService("UserInputService")
local RunService = game:GetService("RunService")

local character = script.Parent
local humanoid = character:WaitForChild("Humanoid")
local NORMAL, FAST = 16, 28
local stamina = 100
local sprinting = false

UserInputService.InputBegan:Connect(function(input, processed)
	if processed then return end
	if input.KeyCode == Enum.KeyCode.LeftShift then
		sprinting = true
	end
end)
UserInputService.InputEnded:Connect(function(input)
	if input.KeyCode == Enum.KeyCode.LeftShift then
		sprinting = false
	end
end)

local function setSprint(on)
	sprinting = on
end

local lowWarned = false
RunService.Heartbeat:Connect(function(dt)
	if sprinting and stamina > 0 then
		stamina = math.max(0, stamina - 50 * dt)
		humanoid.WalkSpeed = stamina > 0 and FAST or NORMAL
	else
		stamina = math.min(100, stamina + 20 * dt)
		humanoid.WalkSpeed = NORMAL
	end
	if stamina == 0 and not lowWarned then
		lowWarned = true
		print("sprint: out of stamina, speed " .. humanoid.WalkSpeed)
	end
end)

print("sprint: ready on " .. character.Name .. " speed " .. humanoid.WalkSpeed)
setSprint(true)
task.wait(0.2)
print("sprint: running at " .. humanoid.WalkSpeed)
task.wait(2.5)
setSprint(false)

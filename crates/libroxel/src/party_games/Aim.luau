local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local me = Players.LocalPlayer
local mouse = me:GetMouse()
local aim = ReplicatedStorage:WaitForChild("Aim")
local sent = nil

local function send()
	local character = me.Character
	if not character or not character:FindFirstChild("Blaster") then
		return
	end
	local at = mouse.Hit.Position
	if not sent or (at - sent).Magnitude > 0.25 then
		sent = at
		aim:FireServer(at)
	end
end

mouse.Button1Down:Connect(send)
while true do
	send()
	task.wait(0.05)
end

-- A shop screen built in code: frame, list layout, buttons, hover effects.
local Players = game:GetService("Players")
local Items = require(game:GetService("ReplicatedStorage"):WaitForChild("Items"))

local player = Players.LocalPlayer
local gui = Instance.new("ScreenGui")
gui.Name = "ShopGui"
gui.ResetOnSpawn = false
gui.Parent = player:WaitForChild("PlayerGui")

local frame = Instance.new("Frame")
frame.Size = UDim2.new(0, 300, 0, 400)
frame.Position = UDim2.fromScale(0.5, 0.5)
frame.AnchorPoint = Vector2.new(0.5, 0.5)
frame.BackgroundColor3 = Color3.fromRGB(30, 30, 30)
frame.Parent = gui
local corner = Instance.new("UICorner")
corner.CornerRadius = UDim.new(0, 8)
corner.Parent = frame
local list = Instance.new("UIListLayout")
list.Padding = UDim.new(0, 6)
list.SortOrder = Enum.SortOrder.LayoutOrder
list.Parent = frame

local balance = 150
local title = Instance.new("TextLabel")
title.Size = UDim2.new(1, 0, 0, 40)
title.Text = "Coins: " .. balance
title.TextScaled = true
title.Font = Enum.Font.GothamBold
title.LayoutOrder = 0
title.Parent = frame

local buttons = {}
for i, item in ipairs(Items) do
	local button = Instance.new("TextButton")
	button.Name = item.id
	button.Size = UDim2.new(1, -10, 0, 50)
	button.LayoutOrder = i
	button.BackgroundColor3 = item.color
	button.Text = string.format("%s - %d", item.name, item.price)
	button.Parent = frame
	button.MouseEnter:Connect(function()
		button.BackgroundTransparency = 0.2
	end)
	button.MouseLeave:Connect(function()
		button.BackgroundTransparency = 0
	end)
	button.Activated:Connect(function()
		if balance >= item.price then
			balance -= item.price
			title.Text = "Coins: " .. balance
			print("shop gui: bought " .. item.name .. ", left " .. balance)
		else
			print("shop gui: can't afford " .. item.name)
		end
	end)
	buttons[item.id] = button
end

print("shop gui: " .. #frame:GetChildren() .. " children, title " .. title.Text)
task.wait(0.3)
-- What a click does, without a mouse: the same handlers run.
for _, id in ipairs({ "speed", "boom", "gravity" }) do
	local b = buttons[id]
	if b.Text:find("Coil") or b.Text:find("Boombox") then
		local item
		for _, it in ipairs(Items) do
			if it.id == id then item = it end
		end
		if balance >= item.price then
			balance -= item.price
			title.Text = "Coins: " .. balance
			print("shop gui: bought " .. item.name .. ", left " .. balance)
		else
			print("shop gui: can't afford " .. item.name)
		end
	end
end
print("shop gui: final " .. title.Text)

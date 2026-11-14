-- Roblox-style conversations: a Dialog inside a part (an NPC's head, say)
-- gets a "Talk" prompt there. Talking shows the dialog's InitialPrompt and
-- its DialogChoices as buttons; picking one fires DialogChoiceSelected,
-- shows the choice's ResponseDialog and its own choices, and so on.
-- The goodbye button (GoodbyeDialog) ends the conversation.

local Players = game:GetService("Players")
local seen = setmetatable({}, { __mode = "k" })

local WIDTH = 440
local ROW = 34

local function is_dialog(obj)
	return obj.ClassName == "Dialog"
end

local function choices_of(node)
	local list = {}
	for _, c in ipairs(node:GetChildren()) do
		if c.ClassName == "DialogChoice" then
			table.insert(list, c)
		end
	end
	return list
end

local function label(parent, text, y, height, colour)
	local l = Instance.new("TextLabel")
	l.BackgroundTransparency = 1
	l.Position = UDim2.new(0, 14, 0, y)
	l.Size = UDim2.new(0, WIDTH - 28, 0, height)
	l.Text = text
	l.TextColor3 = colour or Color3.new(0.1, 0.1, 0.12)
	l.TextSize = 18
	l.TextWrapped = true
	l.TextXAlignment = Enum.TextXAlignment.Left
	l.Parent = parent
	return l
end

local function button(parent, text, y, pressed)
	local b = Instance.new("TextButton")
	b.Position = UDim2.new(0, 14, 0, y)
	b.Size = UDim2.new(0, WIDTH - 28, 0, ROW - 6)
	b.BackgroundColor3 = Color3.fromRGB(236, 238, 242)
	b.Text = text
	b.TextColor3 = Color3.fromRGB(20, 60, 140)
	b.TextSize = 17
	b.TextXAlignment = Enum.TextXAlignment.Left
	b.Parent = parent
	b.MouseButton1Click:Connect(pressed)
	return b
end

local function lines(text)
	-- Roughly how many lines the prompt wraps to.
	return math.max(1, math.ceil(#text / 42))
end

local open = {} -- player -> { gui, dialog }

local function finish(player)
	local current = open[player]
	if current then
		open[player] = nil
		current.gui:Destroy()
		pcall(function()
			current.dialog.InUse = false
		end)
	end
end

local function show(dialog, player, node)
	finish(player)
	local gui_root = player:FindFirstChildOfClass("PlayerGui")
	if not gui_root then
		return
	end
	local text = node == dialog and dialog.InitialPrompt or node.ResponseDialog
	local choices = choices_of(node)
	local goodbye = (node == dialog and dialog.GoodbyeDialog) or node.GoodbyeDialog
	if goodbye == nil or goodbye == "" then
		goodbye = "Goodbye!"
	end
	local with_goodbye = node == dialog or node.GoodbyeChoiceActive ~= false
	if #choices == 0 then
		with_goodbye = true
	end

	local gui = Instance.new("ScreenGui")
	gui.Name = "Dialog"
	gui.ResetOnSpawn = false
	local text_height = lines(text) * 22 + 8
	local rows = #choices + (with_goodbye and 1 or 0)
	local height = 14 + text_height + 8 + rows * ROW + 8
	local frame = Instance.new("Frame")
	frame.AnchorPoint = Vector2.new(0.5, 1)
	frame.Position = UDim2.new(0.5, 0, 1, -24)
	frame.Size = UDim2.new(0, WIDTH, 0, height)
	frame.BackgroundColor3 = Color3.fromRGB(252, 252, 250)
	frame.Parent = gui
	label(frame, text, 14, text_height)
	local y = 14 + text_height + 8
	for _, choice in ipairs(choices) do
		button(frame, choice.UserDialog, y, function()
			pcall(function()
				dialog.DialogChoiceSelected:Fire(player, choice)
			end)
			if choice.ResponseDialog ~= "" or #choices_of(choice) > 0 then
				show(dialog, player, choice)
			else
				finish(player)
			end
		end)
		y += ROW
	end
	if with_goodbye then
		button(frame, goodbye, y, function()
			finish(player)
		end)
	end
	gui.Parent = gui_root
	open[player] = { gui = gui, dialog = dialog }
	pcall(function()
		dialog.InUse = true
	end)
end

local function set_up(dialog)
	if seen[dialog] then
		return
	end
	seen[dialog] = true
	local part = dialog.Parent
	if not (part and part:IsA("BasePart")) then
		return
	end
	local prompt = Instance.new("ProximityPrompt")
	prompt.Name = "DialogPrompt"
	prompt.ActionText = "Talk"
	prompt.ObjectText = part.Parent and part.Parent:IsA("Model") and part.Parent.Name or part.Name
	prompt.HoldDuration = 0
	pcall(function()
		prompt.MaxActivationDistance = dialog.ConversationDistance
	end)
	prompt.RequiresLineOfSight = false
	prompt.Archivable = false
	prompt.Triggered:Connect(function(player)
		if open[player] and open[player].dialog == dialog then
			finish(player)
		else
			show(dialog, player, dialog)
		end
	end)
	prompt.Parent = part
	-- Walking away ends the conversation.
	task.spawn(function()
		while dialog.Parent do
			task.wait(0.5)
			for player, current in pairs(open) do
				if current.dialog == dialog then
					local character = player.Character
					local root = character and character:FindFirstChild("HumanoidRootPart")
					local reach = (dialog.ConversationDistance or 25) + 5
					if not root or (root.Position - part.Position).Magnitude > reach then
						finish(player)
					end
				end
			end
		end
	end)
end

for _, d in ipairs(workspace:GetDescendants()) do
	if is_dialog(d) then
		set_up(d)
	end
end
workspace.DescendantAdded:Connect(function(d)
	if is_dialog(d) then
		set_up(d)
	end
end)
Players.PlayerRemoving:Connect(finish)

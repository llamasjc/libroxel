--!strict
-- The Luau that shows up in real game code: classes and inheritance,
-- typed functions, string patterns, table helpers, coroutines, error
-- objects and task scheduling.
type Item = { name: string, rarity: number }

local Base = {}
Base.__index = Base
function Base.new(name: string)
	local self = setmetatable({}, Base)
	self.name = name
	self.items = {} :: { Item }
	return self
end
function Base:add(item: Item)
	table.insert(self.items, item)
	return self
end
function Base:describe(): string
	return self.name .. " (" .. #self.items .. ")"
end

local Chest = setmetatable({}, { __index = Base })
Chest.__index = Chest
function Chest.new(name: string, locked: boolean)
	local self = Base.new(name)
	setmetatable(self, Chest)
	self.locked = locked
	return self
end
function Chest:describe(): string
	return (self.locked and "locked " or "") .. Base.describe(self)
end

local chest = Chest.new("Loot", true)
chest:add({ name = "Gem", rarity = 3 }):add({ name = "Coin", rarity = 1 }):add({ name = "Crown", rarity = 5 })
table.sort(chest.items, function(a, b) return a.rarity > b.rarity end)
local names = {}
for _, item in ipairs(chest.items) do table.insert(names, item.name) end
print("luau: " .. chest:describe() .. ": " .. table.concat(names, ","))

-- String handling: patterns, gsub with a function, format, split.
local message = "  /give Player1 Sword 3  "
local cmd, target, item, count = message:match("^%s*/(%w+)%s+(%w+)%s+(%w+)%s+(%d+)%s*$")
print("luau: command " .. cmd .. " " .. target .. " " .. item .. " x" .. tonumber(count))
local parts = string.split("a,b,,c", ",")
print("luau: split into " .. #parts)
local shout = ("hello world"):gsub("(%w+)", function(w) return w:upper() end)
print("luau: " .. shout)
print(string.format("luau: %05.1f|%-4s|%x|%q", 3.14159, "ab", 255, "hi"))
print("luau: utf8 " .. utf8.len("héllo") .. " " .. #"héllo")

-- Table library.
local t = table.create(3, 0)
t[2] = 7
print("luau: find " .. tostring(table.find(t, 7)) .. ", clone " .. #table.clone(t))
local packed = table.pack(1, nil, 3)
print("luau: packed n=" .. packed.n)

-- Coroutines and generators.
local function range(n)
	return coroutine.wrap(function()
		for i = 1, n do coroutine.yield(i) end
	end)
end
local sum = 0
for i in range(10) do sum += i end
print("luau: sum " .. sum)

-- Errors: tables as errors, xpcall with traceback, error levels.
local ok, err = pcall(function()
	error({ code = 42 })
end)
print("luau: error table code " .. (not ok and err.code or 0))
local ok2, trace = xpcall(function()
	local x = nil
	return x.field
end, debug.traceback)
print("luau: xpcall caught " .. tostring(not ok2) .. " " .. tostring(type(trace) == "string"))

-- Scheduling order: defer runs after the current thread, delay after time.
local order = {}
task.defer(function() table.insert(order, "defer") end)
task.delay(0.1, function() table.insert(order, "delay") end)
task.spawn(function() table.insert(order, "spawn") end)
table.insert(order, "main")
task.wait(0.3)
print("luau: order " .. table.concat(order, ","))

-- Bit ops, math, buffers.
print("luau: bits " .. bit32.band(0xF0, 0x3C) .. " " .. bit32.lshift(1, 4))
print("luau: clamp " .. math.clamp(15, 0, 10) .. " round " .. math.round(2.5) .. " sign " .. math.sign(-3))
local b = buffer.create(8)
buffer.writeu32(b, 0, 123456)
print("luau: buffer " .. buffer.readu32(b, 0))
print("luau: date " .. tostring(os.date("!%Y", 0)) .. " " .. typeof(os.time()))

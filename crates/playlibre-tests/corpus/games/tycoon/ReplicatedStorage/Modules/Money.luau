-- Number formatting every tycoon has: 1500 -> "1.5K".
local Money = {}

local SUFFIXES = { "", "K", "M", "B", "T" }

function Money.format(n)
	local i = 1
	while math.abs(n) >= 1000 and i < #SUFFIXES do
		n /= 1000
		i += 1
	end
	if i == 1 then
		return tostring(math.floor(n))
	end
	local text = string.format("%.1f", n):gsub("%.0$", "")
	return text .. SUFFIXES[i]
end

return Money

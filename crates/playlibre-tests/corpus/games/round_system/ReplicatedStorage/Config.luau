-- Shared settings in a ModuleScript, frozen so nothing changes them.
local Config = {
	IntermissionTime = 1,
	RoundTime = 2,
	MinPlayers = 1,
	Maps = { "Desert", "Forest", "Castle" },
}
return table.freeze(Config)

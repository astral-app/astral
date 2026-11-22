local reaction = nil
local reactors = {
	["m1"] = { { id = "u1", avatar = "abc" }, { id = "263689920210534400" } },
}

astral = {
	log = function() end,
	i18n = {
		t = function(key)
			return key
		end,
	},
	settings = { switch = function() end, choice = function() end, number = function() end },
	ui = {
		decorate = function() end,
		relabel = function() end,
		reaction = function(fn)
			reaction = fn
		end,
	},
	discord = {
		permission = {},
		reactors = function(_, message)
			return reactors[message]
		end,
	},
}

dofile(arg[2])
dofile(arg[1])

local shown = reaction({ channel_id = "c1", message_id = "m1", emoji = "🔥", count = 7 })
assert(#shown.images == 2)
assert(shown.images[1] == "https://cdn.discordapp.com/avatars/u1/abc.webp?size=48")
assert(shown.images[2] == "https://cdn.discordapp.com/embed/avatars/" .. ((263689920210534400 >> 22) % 6) .. ".png")
assert(shown.total == 7)
assert(reaction({ channel_id = "c1", message_id = "m2", emoji = "🔥", count = 1 }) == nil)

print("reaction-avatars ok")

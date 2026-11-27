local decorator, pressed, tooltip
local now = 1795600000000
local members = {}
astral = {
	log = function() end,
	settings = {
		switch = function(_, _, default)
			return default
		end,
	},
	i18n = {
		t = function(key)
			return key
		end,
	},
	time = {
		now_ms = function()
			return now
		end,
	},
	ui = {
		decorate = function(_, fn, press)
			decorator, pressed = fn, press
		end,
		tooltip = function(text)
			tooltip = text
		end,
	},
	discord = {
		permission = {},
		member = function(_, user)
			return members[user]
		end,
		guild = function()
			return { roles = { iconic = { icon = "abc" } } }
		end,
		message = function()
			return nil
		end,
	},
}
dofile(arg[2])
dofile(arg[1])

local old_guild = "709061970078335027"
local function iso(ms)
	return os.date("!%Y-%m-%dT%H:%M:%S", ms // 1000) .. ".000000+00:00"
end

members.fresh = { roles = {}, joined_at = iso(now - 2 * 24 * 3600 * 1000) }
members.old = { roles = {}, joined_at = iso(now - 9 * 24 * 3600 * 1000) }
members.rejoined = { roles = {}, joined_at = iso(now - 24 * 3600 * 1000), flags = 1 }
members.offset = { roles = {}, joined_at = "2026-11-23T12:00:00.000000+02:00" }

local sprout = decorator({ user_id = "fresh", guild_id = old_guild })
assert(sprout and sprout[1].icon == "sprout.png")
assert(decorator({ user_id = "old", guild_id = old_guild }) == nil)
assert(decorator({ user_id = "rejoined", guild_id = old_guild }) == nil)
assert(decorator({ user_id = "fresh" }) == nil)
local new_guild = tostring((now - 1420070400000) << 22)
assert(decorator({ user_id = "fresh", guild_id = new_guild }) == nil)
assert(decorator({ user_id = "offset", guild_id = old_guild }) ~= nil)
assert(sprout[1].action == "sprout")
pressed({ user_id = "fresh" }, "sprout")
assert(tooltip == "NEW_HERE")
members.decorated = { roles = { "iconic" }, joined_at = iso(now - 2 * 24 * 3600 * 1000) }
assert(decorator({ user_id = "decorated", guild_id = old_guild }) == nil)
print("new-member-sprout ok")

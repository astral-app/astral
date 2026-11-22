local decorator, stored = nil, {}
astral = {
	log = function() end,
	settings = {
		switch = function(key, _, default)
			if stored[key] == nil then
				return default
			end
			return stored[key]
		end,
	},
	ui = {
		decorate = function(_, fn)
			decorator = fn
		end,
	},
	discord = {
		permission = {
			KICK_MEMBERS = 2,
			BAN_MEMBERS = 4,
			ADMINISTRATOR = 8,
			MANAGE_CHANNELS = 16,
			MANAGE_GUILD = 32,
			MANAGE_MESSAGES = 8192,
			MUTE_MEMBERS = 1 << 22,
			DEAFEN_MEMBERS = 1 << 23,
			MOVE_MEMBERS = 1 << 24,
			MANAGE_ROLES = 1 << 28,
			MANAGE_WEBHOOKS = 1 << 29,
			MODERATE_MEMBERS = 1 << 40,
		},
		guild = function()
			return {
				id = "g",
				owner_id = "owner",
				roles = {
					r1 = { name = "Dev", position = 2, colors = { primary_color = 0x2D7FD0 } },
					r2 = {
						name = "Holo",
						position = 3,
						colors = { primary_color = 0xA9C9FF, secondary_color = 0xFFBBEC, tertiary_color = 0xFFC3A0 },
					},
				},
			}
		end,
		member = function(_, user)
			if user == "owner" then
				return { roles = { "r1" } }
			end
			return { roles = { "r2" } }
		end,
		permissions = function(_, user)
			if user == "mod" then
				return 8192
			end
			return 0
		end,
		message = function()
			return { author = { bot = false } }
		end,
	},
}
dofile(arg[1])
local function node(user)
	return { user_id = user, guild_id = "g", channel_id = "c", message_id = "m" }
end

local function expect(label, pills, wanted)
	pills = pills or {}
	assert(#pills == #wanted, label .. ": expected " .. #wanted .. " pills, got " .. #pills)
	for index, want in ipairs(wanted) do
		local pill = pills[index]
		assert(pill.text == want.text, label .. ": text " .. tostring(pill.text))
		assert(pill.color == want.color, label .. ": colour " .. string.format("%08X", pill.color))
		assert(pill.shape == want.shape, label .. ": shape " .. tostring(pill.shape))
		assert((pill.colors and #pill.colors or 0) == (want.stops or 0), label .. ": gradient stops")
	end
end

expect("owner", decorator(node("owner")), { { text = "Owner", color = 0xFF2D7FD0, shape = "pill" } })
expect("mod", decorator(node("mod")), { { text = "Mod", color = 0xFFA9C9FF, shape = "pill", stops = 3 } })
expect("member", decorator(node("member")), {})
astral.on_setting("showRoleName", true)
expect("owner with role", decorator(node("owner")), {
	{ text = "Owner", color = 0xFF2D7FD0, shape = "pill" },
	{ text = "Dev", color = 0xFF2D7FD0, shape = "pill" },
})
astral.on_setting("useOpTagStyle", true)
expect("mod as original poster", decorator(node("mod")), {
	{ text = "Mod", color = 0xFFA9C9FF, shape = "op", stops = 3 },
	{ text = "Holo", color = 0xFFA9C9FF, shape = "pill", stops = 3 },
})
astral.on_setting("useRoleColor", false)
expect("tag colour without role colour", decorator(node("mod")), {
	{ text = "Mod", color = 0xFF4E7FFF, shape = "op" },
	{ text = "Holo", color = 0xFFA9C9FF, shape = "pill", stops = 3 },
})
print("more-user-tags ok")

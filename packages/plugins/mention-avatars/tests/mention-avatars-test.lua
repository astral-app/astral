local mention = nil
local messages = {
	["c:m1"] = {
		mentions = {
			{ id = "224617799434108928", username = "adrian", global_name = "심심한 개발자", avatar = "ae6071" },
			{ id = "263689920210534400", username = "eternal" },
		},
		mention_roles = { "r1" },
	},
}
local guilds = { g = { roles = { r1 = { id = "r1", name = "Mods", icon = "i1" } } } }

astral = {
	log = function() end,
	i18n = {
		t = function(key)
			return key
		end,
	},
	settings = {
		switch = function(_, _, default)
			return default
		end,
		choice = function() end,
		number = function() end,
	},
	ui = {
		decorate = function() end,
		relabel = function() end,
		mention = function(fn)
			mention = fn
		end,
	},
	discord = {
		permission = {},
		message = function(channel, id)
			return messages[channel .. ":" .. id]
		end,
		guild = function(id)
			return guilds[id]
		end,
	},
}

dofile(arg[2])
dofile(arg[1])

local function call(text)
	return mention({ text = text, message_id = "m1", channel_id = "c", guild_id = "g", index = 1 })
end

local display = call("@심심한 개발자")
assert(display.image == "https://cdn.discordapp.com/avatars/224617799434108928/ae6071.webp?size=64")
assert(display.round == true and display.hide_at == false)

local fallback = call("@eternal")
assert(fallback.image == "https://cdn.discordapp.com/embed/avatars/" .. ((263689920210534400 >> 22) % 6) .. ".png")

local role = call("@Mods")
assert(role.image == "https://cdn.discordapp.com/role-icons/r1/i1.png?size=64" and role.round == false)

assert(call("@nobody") == nil)

astral.on_setting("showAtSymbol", false)
assert(call("@eternal").hide_at == true)

print("mention-avatars ok")

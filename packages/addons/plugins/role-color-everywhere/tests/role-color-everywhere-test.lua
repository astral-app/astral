local mention, colorize, names = nil, nil, nil
local messages = {
	["c:m1"] = {
		author = { id = "u1" },
		mentions = { { id = "u2", username = "rae", global_name = "Rae" } },
		mention_roles = { "r2" },
	},
}
local guilds = {
	g = {
		roles = {
			r1 = { id = "r1", name = "Low", color = 0x112233, position = 1 },
			r2 = { id = "r2", name = "Dev", color = 0x277ECD, position = 5 },
			r3 = { id = "r3", name = "Plain", color = 0, position = 9 },
		},
	},
}
local members = { ["g:u1"] = { roles = { "r1", "r2", "r3" } }, ["g:u2"] = { roles = { "r1" } } }

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
		number = function(_, _, options)
			return options.default
		end,
	},
	ui = {
		decorate = function() end,
		relabel = function() end,
		mention = function(fn)
			mention = fn
		end,
		message_color = function(fn)
			colorize = fn
		end,
		name_color = function(fn)
			names = fn
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
		member = function(guild, user)
			return members[guild .. ":" .. user]
		end,
	},
}

dofile(arg[2])
dofile(arg[1])

local function node(text)
	return { text = text, message_id = "m1", channel_id = "c", guild_id = "g", index = 1 }
end

assert(mention(node("@Rae")).color == 0x112233)
assert(mention(node("@Dev")).color == 0x277ECD)
assert(mention(node("@nobody")) == nil)
assert(colorize(node()) == nil)

astral.on_setting("colorChatMessages", true)
assert(colorize(node()) == ((77 << 24) | 0x277ECD))

astral.on_setting("chatMentions", false)
assert(mention(node("@Rae")) == nil)

assert(names({ kind = astral.NameColorKind.Header, text = "Dev \u{2014} 1", guild_id = "g" }) == 0x277ECD)
assert(names({ kind = astral.NameColorKind.Header, text = "Online \u{2014} 1", guild_id = "g" }) == nil)
assert(names({ kind = astral.NameColorKind.Avatar, text = "adrian", user_id = "u1", guild_id = "g" }) == 0x277ECD)
assert(names({ kind = astral.NameColorKind.Avatar, text = "nobody", user_id = "u9", guild_id = "g" }) == nil)

assert(names({ kind = astral.NameColorKind.Voter, text = "adrian", user_id = "u1", guild_id = "g" }) == 0x277ECD)
astral.on_setting("pollResults", false)
assert(names({ kind = astral.NameColorKind.Voter, text = "adrian", user_id = "u1", guild_id = "g" }) == nil)

assert(names({ kind = astral.NameColorKind.Typing, text = "adrian", user_id = "u1", guild_id = "g" }) == 0x277ECD)
astral.on_setting("typingUsers", false)
assert(names({ kind = astral.NameColorKind.Typing, text = "adrian", user_id = "u1", guild_id = "g" }) == nil)

print("role-color-everywhere ok")

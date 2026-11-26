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
		style_text = function(fn)
			local function as(kind)
				return function(node)
					node.kind = node.kind or kind
					return fn(node)
				end
			end
			mention = as("mention")
			colorize = function(node)
				local style = as("message")(node)
				return style and style.color
			end
			names = function(node)
				local style = fn(node)
				return style and style.color
			end
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

assert(names({ kind = astral.TextKind.Header, text = "Dev \u{2014} 1", guild_id = "g" }) == 0x277ECD)
assert(names({ kind = astral.TextKind.Header, text = "Online \u{2014} 1", guild_id = "g" }) == nil)
assert(names({ kind = astral.TextKind.Avatar, text = "adrian", user_id = "u1", guild_id = "g" }) == 0x277ECD)
assert(names({ kind = astral.TextKind.Avatar, text = "nobody", user_id = "u9", guild_id = "g" }) == nil)

assert(names({ kind = astral.TextKind.Voter, text = "adrian", user_id = "u1", guild_id = "g" }) == 0x277ECD)
astral.on_setting("pollResults", false)
assert(names({ kind = astral.TextKind.Voter, text = "adrian", user_id = "u1", guild_id = "g" }) == nil)

assert(names({ kind = astral.TextKind.Typing, text = "adrian", user_id = "u1", guild_id = "g" }) == 0x277ECD)
astral.on_setting("typingUsers", false)
assert(names({ kind = astral.TextKind.Typing, text = "adrian", user_id = "u1", guild_id = "g" }) == nil)

print("role-color-everywhere ok")

local build, press, opened
local messages = {
	["10"] = { content = "look https://discord.com/channels/1/2/3 and https://discord.com/channels/@me/4/5" },
}
local raw = {
	["3"] = {
		id = "3",
		content = "linked text <:wave:42> <@77>",
		author = {
			id = "77",
			global_name = "Ada",
			avatar = "abc",
			primary_guild = { identity_enabled = true, tag = "ADA", badge = "b1", identity_guild_id = "1" },
		},
		attachments = {
			{ content_type = "image/png", url = "https://cdn.discordapp.com/a.png", width = 480, height = 240 },
		},
	},
}
astral = {
	log = function() end,
	i18n = {
		t = function(key)
			return key
		end,
	},
	time = {
		iso = function(_, options)
			return options and options.local_time and "2026-11-25 16:32" or "2026-11-25 15:32 UTC"
		end,
		after = function() end,
	},
	snowflake = {
		timestamp = function()
			return 0
		end,
	},
	discord = {
		guild = function(id)
			return id == "1"
					and {
						id = "1",
						roles = {
							["5"] = { colors = { primary_color = 0x00FF00 }, position = 2 },
							["6"] = { colors = { primary_color = 0xFF0000 }, position = 1 },
						},
					}
				or nil
		end,
		member = function(guild, user)
			return guild == "1" and user == "77" and { roles = { "6", "5" } } or nil
		end,
		user = function(id)
			return id == "77" and { global_name = "Ada" } or nil
		end,
		message = function(_, id)
			return messages[id]
		end,
		raw_message = function(_, id)
			return raw[id]
		end,
	},
	ui = {
		insert = function(_, fn, on_press)
			build, press = fn, on_press
		end,
		open_channel = function(channel, message)
			opened = channel .. "/" .. message
		end,
	},
}
dofile(arg[1])
local cards = build({ message_id = "10", channel_id = "9" })
assert(#cards == 1 and cards[1].action == "2:3")
local avatar, column = cards[1].children[1], cards[1].children[2]
assert(avatar.icon == "https://cdn.discordapp.com/avatars/77/abc.png?size=80" and avatar.radius == 20)
local header = column.children[1].children
assert(header[1].text == "Ada" and header[1].color == 0xFF00FF00)
assert(header[2].children[2].text == "ADA")
assert(header[2].children[1].icon == "https://cdn.discordapp.com/guild-tag-badges/1/b1.png?size=32")
assert(header[3].text == "2026-11-25 16:32")
assert(column.children[2].text == "linked text <:wave:42> @Ada")
assert(cards[1].align == astral.Align.Start)
local image = column.children[3]
assert(image.type == astral.InsertNodeType.Image and image.width == 240 and image.height == 120)
press({}, cards[1].action)
assert(opened == "2/3")
assert(build({ message_id = "99", channel_id = "9" }) == nil)
print("message-link-embeds ok")

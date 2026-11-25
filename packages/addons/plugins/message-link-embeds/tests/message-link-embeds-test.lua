local build, press, opened
local messages = {
	["10"] = { content = "look https://discord.com/channels/1/2/3 and https://discord.com/channels/@me/4/5" },
	["3"] = { content = "linked text", author = { global_name = "Ada" } },
}
astral = {
	log = function() end,
	discord = {
		message = function(_, id)
			return messages[id]
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
assert(#cards == 1 and cards[1].children[2].children[1].text == "Ada" and cards[1].action == "2:3")
press({}, cards[1].action)
assert(opened == "2/3")
assert(build({ message_id = "99", channel_id = "9" }) == nil)
print("message-link-embeds ok")

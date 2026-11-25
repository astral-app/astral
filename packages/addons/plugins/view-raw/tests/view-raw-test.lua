local actions = {}
local sent, copied
astral = {
	log = function() end,
	settings = {},
	ui = {
		message_action = function(title, fn, options)
			actions[title] = { fn = fn, options = options }
		end,
		toast = function() end,
		copy = function(text)
			copied = text
		end,
	},
	messages = {
		send_local = function(channel, message)
			sent = { channel = channel, message = message }
		end,
	},
	json = {
		encode = function()
			return '{"id":"1"}'
		end,
	},
	discord = { permission = {} },
	i18n = {
		t = function(key)
			return key
		end,
	},
}
dofile(arg[1])
local node = {
	channel_id = "9",
	message = function()
		return { id = "1", content = "**bold** ```code```" }
	end,
}
actions["View Raw"].fn(node)
assert(
	sent.channel == "9"
		and sent.message.content:find("**bold**", 1, true)
		and sent.message.content:find('{"id":"1"}', 1, true)
)
assert(not sent.message.content:find("```code```", 1, true))
assert(actions["Copy Raw"].options.when(node))
actions["Copy Raw"].fn(node)
assert(copied == "**bold** ```code```")
print("view-raw ok")

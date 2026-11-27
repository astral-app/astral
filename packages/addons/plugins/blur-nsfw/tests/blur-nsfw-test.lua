local gateway
local intercepts = {}
local values = {}
local section = {}
section.switch = function() end
astral = {
	log = function() end,
	Event = { Gateway = "gateway" },
	settings = {
		get = function(key, fallback)
			if values[key] == nil then
				return fallback
			end
			return values[key]
		end,
		section = function()
			return section
		end,
	},
	events = {
		on = function(_, fn)
			gateway = fn
		end,
	},
	net = {
		intercept = function(definition)
			intercepts[#intercepts + 1] = definition
		end,
	},
	json = {
		decode = function(text)
			return load("return " .. text)()
		end,
		encode = function(value)
			return value
		end,
	},
	i18n = {
		t = function(key)
			return key
		end,
	},
}
dofile(arg[1])
gateway({
	type = "GUILD_CREATE",
	data = { channels = { { id = "1", nsfw = true }, { id = "2" } }, threads = { { id = "3", parent_id = "1" } } },
})
local message = { channel_id = "1", attachments = { { filename = "cat.png", flags = 2 } } }
assert(gateway({ type = "MESSAGE_CREATE", data = message }) == message)
assert(message.attachments[1].flags == 10 and message.attachments[1].filename == "SPOILER_cat.png")
assert(gateway({ type = "MESSAGE_CREATE", data = message }) == nil)
assert(
	gateway({ type = "MESSAGE_CREATE", data = { channel_id = "2", attachments = { { filename = "a.png" } } } }) == nil
)
local thread = { channel_id = "3", attachments = { { filename = "b.png" } } }
assert(gateway({ type = "MESSAGE_CREATE", data = thread }) == thread)
local history = intercepts[2].response(
	{ path = "/api/v9/channels/1/messages" },
	{ status = 200, body = '{{attachments = {{filename = "x.gif"}}}}' }
)
assert(history.body[1].attachments[1].filename == "SPOILER_x.gif")
gateway({ type = "CHANNEL_UPDATE", data = { id = "1", nsfw = false } })
assert(
	gateway({ type = "MESSAGE_CREATE", data = { channel_id = "1", attachments = { { filename = "c.png" } } } }) == nil
)
values.attachments = false
gateway({ type = "CHANNEL_UPDATE", data = { id = "1", nsfw = true } })
assert(
	gateway({ type = "MESSAGE_CREATE", data = { channel_id = "1", attachments = { { filename = "d.png" } } } }) == nil
)
print("blur-nsfw ok")

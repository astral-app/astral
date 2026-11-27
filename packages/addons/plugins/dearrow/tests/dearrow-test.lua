local gateway
local intercepts = {}
local actions = {}
local dispatched = {}
local requests = {}
local values = {}
local section = {}
section.choice = function() end
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
		option = function(value)
			return value
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
		request = function(options, done)
			requests[#requests + 1] = { options = options, done = done }
		end,
	},
	gateway = {
		dispatch = function(kind, data)
			dispatched[#dispatched + 1] = { kind = kind, data = data }
		end,
	},
	ui = {
		message_action = function(label, fn, options)
			actions[label] = { fn = fn, when = options.when }
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
local function message()
	return {
		id = "9",
		channel_id = "1",
		embeds = {
			{
				title = "YOU WON'T BELIEVE THIS",
				url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=1",
				provider = { name = "YouTube" },
				thumbnail = { url = "https://i.ytimg.com/vi/dQw4w9WgXcQ/hq.jpg" },
			},
		},
	}
end
assert(gateway({ type = "MESSAGE_CREATE", data = message() }) == nil)
assert(#requests == 1 and requests[1].options.url:find("videoID=dQw4w9WgXcQ", 1, true))
requests[1].done({
	status = 200,
	json = { titles = { { title = "A >calm title", votes = 2 } }, thumbnails = { { timestamp = 42.5, votes = 1 } } },
})
local update = dispatched[1]
assert(update.kind == "MESSAGE_UPDATE" and update.data.embeds[1].title == "A calm title")
assert(update.data.embeds[1].thumbnail.url:find("time=42.5", 1, true))
assert(gateway({ type = "MESSAGE_UPDATE", data = update.data }) == update.data and update.data.astral_dearrow == nil)
local live = message()
assert(gateway({ type = "MESSAGE_CREATE", data = live }) == live and live.embeds[1].title == "A calm title")
assert(actions.SHOW_ORIGINAL_EMBED.when({ message_id = "9" }))
actions.SHOW_ORIGINAL_EMBED.fn({ message_id = "9" })
assert(dispatched[2].data.embeds[1].title == "YOU WON'T BELIEVE THIS")
assert(actions.DEARROW_EMBED.when({ message_id = "9" }))
values.replace = "titles"
actions.DEARROW_EMBED.fn({ message_id = "9" })
assert(
	dispatched[3].data.embeds[1].title == "A calm title"
		and dispatched[3].data.embeds[1].thumbnail.url:find("ytimg", 1, true)
)
print("dearrow ok")

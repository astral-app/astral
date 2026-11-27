local gateway, transform
local values, stored, dispatched, actions = {}, {}, {}, {}
local cache = {
	["1"] = { id = "1", content = "hello", author = { id = "a" } },
	["2"] = { id = "2", content = "bot", author = { id = "b", bot = true } },
	["3"] = { id = "3", content = "first", author = { id = "a" } },
}
astral = {
	log = function() end,
	settings = {
		get = function(key, fallback)
			if values[key] == nil then
				return fallback
			end
			return values[key]
		end,
		section = function()
			return { switch = function() end }
		end,
	},
	storage = {
		get = function(key)
			return stored[key]
		end,
		set = function(key, value)
			stored[key] = value
		end,
	},
	json = {
		array = function()
			return {}
		end,
	},
	events = {
		on = function(_, fn)
			gateway = fn
		end,
	},
	discord = {
		message = function(_, id)
			return cache[id]
		end,
		raw_message = function(_, id)
			local message = cache[id]
			return message and { id = message.id, content = message.content, flags = 4 } or nil
		end,
	},
	gateway = {
		dispatch = function(kind, data)
			dispatched[#dispatched + 1] = { kind = kind, data = data }
			return true
		end,
	},
	ui = {
		text_transform = function(fn)
			transform = fn
		end,
		refresh = function() end,
		message_action = function(label, fn, options)
			actions[#actions + 1] = { label = label, fn = fn, when = options.when }
		end,
	},
}
dofile(arg[1])
assert(gateway({ type = "MESSAGE_DELETE", data = { channel_id = "c", id = "1" } }) == false)
assert(gateway({ type = "MESSAGE_DELETE", data = { channel_id = "c", id = "2" } }) == nil)
local deleted = transform({ { text = "hello" } }, { message_id = "1" })
assert(deleted[1].color == 0xFFF23F43 and deleted[2].text == "\nThis message was deleted")
assert(#dispatched == 0)
gateway({ type = "MESSAGE_UPDATE", data = { channel_id = "c", id = "3", content = "second" } })
local edited = transform({ { text = "second" } }, { message_id = "3" })
assert(edited[1].text == "first" and edited[1].strike and edited[3].text == "second")
local bulk = gateway({ type = "MESSAGE_DELETE_BULK", data = { channel_id = "c", ids = { "2", "3" } } })
assert(bulk.ids[1] == "2" and #bulk.ids == 1)
assert(stored.log and stored.log.deleted["1"])
assert(transform({ { text = "x" } }, { message_id = "9" }) == nil)
local dismiss = actions[1]
assert(
	dismiss.label == "Dismiss Message" and dismiss.when({ message_id = "1" }) and not dismiss.when({ message_id = "9" })
)
dismiss.fn({ message_id = "1", channel_id = "c" })
assert(dispatched[1].kind == "MESSAGE_DELETE" and dispatched[1].data.id == "1")
assert(gateway({ type = "MESSAGE_DELETE", data = { channel_id = "c", id = "1" } }) == nil)
assert(transform({ { text = "hello" } }, { message_id = "1" }) == nil)
gateway({ type = "MESSAGE_CREATE", data = { id = "50", channel_id = "c", content = "fresh", author = { id = "u" } } })
assert(gateway({ type = "MESSAGE_DELETE", data = { channel_id = "c", id = "50" } }) == false)
assert(transform({ { text = "fresh" } }, { message_id = "50" })[1].color == 0xFFF23F43)
print("message-logger ok")

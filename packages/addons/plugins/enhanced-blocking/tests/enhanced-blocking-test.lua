local callbacks = {}
local values = {}

astral = {
	settings = {
		values = values,
		section = function()
			return {
				text = function(_, key, _, fallback)
					values[key] = fallback
				end,
				switch = function(_, key, _, fallback)
					values[key] = fallback
				end,
			}
		end,
	},
	events = {
		on = function(event, callback)
			callbacks[event] = callback
			return 1
		end,
	},
	ui = {
		patch = function(selector, patcher)
			assert(selector == "message-row")
			callbacks.patch = patcher
		end,
	},
	discord = {
		relationship = function(id)
			return id == "345678901234567890" and "blocked" or nil
		end,
		message = function(_, message_id)
			if message_id == "m1" then
				return { author = { id = "234567890123456789" }, content = "loaded spoiler" }
			end
			if message_id == "m2" then
				return {
					author = { id = "234567890123456789" },
					content = "fine",
					reference = { channel_id = "c", message_id = "m3" },
				}
			end
			if message_id == "m3" then
				return { author = { id = "123456789012345678" }, content = "original" }
			end
			return { author = { id = "234567890123456789" }, content = "fine" }
		end,
	},
}

dofile(arg[1])

local filter = callbacks["message.filter"]
assert(filter)
values.users = " 123456789012345678\ninvalid\n"
values.keywords = "spoiler\nfree nitro"
assert(filter({ author_id = "123456789012345678", content = "hello" }))
assert(filter({ author_id = "234567890123456789", content = "A SPOILER ahead" }))
assert(filter({ author_id = "234567890123456789", content = "join free nitro now" }))
assert(not filter({ author_id = "234567890123456789", content = "ordinary message" }))
assert(filter({
	author_id = "234567890123456789",
	content = "ordinary message",
	referenced_author_id = "123456789012345678",
}))
values.replies = false
assert(not filter({
	author_id = "234567890123456789",
	content = "ordinary message",
	referenced_author_id = "123456789012345678",
}))
assert(not filter({ from_me = true, author_id = "123456789012345678", content = "spoiler" }))
assert(filter({ author_id = "345678901234567890", content = "hello" }))
values.blocked = false
assert(not filter({ author_id = "345678901234567890", content = "hello" }))
assert(filter({ author_id = "234567890123456789", content = "edited spoiler", edited = true }))

local patch = callbacks.patch
assert(patch)
assert(patch({ message_id = "m1", channel_id = "c" }).hidden)
assert(patch({ message_id = "m4", channel_id = "c", author_id = "123456789012345678" }).hidden)
assert(patch({ message_id = "m4", channel_id = "c" }) == nil)
assert(patch({ message_id = "m2", channel_id = "c" }) == nil)
values.replies = true
assert(patch({ message_id = "m2", channel_id = "c" }).hidden)
assert(patch({ channel_id = "c" }) == nil)

print("enhanced-blocking ok")

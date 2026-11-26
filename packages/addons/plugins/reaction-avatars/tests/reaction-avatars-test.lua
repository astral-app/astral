local reaction = nil
local requests, refreshes, timers = {}, 0, {}
local now = 0

astral = {
	log = function() end,
	i18n = {
		t = function(key)
			return key
		end,
	},
	settings = { switch = function() end, choice = function() end, number = function() end },
	json = {
		decode = function(text)
			local list = {}
			for id, avatar in text:gmatch('{"id":"(%d+)","avatar":"?([%w]*)"?}') do
				list[#list + 1] = { id = id, avatar = avatar ~= "null" and avatar ~= "" and avatar or nil }
			end
			local retry = text:match('"retry_after":([%d.]+)')
			if retry then
				return { retry_after = tonumber(retry) }
			end
			return list
		end,
	},
	time = {
		now_ms = function()
			return now
		end,
		after = function(delay, callback)
			timers[#timers + 1] = { delay = delay, callback = callback }
		end,
	},
	ui = {
		decorate = function() end,
		relabel = function() end,
		refresh = function()
			refreshes = refreshes + 1
		end,
		reaction = function(fn)
			reaction = fn
		end,
	},
	discord = {
		permission = {},
		request = function(options, callback)
			requests[#requests + 1] = { options = options, callback = callback }
		end,
	},
}

dofile(arg[2])
dofile(arg[1])

local node = { channel_id = "1", message_id = "2", emoji = "name:3", count = 7 }
assert(reaction(node) == nil)
assert(#requests == 1)
assert(requests[1].options.path == "/channels/1/messages/2/reactions/name%3A3?limit=5&type=0")
assert(reaction(node) == nil and #requests == 1)

requests[1].callback({
	ok = true,
	status = 200,
	body = '[{"id":"11","avatar":"abc"},{"id":"263689920210534400","avatar":null}]',
})
assert(refreshes == 1)
local shown = reaction(node)
assert(#shown.images == 2)
assert(shown.images[1] == "https://cdn.discordapp.com/avatars/11/abc.webp?size=48")
assert(shown.images[2] == "https://cdn.discordapp.com/embed/avatars/" .. ((263689920210534400 >> 22) % 6) .. ".png")
assert(shown.total == 7)
assert(#requests == 1)

local other = { channel_id = "1", message_id = "9", emoji = "🔥", count = 1, burst = true }
assert(reaction(other) == nil)
assert(requests[2].options.path == "/channels/1/messages/9/reactions/%F0%9F%94%A5?limit=5&type=1")
local third = { channel_id = "1", message_id = "10", emoji = "x", count = 1 }
reaction(third)
assert(#requests == 2)
requests[2].callback({ ok = false, status = 429, body = '{"retry_after":1.5}' })
assert(#timers == 1 and timers[1].delay == 1500 and #requests == 2)
timers[1].callback()
assert(#requests == 3 and requests[3].options.path:find("/messages/10/", 1, true))

print("reaction-avatars ok")

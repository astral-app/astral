local build, gateway = nil, nil
local requests, refreshes, timers = {}, 0, {}
local now = 0
local messages = {
	["2"] = {
		id = "2",
		reactions = {
			{ emoji_name = "🔥", count = 7, burst_count = 1, me = false },
			{ emoji_id = "3", emoji_name = "name", count = 2, burst_count = 0, me = true },
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
	events = {
		on = function(_, callback, types)
			gateway = { callback = callback, types = types }
		end,
	},
	ui = {
		decorate = function() end,
		relabel = function() end,
		refresh = function()
			refreshes = refreshes + 1
		end,
		insert = function(selector, _, fn)
			assert(selector == "message.reaction:")
			build = fn
		end,
	},
	discord = {
		permission = {},
		message = function(_, id)
			return messages[id]
		end,
		request = function(options, callback)
			requests[#requests + 1] = { options = options, callback = callback }
		end,
	},
}

dofile(arg[2])
dofile(arg[1])

local fire = { channel_id = "1", key = "2:normal:unicode:1f525" }
assert(build(fire) == nil)
assert(#requests == 1)
assert(requests[1].options.path == "/channels/1/messages/2/reactions/%F0%9F%94%A5?limit=5&type=0")

requests[1].callback({
	ok = true,
	status = 200,
	body = table.concat({
		'[{"id":"11","avatar":"abc"},{"id":"12","avatar":"b"},{"id":"13","avatar":"c"},',
		'{"id":"14","avatar":"d"},{"id":"263689920210534400","avatar":null}]',
	}),
})
assert(refreshes == 1)
local stack = build(fire)
assert(stack.row and stack.fixed and #stack.children == 6)
assert(stack.children[1].icon == "https://cdn.discordapp.com/avatars/11/abc.webp?size=48")
assert(stack.children[1].margin[1] == 0 and stack.children[2].margin[1] < 0)
assert(stack.children[1].cover and stack.children[1].fixed)
assert(stack.children[6].children[1].text == "+2")

assert(build({ channel_id = "1", key = "2:burst:unicode:1f525" }) == nil)
assert(requests[2].options.path:find("type=1", 1, true))
assert(build({ channel_id = "1", key = "2:normal:custom:name:3" }) == nil)
requests[2].callback({ ok = false, status = 429, body = '{"retry_after":1.5}' })
assert(#timers == 1 and timers[1].delay == 1500)
timers[1].callback()
assert(requests[3].options.path == "/channels/1/messages/2/reactions/name%3A3?limit=5&type=0")
assert(build({ channel_id = "1", key = "nonsense" }) == nil)
requests[3].callback({ ok = false, status = 0, error = "Discord has not called its API yet" })
assert(#timers == 2 and timers[2].delay == 3000)

assert(gateway.types[1] == "MESSAGE_REACTION_ADD")
gateway.callback({ type = "MESSAGE_REACTION_ADD" })
assert(refreshes == 2)

print("reaction-avatars ok")

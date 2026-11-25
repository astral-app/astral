local commands, actions, sent, requests, stored = {}, {}, {}, {}, {}
local auth
astral = {
	log = function() end,
	settings = {},
	storage = {
		get = function(key)
			return stored[key]
		end,
		set = function(key, value)
			stored[key] = value
		end,
		remove = function(key)
			stored[key] = nil
		end,
	},
	net = {
		request = function(options, callback)
			requests[#requests + 1] = { options = options, callback = callback }
		end,
	},
	commands = {
		register = function(command)
			commands[command.name] = command
		end,
	},
	messages = {
		send_local = function(channel, message)
			sent[#sent + 1] = { channel = channel, message = message }
		end,
	},
	ui = {
		message_action = function(title, fn, options)
			actions[title] = { fn = fn, options = options }
		end,
		toast = function() end,
		web_auth = function(url, callback_url, callback)
			auth = { url = url, callback_url = callback_url, callback = callback }
			return 1
		end,
	},
	discord = {
		user = function(id)
			return id == "42" and { username = "friend" } or nil
		end,
	},
	json = {
		encode = function(value)
			return "comment=" .. tostring(value.comment)
		end,
		array = function(value)
			return value
		end,
	},
	time = {
		iso = function()
			return "2026-11-25 05:00 UTC"
		end,
	},
}
dofile(arg[1])

assert(commands.reviews.execute({ user = "42" }, { channel_id = "9" }) == nil)
assert(requests[1].options.url:find("/users/42/reviews", 1, true))
requests[1].callback({
	ok = true,
	status = 200,
	json = {
		reviewCount = 2,
		reviews = {
			{ id = 0, type = 3, comment = "rules", sender = { username = "Warning" }, timestamp = 1 },
			{
				id = 7,
				type = 0,
				comment = "great",
				sender = { username = "pal", profilePhoto = "https://x/a.png" },
				timestamp = 2,
			},
		},
	},
})
local shown = sent[1].message
assert(sent[1].channel == "9" and shown.content:find("2 reviews of friend", 1, true))
assert(#shown.embeds == 1 and shown.embeds[1].description == "great" and shown.embeds[1].author.name == "pal")

assert(commands.review.execute({ user = "42", text = "hi" }, { channel_id = "9" }):find("reviewdb-login", 1, true))
commands["reviewdb-login"].execute({}, { channel_id = "9" })
assert(auth.callback_url == "https://manti.vendicated.dev/api/reviewdb/auth")
assert(auth.url:find("redirect_uri=https%3A%2F%2Fmanti.vendicated.dev%2Fapi%2Freviewdb%2Fauth", 1, true))
auth.callback("https://manti.vendicated.dev/api/reviewdb/auth?code=abc", nil, nil)
assert(requests[2].options.url == "https://manti.vendicated.dev/api/reviewdb/auth?code=abc&clientMod=enmity")
requests[2].callback({ ok = true, status = 200, json = { token = "secret" } })
assert(stored.token == "secret")

assert(commands.review.execute({ user = "42", text = "nice" }, { channel_id = "9" }) == nil)
assert(requests[3].options.method == "PUT" and requests[3].options.headers.Authorization == "secret")
assert(requests[3].options.body == "comment=nice")

actions["View Reviews"].fn({ channel_id = "9", user_id = "42" })
assert(requests[4].options.url:find("/users/42/reviews", 1, true))

assert(commands["reviewdb-logout"].execute() and stored.token == nil)
print("reviewdb ok")

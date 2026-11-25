local previous_path = package.path
package.path = arg[1]:gsub("main.lua$", "?.lua;") .. previous_path

local values = {}
local requests = {}
local storage = {}
local action
local web_auth
local toast
local displayed = {}
local message_content = "hello world"
local now_milliseconds = 1000000

astral = {
	settings = {
		choice = function(key, _title, _choices, default)
			values[key] = values[key] or default
			return values[key]
		end,
		get = function(key, fallback)
			if values[key] == nil then
				return fallback
			end
			return values[key]
		end,
	},
	time = {
		now_ms = function()
			return now_milliseconds
		end,
	},
	storage = {
		get = function(key, fallback)
			if storage[key] == nil then
				return fallback
			end
			return storage[key]
		end,
		set = function(key, value)
			storage[key] = value
		end,
	},
	discord = {
		permission = {},
		message = function()
			return { content = message_content }
		end,
	},
	ui = {
		message_action = function(label, callback)
			assert(label == "Translate")
			action = callback
		end,
		toast = function(title, message)
			toast = { title = title, message = message }
		end,
		web_auth = function(url, callback_url, callback)
			web_auth = { url = url, callbackURL = callback_url, callback = callback }
			return 77
		end,
		cancel_web_auth = function(request)
			assert(request == 77)
		end,
	},
	messages = {
		set_text = function(_channel, message, text, options)
			displayed[message] = text and { text = text, note = options and options.note } or nil
		end,
		get_text = function(message)
			return displayed[message] and displayed[message].text
		end,
	},
	net = {
		request = function(options, callback)
			requests[#requests + 1] = { options = options, callback = callback }
			return #requests
		end,
	},
}

dofile(arg[2])
dofile(arg[1])

local node = { channel_id = "1", message_id = "2" }
action(node)
assert(#requests == 1)
assert(requests[1].options.url:find("clients5.google.com/translate_a/t", 1, true))
assert(requests[1].options.url:find("q=hello%20world", 1, true))
requests[1].callback({ ok = true, json = { { "bonjour", "en" } } })
assert(displayed["2"].text == "bonjour" and displayed["2"].note == "translated")
action(node)
assert(displayed["2"] == nil and #requests == 1)

toast = nil
action(node)
requests[2].callback({ ok = false })
assert(#requests == 3)
assert(requests[3].options.url:find("api.mymemory.translated.net/get", 1, true))
assert(requests[3].options.url:find("langpair=autodetect%7Cen", 1, true))
requests[3].callback({ ok = true, json = { responseStatus = 200, responseData = { translatedText = "hola" } } })
assert(displayed["2"].text == "hola")
displayed = {}

values.service = "mymemory"
values.source = "fr"
values.target = "de"
action(node)
assert(requests[4].options.url:find("langpair=fr%7Cde", 1, true))
requests[4].callback({ ok = true, json = { responseStatus = 403, responseData = { translatedText = "invalid" } } })
assert(toast.title == "Translate Error")

local before_long_message = #requests
message_content = string.rep("x", 401)
action(node)
assert(#requests == before_long_message)
assert(toast.title == "Translate")

astral.on_setting("account_action", "connect")
assert(web_auth.url == "https://translate.unbound.rip/auth/login")
assert(web_auth.callbackURL == "https://translate.unbound.rip/auth/callback")
web_auth.callback(
	"https://translate.unbound.rip/auth/callback?code=oauth-code&state=oauth-state",
	"session=oauth-cookie",
	nil
)
assert(
	requests[#requests].options.url == "https://translate.unbound.rip/auth/callback?code=oauth-code&state=oauth-state"
)
assert(requests[#requests].options.method == "GET")
assert(requests[#requests].options.headers.Accept == "application/json")
assert(requests[#requests].options.headers.Cookie == "session=oauth-cookie")
requests[#requests].callback({
	ok = true,
	json = { accessToken = "access-1", refreshToken = "refresh-1", expiresIn = 3600 },
})
assert(storage.access_token == "access-1" and storage.refresh_token == "refresh-1" and storage.expires_in == 3600)
assert(storage.expires_at_ms == 4600000)
assert(toast.title == "Translate" and toast.message == "Discord login completed")

message_content = "bonjour"
values.service = "auto"
action(node)
local first_translate_index = #requests
assert(requests[first_translate_index].options.url == "https://translate.unbound.rip/translate")
assert(requests[first_translate_index].options.headers.Authorization == "Bearer access-1")
assert(requests[first_translate_index].options.body:find('"q":"bonjour"', 1, true))
assert(requests[first_translate_index].options.body:find('"source":"fr"', 1, true))
assert(requests[first_translate_index].options.body:find('"target":"de"', 1, true))

action(node)
local second_translate_index = #requests
assert(requests[second_translate_index].options.url == "https://translate.unbound.rip/translate")
assert(requests[second_translate_index].options.headers.Authorization == "Bearer access-1")

requests[first_translate_index].callback({ status = 401, ok = false })
local refresh_index = #requests
assert(requests[refresh_index].options.url == "https://translate.unbound.rip/auth/refresh")
assert(requests[refresh_index].options.method == "POST")
assert(requests[refresh_index].options.headers["Content-Type"] == "application/json")
assert(requests[refresh_index].options.body:find('"refreshToken":"refresh-1"', 1, true))
requests[second_translate_index].callback({ status = 401, ok = false })
assert(#requests == refresh_index)
requests[refresh_index].callback({
	ok = true,
	json = { accessToken = "access-2", refreshToken = "refresh-2", expiresIn = 3600 },
})
local first_retry_index = refresh_index + 1
local second_retry_index = refresh_index + 2
assert(requests[first_retry_index].options.url == "https://translate.unbound.rip/translate")
assert(requests[first_retry_index].options.headers.Authorization == "Bearer access-2")
assert(requests[second_retry_index].options.url == "https://translate.unbound.rip/translate")
assert(requests[second_retry_index].options.headers.Authorization == "Bearer access-2")
requests[first_retry_index].callback({
	ok = true,
	json = { translations = { { translatedText = "guten tag", detectedSourceLanguage = "fr" } } },
})
requests[second_retry_index].callback({
	ok = true,
	json = { translations = { { translatedText = "guten tag", detectedSourceLanguage = "fr" } } },
})
assert(displayed["2"].text == "guten tag")

displayed = {}
action(node)
local cached_translate_index = #requests
assert(requests[cached_translate_index].options.url == "https://translate.unbound.rip/translate")
assert(requests[cached_translate_index].options.headers.Authorization == "Bearer access-2")
requests[cached_translate_index].callback({
	ok = true,
	json = { translations = { { translatedText = "guten tag", detectedSourceLanguage = "fr" } } },
})

now_milliseconds = storage.expires_at_ms - 10000
displayed = {}
action(node)
local proactive_refresh_index = #requests
assert(requests[proactive_refresh_index].options.url == "https://translate.unbound.rip/auth/refresh")
requests[proactive_refresh_index].callback({
	ok = true,
	json = { accessToken = "access-3", refreshToken = "refresh-3", expiresIn = 3600 },
})
local proactive_translate_index = #requests
assert(requests[proactive_translate_index].options.url == "https://translate.unbound.rip/translate")
assert(requests[proactive_translate_index].options.headers.Authorization == "Bearer access-3")
requests[proactive_translate_index].callback({
	ok = true,
	json = { translations = { { translatedText = "guten tag", detectedSourceLanguage = "fr" } } },
})

astral.on_setting("account_action", "disconnect")
assert(
	storage.access_token == ""
		and storage.refresh_token == ""
		and storage.expires_in == 0
		and storage.expires_at_ms == 0
)
assert(toast.title == "Translate" and toast.message == "Discord account disconnected")

astral.on_setting("account_action", "connect")
web_auth.callback("https://translate.unbound.rip/auth/callback?code=late&state=late", "session=late", nil)
local pending_login_index = #requests
assert(requests[pending_login_index].options.url:find("/auth/callback", 1, true))
astral.on_setting("account_action", "disconnect")
requests[pending_login_index].callback({
	ok = true,
	json = { accessToken = "stale-access", refreshToken = "stale-refresh", expiresIn = 3600 },
})
assert(storage.access_token == "" and storage.refresh_token == "" and storage.expires_at_ms == 0)

astral.on_setting("account_action", "connect")
local before_cancelled_callback = #requests
astral.on_setting("account_action", "disconnect")
web_auth.callback(
	"https://translate.unbound.rip/auth/callback?code=cancelled&state=cancelled",
	"session=cancelled",
	nil
)
assert(#requests == before_cancelled_callback)

values.service = "unbound"
local disconnected_requests = #requests
displayed = {}
action(node)
assert(#requests == disconnected_requests)
assert(toast.title == "Translate Error")

package.path = previous_path
print("translate ok")

local previousPath = package.path
package.path = arg[1]:gsub("main.lua$", "?.lua;") .. previousPath

local values = {}
local requests = {}
local storage = {}
local action
local webAuth
local toast
local clipboard
local messageContent = "hello world"
local nowMilliseconds = 1000000

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
			return nowMilliseconds
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
			return { content = messageContent }
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
		copy = function(value)
			clipboard = value
			return true
		end,
		web_auth = function(url, callbackURL, callback)
			webAuth = { url = url, callbackURL = callbackURL, callback = callback }
			return 77
		end,
		cancel_web_auth = function(request)
			assert(request == 77)
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
assert(clipboard == "bonjour")
assert(toast.title == "Translated" and toast.message == "bonjour")

toast = nil
action(node)
requests[2].callback({ ok = false })
assert(#requests == 3)
assert(requests[3].options.url:find("api.mymemory.translated.net/get", 1, true))
assert(requests[3].options.url:find("langpair=autodetect%7Cen", 1, true))
requests[3].callback({ ok = true, json = { responseStatus = 200, responseData = { translatedText = "hola" } } })
assert(clipboard == "hola")
assert(toast.title == "Translated" and toast.message == "hola")

values.service = "mymemory"
values.source = "fr"
values.target = "de"
action(node)
assert(requests[4].options.url:find("langpair=fr%7Cde", 1, true))
requests[4].callback({ ok = true, json = { responseStatus = 403, responseData = { translatedText = "invalid" } } })
assert(toast.title == "Translate Error")

local beforeLongMessage = #requests
messageContent = string.rep("x", 401)
action(node)
assert(#requests == beforeLongMessage)
assert(toast.title == "Translate")

astral.on_setting("account_action", "connect")
assert(webAuth.url == "https://translate.unbound.rip/auth/login")
assert(webAuth.callbackURL == "https://translate.unbound.rip/auth/callback")
webAuth.callback(
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

messageContent = "bonjour"
values.service = "auto"
action(node)
local firstTranslateIndex = #requests
assert(requests[firstTranslateIndex].options.url == "https://translate.unbound.rip/translate")
assert(requests[firstTranslateIndex].options.headers.Authorization == "Bearer access-1")
assert(requests[firstTranslateIndex].options.body:find('"q":"bonjour"', 1, true))
assert(requests[firstTranslateIndex].options.body:find('"source":"fr"', 1, true))
assert(requests[firstTranslateIndex].options.body:find('"target":"de"', 1, true))

action(node)
local secondTranslateIndex = #requests
assert(requests[secondTranslateIndex].options.url == "https://translate.unbound.rip/translate")
assert(requests[secondTranslateIndex].options.headers.Authorization == "Bearer access-1")

requests[firstTranslateIndex].callback({ status = 401, ok = false })
local refreshIndex = #requests
assert(requests[refreshIndex].options.url == "https://translate.unbound.rip/auth/refresh")
assert(requests[refreshIndex].options.method == "POST")
assert(requests[refreshIndex].options.headers["Content-Type"] == "application/json")
assert(requests[refreshIndex].options.body:find('"refreshToken":"refresh-1"', 1, true))
requests[secondTranslateIndex].callback({ status = 401, ok = false })
assert(#requests == refreshIndex)
requests[refreshIndex].callback({
	ok = true,
	json = { accessToken = "access-2", refreshToken = "refresh-2", expiresIn = 3600 },
})
local firstRetryIndex = refreshIndex + 1
local secondRetryIndex = refreshIndex + 2
assert(requests[firstRetryIndex].options.url == "https://translate.unbound.rip/translate")
assert(requests[firstRetryIndex].options.headers.Authorization == "Bearer access-2")
assert(requests[secondRetryIndex].options.url == "https://translate.unbound.rip/translate")
assert(requests[secondRetryIndex].options.headers.Authorization == "Bearer access-2")
requests[firstRetryIndex].callback({
	ok = true,
	json = { translations = { { translatedText = "guten tag", detectedSourceLanguage = "fr" } } },
})
requests[secondRetryIndex].callback({
	ok = true,
	json = { translations = { { translatedText = "guten tag", detectedSourceLanguage = "fr" } } },
})
assert(clipboard == "guten tag")
assert(toast.title == "Translated" and toast.message == "guten tag")

action(node)
local cachedTranslateIndex = #requests
assert(requests[cachedTranslateIndex].options.url == "https://translate.unbound.rip/translate")
assert(requests[cachedTranslateIndex].options.headers.Authorization == "Bearer access-2")
requests[cachedTranslateIndex].callback({
	ok = true,
	json = { translations = { { translatedText = "guten tag", detectedSourceLanguage = "fr" } } },
})

nowMilliseconds = storage.expires_at_ms - 10000
action(node)
local proactiveRefreshIndex = #requests
assert(requests[proactiveRefreshIndex].options.url == "https://translate.unbound.rip/auth/refresh")
requests[proactiveRefreshIndex].callback({
	ok = true,
	json = { accessToken = "access-3", refreshToken = "refresh-3", expiresIn = 3600 },
})
local proactiveTranslateIndex = #requests
assert(requests[proactiveTranslateIndex].options.url == "https://translate.unbound.rip/translate")
assert(requests[proactiveTranslateIndex].options.headers.Authorization == "Bearer access-3")
requests[proactiveTranslateIndex].callback({
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
webAuth.callback("https://translate.unbound.rip/auth/callback?code=late&state=late", "session=late", nil)
local pendingLoginIndex = #requests
assert(requests[pendingLoginIndex].options.url:find("/auth/callback", 1, true))
astral.on_setting("account_action", "disconnect")
requests[pendingLoginIndex].callback({
	ok = true,
	json = { accessToken = "stale-access", refreshToken = "stale-refresh", expiresIn = 3600 },
})
assert(storage.access_token == "" and storage.refresh_token == "" and storage.expires_at_ms == 0)

astral.on_setting("account_action", "connect")
local beforeCancelledCallback = #requests
astral.on_setting("account_action", "disconnect")
webAuth.callback("https://translate.unbound.rip/auth/callback?code=cancelled&state=cancelled", "session=cancelled", nil)
assert(#requests == beforeCancelledCallback)

values.service = "unbound"
local disconnectedRequests = #requests
action(node)
assert(#requests == disconnectedRequests)
assert(toast.title == "Translate Error")

package.path = previousPath
print("translate ok")

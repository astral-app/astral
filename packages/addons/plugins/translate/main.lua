local languages = require("lib.languages")
local languageCodes = {}
local languageOptions = {}
local sourceOptions = { astral.settings.option("auto", "Auto-detect") }
local apiBaseURL = "https://translate.unbound.rip"
local callbackURL = apiBaseURL .. "/auth/callback"
local authorizationURL = apiBaseURL .. "/auth/login"

for _, language in ipairs(languages) do
	local code, name, nativeName = language[1], language[2], language[3]
	languageCodes[code] = true
	local title = nativeName ~= name and name .. " (" .. nativeName .. ")" or name
	languageOptions[#languageOptions + 1] = astral.settings.option(code, title)
	sourceOptions[#sourceOptions + 1] = astral.settings.option(code, title)
end

local settings = astral.settings.section("Translation")
settings:choice("service", "Translation service", {
	astral.settings.option("auto", "Automatic", "Unbound when connected; Google, then MyMemory otherwise"),
	astral.settings.option("unbound", "Unbound Translate", "Uses the connected Discord account"),
	astral.settings.option("google", "Google Translate"),
	astral.settings.option("mymemory", "MyMemory"),
}, "auto")
settings:choice("source", "Source language", sourceOptions, "auto")
settings:choice("target", "Target language", languageOptions, "en")
settings:choice("account_action", "Translate API account", {
	astral.settings.option("connect", "Connect or reconnect"),
	astral.settings.option("disconnect", "Disconnect"),
}, "connect")

local function stored(key)
	local value = astral.storage.get(key, "")
	return type(value) == "string" and value or ""
end

local function saveSession(payload)
	if
		type(payload) ~= "table"
		or type(payload.accessToken) ~= "string"
		or payload.accessToken == ""
		or type(payload.refreshToken) ~= "string"
		or payload.refreshToken == ""
		or type(payload.expiresIn) ~= "number"
		or payload.expiresIn ~= payload.expiresIn
		or payload.expiresIn <= 0
		or payload.expiresIn > 31536000
	then
		return false
	end
	astral.storage.set("access_token", payload.accessToken)
	astral.storage.set("refresh_token", payload.refreshToken)
	astral.storage.set("expires_in", payload.expiresIn)
	astral.storage.set("expires_at_ms", astral.time.now_ms() + math.floor(payload.expiresIn * 1000))
	return true
end

local function clearSession()
	astral.storage.set("access_token", "")
	astral.storage.set("refresh_token", "")
	astral.storage.set("expires_in", 0)
	astral.storage.set("expires_at_ms", 0)
end

local function showToast(title, message)
	pcall(astral.ui.toast, title, message)
end

local function showError(message)
	showToast("Translate Error", message)
end

local function jsonString(value)
	local parts = { '"' }
	for index = 1, #value do
		local byte = value:byte(index)
		if byte == 34 then
			parts[#parts + 1] = '\\"'
		elseif byte == 92 then
			parts[#parts + 1] = "\\\\"
		elseif byte == 8 then
			parts[#parts + 1] = "\\b"
		elseif byte == 9 then
			parts[#parts + 1] = "\\t"
		elseif byte == 10 then
			parts[#parts + 1] = "\\n"
		elseif byte == 12 then
			parts[#parts + 1] = "\\f"
		elseif byte == 13 then
			parts[#parts + 1] = "\\r"
		elseif byte < 32 then
			parts[#parts + 1] = string.format("\\u%04x", byte)
		else
			parts[#parts + 1] = value:sub(index, index)
		end
	end
	parts[#parts + 1] = '"'
	return table.concat(parts)
end

local function jsonObject(fields)
	local parts = {}
	for key, value in pairs(fields) do
		parts[#parts + 1] = jsonString(key) .. ":" .. jsonString(value)
	end
	return "{" .. table.concat(parts, ",") .. "}"
end

local function request(options, callback)
	local ok, requestID = pcall(astral.net.request, options, callback)
	if ok then
		return requestID
	end
	callback({ ok = false, error = "network request could not start" })
	return nil
end

local function requestJSON(url, method, body, headers, callback)
	headers = headers or {}
	headers.Accept = "application/json"
	if body then
		headers["Content-Type"] = "application/json"
	end
	request({ url = url, method = method, headers = headers, body = body, timeout_ms = 15000 }, callback)
end

local function responseError(response, fallback)
	if
		response
		and type(response.json) == "table"
		and type(response.json.error) == "string"
		and response.json.error ~= ""
	then
		return response.json.error
	end
	if response and tonumber(response.status) then
		return fallback .. " (HTTP " .. tostring(response.status) .. ")"
	end
	return fallback
end

local authGeneration = 0
local activeAuthRequest

local function invalidateAuth()
	authGeneration = authGeneration + 1
	local pendingRequest = activeAuthRequest
	activeAuthRequest = nil
	if pendingRequest then
		pcall(astral.ui.cancel_web_auth, pendingRequest)
	end
	return authGeneration
end

local function completeLogin(response, generation)
	if generation ~= authGeneration then
		return
	end
	if not response or not response.ok or type(response.json) ~= "table" then
		showError(responseError(response, "Discord login failed"))
		return
	end
	if not saveSession(response.json) then
		showError("Translate API returned an invalid login response")
		return
	end
	showToast("Translate", "Discord login completed")
end

local function connectAccount()
	local generation = invalidateAuth()
	local callbackCompleted = false
	local ok, requestID = pcall(astral.ui.web_auth, authorizationURL, callbackURL, function(url, cookies, error)
		callbackCompleted = true
		if generation ~= authGeneration then
			return
		end
		activeAuthRequest = nil
		if error then
			if error ~= "cancelled" then
				showError("Could not finish Discord sign-in")
			end
			return
		end
		if type(url) ~= "string" or url == "" then
			showError("Discord sign-in did not return a callback")
			return
		end
		if url:find("[?&]error=access_denied") then
			return
		end
		local headers = { Accept = "application/json" }
		if type(cookies) == "string" and cookies ~= "" then
			headers.Cookie = cookies
		end
		requestJSON(url, "GET", nil, headers, function(response)
			completeLogin(response, generation)
		end)
	end)
	if not ok or type(requestID) ~= "number" then
		showError("Could not open Discord sign-in")
		return
	end
	if not callbackCompleted and generation == authGeneration then
		activeAuthRequest = requestID
	end
end

astral.settings.on_change("account_action", function(action)
	if action == "connect" then
		connectAccount()
	elseif action == "disconnect" then
		invalidateAuth()
		clearSession()
		showToast("Translate", "Discord account disconnected")
	end
end)

local function urlEncode(value)
	return value:gsub("([^A-Za-z0-9%-._~])", function(character)
		return string.format("%%%02X", string.byte(character))
	end)
end

local function anonymousURL(service, content, source, target)
	local query = urlEncode(content)
	if service == "google" then
		return "https://clients5.google.com/translate_a/t?client=dict-chrome-ex&sl="
			.. urlEncode(source)
			.. "&tl="
			.. urlEncode(target)
			.. "&q="
			.. query
	end
	local from = source == "auto" and "autodetect" or source
	return "https://api.mymemory.translated.net/get?langpair=" .. urlEncode(from .. "|" .. target) .. "&q=" .. query
end

local function readGoogle(payload)
	if type(payload) ~= "table" then
		return nil
	end
	local first = payload[1]
	if type(first) == "string" and first ~= "" then
		return { text = first }
	end
	if type(first) ~= "table" then
		return nil
	end
	if type(first[1]) == "string" and first[1] ~= "" then
		return { text = first[1], detected = first[2] }
	end
	local parts = {}
	for _, sentence in ipairs(first) do
		if type(sentence) == "table" and type(sentence[1]) == "string" then
			parts[#parts + 1] = sentence[1]
		end
	end
	return #parts > 0 and { text = table.concat(parts), detected = payload[3] } or nil
end

local function readMyMemory(payload)
	if type(payload) ~= "table" or tonumber(payload.responseStatus) ~= 200 then
		return nil
	end
	local data = payload.responseData
	if type(data) ~= "table" or type(data.translatedText) ~= "string" or data.translatedText == "" then
		return nil
	end
	return { text = data.translatedText, detected = data.detectedLanguage }
end

local function readUnbound(payload)
	if type(payload) ~= "table" then
		return nil
	end
	local first = type(payload.translations) == "table" and payload.translations[1] or nil
	local text = payload.translatedText or (type(first) == "table" and (first.translatedText or first.text))
	if type(text) ~= "string" or text == "" then
		return nil
	end
	local detected = payload.detectedLanguage
	if type(detected) == "table" then
		detected = detected.language
	end
	if type(detected) ~= "string" and type(first) == "table" then
		detected = first.detectedSourceLanguage
	end
	return { text = text, detected = detected }
end

local function saveTranslation(translation)
	if type(translation.text) ~= "string" or translation.text == "" then
		showError("Translation service returned an empty result")
		return
	end
	local copiedOK, copyResult = pcall(astral.ui.copy, translation.text)
	local copied = copiedOK and copyResult == true
	if #translation.text > 1800 then
		showToast("Translated", copied and "Translation copied to the clipboard" or "Translation is too long to copy")
	else
		showToast("Translated", translation.text)
	end
end

local function translateAnonymous(serviceList, index, content, source, target)
	local service = serviceList[index]
	if not service then
		showError("No translation provider returned a usable result")
		return
	end
	local callback = function(response)
		local translation
		if response and response.ok and type(response.json) == "table" then
			translation = service == "google" and readGoogle(response.json) or readMyMemory(response.json)
		end
		if translation then
			if source == "auto" and type(translation.detected) == "string" then
				translation.source = translation.detected
			end
			saveTranslation(translation)
			return
		end
		translateAnonymous(serviceList, index + 1, content, source, target)
	end
	request({ url = anonymousURL(service, content, source, target), timeout_ms = 15000 }, callback)
end

local refreshCallbacks
local refreshInFlightToken

local function completeRefresh(accessToken, failure)
	local callbacks = refreshCallbacks
	refreshCallbacks = nil
	refreshInFlightToken = nil
	for _, callback in ipairs(callbacks or {}) do
		callback(accessToken, failure)
	end
end

local function refreshSession(callback)
	local refreshToken = stored("refresh_token")
	if refreshToken == "" then
		callback(nil, "Connect a Discord account in Translate settings")
		return
	end
	if refreshCallbacks then
		refreshCallbacks[#refreshCallbacks + 1] = callback
		return
	end
	refreshCallbacks = { callback }
	refreshInFlightToken = refreshToken
	requestJSON(
		apiBaseURL .. "/auth/refresh",
		"POST",
		jsonObject({ refreshToken = refreshToken }),
		nil,
		function(response)
			if refreshInFlightToken ~= refreshToken then
				return
			end
			if stored("refresh_token") ~= refreshToken then
				local currentAccessToken = stored("access_token")
				completeRefresh(
					currentAccessToken ~= "" and currentAccessToken or nil,
					currentAccessToken ~= "" and nil or "Discord account changed while refreshing Translate"
				)
				return
			end
			if response and response.ok and type(response.json) == "table" and saveSession(response.json) then
				completeRefresh(stored("access_token"))
				return
			end
			if response and tonumber(response.status) == 401 then
				clearSession()
			end
			completeRefresh(nil, responseError(response, "Translate API token refresh failed"))
		end
	)
end

local function sendUnbound(content, source, target, accessToken, retried)
	requestJSON(
		apiBaseURL .. "/translate",
		"POST",
		jsonObject({ q = content, source = source, target = target }),
		{ Authorization = "Bearer " .. accessToken },
		function(response)
			if response and tonumber(response.status) == 401 and not retried then
				refreshSession(function(retryToken, refreshFailure)
					if not retryToken then
						showError(refreshFailure or "Translate API authentication failed")
						return
					end
					sendUnbound(content, source, target, retryToken, true)
				end)
				return
			end
			if not response or not response.ok or type(response.json) ~= "table" then
				showError(responseError(response, "Translate API request failed"))
				return
			end
			local translation = readUnbound(response.json)
			if not translation then
				showError("Translate API returned an invalid translation")
				return
			end
			if source == "auto" and type(translation.detected) == "string" then
				translation.source = translation.detected
			end
			saveTranslation(translation)
		end
	)
end

local function translateUnbound(content, source, target)
	local accessToken = stored("access_token")
	local expiresAt = astral.storage.get("expires_at_ms", 0)
	if accessToken ~= "" and type(expiresAt) == "number" and expiresAt - 15000 > astral.time.now_ms() then
		sendUnbound(content, source, target, accessToken, false)
		return
	end
	refreshSession(function(refreshedAccessToken, failure)
		if not refreshedAccessToken then
			showError(failure or "Translate API authentication failed")
			return
		end
		sendUnbound(content, source, target, refreshedAccessToken, false)
	end)
end

local function translateMessage(node)
	local message = node:message()
	local content = message and message.content
	if type(content) ~= "string" or content == "" then
		showToast("Translate", "This message has no cached text to translate")
		return
	end
	local service = astral.settings.get("service", "auto")
	local hasAccount = stored("refresh_token") ~= ""
	local useUnbound = service == "unbound" or service == "auto" and hasAccount
	local maximumBytes = useUnbound and 16000 or 400
	if #content > maximumBytes then
		showToast("Translate", "This message is too long for the selected translation service")
		return
	end
	local source = astral.settings.get("source", "auto")
	local target = astral.settings.get("target", "en")
	if source ~= "auto" and not languageCodes[source] then
		source = "auto"
	end
	if not languageCodes[target] then
		target = "en"
	end
	if useUnbound then
		translateUnbound(content, source, target)
		return
	end
	if service == "google" then
		translateAnonymous({ "google" }, 1, content, source, target)
	elseif service == "mymemory" then
		translateAnonymous({ "mymemory" }, 1, content, source, target)
	else
		translateAnonymous({ "google", "mymemory" }, 1, content, source, target)
	end
end

astral.ui.message_action("Translate", translateMessage)

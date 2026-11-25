local languages = require("lib.languages")
local language_codes = {}
local language_options = {}
local source_options = { astral.settings.option("auto", "Auto-detect") }
local api_base_url = "https://translate.unbound.rip"
local callback_url = api_base_url .. "/auth/callback"
local authorization_url = api_base_url .. "/auth/login"

for _, language in ipairs(languages) do
	local code, name, native_name = language[1], language[2], language[3]
	language_codes[code] = true
	local title = native_name ~= name and name .. " (" .. native_name .. ")" or name
	language_options[#language_options + 1] = astral.settings.option(code, title)
	source_options[#source_options + 1] = astral.settings.option(code, title)
end

local settings = astral.settings.section("Translation")
settings:choice("service", "Translation service", {
	astral.settings.option("auto", "Automatic", "Unbound when connected; Google, then MyMemory otherwise"),
	astral.settings.option("unbound", "Unbound Translate", "Uses the connected Discord account"),
	astral.settings.option("google", "Google Translate"),
	astral.settings.option("mymemory", "MyMemory"),
}, "auto")
settings:choice("source", "Source language", source_options, "auto")
settings:choice("target", "Target language", language_options, "en")
settings:choice("display", "Show translations", {
	astral.settings.option("toast", "Copy and show", "Copies the translation and shows it in a toast"),
	astral.settings.option(
		"inline",
		"In the message",
		"Shows the translation under the message's text with its languages"
	),
}, "toast")
settings:choice("account_action", "Translate API account", {
	astral.settings.option("connect", "Connect or reconnect"),
	astral.settings.option("disconnect", "Disconnect"),
}, "connect")

local function stored(key)
	local value = astral.storage.get(key, "")
	return type(value) == "string" and value or ""
end

local function save_session(payload)
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

local function clear_session()
	astral.storage.set("access_token", "")
	astral.storage.set("refresh_token", "")
	astral.storage.set("expires_in", 0)
	astral.storage.set("expires_at_ms", 0)
end

local function show_toast(title, message)
	pcall(astral.ui.toast, title, message)
end

local function show_error(message)
	show_toast("Translate Error", message)
end

local function json_string(value)
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

local function json_object(fields)
	local parts = {}
	for key, value in pairs(fields) do
		parts[#parts + 1] = json_string(key) .. ":" .. json_string(value)
	end
	return "{" .. table.concat(parts, ",") .. "}"
end

local function request(options, callback)
	local ok, request_id = pcall(astral.net.request, options, callback)
	if ok then
		return request_id
	end
	callback({ ok = false, error = "network request could not start" })
	return nil
end

local function request_json(url, method, body, headers, callback)
	headers = headers or {}
	headers.Accept = "application/json"
	if body then
		headers["Content-Type"] = "application/json"
	end
	request({ url = url, method = method, headers = headers, body = body, timeout_ms = 15000 }, callback)
end

local function response_error(response, fallback)
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

local auth_generation = 0
local active_auth_request

local function invalidate_auth()
	auth_generation = auth_generation + 1
	local pending_request = active_auth_request
	active_auth_request = nil
	if pending_request then
		pcall(astral.ui.cancel_web_auth, pending_request)
	end
	return auth_generation
end

local function complete_login(response, generation)
	if generation ~= auth_generation then
		return
	end
	if not response or not response.ok or type(response.json) ~= "table" then
		show_error(response_error(response, "Discord login failed"))
		return
	end
	if not save_session(response.json) then
		show_error("Translate API returned an invalid login response")
		return
	end
	show_toast("Translate", "Discord login completed")
end

local function connect_account()
	local generation = invalidate_auth()
	local callback_completed = false
	local ok, request_id = pcall(astral.ui.web_auth, authorization_url, callback_url, function(url, cookies, error)
		callback_completed = true
		if generation ~= auth_generation then
			return
		end
		active_auth_request = nil
		if error then
			if error ~= "cancelled" then
				show_error("Could not finish Discord sign-in")
			end
			return
		end
		if type(url) ~= "string" or url == "" then
			show_error("Discord sign-in did not return a callback")
			return
		end
		if url:find("[?&]error=access_denied") then
			return
		end
		local headers = { Accept = "application/json" }
		if type(cookies) == "string" and cookies ~= "" then
			headers.Cookie = cookies
		end
		request_json(url, "GET", nil, headers, function(response)
			complete_login(response, generation)
		end)
	end)
	if not ok or type(request_id) ~= "number" then
		show_error("Could not open Discord sign-in")
		return
	end
	if not callback_completed and generation == auth_generation then
		active_auth_request = request_id
	end
end

astral.settings.on_change("account_action", function(action)
	if action == "connect" then
		connect_account()
	elseif action == "disconnect" then
		invalidate_auth()
		clear_session()
		show_toast("Translate", "Discord account disconnected")
	end
end)

local function url_encode(value)
	return value:gsub("([^A-Za-z0-9%-._~])", function(character)
		return string.format("%%%02X", string.byte(character))
	end)
end

local function anonymous_url(service, content, source, target)
	local query = url_encode(content)
	if service == "google" then
		return "https://clients5.google.com/translate_a/t?client=dict-chrome-ex&sl="
			.. url_encode(source)
			.. "&tl="
			.. url_encode(target)
			.. "&q="
			.. query
	end
	local from = source == "auto" and "autodetect" or source
	return "https://api.mymemory.translated.net/get?langpair=" .. url_encode(from .. "|" .. target) .. "&q=" .. query
end

local function read_google(payload)
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

local function read_my_memory(payload)
	if type(payload) ~= "table" or tonumber(payload.responseStatus) ~= 200 then
		return nil
	end
	local data = payload.responseData
	if type(data) ~= "table" or type(data.translatedText) ~= "string" or data.translatedText == "" then
		return nil
	end
	return { text = data.translatedText, detected = data.detectedLanguage }
end

local function read_unbound(payload)
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

local function save_translation(translation, destination)
	if type(translation.text) ~= "string" or translation.text == "" then
		show_error("Translation service returned an empty result")
		return
	end
	if astral.settings.get("display", "toast") ~= "inline" then
		pcall(astral.ui.copy, translation.text)
		show_toast("Translated", translation.text)
		return
	end
	local from = type(translation.detected) == "string" and translation.detected or destination.source
	local note = from .. " -> " .. destination.target
	local text = destination.content .. "\n" .. translation.text
	astral.messages.set_text(destination.channel_id, destination.message_id, text, { note = note })
end

local function translate_anonymous(service_list, index, content, source, target, destination)
	local service = service_list[index]
	if not service then
		show_error("No translation provider returned a usable result")
		return
	end
	local callback = function(response)
		local translation
		if response and response.ok and type(response.json) == "table" then
			translation = service == "google" and read_google(response.json) or read_my_memory(response.json)
		end
		if translation then
			if source == "auto" and type(translation.detected) == "string" then
				translation.source = translation.detected
			end
			save_translation(translation, destination)
			return
		end
		translate_anonymous(service_list, index + 1, content, source, target, destination)
	end
	request({ url = anonymous_url(service, content, source, target), timeout_ms = 15000 }, callback)
end

local refresh_callbacks
local refresh_in_flight_token

local function complete_refresh(access_token, failure)
	local callbacks = refresh_callbacks
	refresh_callbacks = nil
	refresh_in_flight_token = nil
	for _, callback in ipairs(callbacks or {}) do
		callback(access_token, failure)
	end
end

local function refresh_session(callback)
	local refresh_token = stored("refresh_token")
	if refresh_token == "" then
		callback(nil, "Connect a Discord account in Translate settings")
		return
	end
	if refresh_callbacks then
		refresh_callbacks[#refresh_callbacks + 1] = callback
		return
	end
	refresh_callbacks = { callback }
	refresh_in_flight_token = refresh_token
	request_json(
		api_base_url .. "/auth/refresh",
		"POST",
		json_object({ refreshToken = refresh_token }),
		nil,
		function(response)
			if refresh_in_flight_token ~= refresh_token then
				return
			end
			if stored("refresh_token") ~= refresh_token then
				local current_access_token = stored("access_token")
				complete_refresh(
					current_access_token ~= "" and current_access_token or nil,
					current_access_token ~= "" and nil or "Discord account changed while refreshing Translate"
				)
				return
			end
			if response and response.ok and type(response.json) == "table" and save_session(response.json) then
				complete_refresh(stored("access_token"))
				return
			end
			if response and tonumber(response.status) == 401 then
				clear_session()
			end
			complete_refresh(nil, response_error(response, "Translate API token refresh failed"))
		end
	)
end

local function send_unbound(content, source, target, access_token, retried, destination)
	request_json(
		api_base_url .. "/translate",
		"POST",
		json_object({ q = content, source = source, target = target }),
		{ Authorization = "Bearer " .. access_token },
		function(response)
			if response and tonumber(response.status) == 401 and not retried then
				refresh_session(function(retry_token, refresh_failure)
					if not retry_token then
						show_error(refresh_failure or "Translate API authentication failed")
						return
					end
					send_unbound(content, source, target, retry_token, true, destination)
				end)
				return
			end
			if not response or not response.ok or type(response.json) ~= "table" then
				show_error(response_error(response, "Translate API request failed"))
				return
			end
			local translation = read_unbound(response.json)
			if not translation then
				show_error("Translate API returned an invalid translation")
				return
			end
			if source == "auto" and type(translation.detected) == "string" then
				translation.source = translation.detected
			end
			save_translation(translation, destination)
		end
	)
end

local function translate_unbound(content, source, target, destination)
	local access_token = stored("access_token")
	local expires_at = astral.storage.get("expires_at_ms", 0)
	if access_token ~= "" and type(expires_at) == "number" and expires_at - 15000 > astral.time.now_ms() then
		send_unbound(content, source, target, access_token, false, destination)
		return
	end
	refresh_session(function(refreshed_access_token, failure)
		if not refreshed_access_token then
			show_error(failure or "Translate API authentication failed")
			return
		end
		send_unbound(content, source, target, refreshed_access_token, false, destination)
	end)
end

local function show_original(node)
	astral.messages.set_text(node.channel_id, node.message_id, nil)
end

local function translatable(node)
	if astral.messages.get_text(node.message_id) then
		return false
	end
	local message = node:message()
	return message ~= nil and type(message.content) == "string" and message.content ~= ""
end

local function translated(node)
	return astral.messages.get_text(node.message_id) ~= nil
end

local function translate_message(node)
	local message = node:message()
	local content = message and message.content
	if type(content) ~= "string" or content == "" then
		show_toast("Translate", "This message has no cached text to translate")
		return
	end
	local service = astral.settings.get("service", "auto")
	local has_account = stored("refresh_token") ~= ""
	local use_unbound = service == "unbound" or service == "auto" and has_account
	local maximum_bytes = use_unbound and 16000 or 400
	if #content > maximum_bytes then
		show_toast("Translate", "This message is too long for the selected translation service")
		return
	end
	local source = astral.settings.get("source", "auto")
	local target = astral.settings.get("target", "en")
	if source ~= "auto" and not language_codes[source] then
		source = "auto"
	end
	if not language_codes[target] then
		target = "en"
	end
	local destination = {
		channel_id = node.channel_id,
		message_id = node.message_id,
		content = content,
		source = source,
		target = target,
	}
	if use_unbound then
		translate_unbound(content, source, target, destination)
		return
	end
	if service == "google" then
		translate_anonymous({ "google" }, 1, content, source, target, destination)
	elseif service == "mymemory" then
		translate_anonymous({ "mymemory" }, 1, content, source, target, destination)
	else
		translate_anonymous({ "google", "mymemory" }, 1, content, source, target, destination)
	end
end

astral.ui.message_action("Translate", translate_message, { icon = "res://ic_mana_chat", when = translatable })
astral.ui.message_action("Show Original", show_original, { icon = "res://ic_mana_refresh", when = translated })

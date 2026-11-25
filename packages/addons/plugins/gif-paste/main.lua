local GIF_LOCATION = "gif_reply"

local function header(request, name)
	for key, value in pairs(request.headers or {}) do
		if type(key) == "string" and key:lower() == name then
			return value
		end
	end
	return nil
end

local function from_gif_picker(request)
	local encoded = header(request, "x-context-properties")
	if type(encoded) ~= "string" then
		return false
	end
	local properties = astral.base64.decode(encoded)
	local decoded = properties and astral.json.decode(properties)
	return type(decoded) == "table" and decoded.location == GIF_LOCATION
end

local function sent_message(request)
	local message = astral.json.decode(request.body or "")
	if type(message) ~= "table" or type(message.content) ~= "string" or message.content == "" then
		return nil
	end
	return message
end

astral.net.intercept({
	method = "POST",
	path = "^/api/v%d+/channels/%d+/messages$",
	request = function(request)
		if not from_gif_picker(request) then
			return nil
		end
		local message = sent_message(request)
		if not message then
			return nil
		end
		local channel_id = request.path:match("/channels/(%d+)/messages")
		local nonce = type(message.nonce) == "string" and message.nonce or nil
		astral.ui.insert_text(message.content .. " ")
		if nonce then
			astral.time.after(0, function()
				astral.gateway.dispatch("MESSAGE_DELETE", { id = nonce, channel_id = channel_id })
			end)
		end
		return {
			status = 200,
			body = astral.json.encode({
				id = nonce or "0",
				channel_id = channel_id,
				content = message.content,
				nonce = nonce,
			}),
		}
	end,
})

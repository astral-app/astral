local STICKER_LOCATION = "sticker_reply"
local SEND_PLACEHOLDER = "\u{200B}"
local PREVIEW_SIZE = 72
local STICKER_URL = "https://media.discordapp.net/stickers/%s.webp?size=160&quality=lossless"

local staged = {}
local pending = {}

local function header(request, name)
	for key, value in pairs(request.headers or {}) do
		if type(key) == "string" and key:lower() == name then
			return value
		end
	end
	return nil
end

local function location(request)
	local encoded = header(request, "x-context-properties")
	if type(encoded) ~= "string" then
		return nil
	end
	local decoded = astral.base64.decode(encoded)
	local properties = decoded and astral.json.decode(decoded)
	return type(properties) == "table" and properties.location or nil
end

local function stickers_in(channel_id)
	staged[channel_id] = staged[channel_id] or {}
	return staged[channel_id]
end

local function show_send_button()
	if astral.ui.composer_text() == "" then
		astral.ui.insert_text(SEND_PLACEHOLDER)
	end
end

local function hide_send_button()
	if astral.ui.composer_text() == SEND_PLACEHOLDER then
		astral.ui.insert_text("", { replace = true })
	end
end

local function stage(channel_id, sticker_ids)
	local stickers = stickers_in(channel_id)
	for _, id in ipairs(sticker_ids) do
		local known = false
		for _, existing in ipairs(stickers) do
			known = known or existing == id
		end
		if not known then
			stickers[#stickers + 1] = id
		end
	end
	astral.time.after(0, function()
		show_send_button()
		astral.ui.refresh()
	end)
end

local function unstage(channel_id, index)
	local stickers = stickers_in(channel_id)
	table.remove(stickers, index)
	if #stickers == 0 then
		hide_send_button()
	end
	astral.ui.refresh()
end

local function cancel_picker_send(message, channel_id)
	if type(message.nonce) == "string" then
		astral.time.after(0, function()
			astral.gateway.dispatch("MESSAGE_DELETE", { id = message.nonce, channel_id = channel_id })
		end)
	end
	return {
		status = 200,
		body = astral.json.encode({
			id = message.nonce or "0",
			channel_id = channel_id,
			content = "",
			nonce = message.nonce,
		}),
	}
end

local function attach_staged(request, message, channel_id)
	local stickers = stickers_in(channel_id)
	if #stickers == 0 then
		return nil
	end
	local ids = astral.json.array()
	for _, id in ipairs(type(message.sticker_ids) == "table" and message.sticker_ids or {}) do
		ids[#ids + 1] = id
	end
	for _, id in ipairs(stickers) do
		ids[#ids + 1] = id
	end
	message.sticker_ids = ids
	message.content = tostring(message.content or ""):gsub(SEND_PLACEHOLDER, "")
	pending[request.id] = { channel_id = channel_id, stickers = stickers }
	staged[channel_id] = {}
	astral.ui.refresh()
	return { body = astral.json.encode(message) }
end

astral.net.intercept({
	method = "POST",
	path = "^/api/v%d+/channels/%d+/messages$",
	request = function(request)
		local channel_id = request.path:match("/channels/(%d+)/messages")
		local message = astral.json.decode(request.body or "")
		if not channel_id or type(message) ~= "table" then
			return nil
		end
		if location(request) == STICKER_LOCATION and type(message.sticker_ids) == "table" then
			stage(channel_id, message.sticker_ids)
			return cancel_picker_send(message, channel_id)
		end
		return attach_staged(request, message, channel_id)
	end,
	response = function(request, response)
		local sent = pending[request.id]
		pending[request.id] = nil
		if sent and (response.status or 0) >= 400 then
			stage(sent.channel_id, sent.stickers)
		end
		return nil
	end,
})

astral.ui.insert({ id = "composer.attachments^^^^^^^^" }, function(node)
	local stickers = node.channel_id and staged[node.channel_id] or nil
	if not stickers or #stickers == 0 then
		return nil
	end
	local previews = {}
	for index, id in ipairs(stickers) do
		previews[#previews + 1] = {
			type = "view",
			background = "#2B2D31",
			radius = 16,
			padding = 8,
			margin = { 8, 0, 0, 0 },
			action = "remove_" .. index,
			children = {
				{ type = "image", icon = string.format(STICKER_URL, id), width = PREVIEW_SIZE, height = PREVIEW_SIZE },
			},
		}
	end
	return {
		type = "view",
		row = true,
		fill = true,
		justify = "end",
		padding = { 16, 0, 16, 8 },
		position = "start",
		children = previews,
	}
end, function(node, action)
	local index = tonumber(action:match("^remove_(%d+)$"))
	if index and node.channel_id then
		unstage(node.channel_id, index)
	end
end)

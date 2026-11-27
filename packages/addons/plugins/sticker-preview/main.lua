local STICKER_LOCATION = "sticker_reply"
local SEND_PLACEHOLDER = "\u{200B}"
local PREVIEW_SIZE = 80
local STICKER_SIZE = 76
local FRAME_RADIUS = 12
local STICKER_RADIUS = 8
local FRAME_BACKGROUND = "#2B2D31"
local FRAME_BORDER = 0x14FFFFFF
local MEDIA = "https://media.discordapp.net/stickers/"
local FORMAT_APNG, FORMAT_LOTTIE, FORMAT_GIF = 2, 3, 4

local formats = {}

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

local function learn_format(id)
	if formats[id] ~= nil then
		return
	end
	formats[id] = false
	pcall(function()
		astral.discord.request({ path = "/stickers/" .. id }, function(response)
			local ok, sticker = pcall(astral.json.decode, response and response.ok and response.body or "")
			formats[id] = ok and type(sticker) == "table" and math.tointeger(sticker.format_type) or false
			astral.ui.refresh()
		end)
	end)
end

local function sticker_node(id)
	learn_format(id)
	local format = formats[id]
	local node = { width = STICKER_SIZE, height = STICKER_SIZE, radius = STICKER_RADIUS, fixed = true }
	if format == FORMAT_LOTTIE then
		node.type = astral.InsertNodeType.Lottie
		node.uri = MEDIA .. id .. ".json"
		return node
	end
	node.type = astral.InsertNodeType.Image
	if format == FORMAT_GIF then
		node.icon = MEDIA .. id .. ".gif?size=160"
		node.animated = true
	elseif format == FORMAT_APNG then
		node.icon = MEDIA .. id .. ".png?size=160"
		node.animated = true
	else
		node.icon = MEDIA .. id .. ".webp?size=160&quality=lossless"
	end
	return node
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
			type = astral.InsertNodeType.View,
			background = FRAME_BORDER,
			radius = FRAME_RADIUS,
			padding = 1,
			width = PREVIEW_SIZE,
			height = PREVIEW_SIZE,
			fixed = true,
			margin = { 8, 0, 0, 0 },
			action = "remove_" .. index,
			children = {
				{
					type = astral.InsertNodeType.View,
					background = FRAME_BACKGROUND,
					radius = FRAME_RADIUS - 1,
					fill = true,
					fill_height = true,
					justify = astral.Justify.Center,
					align = astral.Align.Center,
					children = { sticker_node(id) },
				},
			},
		}
	end
	return {
		type = astral.InsertNodeType.View,
		row = true,
		fill = true,
		justify = astral.Justify.End,
		padding = { 16, 0, 16, 8 },
		position = astral.InsertPosition.Start,
		children = previews,
	}
end, function(node, action)
	local index = tonumber(action:match("^remove_(%d+)$"))
	if index and node.channel_id then
		unstage(node.channel_id, index)
	end
end)

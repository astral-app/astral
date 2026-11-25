local function t(key, variables)
	local text = astral.i18n.t(key)
	for name, value in pairs(variables or {}) do
		text = text:gsub("{" .. name .. "}", function()
			return value
		end)
	end
	return text
end

local function raw_record(node)
	local message = astral.discord.raw_message(node.channel_id, node.message_id)
	if message then
		return message
	end
	return node:message()
end

local function sheet_for(message)
	local content = type(message.content) == "string" and message.content or ""
	local data = astral.json.encode(message, { indent = 2 })
	local type_name = t("TYPE_MESSAGE")
	local items = {}
	if content ~= "" then
		items[#items + 1] = { type = "heading", text = t("MESSAGE_CONTENT") }
		items[#items + 1] = { type = "code", text = content }
		items[#items + 1] = { type = "heading", text = t("MESSAGE_DATA") }
	end
	items[#items + 1] = { type = "code", text = data, fill = true }
	local buttons = { { label = t("COPY_DATA"), action = "copy_data", style = "primary" } }
	if content ~= "" then
		buttons[#buttons + 1] = { label = t("COPY_CONTENT"), action = "copy_content" }
	end
	buttons[#buttons + 1] = { label = t("CLOSE"), action = "close" }
	items[#items + 1] = { type = "buttons", buttons = buttons }
	return { title = t("RAW_TITLE", { type = type_name }), expanded = true, items = items }, data, content, type_name
end

local function show_raw(node)
	local message = raw_record(node)
	if not message then
		astral.ui.toast(t("VIEW_RAW"), "This message isn't cached yet")
		return
	end
	local spec, data, content, type_name = sheet_for(message)
	local sheet
	sheet = astral.ui.sheet(spec, function(action)
		if action == "copy_data" then
			astral.ui.copy(data)
			astral.ui.toast(t("VIEW_RAW"), t("DATA_COPIED", { type = type_name }))
		elseif action == "copy_content" then
			astral.ui.copy(content)
			astral.ui.toast(t("VIEW_RAW"), t("CONTENT_COPIED"))
		elseif action == "close" then
			sheet:close()
		end
	end)
end

astral.ui.message_action(t("VIEW_RAW"), show_raw, { icon = "res://ic_mana_angle_brackets" })

local STORAGE_KEY = "commands"
local DESCRIPTION_LIMIT = 100
local MEDIA_LIMIT = 360 * 1024
local MEDIA_EXTENSIONS = { "png", "jpg", "jpeg", "gif", "webp", "heic", "mp4", "mov", "m4v", "webm" }
local EPHEMERAL = "ephemeral"
local t = astral.i18n.t

local function unquoted(text)
	return (text:gsub("'({{)'", "%1"):gsub("'(}})'", "%1"))
end

local function saved()
	local list = astral.storage.get(STORAGE_KEY)
	return type(list) == "table" and list or {}
end

local function store(list)
	astral.storage.set(STORAGE_KEY, list)
end

local function find(list, name)
	for index, command in ipairs(list) do
		if command.name == name then
			return index, command
		end
	end
	return nil
end

local function arguments(response)
	local found, order = {}, {}
	for inside in tostring(response or ""):gmatch("{{(.-)}}") do
		local name, default = inside:match("^%s*([%w_-]+)%s*=%s*(.-)%s*$")
		name = name or inside:match("^%s*([%w_-]+)%s*$")
		if name and not found[name] then
			found[name] = { name = name:lower(), default = default }
			order[#order + 1] = found[name]
		end
	end
	return order
end

local function fill(response, values)
	local text = tostring(response or ""):gsub("{{(.-)}}", function(inside)
		local name, default = inside:match("^%s*([%w_-]+)%s*=%s*(.-)%s*$")
		name = name or inside:match("^%s*([%w_-]+)%s*$")
		if not name then
			return "{{" .. inside .. "}}"
		end
		local value = values[name:lower()]
		if value == nil or value == "" then
			return default or ""
		end
		return tostring(value)
	end)
	return (text:gsub("\\n", "\n"))
end

local function validate(command, list, original)
	if not tostring(command.name or ""):match("^[a-z0-9_-]+$") or #command.name > 32 then
		return t("INVALID_NAME")
	end
	if command.name ~= original and find(list, command.name) then
		return t("DUPLICATE_NAME")
	end
	if #(command.description or "") > DESCRIPTION_LIMIT then
		return t("DESCRIPTION_TOO_LONG", { max = DESCRIPTION_LIMIT })
	end
	if (command.response or "") == "" and not command.media then
		return t("EMPTY_COMMAND")
	end
	for _, argument in ipairs(arguments(command.response)) do
		if argument.name == EPHEMERAL then
			return t("RESERVED_ARGUMENT")
		end
	end
	return nil
end

local function register(command)
	local options = {}
	for _, argument in ipairs(arguments(command.response)) do
		options[#options + 1] = {
			name = argument.name,
			description = argument.name,
			type = astral.OptionType.String,
			required = argument.default == nil,
		}
	end
	table.sort(options, function(left, right)
		return left.required and not right.required
	end)
	options[#options + 1] = {
		name = EPHEMERAL,
		description = t("EPHEMERAL_DESCRIPTION"),
		type = astral.OptionType.Boolean,
		required = false,
	}
	local description = command.description ~= "" and command.description
		or t("DEFAULT_DESCRIPTION", { name = command.name })
	astral.commands.register({
		name = command.name,
		description = description:sub(1, DESCRIPTION_LIMIT),
		options = options,
		execute = function(values)
			local text = fill(command.response, values)
			if values[EPHEMERAL] then
				if command.media then
					return t("EPHEMERAL_WITH_ATTACHMENTS")
				end
				return text
			end
			local send = { content = text }
			if command.media then
				send.file = {
					name = command.media.name,
					data = astral.base64.decode(command.media.data),
					content_type = command.media.content_type,
				}
			end
			return { send = send }
		end,
	})
end

for _, command in ipairs(saved()) do
	register(command)
end

local function content_type(name)
	local extension = tostring(name):match("%.([%w]+)$")
	extension = extension and extension:lower() or ""
	local video = { mp4 = "video/mp4", mov = "video/quicktime", m4v = "video/x-m4v", webm = "video/webm" }
	if video[extension] then
		return video[extension]
	end
	return extension == "jpg" and "image/jpeg" or "image/" .. extension
end

local open_list

local function open_editor(original)
	local list = saved()
	local _, existing = find(list, original or "")
	local draft = {
		name = existing and existing.name or "",
		description = existing and existing.description or "",
		response = existing and existing.response or "",
		media = existing and existing.media or nil,
	}
	local sheet
	local function items(message)
		local list_items = {
			{
				type = astral.SheetItemType.Heading,
				text = original and t("EDIT_COMMAND_LABEL", { name = original }) or t("CREATE_COMMAND"),
			},
			{
				type = astral.SheetItemType.Text,
				text = t("COMMAND_NAME") .. (draft.name ~= "" and (": /" .. draft.name) or ""),
			},
			{
				type = astral.SheetItemType.Input,
				action = "name",
				value = draft.name,
				placeholder = t("COMMAND_NAME_PLACEHOLDER"),
				submit_label = t("COMMAND_NAME"),
			},
			{
				type = astral.SheetItemType.Text,
				text = t("DESCRIPTION_OPTIONAL") .. (draft.description ~= "" and (": " .. draft.description) or ""),
			},
			{
				type = astral.SheetItemType.Input,
				action = "description",
				value = draft.description,
				placeholder = t("DESCRIPTION_PLACEHOLDER"),
				submit_label = t("DESCRIPTION_OPTIONAL"),
			},
			{ type = astral.SheetItemType.Text, text = t("RESPONSE") },
			{
				type = astral.SheetItemType.Code,
				text = draft.response ~= "" and draft.response or unquoted(t("RESPONSE_PLACEHOLDER")),
			},
			{
				type = astral.SheetItemType.Input,
				action = "response",
				value = draft.response,
				placeholder = unquoted(t("RESPONSE_PLACEHOLDER")),
				submit_label = t("RESPONSE"),
			},
			{ type = astral.SheetItemType.Text, text = unquoted(t("RESPONSE_HELP")) },
		}
		local names = {}
		for _, argument in ipairs(arguments(draft.response)) do
			names[#names + 1] = argument.name
		end
		if #names > 0 then
			list_items[#list_items + 1] =
				{ type = astral.SheetItemType.Text, text = t("ARGUMENTS", { names = table.concat(names, ", ") }) }
		end
		if draft.media then
			list_items[#list_items + 1] = {
				type = astral.SheetItemType.Row,
				action = "remove-media",
				items = {
					{ type = astral.SheetItemType.Text, text = draft.media.name },
				},
			}
		end
		local buttons = {
			{ label = t("ATTACH_MEDIA"), action = "attach" },
			{ label = t("SAVE_COMMAND"), action = "save", style = astral.ButtonStyle.Primary },
		}
		if original then
			buttons[#buttons + 1] = { label = t("DELETE"), action = "delete", style = astral.ButtonStyle.Danger }
		end
		list_items[#list_items + 1] = { type = astral.SheetItemType.Buttons, buttons = buttons }
		if message then
			list_items[#list_items + 1] = { type = astral.SheetItemType.Text, text = message }
		end
		return list_items
	end
	local function refresh(message)
		sheet:update({ title = t("PLUGIN_NAME"), items = items(message), expanded = true })
	end
	sheet = astral.ui.sheet({ title = t("PLUGIN_NAME"), items = items(), expanded = true }, function(action, value)
		if action == "name" then
			draft.name = tostring(value):lower():gsub("^/", "")
			refresh()
		elseif action == "description" or action == "response" then
			draft[action] = tostring(value)
			refresh()
		elseif action == "remove-media" then
			draft.media = nil
			refresh()
		elseif action == "attach" then
			astral.files.pick(MEDIA_EXTENSIONS, function(file)
				if not file then
					return
				end
				if #file.data > MEDIA_LIMIT then
					refresh(t("MEDIA_TOO_LARGE"))
					return
				end
				draft.media =
					{ name = file.name, data = astral.base64.encode(file.data), content_type = content_type(file.name) }
				refresh()
			end)
		elseif action == "save" then
			local current = saved()
			local problem = validate(draft, current, original)
			if problem then
				refresh(problem)
				return
			end
			local index = original and find(current, original)
			if index then
				current[index] = draft
			else
				current[#current + 1] = draft
			end
			if original and original ~= draft.name then
				astral.commands.unregister(original)
			end
			local ok = pcall(store, current)
			if not ok then
				refresh(t("SAVE_FAILED"))
				return
			end
			register(draft)
			sheet:close()
			astral.ui.toast(t("PLUGIN_NAME"), t("COMMAND_SAVED", { name = draft.name }))
		elseif action == "delete" then
			astral.ui.confirm({
				title = t("DELETE_COMMAND"),
				message = t("DELETE_COMMAND_BODY", { name = original }),
				confirm = t("DELETE"),
				destructive = true,
			}, function(confirmed)
				if not confirmed then
					return
				end
				local current = saved()
				local index = find(current, original)
				if index then
					table.remove(current, index)
					store(current)
				end
				astral.commands.unregister(original)
				sheet:close()
			end)
		end
	end)
end

open_list = function()
	local items = {
		{ type = astral.SheetItemType.Text, text = t("SETTINGS_INTRO") },
		{ type = astral.SheetItemType.Heading, text = t("REGISTERED_COMMANDS") },
	}
	for _, command in ipairs(saved()) do
		items[#items + 1] = {
			type = astral.SheetItemType.Row,
			action = "edit:" .. command.name,
			items = {
				{ type = astral.SheetItemType.Text, text = t("COMMAND_TITLE", { name = command.name }) },
			},
		}
	end
	items[#items + 1] = {
		type = astral.SheetItemType.Buttons,
		buttons = { { label = t("CREATE_COMMAND"), action = "create", style = astral.ButtonStyle.Primary } },
	}
	local sheet
	sheet = astral.ui.sheet({ title = t("PLUGIN_NAME"), items = items, expanded = true }, function(action)
		local name = tostring(action):match("^edit:(.+)$")
		if name or action == "create" then
			sheet:close()
			open_editor(name)
		end
	end)
end

astral.settings.button("manage", t("REGISTERED_COMMANDS"), open_list, { subtitle = t("SETTINGS_INTRO") })

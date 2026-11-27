local t = astral.i18n.t

local MAXIMUM_ENTRIES = 500
local MAXIMUM_EDITS = 5
local MAXIMUM_RECENT = 500
local MAXIMUM_CHANNELS = 100
local DELETED = 0xFFF23F43
local EDITED = 0xFF949BA4
local HIGHLIGHT = "#F23F4326"
local REDRAW_DELAY_MS = 300

local logging = astral.settings.section(t("LOGGING"))
logging:switch("logDeletes", t("LOG_DELETES"), true, t("LOG_DELETES_SUBTITLE"))
logging:switch("logEdits", t("LOG_EDITS"), true, t("LOG_EDITS_SUBTITLE"))
logging:switch("logDeletedAttachments", t("LOG_DELETED_ATTACHMENTS"), true, t("LOG_DELETED_ATTACHMENTS_SUBTITLE"))
logging:switch("logDeletedChannels", t("LOG_DELETED_CHANNELS"), true, t("LOG_DELETED_CHANNELS_SUBTITLE"))
logging:switch("persistLog", t("PERSIST_LOG"), true, t("PERSIST_LOG_SUBTITLE"))

local deleted_section = astral.settings.section(t("DELETED_MESSAGES"))
deleted_section:switch("redText", t("RED_TEXT"), true, t("RED_TEXT_SUBTITLE"))
deleted_section:switch("deletedMarker", t("DELETED_MARKER"), true, t("DELETED_MARKER_SUBTITLE"))
deleted_section:switch("deletedHighlight", t("DELETED_HIGHLIGHT"), false, t("DELETED_HIGHLIGHT_TINT_SUBTITLE"))
deleted_section:color("highlightBackground", t("HIGHLIGHT_BACKGROUND"), HIGHLIGHT)

local edited_section = astral.settings.section(t("EDITED_MESSAGES"))
edited_section:switch("inlineEdits", t("INLINE_EDITS"), true, t("INLINE_EDITS_SUBTITLE"))
edited_section:text("editedText", t("EDITED_TEXT"), "", { subtitle = t("EDITED_TEXT_SUBTITLE"), max_length = 64 })

local preferences = astral.settings.section(t("PREFERENCES"))
preferences:switch("ignoreBots", t("IGNORE_BOTS"), true, t("IGNORE_BOTS_SUBTITLE"))
preferences:switch("ignoreSelf", t("IGNORE_SELF"), false, t("IGNORE_SELF_SUBTITLE"))
preferences:text("ignoreUsers", t("IGNORE_USERS"), "", { subtitle = t("IGNORE_USERS_SUBTITLE"), max_length = 4000 })
preferences:text(
	"ignoreChannels",
	t("IGNORE_CHANNELS"),
	"",
	{ subtitle = t("IGNORE_CHANNELS_SUBTITLE"), max_length = 4000 }
)
preferences:text("ignoreGuilds", t("IGNORE_GUILDS"), "", { subtitle = t("IGNORE_GUILDS_SUBTITLE"), max_length = 4000 })

local log = { deleted = {}, edits = {}, attachments = {}, order = {} }
local deleted_channels = {}
local dismissed = {}
local recent, recent_order = {}, {}
local current_user

local function setting(key, fallback)
	local value = astral.settings.get(key, fallback)
	if value == nil then
		return fallback
	end
	return value
end

local function id_listed(key, id)
	if not id then
		return false
	end
	for entry in tostring(setting(key, "") or ""):gmatch("%d+") do
		if entry == id then
			return true
		end
	end
	return false
end

local function attachment_names(attachments)
	local names = {}
	for _, attachment in ipairs(type(attachments) == "table" and attachments or {}) do
		if type(attachment) == "table" and type(attachment.id) == "string" then
			names[attachment.id] = tostring(attachment.filename or attachment.id)
		end
	end
	return names
end

local function remember_recent(data)
	if type(data.id) ~= "string" or type(data.author) ~= "table" then
		return
	end
	if not recent[data.id] then
		recent_order[#recent_order + 1] = data.id
		while #recent_order > MAXIMUM_RECENT do
			recent[table.remove(recent_order, 1)] = nil
		end
	end
	recent[data.id] = {
		author = { id = data.author.id, bot = data.author.bot == true },
		content = data.content,
		channel_id = data.channel_id,
		attachments = data.attachments,
	}
end

local function message_of(channel, id)
	return astral.discord.message(channel, id) or recent[id]
end

local function load()
	if not setting("persistLog", true) then
		return
	end
	local saved = astral.storage.get("log")
	if type(saved) == "table" then
		log.deleted = saved.deleted or {}
		log.edits = saved.edits or {}
		log.attachments = saved.attachments or {}
		log.order = saved.order or {}
	end
	local channels = astral.storage.get("deleted_channels")
	deleted_channels = type(channels) == "table" and channels or {}
end

local function save()
	if setting("persistLog", true) then
		astral.storage.set("log", log)
	end
end

local function track(id)
	if log.deleted[id] or log.edits[id] or log.attachments[id] then
		return
	end
	log.order[#log.order + 1] = id
	while #log.order > MAXIMUM_ENTRIES do
		local oldest = table.remove(log.order, 1)
		log.deleted[oldest] = nil
		log.edits[oldest] = nil
		log.attachments[oldest] = nil
	end
end

local function ignored(message, channel)
	if not message or not message.author then
		return true
	end
	if setting("ignoreBots", true) and message.author.bot then
		return true
	end
	if setting("ignoreSelf", false) and message.author.id == current_user then
		return true
	end
	if id_listed("ignoreUsers", message.author.id) or id_listed("ignoreChannels", channel) then
		return true
	end
	return id_listed("ignoreGuilds", channel and astral.discord.channel_guild(channel))
end

local function on_delete(channel, id)
	if ignored(message_of(channel, id), channel) then
		return false
	end
	track(id)
	log.deleted[id] = true
	return true
end

local function note_removed_attachments(data, message)
	if not setting("logDeletedAttachments", true) or type(data.attachments) ~= "table" then
		return false
	end
	local now = attachment_names(data.attachments)
	local removed = log.attachments[data.id] or {}
	local found = false
	for id, name in pairs(attachment_names(message and message.attachments)) do
		if not now[id] then
			removed[#removed + 1] = name
			found = true
		end
	end
	if found then
		track(data.id)
		log.attachments[data.id] = removed
	end
	return found
end

local function on_update(data)
	local message = message_of(data.channel_id, data.id)
	if ignored(message, data.channel_id) then
		return
	end
	note_removed_attachments(data, message)
	if recent[data.id] and type(data.attachments) == "table" then
		recent[data.id].attachments = data.attachments
	end
	if type(data.content) ~= "string" then
		return
	end
	local previous = message and message.content
	if recent[data.id] then
		recent[data.id].content = data.content
	end
	if type(previous) ~= "string" or previous == data.content then
		return
	end
	track(data.id)
	local edits = log.edits[data.id] or {}
	edits[#edits + 1] = { text = previous, at = astral.time.now_ms() }
	while #edits > MAXIMUM_EDITS do
		table.remove(edits, 1)
	end
	log.edits[data.id] = edits
end

local function on_channel_delete(data)
	if not setting("logDeletedChannels", true) or type(data.id) ~= "string" or not data.guild_id then
		return
	end
	table.insert(deleted_channels, 1, {
		id = data.id,
		name = tostring(data.name or data.id),
		parent = data.parent_id,
		thread = data.type == 10 or data.type == 11 or data.type == 12,
		at = astral.time.now_ms(),
	})
	while #deleted_channels > MAXIMUM_CHANNELS do
		table.remove(deleted_channels)
	end
	astral.storage.set("deleted_channels", deleted_channels)
end

astral.events.on(astral.Event.Gateway, function(event)
	local data = event.data
	if event.type == "READY" then
		current_user = type(data.user) == "table" and data.user.id or nil
		return nil
	end
	if event.type == "MESSAGE_CREATE" then
		remember_recent(data)
		return nil
	end
	if event.type == "CHANNEL_DELETE" or event.type == "THREAD_DELETE" then
		on_channel_delete(data)
		return nil
	end
	if event.type == "MESSAGE_UPDATE" then
		if setting("logEdits", true) then
			on_update(data)
			save()
			astral.ui.refresh()
			astral.time.after(REDRAW_DELAY_MS, astral.ui.refresh)
		end
		return nil
	end
	if not setting("logDeletes", true) then
		return nil
	end
	if event.type == "MESSAGE_DELETE" then
		if dismissed[data.id] then
			dismissed[data.id] = nil
			return nil
		end
		if not on_delete(data.channel_id, data.id) then
			return nil
		end
		save()
		astral.ui.refresh()
		return false
	end
	local kept = astral.json.array()
	for _, id in ipairs(data.ids or {}) do
		if not on_delete(data.channel_id, id) then
			kept[#kept + 1] = id
		end
	end
	save()
	astral.ui.refresh()
	if #kept == 0 then
		return false
	end
	data.ids = kept
	return data
end, {
	"READY",
	"MESSAGE_CREATE",
	"MESSAGE_DELETE",
	"MESSAGE_DELETE_BULK",
	"MESSAGE_UPDATE",
	"CHANNEL_DELETE",
	"THREAD_DELETE",
})

local function edited_label()
	local custom = tostring(setting("editedText", "") or ""):match("^%s*(.-)%s*$")
	return custom ~= "" and (" " .. custom) or t("EDITED_SUFFIX")
end

local function edit_text(entry)
	return type(entry) == "table" and tostring(entry.text or "") or tostring(entry or "")
end

astral.ui.text_transform(function(spans, context)
	local id = context.message_id
	local deleted = log.deleted[id]
	local edits = setting("inlineEdits", true) and log.edits[id] or nil
	local attachments = log.attachments[id]
	if not deleted and not edits and not attachments then
		return nil
	end
	local out = {}
	for _, previous in ipairs(edits or {}) do
		out[#out + 1] = { text = edit_text(previous), color = EDITED, strike = true }
		out[#out + 1] = { text = edited_label() .. "\n", color = EDITED, scale = 0.8 }
	end
	for _, span in ipairs(spans) do
		if deleted and setting("redText", true) and not span._raw then
			span.color = DELETED
		end
		out[#out + 1] = span
	end
	for _, name in ipairs(attachments or {}) do
		out[#out + 1] = { text = "\n" .. t("DELETED_ATTACHMENT_PREFIX") .. name, color = DELETED, scale = 0.9 }
	end
	if deleted and setting("deletedMarker", true) then
		out[#out + 1] = { text = "\n" .. t("MESSAGE_DELETED"), color = EDITED, scale = 0.8 }
	end
	return out
end)

astral.ui.patch("message-row", function(node)
	if not node.message_id or not log.deleted[node.message_id] or not setting("deletedHighlight", false) then
		return nil
	end
	return { background = setting("highlightBackground", HIGHLIGHT) }
end)

local function forget(id)
	log.deleted[id] = nil
	log.edits[id] = nil
	log.attachments[id] = nil
end

local function dismiss(node)
	local id = node.message_id
	if not id or not log.deleted[id] then
		return
	end
	forget(id)
	dismissed[id] = true
	save()
	astral.gateway.dispatch("MESSAGE_DELETE", { id = id, channel_id = node.channel_id })
end

local function clear_history(node)
	local id = node.message_id
	if not id then
		return
	end
	log.edits[id] = nil
	log.attachments[id] = nil
	save()
	astral.ui.refresh()
end

local function history_items(node)
	local items = {}
	for _, entry in ipairs(log.edits[node.message_id] or {}) do
		local at = type(entry) == "table" and entry.at
		local time = at and astral.time.iso(at, { local_time = true }) or "?"
		items[#items + 1] = { type = astral.SheetItemType.Heading, text = t("BEFORE_EDIT", { time = time }) }
		local text = edit_text(entry)
		items[#items + 1] = { type = astral.SheetItemType.Text, text = text ~= "" and text or t("EMPTY_MESSAGE") }
	end
	local current = astral.discord.message(node.channel_id, node.message_id)
	local text = current and tostring(current.content or "") or ""
	items[#items + 1] = { type = astral.SheetItemType.Heading, text = t("CURRENT_VERSION") }
	items[#items + 1] = { type = astral.SheetItemType.Text, text = text ~= "" and text or t("EMPTY_MESSAGE") }
	return items
end

local function view_history(node)
	astral.ui.sheet({ title = t("HISTORY_TITLE"), expanded = true, items = history_items(node) }, function() end)
end

astral.ui.message_action(t("DISMISS_MESSAGE"), dismiss, {
	icon = "res://ic_mana_trash",
	when = function(node)
		return node.message_id ~= nil and log.deleted[node.message_id] == true
	end,
})

astral.ui.message_action(t("VIEW_EDIT_HISTORY"), view_history, {
	icon = "res://ic_mana_clock",
	when = function(node)
		return node.message_id ~= nil and log.edits[node.message_id] ~= nil
	end,
})

astral.ui.message_action(t("CLEAR_MESSAGE_HISTORY"), clear_history, {
	icon = "res://ic_mana_trash",
	when = function(node)
		return node.message_id ~= nil
			and not log.deleted[node.message_id]
			and (log.edits[node.message_id] ~= nil or log.attachments[node.message_id] ~= nil)
	end,
})

local function clear_log()
	local count = #log.order
	log = { deleted = {}, edits = {}, attachments = {}, order = {} }
	save()
	astral.ui.refresh()
	astral.ui.toast(t("LOG_CLEARED", { count = count }))
end

local function channel_label(entry)
	if entry.thread and entry.parent then
		return t("DELETED_THREAD_IN", { channel = entry.parent })
	end
	return "#" .. entry.name
end

local function show_deleted_channels()
	local items = {
		{
			type = astral.SheetItemType.Text,
			text = t("DELETED_CHANNELS_DESCRIPTION"),
			size = 13,
			color = astral.SheetColor.Muted,
		},
	}
	if #deleted_channels == 0 then
		items[#items + 1] = { type = astral.SheetItemType.Text, text = t("NO_DELETED_CHANNELS") }
	end
	for _, entry in ipairs(deleted_channels) do
		items[#items + 1] = {
			type = astral.SheetItemType.Column,
			spacing = 2,
			items = {
				{
					type = astral.SheetItemType.Text,
					text = entry.thread and entry.name or channel_label(entry),
					bold = true,
				},
				{
					type = astral.SheetItemType.Text,
					text = (entry.thread and (channel_label(entry) .. " · ") or "")
						.. t("DELETED_CHANNEL_AT", { time = astral.time.iso(entry.at, { local_time = true }) }),
					size = 12,
					color = astral.SheetColor.Muted,
				},
			},
		}
	end
	local sheet
	sheet = astral.ui.sheet({
		title = t("DELETED_CHANNELS"),
		expanded = true,
		items = items,
		header = {
			{
				type = astral.SheetItemType.Buttons,
				buttons = {
					{ label = t("CLEAR_DELETED_CHANNELS"), action = "clear", style = astral.ButtonStyle.Danger },
				},
			},
		},
	}, function(action)
		if action == "clear" then
			deleted_channels = {}
			astral.storage.set("deleted_channels", deleted_channels)
			sheet:close()
		end
	end)
end

logging:button("deletedChannels", t("DELETED_CHANNELS"), show_deleted_channels)
local manage = astral.settings.section(t("ABOUT_MESSAGE_LOGGER"))
manage:button("clearLog", t("CLEAR_MESSAGE_LOG"), clear_log)

load()

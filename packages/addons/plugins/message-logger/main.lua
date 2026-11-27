local MAXIMUM_ENTRIES = 500
local MAXIMUM_EDITS = 5
local MAXIMUM_RECENT = 500
local DELETED = 0xFFF23F43
local EDITED = 0xFF949BA4
local DELETED_NOTE = "This message was deleted"

local settings = astral.settings.section("Logging")
settings:switch("log_deletes", "Keep deleted messages", true, "Leaves deleted messages in the chat")
settings:switch("log_edits", "Keep edit history", true, "Shows what an edited message said before")
settings:switch("red_text", "Red text", true, "Draws deleted messages in red")
settings:switch("marker", "Deleted marker", true, "Shows This message was deleted under deleted messages")
settings:switch("ignore_bots", "Ignore bots", true, "Doesn't log messages from bots")
settings:switch("ignore_self", "Ignore my messages", false, "Doesn't log your own messages")
settings:switch("persist", "Keep the log after restarting", true, "Saves the log on this device")

local log = { deleted = {}, edits = {}, order = {} }
local dismissed = {}
local recent, recent_order = {}, {}

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
	}
end

local function message_of(channel, id)
	return astral.discord.message(channel, id) or recent[id]
end
local current_user

local function setting(key, fallback)
	local value = astral.settings.get(key, fallback)
	if value == nil then
		return fallback
	end
	return value
end

local function load()
	if not setting("persist", true) then
		return
	end
	local saved = astral.storage.get("log")
	if type(saved) == "table" then
		log.deleted = saved.deleted or {}
		log.edits = saved.edits or {}
		log.order = saved.order or {}
	end
end

local function save()
	if setting("persist", true) then
		astral.storage.set("log", log)
	end
end

local function track(id)
	if log.deleted[id] or log.edits[id] then
		return
	end
	log.order[#log.order + 1] = id
	while #log.order > MAXIMUM_ENTRIES do
		local oldest = table.remove(log.order, 1)
		log.deleted[oldest] = nil
		log.edits[oldest] = nil
	end
end

local function ignored(message)
	if not message or not message.author then
		return true
	end
	if setting("ignore_bots", true) and message.author.bot then
		return true
	end
	return setting("ignore_self", false) and message.author.id == current_user
end

local function on_delete(channel, id)
	local message = message_of(channel, id)
	if ignored(message) then
		return false
	end
	track(id)
	log.deleted[id] = true
	return true
end

local function on_update(data)
	if type(data.content) ~= "string" then
		return
	end
	local message = message_of(data.channel_id, data.id)
	local previous = message and message.content
	if recent[data.id] then
		recent[data.id].content = data.content
	end
	if ignored(message) or type(previous) ~= "string" or previous == data.content then
		return
	end
	track(data.id)
	local edits = log.edits[data.id] or {}
	edits[#edits + 1] = previous
	while #edits > MAXIMUM_EDITS do
		table.remove(edits, 1)
	end
	log.edits[data.id] = edits
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
	if event.type == "MESSAGE_UPDATE" then
		if setting("log_edits", true) then
			on_update(data)
			save()
			astral.ui.refresh()
		end
		return nil
	end
	if not setting("log_deletes", true) then
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
end, { "READY", "MESSAGE_CREATE", "MESSAGE_DELETE", "MESSAGE_DELETE_BULK", "MESSAGE_UPDATE" })

astral.ui.text_transform(function(spans, context)
	local id = context.message_id
	local deleted = log.deleted[id]
	local edits = log.edits[id]
	if not deleted and not edits then
		return nil
	end
	local out = {}
	for _, previous in ipairs(edits or {}) do
		out[#out + 1] = { text = previous, color = EDITED, strike = true }
		out[#out + 1] = { text = " (edited)\n", color = EDITED, scale = 0.8 }
	end
	for _, span in ipairs(spans) do
		if deleted and setting("red_text", true) and not span._raw then
			span.color = DELETED
		end
		out[#out + 1] = span
	end
	if deleted and setting("marker", true) then
		out[#out + 1] = { text = "\n" .. DELETED_NOTE, color = EDITED, scale = 0.8 }
	end
	return out
end)

local function dismiss(node)
	local id = node.message_id
	if not id or not log.deleted[id] then
		return
	end
	log.deleted[id] = nil
	log.edits[id] = nil
	dismissed[id] = true
	save()
	astral.gateway.dispatch("MESSAGE_DELETE", { id = id, channel_id = node.channel_id })
end

astral.ui.message_action("Dismiss Message", dismiss, {
	icon = "res://ic_mana_trash",
	when = function(node)
		return node.message_id ~= nil and log.deleted[node.message_id] == true
	end,
})

load()

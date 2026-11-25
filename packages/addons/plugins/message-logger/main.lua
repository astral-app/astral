local MAXIMUM_ENTRIES = 500
local MAXIMUM_EDITS = 5
local DELETED = 0xFFF23F43
local EDITED = 0xFF949BA4
local EPHEMERAL_FLAG = 64

local settings = astral.settings.section("Logging")
settings:switch("log_deletes", "Keep deleted messages", true, "Leaves deleted messages in the chat")
settings:switch("log_edits", "Keep edit history", true, "Shows what an edited message said before")
settings:switch("red_text", "Red text", true, "Draws deleted messages in red")
settings:switch(
	"marker",
	"Deleted marker",
	true,
	"Marks deleted messages as only visible to you, with Discord's Dismiss link"
)
settings:switch("ignore_bots", "Ignore bots", true, "Doesn't log messages from bots")
settings:switch("ignore_self", "Ignore my messages", false, "Doesn't log your own messages")
settings:switch("persist", "Keep the log after restarting", true, "Saves the log on this device")

local log = { deleted = {}, edits = {}, order = {} }
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
	local message = astral.discord.message(channel, id)
	if ignored(message) then
		return false
	end
	track(id)
	log.deleted[id] = true
	return true
end

local function mark_ephemeral(channel, id)
	if not setting("marker", true) then
		return
	end
	astral.time.after(0, function()
		local message = astral.discord.raw_message(channel, id)
		if type(message) ~= "table" then
			return
		end
		message.flags = (tonumber(message.flags) or 0) | EPHEMERAL_FLAG
		astral.gateway.dispatch("MESSAGE_UPDATE", message)
	end)
end

local function on_update(data)
	if type(data.content) ~= "string" then
		return
	end
	local message = astral.discord.message(data.channel_id, data.id)
	local previous = message and message.content
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

astral.events.on("gateway", function(event)
	local data = event.data
	if event.type == "READY" then
		current_user = type(data.user) == "table" and data.user.id or nil
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
		if not on_delete(data.channel_id, data.id) then
			return nil
		end
		mark_ephemeral(data.channel_id, data.id)
		save()
		astral.ui.refresh()
		return false
	end
	local kept = astral.json.array()
	for _, id in ipairs(data.ids or {}) do
		if on_delete(data.channel_id, id) then
			mark_ephemeral(data.channel_id, id)
		else
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
end, { "READY", "MESSAGE_DELETE", "MESSAGE_DELETE_BULK", "MESSAGE_UPDATE" })

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
	return out
end)

load()

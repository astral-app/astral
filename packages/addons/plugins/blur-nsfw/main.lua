local IS_SPOILER = 8
local SPOILER_PREFIX = "SPOILER_"
local t = astral.i18n.t

local section = astral.settings.section(t("BLUR"))
section:switch("attachments", t("ATTACHMENTS"), true, t("ATTACHMENTS_SUBTITLE"))

local nsfw = {}
local parents = {}

local function remember(channel)
	if type(channel) ~= "table" or not channel.id then
		return
	end
	nsfw[channel.id] = channel.nsfw == true or nil
	parents[channel.id] = channel.parent_id
end

local function remember_guild(guild)
	if type(guild) ~= "table" then
		return
	end
	for _, list in ipairs({ guild.channels or {}, guild.threads or {} }) do
		for _, channel in ipairs(list) do
			remember(channel)
		end
	end
end

local function is_nsfw(channel)
	return channel ~= nil and (nsfw[channel] or nsfw[parents[channel] or ""]) == true
end

local function has_flag(flags, flag)
	return math.floor((tonumber(flags) or 0) / flag) % 2 == 1
end

local function spoil(attachment)
	if type(attachment) ~= "table" then
		return false
	end
	local changed = false
	if not has_flag(attachment.flags, IS_SPOILER) then
		attachment.flags = (tonumber(attachment.flags) or 0) + IS_SPOILER
		changed = true
	end
	if type(attachment.filename) == "string" and attachment.filename:sub(1, #SPOILER_PREFIX) ~= SPOILER_PREFIX then
		attachment.filename = SPOILER_PREFIX .. attachment.filename
		changed = true
	end
	return changed
end

local function blur(message)
	if type(message) ~= "table" or not astral.settings.get("attachments", true) or not is_nsfw(message.channel_id) then
		return false
	end
	local changed = false
	for _, attachment in ipairs(message.attachments or {}) do
		changed = spoil(attachment) or changed
	end
	for _, snapshot in ipairs(message.message_snapshots or {}) do
		for _, attachment in ipairs(type(snapshot.message) == "table" and snapshot.message.attachments or {}) do
			changed = spoil(attachment) or changed
		end
	end
	return changed
end

astral.events.on(astral.Event.Gateway, function(event)
	local data = event.data
	if event.type == "READY" then
		for _, guild in ipairs(type(data) == "table" and data.guilds or {}) do
			remember_guild(guild)
		end
	elseif event.type == "GUILD_CREATE" then
		remember_guild(data)
	elseif
		event.type == "CHANNEL_CREATE"
		or event.type == "CHANNEL_UPDATE"
		or event.type == "THREAD_CREATE"
		or event.type == "THREAD_UPDATE"
	then
		remember(data)
	elseif event.type == "MESSAGE_CREATE" or event.type == "MESSAGE_UPDATE" then
		return blur(data) and data or nil
	end
	return nil
end, {
	"READY",
	"GUILD_CREATE",
	"CHANNEL_CREATE",
	"CHANNEL_UPDATE",
	"THREAD_CREATE",
	"THREAD_UPDATE",
	"MESSAGE_CREATE",
	"MESSAGE_UPDATE",
})

astral.net.intercept({
	method = "GET",
	path = "^/api/v%d+/guilds/%d+/channels$",
	response = function(_, response)
		local channels = response.status == 200 and astral.json.decode(response.body)
		for _, channel in ipairs(type(channels) == "table" and channels or {}) do
			remember(channel)
		end
		return nil
	end,
})

astral.net.intercept({
	method = "GET",
	path = "^/api/v%d+/channels/%d+/messages$",
	response = function(request, response)
		if response.status ~= 200 or not is_nsfw(request.path:match("/channels/(%d+)/")) then
			return nil
		end
		local messages = astral.json.decode(response.body)
		if type(messages) ~= "table" then
			return nil
		end
		local changed = false
		for _, message in ipairs(messages) do
			message.channel_id = message.channel_id or request.path:match("/channels/(%d+)/")
			changed = blur(message) or changed
		end
		return changed and { body = astral.json.encode(messages) } or nil
	end,
})

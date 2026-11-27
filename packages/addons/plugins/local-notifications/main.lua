local t = astral.i18n.t
local MAXIMUM_BODY = 180

local settings = astral.settings.section(t("NOTIFICATIONS"))
settings:switch("keep_alive", t("KEEP_ALIVE"), true, t("KEEP_ALIVE_SUBTITLE"))
settings:switch("direct", t("DIRECT_MESSAGES"), true, t("DIRECT_MESSAGES_SUBTITLE"))
settings:switch("mentions", t("MENTIONS"), true)
settings:switch("everyone", t("EVERYONE"), false)
settings:switch("preview", t("PREVIEW"), true, t("PREVIEW_SUBTITLE"))

local in_background = false
local current_user

local function setting(key, fallback)
	local value = astral.settings.get(key, fallback)
	if value == nil then
		return fallback
	end
	return value
end

local function mentions_me(data)
	for _, mention in ipairs(data.mentions or {}) do
		if mention.id == current_user then
			return true
		end
	end
	return false
end

local function wanted(data)
	if not current_user or type(data.author) ~= "table" or data.author.id == current_user then
		return false
	end
	if not data.guild_id then
		return setting("direct", true)
	end
	if setting("mentions", true) and mentions_me(data) then
		return true
	end
	return setting("everyone", false) and data.mention_everyone == true
end

local function body_of(data)
	local text = setting("preview", true) and tostring(data.content or "") or ""
	if text == "" then
		text = (data.attachments and #data.attachments > 0) and t("ATTACHMENT") or t("NEW_MESSAGE")
	end
	if #text > MAXIMUM_BODY then
		text = text:sub(1, MAXIMUM_BODY) .. "…"
	end
	return text
end

local function title_of(data)
	local author = data.author.global_name or data.author.username or t("SOMEONE")
	if not data.guild_id then
		return author
	end
	local guild = astral.discord.guild(data.guild_id)
	return guild and guild.name and (author .. " in " .. guild.name) or author
end

astral.events.on(astral.Event.AppBackground, function()
	in_background = true
	if setting("keep_alive", true) then
		astral.background.keep_alive(true)
	end
end)

astral.events.on(astral.Event.AppForeground, function()
	in_background = false
	astral.background.keep_alive(false)
end)

astral.events.on(astral.Event.Gateway, function(event)
	local data = event.data
	if event.type == "READY" then
		current_user = type(data.user) == "table" and data.user.id or nil
	elseif in_background and wanted(data) then
		astral.ui.notify(title_of(data), body_of(data), { channel_id = data.channel_id, message_id = data.id })
	end
	return nil
end, { "READY", "MESSAGE_CREATE" })

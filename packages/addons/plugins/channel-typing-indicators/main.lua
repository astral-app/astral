local TYPING_MS = 10000
local DOTS = 0xFF949BA4

local settings = astral.settings.section("Typing")
settings:switch("current", "Include the open channel", true, "Show dots on the channel you are looking at")
settings:switch("blocked", "Include blocked people", false, "Show dots when someone you blocked is typing")

local typing = {}
local current_user
local expiry_timer

local function counts(channel, now)
	local count = 0
	for user, until_ms in pairs(typing[channel] or {}) do
		if until_ms > now then
			count = count + 1
		else
			typing[channel][user] = nil
		end
	end
	return count
end

local function schedule_expiry()
	if expiry_timer then
		astral.time.cancel(expiry_timer)
	end
	expiry_timer = astral.time.after(TYPING_MS + 100, function()
		expiry_timer = nil
		astral.ui.refresh()
	end)
end

astral.events.on(astral.Event.Gateway, function(event)
	local data = event.data
	if event.type == "READY" then
		current_user = type(data.user) == "table" and data.user.id or nil
	elseif event.type == "TYPING_START" then
		if data.user_id == current_user then
			return nil
		end
		if not astral.settings.get("blocked", false) and astral.discord.relationship(data.user_id) == "blocked" then
			return nil
		end
		typing[data.channel_id] = typing[data.channel_id] or {}
		typing[data.channel_id][data.user_id] = astral.time.now_ms() + TYPING_MS
		schedule_expiry()
		astral.ui.refresh()
	elseif event.type == "MESSAGE_CREATE" and type(data.author) == "table" and typing[data.channel_id] then
		typing[data.channel_id][data.author.id] = nil
		astral.ui.refresh()
	end
	return nil
end, { "READY", "TYPING_START", "MESSAGE_CREATE" })

astral.ui.insert({ id = "navigation.channel:" }, function(node)
	local channel = node.key
	if not astral.settings.get("current", true) and channel == node.channel_id then
		return nil
	end
	local count = counts(channel, astral.time.now_ms())
	if count == 0 then
		return nil
	end
	return {
		type = astral.InsertNodeType.Text,
		text = count > 1 and ("••• " .. count) or "•••",
		color = DOTS,
		size = 13,
		bold = true,
	}
end)

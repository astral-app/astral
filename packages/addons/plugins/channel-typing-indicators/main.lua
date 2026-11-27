local t = astral.i18n.t

local TYPING_MS = 10000
local DOTS = 0xFF949BA4
local CDN = "https://cdn.discordapp.com"
local DEFAULT_AVATARS = 6
local MAX_AVATARS = 3
local AVATAR_SIZE = 16
local AVATAR_OVERLAP = -4
local SHOW_DOTS = 1
local DOT_COUNT = 3
local DOT_PULSE_MS = 1200
local DOT_STAGGER_MS = 200
local SHOW_AVATARS = 2
local APPEARANCES = { both = SHOW_DOTS | SHOW_AVATARS, dots = SHOW_DOTS, avatars = SHOW_AVATARS }

local settings = astral.settings.section(t("CHANNEL_TYPING_INDICATORS"))
settings:choice("appearance", t("APPEARANCE"), {
	astral.settings.option("both", t("APPEARANCE_BOTH")),
	astral.settings.option("dots", t("APPEARANCE_DOTS")),
	astral.settings.option("avatars", t("APPEARANCE_AVATARS")),
}, "both")
settings:switch("current", t("INCLUDE_CURRENT_CHANNEL"), true, t("INCLUDE_CURRENT_CHANNEL_SUBTITLE"))
settings:switch("muted", t("INCLUDE_MUTED_CHANNELS"), false, t("INCLUDE_MUTED_CHANNELS_SUBTITLE"))
settings:switch("ignored", t("INCLUDE_IGNORED_USERS"), false, t("INCLUDE_IGNORED_USERS_SUBTITLE"))
settings:switch("blocked", t("INCLUDE_BLOCKED_USERS"), false, t("INCLUDE_BLOCKED_USERS_SUBTITLE"))

local typing = {}
local current_user
local expiry_timer

local function hidden_user(user)
	if not astral.settings.get("blocked", false) and astral.discord.relationship(user) == "blocked" then
		return true
	end
	return not astral.settings.get("ignored", false) and astral.discord.ignored(user)
end

local function typers(channel, now)
	local users = {}
	for user, until_ms in pairs(typing[channel] or {}) do
		if until_ms <= now then
			typing[channel][user] = nil
		elseif not hidden_user(user) then
			users[#users + 1] = user
		end
	end
	table.sort(users)
	return users
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

local function default_avatar(id)
	local number = math.tointeger(tonumber(id)) or 0
	return string.format("%s/embed/avatars/%d.png", CDN, (number >> 22) % DEFAULT_AVATARS)
end

local function avatar_url(id)
	local user = astral.discord.user(id)
	if user and type(user.avatar) == "string" and user.avatar ~= "" then
		return string.format("%s/avatars/%s/%s.webp?size=64", CDN, id, user.avatar)
	end
	return default_avatar(id)
end

local function display_name(id)
	local user = astral.discord.user(id)
	return user and (user.global_name or user.username) or t("SOMEONE")
end

local function typing_text(users)
	local names = {}
	for index = 1, math.min(#users, 3) do
		names[index] = display_name(users[index])
	end
	local key = ({ "TYPING_ONE", "TYPING_TWO", "TYPING_THREE" })[#users] or "TYPING_SEVERAL"
	return (t(key):gsub("{(%a)}", function(slot)
		return names[slot:byte() - ("a"):byte() + 1] or ""
	end))
end

local function avatars(users)
	local nodes = {}
	for index = 1, math.min(#users, MAX_AVATARS) do
		nodes[index] = {
			type = astral.InsertNodeType.Image,
			icon = avatar_url(users[index]),
			cover = true,
			width = AVATAR_SIZE,
			height = AVATAR_SIZE,
			radius = AVATAR_SIZE / 2,
			margin = index > 1 and { AVATAR_OVERLAP, 0, 0, 0 } or 0,
		}
	end
	return nodes
end

local function indicator(users)
	local mode = APPEARANCES[astral.settings.get("appearance", "both")] or APPEARANCES.both
	local children = {}
	if mode & SHOW_AVATARS ~= 0 then
		for _, node in ipairs(avatars(users)) do
			children[#children + 1] = node
		end
	end
	if mode & SHOW_DOTS ~= 0 then
		for index = 1, DOT_COUNT do
			children[#children + 1] = {
				type = astral.InsertNodeType.Text,
				text = "•",
				color = DOTS,
				size = 13,
				bold = true,
				pulse = DOT_PULSE_MS,
				pulse_delay = (index - 1) * DOT_STAGGER_MS,
				margin = (index == 1 and #children > 0) and { 4, 0, 0, 0 } or { 1, 0, 0, 0 },
			}
		end
		if #users > 1 and mode == SHOW_DOTS then
			children[#children + 1] = {
				type = astral.InsertNodeType.Text,
				text = tostring(#users),
				color = DOTS,
				size = 13,
				bold = true,
				margin = { 4, 0, 0, 0 },
			}
		end
	end
	return { type = astral.InsertNodeType.View, row = true, action = "typing", children = children }
end

astral.events.on(astral.Event.Gateway, function(event)
	local data = event.data
	if event.type == "READY" then
		current_user = type(data.user) == "table" and data.user.id or nil
	elseif event.type == "TYPING_START" then
		if data.user_id == current_user then
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

local function visible(channel, node)
	if not astral.settings.get("current", true) and channel == node.channel_id then
		return false
	end
	return astral.settings.get("muted", false) or not astral.discord.muted(channel)
end

astral.ui.insert({ id = "navigation.channel:", path = ">" }, function(node)
	local channel = node.key
	if not visible(channel, node) then
		return nil
	end
	local users = typers(channel, astral.time.now_ms())
	if #users == 0 then
		return nil
	end
	return indicator(users)
end, function(node)
	local users = typers(node.key, astral.time.now_ms())
	if #users > 0 then
		astral.ui.toast(t("CHANNEL_TYPING_INDICATORS"), typing_text(users))
	end
end)

for _, key in ipairs({ "appearance", "current", "muted", "ignored", "blocked" }) do
	astral.settings.on_change(key, astral.ui.refresh)
end

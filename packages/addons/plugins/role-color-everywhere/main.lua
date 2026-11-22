local t = astral.i18n.t

astral.settings.switch("chatMentions", t("COLOR_MENTIONS"), true, { subtitle = t("COLOR_MENTIONS_SUBTITLE") })
astral.settings.switch(
	"colorChatMessages",
	t("COLOR_MESSAGE_CONTENT"),
	false,
	{ subtitle = t("COLOR_MESSAGE_CONTENT_SUBTITLE") }
)
astral.settings.number(
	"messageSaturation",
	t("MESSAGE_COLOR_INTENSITY"),
	{ min = 0, max = 100, step = 10, default = 30 }
)

astral.settings.switch(
	"memberList",
	t("MEMBER_LIST_ROLE_HEADERS"),
	true,
	{ subtitle = t("MEMBER_LIST_ROLE_HEADERS_SUBTITLE") }
)
astral.settings.switch("typingUsers", t("TYPING_NAMES"), true, { subtitle = t("TYPING_NAMES_SUBTITLE") })
astral.settings.switch("pollResults", t("POLL_VOTERS"), true, { subtitle = t("POLL_VOTERS_SUBTITLE") })
astral.settings.switch("avatarNames", t("AVATAR_NAMES"), true, { subtitle = t("AVATAR_NAMES_SUBTITLE") })

local function roleColor(guild, roleIds)
	if not guild or not guild.roles then
		return nil
	end
	local best, bestPosition = nil, -1
	for _, id in ipairs(roleIds or {}) do
		local role = guild.roles[id]
		local color = role and math.tointeger(role.color) or 0
		if color > 0 and (role.position or 0) > bestPosition then
			best, bestPosition = color, role.position or 0
		end
	end
	return best
end

local function memberColor(guildId, guild, userId)
	if not guildId or not userId then
		return nil
	end
	local member = astral.discord.member(guildId, userId)
	return member and roleColor(guild, member.roles)
end

local function mentionedUser(message, name)
	for _, user in ipairs(message.mentions or {}) do
		for _, field in ipairs({ "nick", "global_name", "username" }) do
			if user[field] == name then
				return user
			end
		end
	end
	return nil
end

local function mentionedRole(guild, message, name)
	for _, id in ipairs(message.mention_roles or {}) do
		local role = guild and guild.roles and guild.roles[id]
		if role and role.name == name then
			return role
		end
	end
	return nil
end

local function mentionColor(node)
	local message = node:message()
	local guild = node:guild()
	if not message or not guild then
		return nil
	end
	local name = node.text:sub(2)
	local user = mentionedUser(message, name)
	if user then
		return memberColor(node.guild_id, guild, user.id)
	end
	local role = mentionedRole(guild, message, name)
	local color = role and math.tointeger(role.color) or 0
	return color > 0 and color or nil
end

astral.ui.mention(function(node)
	if not astral.settings.values.chatMentions then
		return nil
	end
	local color = mentionColor(node)
	return color and { color = color } or nil
end)

astral.ui.message_color(function(node)
	if not astral.settings.values.colorChatMessages then
		return nil
	end
	local message = node:message()
	local author = message and message.author
	local color = author and memberColor(node.guild_id, node:guild(), author.id)
	if not color then
		return nil
	end
	local intensity = math.floor((astral.settings.values.messageSaturation or 30) * 255 / 100 + 0.5)
	return astral.Color.argb(color):with_alpha(intensity)
end)

local COUNT_SEPARATOR = " \u{2014} "

local function headerRole(guild, text)
	local name = text:match("^(.-)" .. COUNT_SEPARATOR .. "%d+$")
	if not name or not guild or not guild.roles then
		return nil
	end
	for _, role in pairs(guild.roles) do
		if role.name == name then
			return role
		end
	end
	return nil
end

astral.ui.name_color(function(node)
	if node.kind == "header" then
		if not astral.settings.values.memberList then
			return nil
		end
		local role = headerRole(node:guild(), node.text)
		local color = role and math.tointeger(role.color) or 0
		return color > 0 and color or nil
	end
	local setting = ({ voter = "pollResults", typing = "typingUsers" })[node.kind] or "avatarNames"
	if not astral.settings.values[setting] then
		return nil
	end
	return memberColor(node.guild_id, node:guild(), node.user_id)
end)

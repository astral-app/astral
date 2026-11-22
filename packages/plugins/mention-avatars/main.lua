local t = astral.i18n.t

local CDN = "https://cdn.discordapp.com"
local DEFAULT_AVATARS = 6

local section = astral.settings.section(t("MENTIONS"))
section:switch("showAtSymbol", t("SHOW_AT_SYMBOL"), true, t("SHOW_AT_SYMBOL_SUBTITLE"))

local function defaultAvatar(id)
	local number = math.tointeger(tonumber(id)) or 0
	return string.format("%s/embed/avatars/%d.png", CDN, (number >> 22) % DEFAULT_AVATARS)
end

local function avatarUrl(user)
	if type(user.avatar) == "string" and user.avatar ~= "" then
		return string.format("%s/avatars/%s/%s.webp?size=64", CDN, user.id, user.avatar)
	end
	return defaultAvatar(user.id)
end

local function namesOf(user)
	local names = {}
	for _, field in ipairs({ "nick", "global_name", "username" }) do
		if type(user[field]) == "string" then
			names[#names + 1] = user[field]
		end
	end
	return names
end

local function findUser(message, name)
	for _, user in ipairs(message.mentions or {}) do
		for _, candidate in ipairs(namesOf(user)) do
			if candidate == name then
				return user
			end
		end
	end
	return nil
end

local function mentionedRole(guild, message, name)
	if not guild then
		return nil
	end
	for _, id in ipairs(message.mention_roles or {}) do
		local role = guild.roles and guild.roles[id]
		if role and role.name == name then
			return role
		end
	end
	return nil
end

local function roleImage(role)
	if type(role.icon) == "string" and role.icon ~= "" then
		return string.format("%s/role-icons/%s/%s.png?size=32&quality=lossless", CDN, role.id, role.icon), false
	end
	local color = math.tointeger(role.color) or 0
	local tint = color > 0 and string.format("%06X", color & 0xFFFFFF) or "label"
	return string.format("res://astral_tinted_%s_people", tint), false
end

local function decorate(node)
	local name = node.text:sub(2)
	local message = node:message()
	if not message then
		return nil
	end
	local user = findUser(message, name)
	if user then
		return { image = avatarUrl(user), round = true, hide_at = not astral.settings.values.showAtSymbol }
	end
	local role = mentionedRole(node:guild(), message, name)
	if role then
		local image, round = roleImage(role)
		return { image = image, round = round, after = true }
	end
	return nil
end

astral.ui.mention(decorate)

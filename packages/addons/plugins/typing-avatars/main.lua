local CDN = "https://cdn.discordapp.com"
local DEFAULT_AVATARS = 6

local function avatar_url(id)
	local user = astral.discord.user(id)
	if user and type(user.avatar) == "string" and user.avatar ~= "" then
		return string.format("%s/avatars/%s/%s.webp?size=48", CDN, id, user.avatar)
	end
	local number = math.tointeger(tonumber(id)) or 0
	return string.format("%s/embed/avatars/%d.png", CDN, (number >> 22) % DEFAULT_AVATARS)
end

astral.ui.typing(function(node)
	if not node.user_id then
		return nil
	end
	return { image = avatar_url(node.user_id), round = true }
end)

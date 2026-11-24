local CDN = "https://cdn.discordapp.com"
local DEFAULT_AVATARS = 6

local function avatar_url(user)
	if type(user.avatar) == "string" and user.avatar ~= "" then
		return string.format("%s/avatars/%s/%s.webp?size=48", CDN, user.id, user.avatar)
	end
	local number = math.tointeger(tonumber(user.id)) or 0
	return string.format("%s/embed/avatars/%d.png", CDN, (number >> 22) % DEFAULT_AVATARS)
end

astral.ui.reaction(function(node)
	local users = astral.discord.reactors(node.channel_id, node.message_id, node.emoji, node.burst, node.count)
	if not users or #users == 0 then
		return nil
	end
	local images = {}
	for _, user in ipairs(users) do
		images[#images + 1] = avatar_url(user)
	end
	return { images = images, total = node.count }
end)

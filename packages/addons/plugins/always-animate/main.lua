local CDN = "https://cdn.discordapp.com/"
local MEDIA = "https://media.discordapp.net/"

local function with_animated(url)
	if url:find("animated=true", 1, true) then
		return nil
	end
	local base, query = url:match("^([^?]*)%??(.*)$")
	if query == "" then
		return base .. "?animated=true"
	end
	return base .. "?" .. query .. "&animated=true"
end

local function animated_hash(url)
	local hash = url:match("/(a_%x+)%.%a+")
	return hash ~= nil
end

local function rewrite(url)
	if url:find("/emojis/", 1, true) then
		return with_animated(url)
	end
	if animated_hash(url) then
		return with_animated(url)
	end
	return nil
end

for _, path in ipairs({ "avatars/", "icons/", "banners/", "guilds/", "emojis/", "role-icons/" }) do
	astral.ui.image_rewrite(CDN .. path, rewrite)
	astral.ui.image_rewrite(MEDIA .. path, rewrite)
end

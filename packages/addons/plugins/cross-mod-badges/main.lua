local API = "https://api.domi-btnr.dev/clientmodbadges"
local ICON_PREFIX = "https://cdn.discordapp.com/badge-icons/astral-cross-mod-"

local badges_by_user = {}
local icons = {}

local function badges_from(body)
	local badges = {}
	if type(body) ~= "table" then
		return badges
	end
	for mod, list in pairs(body) do
		for _, entry in ipairs(type(list) == "table" and list or {}) do
			local name = type(entry) == "string" and entry or (type(entry) == "table" and entry.name)
			local image = type(entry) == "table" and type(entry.badge) == "string" and entry.badge
				or (name and (API .. "/badges/" .. mod .. "/" .. name))
			if type(name) == "string" and image then
				local key = tostring(#icons + 1)
				icons[key] = image
				badges[#badges + 1] = {
					id = "astral-cross-mod-" .. mod .. "-" .. name,
					description = mod .. " " .. name,
					icon = "astral-cross-mod-" .. key,
				}
			end
		end
	end
	return badges
end

astral.ui.image_rewrite(ICON_PREFIX, function(url)
	local key = url:match("^" .. ICON_PREFIX:gsub("%p", "%%%0") .. "(%d+)")
	return key and icons[key] or nil
end)

astral.net.intercept({
	method = "GET",
	path = "^/api/v%d+/users/%d+/profile$",
	request = function(request)
		local user = request.path:match("/users/(%d+)/profile")
		if not user or badges_by_user[user] then
			return nil
		end
		astral.net.request({ url = API .. "/users/" .. user }, function(response)
			badges_by_user[user] = badges_from(response and response.json)
			astral.net.release(request.id)
		end)
		return astral.net.HOLD
	end,
	response = function(request, response)
		local user = request.path:match("/users/(%d+)/profile")
		local extra = user and badges_by_user[user]
		if response.status ~= 200 or not extra or #extra == 0 then
			return nil
		end
		local profile = astral.json.decode(response.body)
		if type(profile) ~= "table" then
			return nil
		end
		profile.badges = type(profile.badges) == "table" and profile.badges or astral.json.array()
		for _, badge in ipairs(extra) do
			profile.badges[#profile.badges + 1] = badge
		end
		return { body = astral.json.encode(profile) }
	end,
})

local t = astral.i18n.t

local API = "https://api.domi-btnr.dev/clientmodbadges"
local ICON_PREFIX = "https://cdn.discordapp.com/badge-icons/astral-cross-mod-"
local CATALOG_ID_BASE = 2000000000

local badges_by_user = {}
local icons = {}
local waiting = {}

local function badge_url(mod, entry, name)
	if type(entry) == "table" and type(entry.badge) == "string" then
		return entry.badge
	end
	return name and (API .. "/badges/" .. mod .. "/" .. name) or nil
end

local function badges_from(body)
	local badges = {}
	if type(body) ~= "table" then
		return badges
	end
	for mod, list in pairs(body) do
		for _, entry in ipairs(type(list) == "table" and list or {}) do
			local name = type(entry) == "string" and entry or (type(entry) == "table" and entry.name)
			local image = type(name) == "string" and badge_url(mod, entry, name)
			if image then
				local key = tostring(#icons + 1)
				icons[key] = image
				badges[#badges + 1] = {
					id = "astral-cross-mod-" .. mod .. "-" .. tostring(name),
					description = (t("BADGE"):gsub("{mod}", mod):gsub("{name}", tostring(name))),
					icon = "astral-cross-mod-" .. key,
					image = image,
				}
			end
		end
	end
	return badges
end

local function load(user, request_id)
	if waiting[user] then
		waiting[user][#waiting[user] + 1] = request_id
		return
	end
	waiting[user] = { request_id }
	astral.net.request({ url = API .. "/users/" .. user }, function(response)
		badges_by_user[user] = badges_from(response and response.json)
		for _, id in ipairs(waiting[user]) do
			astral.net.release(id)
		end
		waiting[user] = nil
	end)
end

local function hold_for(user, request)
	if not user or badges_by_user[user] then
		return nil
	end
	load(user, request.id)
	return astral.net.HOLD
end

astral.ui.image_rewrite(ICON_PREFIX, function(url)
	local key = url:match("^" .. ICON_PREFIX:gsub("%p", "%%%0") .. "(%d+)")
	return key and icons[key] or nil
end)

astral.net.intercept({
	method = "GET",
	path = "^/api/v%d+/users/%d+/profile$",
	request = function(request)
		return hold_for(request.path:match("/users/(%d+)/profile"), request)
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
			profile.badges[#profile.badges + 1] = { id = badge.id, description = badge.description, icon = badge.icon }
		end
		return { body = astral.json.encode(profile) }
	end,
})

astral.net.intercept({
	method = "GET",
	path = "^/api/v%d+/users/@me/badges$",
	request = function(request)
		return hold_for(astral.discord.me(), request)
	end,
	response = function(_, response)
		local extra = badges_by_user[astral.discord.me() or ""]
		if response.status ~= 200 or not extra or #extra == 0 then
			return nil
		end
		local catalog = astral.json.decode(response.body)
		if type(catalog) ~= "table" or type(catalog.badges) ~= "table" then
			return nil
		end
		for index, badge in ipairs(extra) do
			catalog.badges[#catalog.badges + 1] = {
				badge_id = CATALOG_ID_BASE + index,
				name = badge.description,
				rarity = 1,
				owned = true,
				hidden = false,
				has_updates = false,
				is_earnable = false,
				simple_icon_url = badge.image,
				simple_icon_raster_url = badge.image,
			}
		end
		return { body = astral.json.encode(catalog) }
	end,
})

local BUTTONS = {
	{ key = "gift", title = "Gift button", default = true, ids = { "composer.gift" } },
	{ key = "server_boosts", title = "Server Boosts row", default = false, ids = { "guild.action:server_boosts" } },
	{ key = "voice_call", title = "Voice call buttons", default = false, ids = { "dm.call", "dm-call-button" } },
	{ key = "quest_dock", title = "Quest dock", default = false, ids = { "quest-dock", "quest_dock" } },
}

local DECORATIONS = {
	{ key = "avatar_decorations", title = "Avatar decorations", fields = { "avatar_decoration_data" } },
	{ key = "nameplates", title = "Nameplates", fields = { "collectibles" } },
	{ key = "profile_effects", title = "Profile effects", fields = { "profile_effect" } },
	{ key = "server_tags", title = "Server tags", fields = { "primary_guild", "clan" } },
	{ key = "display_name_styles", title = "Display name styles", fields = { "display_name_styles" } },
}

local buttons = astral.settings.section("Buttons")
for _, button in ipairs(BUTTONS) do
	buttons:switch(button.key, button.title, button.default)
end
local decorations = astral.settings.section("Profile decorations")
for _, decoration in ipairs(DECORATIONS) do
	decorations:switch(decoration.key, decoration.title, false)
end

local function enabled(key, fallback)
	local value = astral.settings.get(key, fallback)
	if value == nil then
		return fallback
	end
	return value
end

for _, button in ipairs(BUTTONS) do
	for _, id in ipairs(button.ids) do
		astral.ui.patch({ id = id }, function()
			if enabled(button.key, button.default) then
				return { hidden = true }
			end
			return nil
		end)
	end
end

local function stripped_fields()
	local fields = {}
	for _, decoration in ipairs(DECORATIONS) do
		if enabled(decoration.key, false) then
			for _, field in ipairs(decoration.fields) do
				fields[#fields + 1] = field
			end
		end
	end
	return fields
end

local function strip_user(user, fields)
	if type(user) ~= "table" then
		return false
	end
	local changed = false
	for _, field in ipairs(fields) do
		if user[field] ~= nil then
			user[field] = nil
			changed = true
		end
	end
	return changed
end

local function strip_message(message, fields)
	if type(message) ~= "table" then
		return false
	end
	local changed = strip_user(message.author, fields)
	if type(message.member) == "table" then
		changed = strip_user(message.member, fields) or changed
		changed = strip_user(message.member.user, fields) or changed
	end
	for _, mention in ipairs(message.mentions or {}) do
		changed = strip_user(mention, fields) or changed
	end
	changed = strip_message(message.referenced_message, fields) or changed
	return changed
end

astral.events.on(astral.Event.Gateway, function(event)
	local fields = stripped_fields()
	if #fields == 0 then
		return nil
	end
	return strip_message(event.data, fields) and event.data or nil
end, { "MESSAGE_CREATE", "MESSAGE_UPDATE" })

astral.net.intercept({
	method = "GET",
	path = "^/api/v%d+/channels/%d+/messages$",
	response = function(_, response)
		local fields = stripped_fields()
		if #fields == 0 or response.status ~= 200 then
			return nil
		end
		local messages = astral.json.decode(response.body)
		if type(messages) ~= "table" then
			return nil
		end
		local changed = false
		for _, message in ipairs(messages) do
			changed = strip_message(message, fields) or changed
		end
		return changed and { body = astral.json.encode(messages) } or nil
	end,
})

astral.net.intercept({
	method = "GET",
	path = "^/api/v%d+/users/%d+/profile$",
	response = function(_, response)
		local fields = stripped_fields()
		if #fields == 0 or response.status ~= 200 then
			return nil
		end
		local profile = astral.json.decode(response.body)
		if type(profile) ~= "table" then
			return nil
		end
		local changed = strip_user(profile.user, fields)
		changed = strip_user(profile.user_profile, fields) or changed
		changed = strip_user(profile.guild_member, fields) or changed
		if enabled("profile_effects", false) and type(profile.user_profile) == "table" then
			profile.user_profile.profile_effect = nil
			changed = true
		end
		return changed and { body = astral.json.encode(profile) } or nil
	end,
})

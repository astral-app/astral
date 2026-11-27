local t = astral.i18n.t

local BUTTONS = {
	{ key = "gift", label = "GIFT", default = true, ids = { "composer.gift" } },
	{ key = "voiceCall", label = "VOICE_CALL", default = false, ids = { "dm.call", "dm-call-button" } },
	{ key = "videoCall", label = "VIDEO_CALL", default = false, ids = { "dm.video" } },
	{ key = "serverBoosts", label = "SERVER_BOOSTS", default = false, ids = { "guild.action:server_boosts" } },
	{ key = "questDock", label = "QUEST_DOCK", default = false, ids = { "quest-dock", "quest_dock" } },
	{ key = "paymentQuests", label = "PAYMENT_QUESTS", default = false, ids = { "Quests" } },
	{ key = "paymentShop", label = "PAYMENT_SHOP", default = false, ids = { "Shop" } },
	{ key = "paymentNitro", label = "PAYMENT_NITRO", default = false, ids = { "Nitro Home" } },
	{ key = "paymentSubscriptions", label = "PAYMENT_SUBSCRIPTIONS", default = false, ids = { "My Subscriptions" } },
	{ key = "paymentBoosts", label = "PAYMENT_BOOSTS", default = false, ids = { "Server Boost" } },
	{ key = "paymentGifts", label = "PAYMENT_GIFTS", default = false, ids = { "Gift Inventory" } },
}

local DECORATIONS = {
	{ key = "avatarDecorations", label = "AVATAR_DECORATIONS", fields = { "avatar_decoration_data" } },
	{ key = "nameplates", label = "NAMEPLATES", fields = { "collectibles" } },
	{ key = "profileEffects", label = "PROFILE_EFFECTS", fields = { "profile_effect" } },
	{ key = "badges", label = "BADGES", fields = { "badges" } },
	{ key = "serverTags", label = "SERVER_TAGS", fields = { "primary_guild", "clan" } },
	{ key = "displayNameStyles", label = "DISPLAY_NAME_STYLES", fields = { "display_name_styles" } },
}

local buttons = astral.settings.section(t("SECTION_BUTTONS"))
for _, button in ipairs(BUTTONS) do
	buttons:switch(button.key, t(button.label), button.default, t(button.label .. "_SUBTITLE"))
end
local decorations = astral.settings.section(t("SECTION_DECORATIONS"))
for _, decoration in ipairs(DECORATIONS) do
	decorations:switch(decoration.key, t(decoration.label), false, t(decoration.label .. "_SUBTITLE"))
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
		if enabled("profileEffects", false) and type(profile.user_profile) == "table" then
			profile.user_profile.profile_effect = nil
			changed = true
		end
		return changed and { body = astral.json.encode(profile) } or nil
	end,
})

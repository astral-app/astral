local t = astral.i18n.t

local ON_COLOR = 0xFF949CF7
local OFF_COLOR = 0xFF949BA4

local exceptions = astral.settings.section(t("EXCEPTIONS"))
exceptions:text("userList", t("ADD_USER"), "", { subtitle = t("ADD_USER_ID_SUBTITLE"), max_length = 4000 })
local options = astral.settings.section(t("OPTIONS"))
options:switch("shouldPingListed", t("ONLY_PING_LISTED"), false, t("ONLY_PING_LISTED_ON"))

local toggle = { node = nil, initial = nil, current = nil }

local function listed(id)
	for entry in tostring(astral.settings.get("userList", "") or ""):gmatch("%d+") do
		if entry == id then
			return true
		end
	end
	return false
end

local function should_mention(author)
	return astral.settings.get("shouldPingListed", false) == true and author ~= nil and listed(author)
end

local function toggled()
	return toggle.initial ~= nil and toggle.current ~= nil and toggle.current ~= toggle.initial
end

astral.ui.patch("composer.reply_mention", function(node)
	if node.checked == nil then
		return nil
	end
	if node.node ~= toggle.node then
		toggle.node, toggle.initial = node.node, node.checked
	end
	toggle.current = node.checked
	local shown = toggled()
	if shown == node.checked then
		return nil
	end
	local color = shown and ON_COLOR or OFF_COLOR
	return { label = { text = shown and t("ON") or t("OFF"), color = color, tint = color } }
end)

local function replied_author(message, channel)
	local reference = message.message_reference
	local replied = reference.message_id
		and astral.discord.message(reference.channel_id or channel, reference.message_id)
	return replied and replied.author and replied.author.id or nil
end

local function with_reply_choice(body, channel)
	local message = astral.json.decode(body)
	if type(message) ~= "table" or type(message.message_reference) ~= "table" then
		return nil
	end
	local mention = toggled() or should_mention(replied_author(message, channel))
	message.allowed_mentions = type(message.allowed_mentions) == "table" and message.allowed_mentions
		or { parse = { "users", "roles", "everyone" } }
	message.allowed_mentions.replied_user = mention
	toggle.node, toggle.initial, toggle.current = nil, nil, nil
	return astral.json.encode(message)
end

astral.net.intercept({
	method = "POST",
	path = "^/api/v%d+/channels/%d+/messages$",
	request = function(request)
		if not tostring(request.headers["content-type"] or ""):find("application/json", 1, true) then
			return nil
		end
		local body = with_reply_choice(request.body, request.path:match("/channels/(%d+)/"))
		return body and { body = body } or nil
	end,
})

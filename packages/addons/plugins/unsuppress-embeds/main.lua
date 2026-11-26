local SUPPRESS_EMBEDS = 1 << 2

local function t(key, variables)
	local text = astral.i18n.t(key)
	for name, value in pairs(variables or {}) do
		text = text:gsub("{" .. name .. "}", function()
			return value
		end)
	end
	return text
end

local function suppressed(message)
	return (message.flags or 0) & SUPPRESS_EMBEDS ~= 0
end

local function may_change_flags(node, message)
	local me = astral.discord.me()
	local private = node.guild_id == nil
	if me and message.author and message.author.id == me then
		if private then
			return true
		end
		local granted = astral.discord.permissions(node.guild_id, me, node.channel_id)
		return astral.permissions.any(granted, astral.Permission.EMBED_LINKS, astral.Permission.ADMINISTRATOR)
	end
	if private or not me then
		return false
	end
	local granted = astral.discord.permissions(node.guild_id, me, node.channel_id)
	return astral.permissions.any(granted, astral.Permission.MANAGE_MESSAGES, astral.Permission.ADMINISTRATOR)
end

local function action_for(node)
	local message = node:message()
	if not message or not may_change_flags(node, message) then
		return nil
	end
	if suppressed(message) then
		return "unsuppress"
	end
	return (message.embeds or 0) > 0 and "suppress" or nil
end

local function label(action)
	return t(action == "suppress" and "SUPPRESS_EMBEDS" or "UNSUPPRESS_EMBEDS")
end

local function failure_reason(response)
	local ok, body = pcall(astral.json.decode, response.body or "")
	if ok and type(body) == "table" and type(body.message) == "string" then
		return body.message
	end
	return response.error or t("CHANGE_REFUSED")
end

local function change(node, action)
	local message = node:message()
	if not message then
		return
	end
	local flags = message.flags or 0
	local wanted = action == "suppress" and flags | SUPPRESS_EMBEDS or flags & ~SUPPRESS_EMBEDS
	astral.discord.request({
		method = "PATCH",
		path = "/channels/" .. node.channel_id .. "/messages/" .. node.message_id,
		body = { flags = wanted },
	}, function(response)
		if not response.ok then
			astral.ui.toast(
				"Unsuppress Embeds",
				t("CHANGE_FAILED", { action = label(action), reason = failure_reason(response) })
			)
		end
	end)
end

for _, action in ipairs({ "suppress", "unsuppress" }) do
	astral.ui.message_action(label(action), function(node)
		change(node, action)
	end, {
		icon = action == "suppress" and "res://ic_mana_eye_slash" or "res://ic_mana_eye",
		danger = action == "suppress",
		when = function(node)
			return action_for(node) == action
		end,
	})
end

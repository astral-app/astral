local AUTOMOD_MESSAGE_TYPE = 24

local MODES = {
	{ value = "nick-user", title = "Display name then username", subtitle = "Display Name (@username)" },
	{ value = "user-nick", title = "Username then display name", subtitle = "@username (Display Name)" },
	{ value = "user", title = "Username only", subtitle = "@username" },
}

local settings = {
	mode = astral.settings.choice("mode", "Display Mode", MODES, "nick-user", { section = "Preferences" }),
	friendNicknames = astral.settings.switch("friendNicknames", "Show Friend Nicknames", true, {
		subtitle = "Prefer a friend's nickname wherever it applies",
		section = "Preferences",
	}),
	displayNames = astral.settings.switch("displayNames", "Use Global Names", false, {
		subtitle = "Show the account's global name instead of its username",
		section = "Preferences",
	}),
	inReplies = astral.settings.switch("inReplies", "Apply To Replies", false, {
		subtitle = "Also apply to reply previews",
		section = "Preferences",
	}),
}

function astral.on_setting(key, value)
	settings[key] = value
end

local function formatName(displayed, username, nickname, mode)
	local prefix = displayed:sub(1, 1) == "@" and "@" or ""
	local nick = nickname or displayed:sub(#prefix + 1)
	local tag = "@" .. username

	if username == nick then
		return displayed
	end
	if mode == "user" then
		return tag
	end
	if mode == "user-nick" then
		return tag .. " (" .. nick .. ")"
	end
	return prefix .. nick .. " (" .. tag .. ")"
end

local function isAutomod(message)
	return message ~= nil and message.type == AUTOMOD_MESSAGE_TYPE
end

local function authorOf(node, message)
	if node.kind ~= "reply" then
		if isAutomod(message) then
			return nil
		end
		return message.author
	end
	if not settings.inReplies or isAutomod(message) or not message.reference then
		return nil
	end
	local replied = astral.discord.message(message.reference.channel_id, message.reference.message_id)
	if not replied or isAutomod(replied) then
		return nil
	end
	return replied.author
end

astral.ui.relabel("message.author", function(node)
	if not node.channel_id then
		return nil
	end
	local message = astral.discord.message(node.channel_id, node.message_id)
	if not message then
		return nil
	end
	local author = authorOf(node, message)
	if not author or not author.id then
		return nil
	end
	local username = (settings.displayNames and author.global_name) or author.username
	if not username then
		return nil
	end
	local nickname = settings.friendNicknames and astral.discord.nickname(author.id) or nil
	return formatName(node.text, username, nickname, settings.mode)
end)

astral.log("ready")

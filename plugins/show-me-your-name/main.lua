local MessageType, NodeKind = astral.MessageType, astral.NodeKind
local option = astral.settings.option
local settings = astral.settings.values
local t = astral.i18n.t

local preferences = astral.settings.section(t("PREFERENCES"))
preferences:choice("mode", t("DISPLAY_MODE"), {
	option("nick-user", t("MODE_NICK_USER"), t("MODE_NICK_USER_SUBTITLE")),
	option("user-nick", t("MODE_USER_NICK"), t("MODE_USER_NICK_SUBTITLE")),
	option("user", t("MODE_USER"), t("MODE_USER_SUBTITLE")),
}, "nick-user")
preferences:switch("friendNicknames", t("FRIEND_NICKNAMES"), true, t("FRIEND_NICKNAMES_SUBTITLE"))
preferences:switch("displayNames", t("DISPLAY_NAMES"), false, t("DISPLAY_NAMES_SUBTITLE"))
preferences:switch("inReplies", t("IN_REPLIES"), false, t("IN_REPLIES_SUBTITLE"))

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
	return message ~= nil and message.type == MessageType.AutoModerationAction
end

local function authorOf(node, message)
	if node.kind ~= NodeKind.Reply then
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

astral.ui.relabel(astral.Target.MessageAuthor, function(node)
	local message = node:message()
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

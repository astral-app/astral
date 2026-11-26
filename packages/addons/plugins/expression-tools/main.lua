local function t(key, values)
	local text = astral.i18n.t(key)
	for name, value in pairs(values or {}) do
		text = text:gsub("{" .. name .. "}", function()
			return value
		end)
	end
	return text
end

local EMOJI_CDN = "https://cdn.discordapp.com/emojis/"
local MENU = "Copy Emoji Image Link^"
local ROW_BACKGROUND = 0x1497979F
local DIVIDER = 0x1F97979F
local DIVIDER_INSET = 55
local FOREGROUND = 0xFFFBFBFB
local ADMINISTRATOR = 8
local MANAGE_EXPRESSIONS = 1073741824
local UPLOAD_DELAY_MS = 1400
local DISMISS_DELAY_MS = 450

local current

local function emoji_from_url(key)
	local id = key:match("^(%d+)%.")
	if not id or not key:find("size=160", 1, true) then
		return nil
	end
	return { id = id }
end

local function describe(emoji)
	local found = astral.discord.emoji(emoji.id)
	if not found then
		return nil
	end
	return { id = emoji.id, name = found.name, animated = found.animated == true }
end

local function markup(emoji)
	return string.format("<%s:%s:%s>", emoji.animated and "a" or "", emoji.name, emoji.id)
end

local function divider()
	return {
		type = astral.InsertNodeType.View,
		background = ROW_BACKGROUND,
		padding = { DIVIDER_INSET, 0, 0, 0 },
		fill = true,
		children = { { type = astral.InsertNodeType.View, background = DIVIDER, height = 0.5, fill = true } },
	}
end

local function menu_row(label, icon, action)
	return {
		type = astral.InsertNodeType.View,
		background = ROW_BACKGROUND,
		row = true,
		padding = 12,
		fill = true,
		action = action,
		children = {
			{
				type = astral.InsertNodeType.Image,
				icon = icon,
				width = 24,
				height = 24,
				margin = { 4, 4, 16, 4 },
				color = FOREGROUND,
			},
			{ type = astral.InsertNodeType.Text, text = label, size = 16, weight = 500, color = FOREGROUND },
		},
	}
end

local function can_manage(guild, me)
	local granted = astral.discord.permissions(guild.id, me)
	return granted ~= nil and (granted & (ADMINISTRATOR | MANAGE_EXPRESSIONS)) ~= 0
end

local function managed_guilds()
	local me = astral.discord.me()
	local list = {}
	for _, guild in ipairs(me and astral.discord.guilds() or {}) do
		if can_manage(guild, me) then
			list[#list + 1] = guild
		end
	end
	table.sort(list, function(a, b)
		return a.name:lower() < b.name:lower()
	end)
	return list
end

local function upload(emoji, guild)
	local extension = emoji.animated and "gif" or "png"
	local url = string.format("%s%s.%s?size=128&quality=lossless", EMOJI_CDN, emoji.id, extension)
	astral.net.request({ url = url, timeout_ms = 15000 }, function(response)
		local bytes = response
			and response.status == 200
			and response.body_base64
			and astral.base64.decode(response.body_base64)
		if not bytes or bytes == "" then
			astral.ui.toast(t("EXPRESSION_TOOLS"), t("COULD_NOT_FETCH", { name = emoji.name }))
			return
		end
		if not astral.media.offer_pick(bytes, emoji.name .. "." .. extension) then
			astral.ui.toast(t("EXPRESSION_TOOLS"), t("COULD_NOT_FETCH", { name = emoji.name }))
			return
		end
		astral.ui.dismiss()
		astral.time.after(DISMISS_DELAY_MS, function()
			astral.ui.open("discord://-/guilds/" .. guild.id .. "/settings/emoji")
		end)
		astral.ui.toast(t("EXPRESSION_TOOLS"), t("CLONING", { name = emoji.name, guild = guild.name }))
		astral.time.after(UPLOAD_DELAY_MS, function()
			astral.ui.press("Upload Emoji")
		end)
	end)
end

local function clone_sheet(emoji)
	local guilds = managed_guilds()
	local items = {
		{ type = astral.SheetItemType.Heading, text = t("YOUR_SERVERS") },
	}
	if #guilds == 0 then
		items[#items + 1] = { type = astral.SheetItemType.Text, text = t("NO_OWNED_SERVERS") }
	end
	local by_action = {}
	for index, guild in ipairs(guilds) do
		local action = "guild:" .. index
		by_action[action] = guild
		items[#items + 1] = {
			type = astral.SheetItemType.Row,
			action = action,
			items = { { type = astral.SheetItemType.Text, text = guild.name } },
		}
	end
	local sheet
	sheet = astral.ui.sheet({ title = t("CLONE_TO_SERVER"), items = items, expanded = true }, function(action)
		local guild = by_action[action]
		if guild then
			sheet:close()
			upload(emoji, guild)
		end
	end)
end

astral.ui.insert("image:" .. EMOJI_CDN, function(node)
	local emoji = emoji_from_url(node.key)
	if emoji then
		current = emoji
	end
	return nil
end)

astral.ui.patch("Copy Emoji Image Link", function()
	return current and { radius = 0 } or nil
end)

astral.ui.insert(MENU, function()
	if not current then
		return nil
	end
	return {
		divider(),
		menu_row(t("COPY_MARKUP"), "res://ic_mana_copy", "markup"),
		divider(),
		menu_row(t("CLONE"), "res://ic_mana_circle_plus", "clone"),
	}
end, function(_, action)
	local emoji = current and describe(current)
	if not emoji then
		astral.ui.toast(t("EXPRESSION_TOOLS"), t("UNKNOWN_EMOJI"))
		return
	end
	if action == "markup" then
		local copied = astral.ui.copy(markup(emoji))
		astral.ui.toast(t("EXPRESSION_TOOLS"), copied == false and t("COULD_NOT_COPY") or t("MARKUP_COPIED"))
	elseif action == "clone" then
		clone_sheet(emoji)
	end
end)

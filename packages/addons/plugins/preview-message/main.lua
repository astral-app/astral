local t = astral.i18n.t

local CDN = "https://cdn.discordapp.com"
local DEFAULT_AVATARS = 6
local COMPOSER_ICON = 0xFFABACB2
local AVATAR_SIZE = 40
local EMPTY_CHECK_MS = 500

local current_user
local composing = false
local open_sheet

local function avatar_url(user)
	if type(user.avatar) == "string" and user.avatar ~= "" then
		return string.format("%s/avatars/%s/%s.webp?size=80", CDN, user.id, user.avatar)
	end
	local number = math.tointeger(tonumber(user.id)) or 0
	return string.format("%s/embed/avatars/%d.png", CDN, (number >> 22) % DEFAULT_AVATARS)
end

local function preview_items(text)
	local id = astral.discord.me() or current_user
	local user = id and astral.discord.user(id) or nil
	local name = user and (user.global_name or user.username) or "You"
	local header = { type = astral.SheetItemType.Text, text = name, bold = true }
	local body = {
		type = astral.SheetItemType.Column,
		spacing = 2,
		items = { header, { type = astral.SheetItemType.Text, text = text } },
	}
	local row = { type = astral.SheetItemType.Row, spacing = 12, align = astral.SheetAlign.Top, items = {} }
	if user then
		row.items[#row.items + 1] =
			{ type = astral.SheetItemType.Image, url = avatar_url(user), round = true, size = AVATAR_SIZE }
	end
	row.items[#row.items + 1] = body
	return { row }
end

local function show_preview()
	local text = astral.ui.composer_text()
	if text:match("^%s*$") then
		return
	end
	if open_sheet then
		open_sheet:close()
	end
	open_sheet = astral.ui.sheet({ title = t("MESSAGE_PREVIEW"), items = preview_items(text) }, function(action)
		if action == "closed" then
			open_sheet = nil
		end
	end)
end

astral.ui.composer(function(text)
	local now = not text:match("^%s*$")
	if now ~= composing then
		composing = now
		astral.time.after(0, astral.ui.refresh)
	end
	return {}
end)

local watching = false

local function watch_for_empty()
	if watching then
		return
	end
	watching = true
	astral.time.after(EMPTY_CHECK_MS, function()
		watching = false
		if astral.ui.composer_text():match("^%s*$") then
			composing = false
			astral.ui.refresh()
		else
			watch_for_empty()
		end
	end)
end

astral.ui.insert({ id = "composer.expressions", path = "^" }, function()
	if astral.ui.composer_text():match("^%s*$") then
		return nil
	end
	watch_for_empty()
	return {
		type = astral.InsertNodeType.View,
		row = true,
		justify = astral.Justify.Center,
		align = astral.Align.Center,
		width = 32,
		height = 32,
		radius = 8,
		action = "preview",
		children = {
			{
				type = astral.InsertNodeType.Image,
				icon = "res://ic_mana_eye",
				width = 20,
				height = 20,
				color = COMPOSER_ICON,
			},
		},
	}
end, function()
	show_preview()
end)

astral.events.on(astral.Event.Gateway, function(event)
	if type(event.data.user) == "table" then
		current_user = event.data.user.id
	end
	return nil
end, { "READY" })

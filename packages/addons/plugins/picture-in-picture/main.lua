local t = astral.i18n.t

local BUTTON_BACKGROUND = 0x8F000000
local BUTTON_PADDING = 10
local BUTTON_GAP = 8
local ICON_SIZE = 24
local CHROME_IDS = { "viewer-header", "viewer-seek-bar", "viewer-message" }
local picture_in_picture = false

for _, id in ipairs(CHROME_IDS) do
	astral.ui.patch({ id = id }, function()
		return picture_in_picture and { hidden = true } or nil
	end)
end

astral.events.on(astral.Event.PictureInPictureStart, function()
	picture_in_picture = true
	astral.ui.refresh()
end)

astral.events.on(astral.Event.PictureInPictureStop, function()
	picture_in_picture = false
	astral.ui.refresh()
end)

astral.ui.insert({ id = "viewer-more", path = "^" }, function()
	return {
		type = astral.InsertNodeType.View,
		row = true,
		position = astral.InsertPosition.Start,
		padding = BUTTON_PADDING,
		margin = { 0, 0, BUTTON_GAP, 0 },
		background = BUTTON_BACKGROUND,
		radius = ICON_SIZE / 2 + BUTTON_PADDING,
		action = "picture_in_picture",
		children = {
			{
				type = astral.InsertNodeType.Image,
				icon = "res://ic_mana_window_launch",
				width = ICON_SIZE,
				height = ICON_SIZE,
				color = 0xFFFFFFFF,
			},
		},
	}
end, function()
	if not astral.ui.picture_in_picture() then
		astral.toasts.error(t("START_FAILED"))
	end
end)

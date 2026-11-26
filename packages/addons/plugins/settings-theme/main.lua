local t = astral.i18n.t

astral.settings.number("dim", t("DIM"), {
	min = 0,
	max = 90,
	step = 5,
	default = 0,
	subtitle = t("DIM_SUBTITLE"),
})

local function dimmed(color, percent)
	local keep = 1 - percent / 100
	local function channel(shift)
		return math.floor(((color >> shift) & 0xFF) * keep + 0.5)
	end
	return 0xFF000000 | channel(16) << 16 | channel(8) << 8 | channel(0)
end

astral.ui.patch({ id = "settings-phone", type = astral.PatchNodeType.View }, function()
	local background = astral.ui.app_background()
	if not background then
		return nil
	end
	local percent = astral.settings.values.dim or 0
	local colors = {}
	for index, color in ipairs(background.colors) do
		colors[index] = dimmed(color, percent)
	end
	return { background = 0, gradient = { angle = background.angle, colors = colors }, see_through = true }
end)

astral.settings.on_change("dim", function()
	astral.ui.refresh()
end)

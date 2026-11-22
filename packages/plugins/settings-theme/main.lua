local t = astral.i18n.t

local function showBackdrop(dim)
	astral.ui.settings_backdrop({ dim = dim })
end

local dim = astral.settings.number("dim", t("DIM"), {
	min = 0,
	max = 90,
	step = 5,
	default = 55,
	subtitle = t("DIM_SUBTITLE"),
})

showBackdrop(dim)

astral.settings.on_change("dim", function(value)
	showBackdrop(value)
end)

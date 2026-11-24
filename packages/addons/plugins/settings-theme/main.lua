local t = astral.i18n.t

local function show_backdrop(dim)
	astral.ui.settings_backdrop({ dim = dim })
end

local dim = astral.settings.number("dim", t("DIM"), {
	min = 0,
	max = 90,
	step = 5,
	default = 0,
	subtitle = t("DIM_SUBTITLE"),
})

show_backdrop(dim)

astral.settings.on_change("dim", function(value)
	show_backdrop(value)
end)

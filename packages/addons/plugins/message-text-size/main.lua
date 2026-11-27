local t = astral.i18n.t
local settings = astral.settings.section(t("TEXT"))
settings:number("scale", t("SCALE"), {
	min = 50,
	max = 200,
	step = 5,
	default = 115,
	subtitle = t("SCALE_SUBTITLE"),
})

astral.ui.patch({ id = "message.content:" }, function()
	local scale = (astral.settings.get("scale", 115) or 115) / 100
	if scale == 1 then
		return nil
	end
	return { text_scale = scale }
end)

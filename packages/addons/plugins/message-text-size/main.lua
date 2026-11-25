local settings = astral.settings.section("Text")
settings:number("scale", "Message text size", {
	min = 50,
	max = 200,
	step = 5,
	default = 115,
	subtitle = "Percent of Discord's size for the text of messages",
})

astral.ui.patch({ id = "message.content:" }, function()
	local scale = (astral.settings.get("scale", 115) or 115) / 100
	if scale == 1 then
		return nil
	end
	return { text_scale = scale }
end)

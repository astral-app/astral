local settings = astral.settings.section("Chat Bubbles")

settings:choice("color", "Card color", {
	astral.settings.option("neutral", "Neutral"),
	astral.settings.option("blue", "Blue"),
	astral.settings.option("violet", "Violet"),
}, "neutral")
settings:number("radius", "Corner radius", { min = 0, max = 32, step = 1, default = 12 })

local colors = {
	neutral = 0x1A000000,
	blue = 0x265865F2,
	violet = 0x268B5CF6,
}

astral.ui.message_background(function()
	local selected = colors[astral.settings.values.color] or colors.neutral
	return { color = selected, radius = astral.settings.values.radius or 12 }
end)

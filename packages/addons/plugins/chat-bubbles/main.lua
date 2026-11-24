local settings = astral.settings.section("Chat Bubbles")

settings:choice("color", "Bubble color", {
	astral.settings.option("neutral", "Neutral"),
	astral.settings.option("blue", "Blue"),
	astral.settings.option("violet", "Violet"),
}, "neutral")
settings:number("radius", "Corner radius", { min = 0, max = 32, step = 1, default = 12 })
settings:choice("reply", "Replies", {
	astral.settings.option("inside", "Inside", "The message being replied to shows in a box inside the bubble"),
	astral.settings.option("above", "Above", "The message being replied to sits above the bubble"),
	astral.settings.option("bubble", "Bubble", "The message being replied to gets its own small bubble"),
}, "inside")

local colors = {
	neutral = 0x24FFFFFF,
	blue = 0x665865F2,
	violet = 0x668B5CF6,
}

astral.ui.message_background(function()
	local values = astral.settings.values
	return {
		color = colors[values.color] or colors.neutral,
		radius = values.radius or 12,
		bubble = true,
		reply = values.reply,
	}
end)

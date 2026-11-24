local settings = astral.settings.section("Bubbles")

settings:color("color", "Bubble color")
settings:number(
	"opacity",
	"Opacity",
	{ min = 5, max = 100, step = 5, default = 40, subtitle = "Applies to a picked color" }
)
settings:number("radius", "Corner radius", { min = 0, max = 32, step = 1, default = 12 })
settings:choice("reply", "Replies", {
	astral.settings.option("inside", "Inside", "The message being replied to shows in a box inside the bubble"),
	astral.settings.option("above", "Above", "The message being replied to sits above the bubble"),
	astral.settings.option("bubble", "Bubble", "The message being replied to gets its own small bubble"),
}, "inside")

local AUTOMATIC = 0x24FFFFFF

local function bubble_color(values)
	if values.color == nil or values.color == "" then
		return AUTOMATIC
	end
	local alpha = math.floor((values.opacity or 40) * 255 / 100 + 0.5)
	return astral.Color.from(values.color):with_alpha(alpha)
end

astral.ui.message_background(function()
	local values = astral.settings.values
	return {
		color = bubble_color(values),
		radius = values.radius or 12,
		bubble = true,
		reply = values.reply,
	}
end)

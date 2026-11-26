local settings = astral.settings.section("Bubbles")

settings:color("color", "Bubble color", "", "Automatic is a shade darker than the chat's background")
settings:number(
	"opacity",
	"Opacity",
	{ min = 5, max = 100, step = 5, default = 100, subtitle = "Applies to a picked color" }
)
settings:number("radius", "Corner radius", { min = 0, max = 32, step = 1, default = 16 })

local layout = astral.settings.section("Name, time and replies")
layout:choice("name", "Name", {
	astral.settings.option(
		astral.NamePlacement.Inside,
		"Inside the bubble",
		"The author line sits at the top of the group's first bubble"
	),
	astral.settings.option(astral.NamePlacement.Above, "Above the bubble", "The author line sits above the bubbles"),
}, astral.NamePlacement.Inside)
layout:choice("time", "Time", {
	astral.settings.option(astral.TimePlacement.Header, "With the name", "The time sits in the author line"),
	astral.settings.option(
		astral.TimePlacement.Inline,
		"Beside the text",
		"The time sits after the text, in small type"
	),
	astral.settings.option(astral.TimePlacement.Hidden, "Hidden", "Bubbles show no time"),
}, astral.TimePlacement.Header)
layout:choice("reply", "Replies", {
	astral.settings.option(
		astral.ReplyPlacement.Inside,
		"Inside",
		"The message being replied to shows in a box inside the bubble"
	),
	astral.settings.option(astral.ReplyPlacement.Above, "Above", "The message being replied to sits above the bubble"),
	astral.settings.option(
		astral.ReplyPlacement.Bubble,
		"Bubble",
		"The message being replied to gets its own small bubble"
	),
}, astral.ReplyPlacement.Inside)

local AUTOMATIC = 0x33000000

local function bubble_color(values)
	if values.color == nil or values.color == "" then
		return AUTOMATIC
	end
	local alpha = math.floor((values.opacity or 100) * 255 / 100 + 0.5)
	return astral.Color.from(values.color):with_alpha(alpha)
end

astral.ui.message_background(function()
	local values = astral.settings.values
	return {
		color = bubble_color(values),
		radius = values.radius or 16,
		bubble = true,
		reply = values.reply,
		name = values.name,
		time = values.time,
	}
end)

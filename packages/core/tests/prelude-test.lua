local registered = {}
astral = {
	settings = {
		switch = function(_, _, default)
			return default
		end,
		choice = function(_, _, _, default)
			return default
		end,
		set = function(key, value)
			registered.stored = { key, value }
		end,
	},
	ui = {
		message_background = function(decorator)
			registered.message_background = decorator
		end,
		decorate = function(target, decorator)
			registered[target] = decorator
		end,
		relabel = function(target, relabeler)
			registered[target] = relabeler
		end,
	},
	discord = {
		permission = { KICK_MEMBERS = 2, BAN_MEMBERS = 4, ADMINISTRATOR = 8 },
		message = function(channel, message)
			return { id = message, channel_id = channel, author = { bot = true } }
		end,
	},
}
dofile(arg[1])

local Color, Pill, Permission = astral.Color, astral.Pill, astral.Permission

assert(Permission.ADMINISTRATOR == 8)
assert(astral.discord.permission == nil)
assert(not pcall(function()
	return Permission.MISSING
end))
assert(not pcall(function()
	Permission.ADMINISTRATOR = 1
end))
assert(astral.permissions.any(4, Permission.KICK_MEMBERS, Permission.BAN_MEMBERS))
assert(not astral.permissions.all(4, Permission.KICK_MEMBERS, Permission.BAN_MEMBERS))
assert(astral.permissions.all(6, Permission.KICK_MEMBERS, Permission.BAN_MEMBERS))
assert(not astral.permissions.any(nil, Permission.ADMINISTRATOR))

assert(Color.hex("#5865F2").value == 0xFF5865F2)
assert(Color.hex("fff").value == 0xFFFFFFFF)
assert(Color.hex("#805865F2").value == 0x805865F2)
assert(not pcall(Color.hex, "nope"))
assert(Color.rgb(88, 101, 242) == Color.Blurple)
assert(Color.from(0x2D7FD0):opaque().value == 0xFF2D7FD0)
assert(Color.contrasting({ Color.White }) == Color.Black)
assert(Color.contrasting({ Color.Black }) == Color.White)
assert(tostring(Color.Blurple) == "Color(#FF5865F2)")

local single = Pill("Mod"):color("#4E7FFF"):build()
assert(single.text == "Mod" and single.color == 0xFF4E7FFF and single.shape == "pill" and single.colors == nil)
assert(single.text_color == 0xFF000000 or single.text_color == 0xFFFFFFFF)
local gradient = Pill("Holo", { colors = { 0xFFA9C9FF, 0xFFFFBBEC } }):shaped(astral.Shape.Op):build()
assert(gradient.shape == "op" and #gradient.colors == 2)
assert(Pill("Plain"):build().color == Color.Blurple.value)

local changed
astral.settings.on_change("enabled", function(value)
	changed = value
end)
local section = astral.settings.section("General")
assert(section:switch("enabled", "Enabled", true) == true)
assert(astral.settings.values.enabled == true)
astral.on_setting("enabled", false)
assert(astral.settings.values.enabled == false and changed == false)
astral.settings.set("enabled", true)
assert(registered.stored[1] == "enabled" and registered.stored[2] == true)
assert(astral.settings.values.enabled == true and changed == true)

astral.ui.decorate(astral.Target.MessageAuthor, function(node)
	return { Pill(node:is_bot() and "Bot" or "User"), { text = "Raw", color = 0xFF000000 } }
end)
local pills = registered["message.author"]({ channel_id = "c", message_id = "m", user_id = "u" })
assert(#pills == 2 and pills[1].text == "Bot" and pills[2].text_color == 0xFFFFFFFF)

astral.ui.message_background(function(node)
	assert(node.message_id == "m" and node.author_id == "u" and node.channel_id == "c")
	return { color = "#805865F2", radius = 12 }
end)
local background = registered.message_background({ message_id = "m", author_id = "u", channel_id = "c" })
assert(background.color == 0x805865F2 and background.radius == 12)

local Animal = astral.class("Animal")
function Animal:init(name)
	self.name = name
end
function Animal:describe()
	return self.name
end
local Dog = astral.class("Dog", Animal)
local dog = Dog("Rex")
assert(Dog.is(dog) and Animal.is(dog) and tostring(dog) == "Dog(Rex)")

assert(type(astral.state) == "table" and next(astral.state) == nil)

assert(astral.snowflake.timestamp("175928847299117063") == 1462015105796)
assert(astral.snowflake.timestamp("nope") == nil)
assert(astral.snowflake.from_time(1462015105796) == "175928847298985984")
assert(astral.snowflake.from_time(0) == nil)
assert(astral.snowflake.is_valid("175928847299117063") and not astral.snowflake.is_valid("17x"))
assert(astral.snowflake.compare("2", "10") == -1 and astral.snowflake.compare("10", "10") == 0)
assert(astral.snowflake.compare("a", "1") == nil)

print("prelude ok")

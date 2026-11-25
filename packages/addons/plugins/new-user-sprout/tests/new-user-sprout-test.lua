local decorator
local now = 1795600000000
astral = {
	log = function() end,
	settings = {
		get = function(_, fallback)
			return fallback
		end,
		section = function()
			return { number = function() end }
		end,
	},
	i18n = {
		t = function(key)
			return key
		end,
	},
	time = {
		now_ms = function()
			return now
		end,
	},
	ui = {
		decorate = function(_, fn)
			decorator = fn
		end,
	},
	discord = { permission = {} },
}
astral.Target = { MessageAuthor = "message.author" }
astral.Pill = function(text, options)
	return { text = text, icon = options.icon }
end
dofile(arg[1])
local function snowflake(ms)
	return tostring((ms - 1420070400000) << 22)
end
local function node(id, bot)
	return {
		user_id = id,
		is_bot = function()
			return bot
		end,
	}
end
local fresh = decorator(node(snowflake(now - 2 * 86400000)))
assert(fresh and fresh.text == "NEW_ACCOUNT" and fresh.icon == "sprout.png")
assert(decorator(node(snowflake(now - 30 * 86400000))) == nil)
assert(decorator(node(snowflake(now - 86400000), true)) == nil)
print("new-user-sprout ok")

local decorator, request_callback, requested, profile_callback
local values = {}
local function pill(text)
	local built = { text = text }
	return setmetatable(built, {
		__index = {
			shaped = function(self, shape)
				self.shape = shape
				return self
			end,
			color = function(self, color)
				self.color_value = color
				return self
			end,
		},
	})
end
astral = {
	log = function() end,
	i18n = {
		t = function(key)
			return key
		end,
	},
	Pill = pill,
	Shape = { Text = "text", Pill = "pill" },
	Target = { MessageAuthor = "message.author" },
	settings = {
		get = function(key, fallback)
			if values[key] == nil then
				return fallback
			end
			return values[key]
		end,
		option = function(value)
			return value
		end,
		section = function()
			local noop = function() end
			return { choice = noop, color = noop, switch = noop }
		end,
	},
	time = {
		after = function(_, fn)
			fn()
		end,
	},
	net = {
		request = function(options, fn)
			requested = options.url
			request_callback = fn
		end,
	},
	discord = {
		me = function()
			return "me"
		end,
		request = function(_, fn)
			profile_callback = fn
		end,
	},
	ui = {
		decorate = function(_, fn)
			decorator = fn
		end,
		refresh = function() end,
	},
}
dofile(arg[1])
assert(decorator({ user_id = "1" }) == nil)
assert(requested:find("ids=1", 1, true))
request_callback({ json = { ["1"] = { sets = { en = { "they" } } } } })
local text = decorator({ user_id = "1" })
assert(text.text == "• they/them" and text.shape == "text")
values.format = "capitalized"
values.style = "pill"
local shown = decorator({ user_id = "1" })
assert(shown.text == "They/Them" and shown.shape == nil)
values.style = "custom"
values.pillColor = "#FF0000"
assert(decorator({ user_id = "1" }).color_value == "#FF0000")
profile_callback({ ok = true, body = { user_profile = { pronouns = "fae/faer" } } })
values.priority = "discord"
values.format = "lowercase"
assert(decorator({ user_id = "1" }).text == "fae/faer")
decorator({ user_id = "me" })
request_callback({ json = { me = { sets = { en = { "he" } } } } })
values.showSelf = false
assert(decorator({ user_id = "me" }) == nil)
values.showSelf = true
values.priority = "pronoundb"
assert(decorator({ user_id = "me" }).text == "he/him")
print("pronoundb ok")

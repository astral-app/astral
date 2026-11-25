local decorator, request_callback, requested
local values = {}
astral = {
	log = function() end,
	Target = { MessageAuthor = "message.author" },
	Pill = function(text)
		return setmetatable({ text = text }, {
			__index = {
				gradient = function(self)
					self.plain = true
					return self
				end,
			},
		})
	end,
	settings = {
		get = function(key, fallback)
			return values[key] or fallback
		end,
		option = function(value)
			return value
		end,
		section = function()
			return { choice = function() end }
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
local pill = decorator({ user_id = "1" })
assert(pill.text == "they/them" and pill.plain)
values.format = "capitalized"
values.style = "pill"
assert(decorator({ user_id = "1" }).text == "they/them")
print("pronoundb ok")

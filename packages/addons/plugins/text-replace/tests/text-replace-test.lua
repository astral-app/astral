local transform
local values = {}
local copied
local picker
local alert

astral = {
	settings = {
		text = function(_, _, default)
			return default
		end,
		choice = function(_, _, _, default)
			return default
		end,
		option = function(value, title)
			return { value = value, title = title }
		end,
		get = function(key, fallback)
			if values[key] == nil then
				return fallback
			end
			return values[key]
		end,
		set = function(key, value)
			values[key] = value
		end,
		section = function()
			return { text = function() end }
		end,
	},
	ui = {
		copy = function(value)
			copied = value
			return true
		end,
		alert = function(title, message)
			alert = { title = title, message = message }
		end,
	},
	files = {
		pick = function(_, callback)
			picker = callback
		end,
	},
	messages = {
		transform = function(callback)
			transform = callback
		end,
	},
	discord = { permission = {} },
	log = function() end,
}

dofile(arg[2])
dofile(arg[1])

values.rules = "teh => the\n/foo(%d+)/ => bar%1\ncat => feline => cat"
assert(transform("teh foo123 cat", { channel_id = "1", operation = "create" }) == "the bar123 feline")
assert(transform("teh foo123", {}) == "the bar123")
assert(transform("no rule matches", {}) == nil)

astral.on_setting("transfer", "export")
assert(copied == values.rules)
assert(values.transfer == "idle")

astral.on_setting("transfer", "import")
assert(picker)
picker({ name = "rules.txt", data = "hello => hi" })
assert(values.rules == "hello => hi")
assert(alert.title == "Rules imported")

print("text-replace ok")

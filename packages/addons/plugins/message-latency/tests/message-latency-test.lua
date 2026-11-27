local gateway, insert
local values = {}
astral = {
	i18n = {
		t = function(key)
			local strings = {}
			return strings[key] or key
		end,
	},
	log = function() end,
	settings = {
		get = function(key, fallback)
			if values[key] == nil then
				return fallback
			end
			return values[key]
		end,
		section = function()
			return { number = function() end, switch = function() end }
		end,
	},
	events = {
		on = function(_, fn)
			gateway = fn
		end,
	},
	ui = {
		insert = function(_, fn)
			insert = fn
		end,
		refresh = function() end,
	},
}
dofile(arg[1])
local function snowflake(ms)
	return tostring(ms << 22)
end
gateway({ type = "READY", data = { user = { id = "me" } } })
gateway({ type = "MESSAGE_CREATE", data = { id = snowflake(5000), nonce = snowflake(1500), author = { id = "u" } } })
gateway({ type = "MESSAGE_CREATE", data = { id = snowflake(9000), nonce = snowflake(8500), author = { id = "u" } } })
assert(insert({ key = snowflake(5000) }).text:find("3s", 1, true))
assert(insert({ key = snowflake(9000) }) == nil)
values.milliseconds = true
assert(insert({ key = snowflake(5000) }).text:find("3.50s", 1, true))
values.ignore_self = true
gateway({ type = "MESSAGE_CREATE", data = { id = snowflake(20000), nonce = snowflake(10000), author = { id = "me" } } })
assert(insert({ key = snowflake(20000) }) == nil)
print("message-latency ok")

local gateway, insert, press
local now = 1000
local values = {}
local toasted
astral = {
	log = function() end,
	InsertNodeType = { View = "view", Text = "text", Image = "image" },
	i18n = {
		t = function(key)
			return ({ TYPING_TWO = "{a} and {b} are typing..." })[key] or key
		end,
	},
	settings = {
		get = function(key, fallback)
			if values[key] == nil then
				return fallback
			end
			return values[key]
		end,
		option = function(value, title)
			return { value = value, title = title }
		end,
		section = function()
			return { switch = function() end, choice = function() end }
		end,
		on_change = function() end,
	},
	time = {
		now_ms = function()
			return now
		end,
		after = function()
			return 1
		end,
		cancel = function() end,
	},
	events = {
		on = function(_, fn)
			gateway = fn
		end,
	},
	discord = {
		relationship = function(id)
			return id == "blocked" and "blocked" or nil
		end,
		ignored = function(id)
			return id == "ignored"
		end,
		muted = function(channel)
			return channel == "muted"
		end,
		user = function(id)
			return { id = id, username = id, avatar = id == "a" and "hash" or nil }
		end,
	},
	ui = {
		insert = function(_, fn, on_press)
			insert, press = fn, on_press
		end,
		refresh = function() end,
		toast = function(_, text)
			toasted = text
		end,
	},
}
dofile(arg[1])

local function texts(node)
	local found = {}
	for _, child in ipairs(node.children) do
		if child.type == astral.InsertNodeType.Text then
			found[#found + 1] = child.text
		end
	end
	return found
end

gateway({ type = "READY", data = { user = { id = "me" } } })
for _, typing in ipairs({
	{ "c1", "a" },
	{ "c1", "b" },
	{ "c2", "me" },
	{ "c3", "blocked" },
	{ "c4", "ignored" },
	{ "muted", "a" },
}) do
	gateway({ type = "TYPING_START", data = { channel_id = typing[1], user_id = typing[2] } })
end

local both = insert({ key = "c1" })
assert(
	#both.children == 3
		and both.children[1].type == astral.InsertNodeType.Image
		and both.children[1].icon:find("/avatars/a/hash")
)
assert(both.children[2].icon:find("/embed/avatars/") and texts(both)[1] == "•••")
assert(insert({ key = "c2" }) == nil and insert({ key = "c3" }) == nil and insert({ key = "c4" }) == nil)
assert(insert({ key = "muted" }) == nil)

values.appearance = "dots"
assert(#insert({ key = "c1" }).children == 1 and texts(insert({ key = "c1" }))[1] == "••• 2")
values.appearance = "avatars"
assert(#texts(insert({ key = "c1" })) == 0)

values.blocked, values.ignored, values.muted = true, true, true
assert(insert({ key = "c3" }) and insert({ key = "c4" }) and insert({ key = "muted" }))
values.current = false
assert(insert({ key = "c1", channel_id = "c1" }) == nil)

press({ key = "c1" }, "typing")
assert(toasted == "a and b are typing...")

gateway({ type = "MESSAGE_CREATE", data = { channel_id = "c1", author = { id = "a" } } })
values.appearance = "dots"
assert(texts(insert({ key = "c1" }))[1] == "•••")
now = now + 11000
assert(insert({ key = "c1" }) == nil)
print("channel-typing-indicators ok")

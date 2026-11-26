local actions, opened, handler, copied, closed = {}, nil, nil, nil, false
local strings = {
	VIEW_RAW = "View Raw",
	RAW_TITLE = "Raw {type} Data",
	TYPE_MESSAGE = "Message",
	MESSAGE_CONTENT = "Message Content",
	MESSAGE_DATA = "Message Data",
	COPY_DATA = "Copy data",
	COPY_CONTENT = "Copy content",
	CLOSE = "Close",
	DATA_COPIED = "{type} data copied to clipboard.",
	CONTENT_COPIED = "Raw content copied to clipboard.",
}
astral = {
	log = function() end,
	settings = {},
	i18n = {
		t = function(key)
			return strings[key] or key
		end,
	},
	discord = {
		raw_message = function(channel, message)
			return channel == "9" and message == "1" and { id = "1", content = "**hi**" } or nil
		end,
	},
	json = {
		encode = function(value, options)
			return (options and options.indent == 2) and ('{"id":"' .. value.id .. '"}') or "?"
		end,
	},
	ui = {
		message_action = function(title, fn, options)
			actions[title] = { fn = fn, options = options }
		end,
		sheet = function(spec, fn)
			opened, handler = spec, fn
			return {
				close = function()
					closed = true
				end,
			}
		end,
		copy = function(text)
			copied = text
		end,
		toast = function() end,
	},
}
dofile(arg[1])

local action = actions["View Raw"]
assert(action and action.options.icon == "res://ic_mana_angle_brackets")
action.fn({ channel_id = "9", message_id = "1" })
assert(opened.title == "Raw Message Data" and opened.expanded)
assert(opened.items[1].text == "Message Content" and opened.items[2].text == "**hi**")
assert(opened.items[4].text == '{"id":"1"}')
assert(#opened.items[5].buttons == 3 and opened.items[5].buttons[1].style == astral.ButtonStyle.Primary)
handler("copy_data", "")
assert(copied == '{"id":"1"}')
handler("copy_content", "")
assert(copied == "**hi**")
handler("close", "")
assert(closed)
print("view-raw ok")

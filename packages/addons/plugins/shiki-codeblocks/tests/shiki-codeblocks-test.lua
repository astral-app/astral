local previous_path = package.path
package.path = arg[1]:gsub("main.lua$", "?.lua;") .. previous_path

local build, press, copied, toast = nil, nil, nil, nil
local section = {
	choice = function() end,
	text = function() end,
	number = function() end,
}

astral = {
	TextStyle = { Bold = 1, Italic = 2, Underline = 4 },
	InsertNodeType = { View = "view", Text = "text", Image = "image" },
	Align = { Start = "start" },
	i18n = {
		t = function(key)
			return key
		end,
	},
	settings = {
		values = { theme = "dark-plus", bgOpacity = 100, useDevIcon = "disabled" },
		option = function(id, label)
			return { id = id, label = label }
		end,
		section = function()
			return section
		end,
		on_change = function() end,
	},
	storage = {
		get = function()
			return '{"type":"dark","colors":{}}'
		end,
		set = function() end,
	},
	time = { after = function() end },
	discord = {
		raw_message = function()
			return { content = "```lua\nlocal x = 1\nprint(x)\n```" }
		end,
	},
	code = {
		highlight = function(_, language)
			assert(language == "lua")
			return {
				found = true,
				language = "Lua",
				background = 0xFF1E1E1E,
				foreground = 0xFFD4D4D4,
				lines = {
					{ { "local", 0xFF569CD6, 1 }, { " x = 1", nil, 0 } },
					{},
				},
			}
		end,
	},
	ui = {
		insert = function(selector, fn, on_press)
			assert(selector == "code:")
			build, press = fn, on_press
		end,
		refresh = function() end,
		copy = function(text)
			copied = text
			return true
		end,
		toast = function(_, text)
			toast = text
		end,
	},
}

dofile(arg[1])

local block = { message_id = "1", channel_id = "2", index = 0, code = "local x = 1\n" }
local card = build(block)
local header, body = card.children[1], card.children[2]
assert(header.row and header.radius[1] == 6 and header.radius[3] == 0)
assert(header.children[1].text == "Lua" and header.children[1].grow)
assert(header.children[2].action == "copy" and header.children[2].children[1].text == "COPY")
assert(#body.children == 2)
local cells = body.children[1].children
assert(cells[1].runs[1].text == " 1 " and cells[1].font == "monospace")
assert(cells[2].runs[1].text == "local" and cells[2].runs[1].bold and cells[2].runs[1].color == 0xFF569CD6)
assert(cells[2].runs[2].color == 0xFFD4D4D4 and cells[2].grow)
assert(body.children[2].children[2].runs[1].text == " ")

press(block, "copy")
assert(copied == "local x = 1\n" and toast == "CODE_COPIED")

print("shiki-codeblocks ok")

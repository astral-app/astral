local commands = {}
local stored = {}
local button
local sheets = {}
astral = {
	log = function() end,
	OptionType = { String = "string", Boolean = "boolean" },
	ButtonStyle = { Primary = "primary", Danger = "danger" },
	SheetItemType = {
		Heading = "heading",
		Text = "text",
		Code = "code",
		Input = "input",
		Row = "row",
		Buttons = "buttons",
	},
	storage = {
		get = function(key)
			return stored[key]
		end,
		set = function(key, value)
			stored[key] = value
		end,
	},
	commands = {
		register = function(command)
			commands[command.name] = command
		end,
		unregister = function(name)
			commands[name] = nil
		end,
	},
	settings = {
		button = function(_, _, fn)
			button = fn
		end,
	},
	ui = {
		sheet = function(spec, fn)
			local sheet = { spec = spec, fn = fn }
			function sheet:update(next)
				self.spec = next
			end
			function sheet:close()
				self.closed = true
			end
			sheets[#sheets + 1] = sheet
			return sheet
		end,
		toast = function() end,
		confirm = function(_, done)
			done(true)
		end,
	},
	files = {
		pick = function(_, done)
			done({ name = "cat.PNG", data = "\137PNG" })
		end,
	},
	base64 = {
		encode = function(text)
			return "b64:" .. text
		end,
		decode = function(text)
			return text:sub(5)
		end,
	},
	i18n = {
		t = function(key, values)
			return key .. (values and values.name and (":" .. values.name) or "")
		end,
	},
}
dofile(arg[1])
button()
sheets[1].fn("create")
local editor = sheets[2]
editor.fn("name", "/Greet")
editor.fn("response", "Hello {{user}}! I am feeling {{mood = great}}.\\nBye")
editor.fn("save")
assert(editor.closed and commands.greet, "saved")
local options = commands.greet.options
assert(options[1].name == "user" and options[1].required and options[2].name == "mood" and not options[2].required)
assert(options[3].name == "ephemeral" and options[3].type == astral.OptionType.Boolean)
local sent = commands.greet.execute({ user = "Ada" })
assert(sent.send.content == "Hello Ada! I am feeling great.\nBye", sent.send.content)
assert(
	commands.greet.execute({ user = "Ada", mood = "fine", ephemeral = true }) == "Hello Ada! I am feeling fine.\nBye"
)
button()
sheets[3].fn("edit:greet")
local edit = sheets[4]
edit.fn("attach")
edit.fn("name", "hello")
edit.fn("save")
assert(commands.greet == nil and commands.hello, "renamed")
local with_file = commands.hello.execute({ user = "Ada" })
assert(
	with_file.send.file.name == "cat.PNG"
		and with_file.send.file.data == "\137PNG"
		and with_file.send.file.content_type == "image/png"
)
assert(commands.hello.execute({ user = "Ada", ephemeral = true }) == "EPHEMERAL_WITH_ATTACHMENTS")
button()
sheets[5].fn("create")
sheets[6].fn("name", "bad name!")
sheets[6].fn("response", "x")
sheets[6].fn("save")
assert(not sheets[6].closed and sheets[6].spec.items[#sheets[6].spec.items].text == "INVALID_NAME")
sheets[6].fn("name", "x")
sheets[6].fn("response", "{{ephemeral}}")
sheets[6].fn("save")
assert(sheets[6].spec.items[#sheets[6].spec.items].text == "RESERVED_ARGUMENT")
button()
sheets[7].fn("edit:hello")
sheets[8].fn("delete")
assert(commands.hello == nil and #stored.commands == 0)
print("custom-commands ok")

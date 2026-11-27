local transform
local stored = {}
local buttons = {}
local sheets = {}
local toasts = {}
local clipboard
local encoded = {}

local function sheet_handle(spec, handler)
	local handle = { spec = spec, handler = handler, closed = false }
	function handle:update(next_spec)
		self.spec = next_spec
	end
	function handle:close()
		self.closed = true
		self.handler("closed", "")
	end
	sheets[#sheets + 1] = handle
	return handle
end

local Section = {}
Section.__index = Section
function Section.button(_, key, _, callback)
	buttons[key] = callback
end

astral = {
	i18n = {
		t = function(key, values)
			if key == "RULES_SECTION_TITLE" then
				return values.title .. " · " .. values.count
			end
			return key
		end,
	},
	time = {
		now_ms = function()
			return 1
		end,
	},
	storage = {
		get = function(key, fallback)
			if stored[key] == nil then
				return fallback
			end
			return stored[key]
		end,
		set = function(key, value)
			stored[key] = value
		end,
	},
	settings = {
		section = function(title)
			return setmetatable({ title = title }, Section)
		end,
	},
	json = {
		array = function()
			return {}
		end,
		encode = function(value)
			local key = "json:" .. (#encoded + 1)
			encoded[#encoded + 1] = value
			return key
		end,
		decode = function(value)
			local index = tonumber(value:match("^json:(%d+)$"))
			if not index then
				error("invalid json")
			end
			return encoded[index]
		end,
	},
	ui = {
		sheet = sheet_handle,
		toast = function(title)
			toasts[#toasts + 1] = title
		end,
		copy = function(value)
			clipboard = value
			return true
		end,
		paste = function()
			return clipboard
		end,
	},
	messages = {
		transform = function(callback)
			transform = callback
		end,
	},
}

dofile(arg[1])

assert(transform("teh", {}) == nil)

buttons.stringRules()
local list = sheets[#sheets]
assert(list.spec.title == "TEXT_RULES_TITLE · 0")
assert(list.spec.items[1].text == "NO_RULES_YET")
assert(list.spec.header[1].buttons[1].action == "add")

list.handler("add", "")
local editor = sheets[#sheets]
assert(editor.spec.title == "NEW_TEXT_RULE")
assert(editor.spec.items[2].items[2].key == "find")
editor.handler("change:find", "teh")
editor.handler("change:replace", "the")
editor.handler("save", "")
assert(editor.closed)
assert(list.spec.title == "TEXT_RULES_TITLE · 1")
assert(list.spec.items[1].action:match("^edit:"))
assert(stored.ruleset.stringRules[1].find == "teh")

assert(transform("teh cat", { channel_id = "1" }) == "the cat")
assert(transform("teh cat", { channel_id = "1102784112584040479" }) == nil)

buttons.regexRules()
local regex_list = sheets[#sheets]
regex_list.handler("add", "")
local regex_editor = sheets[#sheets]
regex_editor.handler("change:find", "/(%d+)kg/")
regex_editor.handler("change:replace", "$1 kilograms")
regex_editor.handler("change:onlyIfIncludes", "weight")
regex_editor.handler("save", "")
assert(transform("weight 5kg", {}) == "weight 5 kilograms")
assert(transform("5kg", {}) == nil)

list.handler(list.spec.items[1].action, "")
local edit = sheets[#sheets]
assert(edit.spec.title == "EDIT_RULE")
assert(edit.spec.items[2].items[2].value == "teh")
edit.handler("delete", "")
assert(#stored.ruleset.stringRules == 0)
assert(list.spec.title == "TEXT_RULES_TITLE · 0")

buttons.test()
local tester = sheets[#sheets]
tester.handler("change:example", "weight 2kg")
assert(tester.spec.items[3].text == "weight 2 kilograms")
assert(tester.spec.items[1].items[2].key == "example")

buttons.export()
assert(toasts[#toasts] == "RULESET_COPIED")
local exported = encoded[tonumber(clipboard:match("%d+"))]
assert(exported.version == 1 and exported.regexRules[1].find == "/(%d+)kg/")

stored.ruleset = nil
clipboard = "not json"
buttons.import()
assert(toasts[#toasts] == "IMPORT_FAILED")
clipboard =
	astral.json.encode({ version = 1, stringRules = { { find = "brb", replace = "be right back" } }, regexRules = {} })
buttons.import()
assert(toasts[#toasts] == "RULESET_IMPORTED")
assert(transform("brb", {}) == "be right back")

print("text-replace ok")

local t = astral.i18n.t

local RULES_CHANNEL_ID = "1102784112584040479"
local RULESET_VERSION = 1
local MAXIMUM_RULES = 128
local MAXIMUM_FIELD_LENGTH = 512
local MAXIMUM_OUTPUT_LENGTH = 20000
local KINDS = { "stringRules", "regexRules" }

local created = 0

local function new_id()
	created = created + 1
	return tostring(astral.time.now_ms()) .. "-" .. created
end

local function text_field(value)
	return type(value) == "string" and value:sub(1, MAXIMUM_FIELD_LENGTH) or ""
end

local function normalize_rules(value)
	local rules = {}
	if type(value) ~= "table" then
		return rules
	end
	for _, rule in ipairs(value) do
		if #rules >= MAXIMUM_RULES then
			break
		end
		if type(rule) == "table" then
			rules[#rules + 1] = {
				find = text_field(rule.find),
				replace = text_field(rule.replace),
				onlyIfIncludes = text_field(rule.onlyIfIncludes),
				id = type(rule.id) == "string" and rule.id ~= "" and rule.id or new_id(),
			}
		end
	end
	return rules
end

local function load_ruleset()
	local stored = astral.storage.get("ruleset", {})
	stored = type(stored) == "table" and stored or {}
	return { stringRules = normalize_rules(stored.stringRules), regexRules = normalize_rules(stored.regexRules) }
end

local ruleset = load_ruleset()

local function save_ruleset()
	astral.storage.set("ruleset", ruleset)
end

local function with_newlines(value)
	return (value:gsub("\\n", "\n"))
end

local function replace_literal(value, find, replacement)
	local output = {}
	local offset = 1
	while offset <= #value do
		local first, last = value:find(find, offset, true)
		if not first then
			output[#output + 1] = value:sub(offset)
			break
		end
		output[#output + 1] = value:sub(offset, first - 1)
		output[#output + 1] = replacement
		offset = last + 1
	end
	return table.concat(output)
end

local function pattern_of(find)
	local body = find:match("^/(.+)/%a*$")
	return body or find
end

local function lua_replacement(replacement)
	local escaped = replacement:gsub("%%", "%%%%")
	return (escaped:gsub("%$(%d)", "%%%1"))
end

local function applies(rule, content)
	return rule.find ~= "" and (rule.onlyIfIncludes == "" or content:find(rule.onlyIfIncludes, 1, true) ~= nil)
end

local function trim(value)
	return (value:gsub("^%s+", ""):gsub("%s+$", ""))
end

local function apply_rules(content)
	if content == "" then
		return content
	end
	for _, rule in ipairs(ruleset.stringRules) do
		if applies(rule, content) then
			content = replace_literal(content, rule.find, with_newlines(rule.replace))
		end
		if #content > MAXIMUM_OUTPUT_LENGTH then
			return content
		end
	end
	for _, rule in ipairs(ruleset.regexRules) do
		if applies(rule, content) then
			local ok, rewritten =
				pcall(string.gsub, content, pattern_of(rule.find), lua_replacement(with_newlines(rule.replace)))
			if ok then
				content = rewritten
			end
		end
		if #content > MAXIMUM_OUTPUT_LENGTH then
			return content
		end
	end
	return trim(content)
end

astral.messages.transform(function(content, context)
	if type(context) == "table" and context.channel_id == RULES_CHANNEL_ID then
		return nil
	end
	local rewritten = apply_rules(content)
	if rewritten ~= content then
		return rewritten
	end
	return nil
end)

local function kind_messages(kind)
	if kind == "stringRules" then
		return {
			title = t("TEXT_RULES_TITLE"),
			description = t("TEXT_RULES_DESCRIPTION"),
			tag = "",
			find_label = t("TEXT_FIND_LABEL"),
			find_placeholder = t("TEXT_FIND_PLACEHOLDER"),
			help = t("TEXT_HELP"),
			add_rule = t("ADD_TEXT_RULE"),
			new_rule = t("NEW_TEXT_RULE"),
		}
	end
	return {
		title = t("REGEX_RULES_TITLE"),
		description = t("REGEX_RULES_DESCRIPTION"),
		tag = t("REGEX_RULE_TAG"),
		find_label = t("REGEX_FIND_LABEL"),
		find_placeholder = t("REGEX_FIND_PLACEHOLDER"),
		help = t("REGEX_HELP"),
		add_rule = t("ADD_REGEX_RULE"),
		new_rule = t("NEW_REGEX_RULE"),
	}
end

local function field(label, key, value, placeholder, description)
	local items = {
		{ type = astral.SheetItemType.Text, text = label, bold = true, size = 13, color = astral.SheetColor.Muted },
		{
			type = astral.SheetItemType.Input,
			key = key,
			value = value,
			placeholder = placeholder,
			change_action = "change:" .. key,
		},
	}
	if description then
		items[#items + 1] =
			{ type = astral.SheetItemType.Text, text = description, size = 12, color = astral.SheetColor.Muted }
	end
	return { type = astral.SheetItemType.Column, spacing = 6, items = items }
end

local function find_index(kind, id)
	for index, rule in ipairs(ruleset[kind]) do
		if rule.id == id then
			return index
		end
	end
	return nil
end

local function open_rule_editor(kind, rule, is_new, on_close)
	local draft = { find = rule.find, replace = rule.replace, onlyIfIncludes = rule.onlyIfIncludes }
	local messages = kind_messages(kind)
	local editor
	local function render()
		local buttons = { { label = t("SAVE_RULE"), action = "save", style = astral.ButtonStyle.Primary } }
		if not is_new then
			buttons[#buttons + 1] = { label = t("DELETE_RULE"), action = "delete", style = astral.ButtonStyle.Danger }
		end
		return {
			title = is_new and messages.new_rule or t("EDIT_RULE"),
			expanded = true,
			items = {
				{ type = astral.SheetItemType.Text, text = messages.help, size = 14, color = astral.SheetColor.Muted },
				field(messages.find_label, "find", draft.find, messages.find_placeholder),
				field(t("REPLACE_WITH"), "replace", draft.replace, t("REPLACE_PLACEHOLDER")),
				field(
					t("ONLY_WHEN_CONTAINS"),
					"onlyIfIncludes",
					draft.onlyIfIncludes,
					t("CONDITION_PLACEHOLDER"),
					t("CONDITION_DESCRIPTION")
				),
				{ type = astral.SheetItemType.Buttons, direction = astral.Direction.Column, buttons = buttons },
			},
		}
	end
	editor = astral.ui.sheet(render(), function(action, value)
		if action == "closed" then
			on_close()
			return
		end
		local changed = action:match("^change:(.+)$")
		if changed then
			draft[changed] = text_field(value)
		elseif action == "save" and trim(draft.find) ~= "" then
			local saved =
				{ find = draft.find, replace = draft.replace, onlyIfIncludes = draft.onlyIfIncludes, id = rule.id }
			local index = find_index(kind, rule.id)
			if index then
				ruleset[kind][index] = saved
			elseif #ruleset[kind] < MAXIMUM_RULES then
				ruleset[kind][#ruleset[kind] + 1] = saved
			end
			save_ruleset()
			editor:close()
		elseif action == "delete" then
			local index = find_index(kind, rule.id)
			if index then
				table.remove(ruleset[kind], index)
				save_ruleset()
			end
			editor:close()
		end
	end)
end

local function rule_row(rule, tag)
	local condition = rule.onlyIfIncludes ~= "" and t("ONLY_WHEN_INCLUDES", { text = rule.onlyIfIncludes })
		or t("APPLIES_TO_EVERY_MESSAGE")
	return {
		type = astral.SheetItemType.Row,
		spacing = 12,
		action = "edit:" .. rule.id,
		items = {
			{
				type = astral.SheetItemType.Column,
				spacing = 2,
				items = {
					{ type = astral.SheetItemType.Text, text = rule.find, bold = true, size = 15, lines = 1 },
					{
						type = astral.SheetItemType.Text,
						text = "→ " .. (rule.replace ~= "" and rule.replace or t("REMOVE_IT")),
						size = 12,
						color = astral.SheetColor.Muted,
						lines = 1,
					},
					{
						type = astral.SheetItemType.Text,
						text = tag .. condition,
						size = 12,
						color = astral.SheetColor.Muted,
						lines = 1,
					},
				},
			},
			{ type = astral.SheetItemType.Icon, name = "chevron_right", size = 14, color = astral.SheetColor.Muted },
		},
	}
end

local function open_rule_list(kind)
	local messages = kind_messages(kind)
	local list
	local function render()
		local items = {}
		local rules = ruleset[kind]
		if #rules == 0 then
			items[1] = { type = astral.SheetItemType.Text, text = t("NO_RULES_YET"), bold = true, size = 15 }
			items[2] = {
				type = astral.SheetItemType.Text,
				text = t("NO_RULES_SUBTITLE"),
				size = 13,
				color = astral.SheetColor.Muted,
			}
		end
		for index, rule in ipairs(rules) do
			if index > 1 then
				items[#items + 1] = { type = astral.SheetItemType.Divider }
			end
			items[#items + 1] = rule_row(rule, messages.tag)
		end
		return {
			title = t("RULES_SECTION_TITLE", { title = messages.title, count = #rules }),
			subtitle = messages.description,
			expanded = true,
			header = {
				{ type = astral.SheetItemType.Buttons, buttons = { { label = messages.add_rule, action = "add" } } },
			},
			items = items,
		}
	end
	local function reopen()
		if list then
			list:update(render())
		end
	end
	list = astral.ui.sheet(render(), function(action)
		if action == "closed" then
			list = nil
			return
		end
		local id = action:match("^edit:(.+)$")
		local index = id and find_index(kind, id)
		if index then
			open_rule_editor(kind, ruleset[kind][index], false, reopen)
		elseif action == "add" then
			open_rule_editor(kind, { find = "", replace = "", onlyIfIncludes = "", id = new_id() }, true, reopen)
		end
	end)
end

local function open_tester()
	local example = ""
	local tester
	local function render()
		local result = apply_rules(example)
		return {
			title = t("TEST_RULES"),
			expanded = true,
			items = {
				field(t("EXAMPLE_MESSAGE"), "example", example, t("EXAMPLE_PLACEHOLDER")),
				{ type = astral.SheetItemType.Heading, text = t("RESULT") },
				{
					type = astral.SheetItemType.Text,
					text = result ~= "" and result or t("RESULT_PLACEHOLDER"),
					size = 15,
					color = result ~= "" and astral.SheetColor.Primary or astral.SheetColor.Muted,
				},
			},
		}
	end
	tester = astral.ui.sheet(render(), function(action, value)
		if action == "change:example" then
			example = text_field(value)
			tester:update(render())
		end
	end)
end

local function exported_rules(rules)
	local list = astral.json.array()
	for _, rule in ipairs(rules) do
		if rule.find ~= "" then
			list[#list + 1] = { find = rule.find, replace = rule.replace, onlyIfIncludes = rule.onlyIfIncludes }
		end
	end
	return list
end

local function serialize_ruleset()
	return astral.json.encode({
		version = RULESET_VERSION,
		stringRules = exported_rules(ruleset.stringRules),
		regexRules = exported_rules(ruleset.regexRules),
	})
end

local function parse_ruleset(value)
	local ok, parsed = pcall(astral.json.decode, value)
	if not ok or parsed == nil then
		return nil, t("INVALID_JSON")
	end
	if type(parsed) ~= "table" then
		return nil, t("NOT_A_RULESET")
	end
	if
		parsed.version ~= RULESET_VERSION
		or type(parsed.stringRules) ~= "table"
		or type(parsed.regexRules) ~= "table"
	then
		return nil, t("UNSUPPORTED_RULESET")
	end
	local function filled(rules)
		local kept = {}
		for _, rule in ipairs(normalize_rules(rules)) do
			if rule.find ~= "" then
				kept[#kept + 1] = rule
			end
		end
		return kept
	end
	return { stringRules = filled(parsed.stringRules), regexRules = filled(parsed.regexRules) }
end

local function import_clipboard()
	local value = astral.ui.paste()
	if type(value) ~= "string" or not value:match("%S") then
		astral.ui.toast(t("CLIPBOARD_HAS_NO_RULESET"))
		return
	end
	local parsed, failure = parse_ruleset(value)
	if not parsed then
		astral.ui.toast(t("IMPORT_FAILED"), failure)
		return
	end
	ruleset = parsed
	save_ruleset()
	astral.ui.toast(t("RULESET_IMPORTED"))
end

local function export_ruleset()
	if astral.ui.copy(serialize_ruleset()) then
		astral.ui.toast(t("RULESET_COPIED"))
	else
		astral.ui.toast(t("SHARING_UNAVAILABLE"))
	end
end

local rules_section = astral.settings.section(t("RULES"))
for _, kind in ipairs(KINDS) do
	local messages = kind_messages(kind)
	rules_section:button(kind, messages.title, function()
		open_rule_list(kind)
	end, { subtitle = messages.description })
end
rules_section:button("test", t("TEST_RULES"), open_tester, { subtitle = t("TEST_RULES_DESCRIPTION") })

local share_section = astral.settings.section(t("SHARE_RULES"))
share_section:button("export", t("EXPORT_AND_SHARE"), export_ruleset, { subtitle = t("SHARE_RULES_DESCRIPTION") })
share_section:button("import", t("IMPORT_CLIPBOARD"), import_clipboard)

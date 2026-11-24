local MAXIMUM_RULES = 128
local MAXIMUM_FIND_LENGTH = 256
local MAXIMUM_REPLACEMENT_LENGTH = 512
local MAXIMUM_RULESET_LENGTH = 16384

local section = astral.settings.section("Rules")
section:text("rules", "Rules", "", {
	subtitle = "One rule per line: find => replacement => optional context. Wrap Lua patterns in slashes.",
	max_length = MAXIMUM_RULESET_LENGTH,
})

local transfer_key = "transfer"
local transfer_options = {
	astral.settings.option("idle", "Choose action"),
	astral.settings.option("import", "Import rules from a text file"),
	astral.settings.option("export", "Copy rules to the clipboard"),
}

astral.settings.choice(transfer_key, "Import or export", transfer_options, "idle", {
	section = "Rules",
	subtitle = "Transfer the plain text ruleset between devices.",
})

local function parse_rule(line)
	local find, rest = line:match("^%s*(.-)%s*=>%s*(.-)%s*$")
	if not find or find == "" then
		return nil
	end
	local replacement, context = rest:match("^(.-)%s*=>%s*(.-)%s*$")
	if replacement == nil then
		replacement = rest
	end
	if
		#find > MAXIMUM_FIND_LENGTH
		or #replacement > MAXIMUM_REPLACEMENT_LENGTH
		or (context and #context > MAXIMUM_FIND_LENGTH)
	then
		return nil
	end
	local pattern = find:match("^/(.-)/$")
	if find:sub(1, 1) == "/" and pattern == nil then
		return nil
	end
	if pattern ~= nil then
		local valid = pcall(string.find, "", pattern)
		if not valid then
			return nil
		end
		return { pattern = pattern, replacement = replacement, context = context }
	end
	return { find = find, replacement = replacement, context = context }
end

local function parse_rules(value)
	if type(value) ~= "string" or #value > MAXIMUM_RULESET_LENGTH then
		return nil, "The ruleset is too large."
	end
	local rules = {}
	for line in value:gmatch("[^\r\n]+") do
		if line:match("%S") then
			local rule = parse_rule(line)
			if not rule then
				return nil, "A rule is invalid. Check its separator, length, or Lua pattern syntax."
			end
			rules[#rules + 1] = rule
			if #rules > MAXIMUM_RULES then
				return nil, "The ruleset can contain at most 128 rules."
			end
		end
	end
	return rules
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

local function apply_rules(value, rules)
	for _, rule in ipairs(rules) do
		if not rule.context or value:find(rule.context, 1, true) then
			if rule.pattern then
				local ok, rewritten = pcall(string.gsub, value, rule.pattern, rule.replacement)
				if ok then
					value = rewritten
				end
			else
				value = replace_literal(value, rule.find, rule.replacement)
			end
		end
		if #value > 20000 then
			return value
		end
	end
	return value
end

local function current_rules()
	local rules, failure = parse_rules(astral.settings.get("rules", ""))
	if not rules then
		astral.log("Text Replace skipped an invalid ruleset: " .. failure)
		return {}
	end
	return rules
end

astral.messages.transform(function(content)
	local rewritten = apply_rules(content, current_rules())
	if rewritten ~= content then
		return rewritten
	end
	return nil
end)

local function finish_transfer()
	astral.settings.set(transfer_key, "idle")
end

local function import_rules()
	astral.files.pick({ "txt" }, function(file, reason)
		if not file then
			if reason ~= "cancelled" then
				astral.ui.alert("Import failed", "The rules file could not be read.")
			end
			return
		end
		local rules, failure = parse_rules(file.data)
		if not rules then
			astral.ui.alert("Import failed", failure)
			return
		end
		astral.settings.set("rules", file.data)
		astral.ui.alert("Rules imported", #rules .. " rules are ready to use.")
	end)
end

astral.settings.on_change(transfer_key, function(value)
	if value == "import" then
		import_rules()
	elseif value == "export" then
		local copied = astral.ui.copy(astral.settings.get("rules", ""))
		if copied then
			astral.ui.alert("Rules copied", "Paste the text into a .txt file to export it.")
		else
			astral.ui.alert("Export failed", "The clipboard is unavailable.")
		end
	end
	finish_transfer()
end)

local inline = {
	{ open = "||", style = "spoiler" },
	{ open = "**", style = "bold" },
	{ open = "__", style = "underline" },
	{ open = "~~", style = "strike" },
	{ open = "*", style = "italic" },
	{ open = "_", style = "italic" },
}

local blocks = {
	{ prefix = "### ", style = "heading3" },
	{ prefix = "## ", style = "heading2" },
	{ prefix = "# ", style = "heading1" },
	{ prefix = "-# ", style = "subtext" },
	{ prefix = "> ", style = "quote" },
}

local function add(runs, start, length, style)
	if length > 0 then
		runs[#runs + 1] = { start = start, length = length, style = style }
	end
end

local function mark(covered, start, stop)
	for index = start, stop do
		covered[index] = true
	end
end

local function copy(values)
	local result = {}
	for index, value in pairs(values) do
		result[index] = value
	end
	return result
end

local function code_runs(text, runs, covered)
	local cursor = 1
	while cursor <= #text do
		local start = text:find("`", cursor, true)
		if not start then
			return
		end
		local fence = text:sub(start, start + 2) == "```" and 3 or 1
		local close = text:find(string.rep("`", fence), start + fence, true)
		if not close then
			cursor = start + fence
		elseif fence == 1 and text:find("\n", start + 1, true) and text:find("\n", start + 1, true) < close then
			cursor = start + 1
		else
			add(runs, start, fence, "marker")
			add(runs, start + fence, close - start - fence, "code")
			add(runs, close, fence, "marker")
			mark(covered, start, close + fence - 1)
			cursor = close + fence
		end
	end
end

local function cover_emoji(text, covered)
	local cursor = 1
	while cursor <= #text do
		local start, stop = text:find("<a?:[%w_]+:%d+>", cursor)
		local short_start, short_stop = text:find(":[%w_~%+-]+:", cursor)
		if short_start and (not start or short_start < start) then
			start, stop = short_start, short_stop
		end
		if not start then
			return
		end
		mark(covered, start, stop)
		cursor = stop + 1
	end
end

local function block_runs(text, runs, covered)
	local line_start = 1
	while line_start <= #text do
		local line_end = text:find("\n", line_start, true) or (#text + 1)
		if not covered[line_start] then
			local line = text:sub(line_start, line_end - 1)
			for _, block in ipairs(blocks) do
				if line:sub(1, #block.prefix) == block.prefix and #line > #block.prefix then
					add(runs, line_start, #block.prefix, "marker")
					add(runs, line_start + #block.prefix, #line - #block.prefix, block.style)
					break
				end
			end
		end
		line_start = line_end + 1
	end
end

local function is_word_character(value)
	if value == "" then
		return false
	end
	local byte = value:byte()
	return byte >= 128 or value:match("[%w_]") ~= nil
end

local function has_covered(covered, first, last)
	for index = first, last do
		if covered[index] then
			return true
		end
	end
	return false
end

local function inline_runs(text, runs, covered)
	local taken = copy(covered)
	for _, delimiter in ipairs(inline) do
		local cursor = 1
		while cursor <= #text do
			local start = text:find(delimiter.open, cursor, true)
			if not start then
				break
			end
			local close = text:find(delimiter.open, start + #delimiter.open + 1, true)
			local inner_start = start + #delimiter.open
			local inner_end = close and close - 1 or 0
			local after_close = close and close + #delimiter.open or 0
			local first_character = text:sub(inner_start, inner_start)
			local last_character = close and text:sub(inner_end, inner_end) or ""
			local newline = close and text:find("\n", inner_start, true)
			local underscore_boundary = delimiter.open:sub(1, 1) ~= "_"
				or not is_word_character(text:sub(start - 1, start - 1))
					and not is_word_character(text:sub(after_close, after_close))
			local blocked = not close
				or close - inner_start < 1
				or newline and newline < close
				or has_covered(taken, start, after_close - 1)
				or first_character:match("%s") ~= nil
				or last_character:match("%s") ~= nil
				or not underscore_boundary
			if blocked then
				cursor = start + #delimiter.open
			else
				add(runs, start, #delimiter.open, "marker")
				add(runs, inner_start, close - inner_start, delimiter.style)
				add(runs, close, #delimiter.open, "marker")
				mark(taken, start, start + #delimiter.open - 1)
				mark(taken, close, close + #delimiter.open - 1)
				cursor = after_close
			end
		end
	end
end

local function runs(text)
	if type(text) ~= "string" or #text > 8192 then
		return {}
	end
	local result = {}
	local covered = {}
	code_runs(text, result, covered)
	block_runs(text, result, covered)
	cover_emoji(text, covered)
	inline_runs(text, result, covered)
	return result
end

return { runs = runs }

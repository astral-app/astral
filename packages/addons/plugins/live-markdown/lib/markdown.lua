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

local function codeRuns(text, runs, covered)
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

local function coverEmoji(text, covered)
	local cursor = 1
	while cursor <= #text do
		local start, stop = text:find("<a?:[%w_]+:%d+>", cursor)
		local shortStart, shortStop = text:find(":[%w_~%+-]+:", cursor)
		if shortStart and (not start or shortStart < start) then
			start, stop = shortStart, shortStop
		end
		if not start then
			return
		end
		mark(covered, start, stop)
		cursor = stop + 1
	end
end

local function blockRuns(text, runs, covered)
	local lineStart = 1
	while lineStart <= #text do
		local lineEnd = text:find("\n", lineStart, true) or (#text + 1)
		if not covered[lineStart] then
			local line = text:sub(lineStart, lineEnd - 1)
			for _, block in ipairs(blocks) do
				if line:sub(1, #block.prefix) == block.prefix and #line > #block.prefix then
					add(runs, lineStart, #block.prefix, "marker")
					add(runs, lineStart + #block.prefix, #line - #block.prefix, block.style)
					break
				end
			end
		end
		lineStart = lineEnd + 1
	end
end

local function isWordCharacter(value)
	if value == "" then
		return false
	end
	local byte = value:byte()
	return byte >= 128 or value:match("[%w_]") ~= nil
end

local function hasCovered(covered, first, last)
	for index = first, last do
		if covered[index] then
			return true
		end
	end
	return false
end

local function inlineRuns(text, runs, covered)
	local taken = copy(covered)
	for _, delimiter in ipairs(inline) do
		local cursor = 1
		while cursor <= #text do
			local start = text:find(delimiter.open, cursor, true)
			if not start then
				break
			end
			local close = text:find(delimiter.open, start + #delimiter.open + 1, true)
			local innerStart = start + #delimiter.open
			local innerEnd = close and close - 1 or 0
			local afterClose = close and close + #delimiter.open or 0
			local firstCharacter = text:sub(innerStart, innerStart)
			local lastCharacter = close and text:sub(innerEnd, innerEnd) or ""
			local newline = close and text:find("\n", innerStart, true)
			local underscoreBoundary = delimiter.open:sub(1, 1) ~= "_"
				or not isWordCharacter(text:sub(start - 1, start - 1))
					and not isWordCharacter(text:sub(afterClose, afterClose))
			local blocked = not close
				or close - innerStart < 1
				or newline and newline < close
				or hasCovered(taken, start, afterClose - 1)
				or firstCharacter:match("%s") ~= nil
				or lastCharacter:match("%s") ~= nil
				or not underscoreBoundary
			if blocked then
				cursor = start + #delimiter.open
			else
				add(runs, start, #delimiter.open, "marker")
				add(runs, innerStart, close - innerStart, delimiter.style)
				add(runs, close, #delimiter.open, "marker")
				mark(taken, start, start + #delimiter.open - 1)
				mark(taken, close, close + #delimiter.open - 1)
				cursor = afterClose
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
	codeRuns(text, result, covered)
	blockRuns(text, result, covered)
	coverEmoji(text, covered)
	inlineRuns(text, result, covered)
	return result
end

return { runs = runs }

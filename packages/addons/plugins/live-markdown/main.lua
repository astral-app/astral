local markdown = require("lib.markdown")
local emoji = require("lib.emoji")

astral.ui.composer(function(text, context)
	local runs = markdown.runs(text)
	for _, run in ipairs(emoji.runs(text, context)) do
		runs[#runs + 1] = run
	end
	return runs
end)

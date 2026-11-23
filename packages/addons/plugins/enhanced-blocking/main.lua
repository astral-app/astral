local settings = astral.settings.section("Enhanced Blocking")

settings:text(
	"users",
	"Hidden user IDs",
	"",
	{ max_length = 8192, subtitle = "One Discord user ID per line, up to 400 users." }
)
settings:text("keywords", "Hidden keywords", "", {
	max_length = 8192,
	subtitle = "Hide messages containing any of the first 64 words or phrases, up to 128 bytes each.",
})
settings:switch("replies", "Hide replies", true, "Also hide replies to listed users.")

local cachedUsers
local cachedKeywords
local hiddenUsers = {}
local keywords = {}

local function lines(value, maximumCount, maximumLength)
	local result = {}
	for line in (value .. "\n"):gmatch("(.-)\n") do
		line = line:gsub("\r", ""):match("^%s*(.-)%s*$")
		if line ~= "" and #line <= maximumLength then
			result[#result + 1] = line
			if #result >= maximumCount then
				break
			end
		end
	end
	return result
end

local function refreshRules()
	local users = astral.settings.values.users or ""
	local words = astral.settings.values.keywords or ""
	if users ~= cachedUsers then
		cachedUsers = users
		hiddenUsers = {}
		for _, id in ipairs(lines(users, 400, 20)) do
			if #id >= 15 and #id <= 20 and id:match("^%d+$") then
				hiddenUsers[id] = true
			end
		end
	end
	if words ~= cachedKeywords then
		cachedKeywords = words
		keywords = lines(words, 64, 128)
	end
end

local function hiddenUser(id)
	return type(id) == "string" and hiddenUsers[id] == true
end

local function hidesContent(content)
	local normalized = content:lower()
	for _, keyword in ipairs(keywords) do
		if normalized:find(keyword:lower(), 1, true) then
			return true
		end
	end
	return false
end

astral.events.on("message.filter", function(message)
	if not message or message.from_me then
		return false
	end
	refreshRules()
	if hiddenUser(message.author_id) or hidesContent(message.content or "") then
		return true
	end
	return astral.settings.values.replies and hiddenUser(message.referenced_author_id) or false
end)

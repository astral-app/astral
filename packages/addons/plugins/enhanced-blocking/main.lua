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

local cached_users
local cached_keywords
local hidden_users = {}
local keywords = {}

local function lines(value, maximum_count, maximum_length)
	local result = {}
	for line in (value .. "\n"):gmatch("(.-)\n") do
		line = line:gsub("\r", ""):match("^%s*(.-)%s*$")
		if line ~= "" and #line <= maximum_length then
			result[#result + 1] = line
			if #result >= maximum_count then
				break
			end
		end
	end
	return result
end

local function refresh_rules()
	local users = astral.settings.values.users or ""
	local words = astral.settings.values.keywords or ""
	if users ~= cached_users then
		cached_users = users
		hidden_users = {}
		for _, id in ipairs(lines(users, 400, 20)) do
			if #id >= 15 and #id <= 20 and id:match("^%d+$") then
				hidden_users[id] = true
			end
		end
	end
	if words ~= cached_keywords then
		cached_keywords = words
		keywords = lines(words, 64, 128)
	end
end

local function hidden_user(id)
	return type(id) == "string" and hidden_users[id] == true
end

local function hides_content(content)
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
	refresh_rules()
	if hidden_user(message.author_id) or hides_content(message.content or "") then
		return true
	end
	return astral.settings.values.replies and hidden_user(message.referenced_author_id) or false
end)

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
settings:switch("replies", "Hide replies", true, "Also hide replies to listed users")
settings:switch("blocked", "Users you blocked", true, "Hide messages from people you blocked in Discord")

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
	if type(id) ~= "string" then
		return false
	end
	return hidden_users[id] == true or (astral.settings.values.blocked and astral.discord.relationship(id) == "blocked")
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

local function referenced_author(message)
	local reference = message and message.reference
	if type(reference) ~= "table" or not reference.channel_id or not reference.message_id then
		return nil
	end
	local referenced = astral.discord.message(reference.channel_id, reference.message_id)
	return referenced and referenced.author and referenced.author.id
end

local function hides_loaded(node)
	if not node.message_id or not node.channel_id then
		return false
	end
	refresh_rules()
	local message = astral.discord.message(node.channel_id, node.message_id)
	local author = node.author_id or (message and message.author and message.author.id)
	if hidden_user(author) or (message and message.content and hides_content(message.content)) then
		return true
	end
	return astral.settings.values.replies and hidden_user(referenced_author(message)) or false
end

astral.ui.patch("message-row", function(node)
	if hides_loaded(node) then
		return { hidden = true }
	end
	return nil
end)

astral.events.on(astral.Event.MessageFilter, function(message)
	if not message or message.from_me then
		return false
	end
	refresh_rules()
	if hidden_user(message.author_id) or hides_content(message.content or "") then
		return true
	end
	return astral.settings.values.replies and hidden_user(message.referenced_author_id) or false
end)

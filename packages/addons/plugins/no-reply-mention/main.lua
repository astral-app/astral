local settings = astral.settings.section("Replies")
settings:switch(
	"only_listed",
	"Only ping listed people",
	false,
	"Keep the ping for the people below and turn it off for everyone else"
)
settings:text("listed", "People to ping", "", { subtitle = "User IDs, one per line", max_length = 4000 })

local function listed()
	local ids = {}
	for id in tostring(astral.settings.get("listed", "") or ""):gmatch("%d+") do
		ids[id] = true
	end
	return ids
end

local function replied_author(message, channel)
	local reference = message.message_reference
	local replied = reference.message_id
		and astral.discord.message(reference.channel_id or channel, reference.message_id)
	return replied and replied.author and replied.author.id or nil
end

local function without_reply_ping(body, channel)
	local message = astral.json.decode(body)
	if type(message) ~= "table" or type(message.message_reference) ~= "table" then
		return nil
	end
	local author = astral.settings.get("only_listed", false) and replied_author(message, channel)
	if astral.settings.get("only_listed", false) and author and listed()[author] then
		return nil
	end
	message.allowed_mentions = type(message.allowed_mentions) == "table" and message.allowed_mentions
		or { parse = { "users", "roles", "everyone" } }
	message.allowed_mentions.replied_user = false
	return astral.json.encode(message)
end

astral.net.intercept({
	method = "POST",
	path = "^/api/v%d+/channels/%d+/messages$",
	request = function(request)
		if not tostring(request.headers["content-type"] or ""):find("application/json", 1, true) then
			return nil
		end
		local body = without_reply_ping(request.body, request.path:match("/channels/(%d+)/"))
		return body and { body = body } or nil
	end,
})

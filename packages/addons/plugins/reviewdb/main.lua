local API_URL = "https://manti.vendicated.dev/api/reviewdb"
local AUTH_REDIRECT_URI = API_URL .. "/auth"
local CLIENT_ID = "915703782174752809"
local PAGE_SIZE = 10
local COMMENT_MAX_LENGTH = 1000
local SYSTEM_REVIEW = 3
local BLURPLE = 0x5865F2

local function encoded(text)
	return (text:gsub("[^%w%-%._~]", function(character)
		return string.format("%%%02X", character:byte())
	end))
end

local function token()
	local value = astral.storage.get("token")
	return type(value) == "string" and value ~= "" and value or nil
end

local function server_message(response, fallback)
	local body = response and response.json
	if type(body) == "table" and type(body.message) == "string" and body.message ~= "" then
		return body.message
	end
	if response and response.status == 429 then
		return "ReviewDB is rate limiting requests; try again in a minute"
	end
	return fallback
end

local function request(path, method, body, callback)
	local headers = { Accept = "application/json" }
	if body then
		headers["Content-Type"] = "application/json"
	end
	local signed_in = token()
	if signed_in then
		headers.Authorization = signed_in
	end
	astral.net.request({
		url = API_URL .. path,
		method = method or "GET",
		headers = headers,
		body = body and astral.json.encode(body) or nil,
		timeout_ms = 15000,
	}, callback)
end

local function name_of(user_id)
	local user = astral.discord.user(user_id)
	return user and (user.global_name or user.username) or user_id
end

local function review_embed(review)
	local sender = type(review.sender) == "table" and review.sender or {}
	local stamp = tonumber(review.timestamp)
	return {
		color = BLURPLE,
		author = {
			name = tostring(sender.username or "Unknown"),
			icon_url = type(sender.profilePhoto) == "string" and sender.profilePhoto or nil,
		},
		description = tostring(review.comment or ""),
		footer = stamp and { text = astral.time.iso(math.floor(stamp * 1000)) } or nil,
	}
end

local function reviews_message(user_id, body)
	local embeds = {}
	for _, review in ipairs(type(body.reviews) == "table" and body.reviews or {}) do
		if review.type ~= SYSTEM_REVIEW and #embeds < PAGE_SIZE then
			embeds[#embeds + 1] = review_embed(review)
		end
	end
	local count = tonumber(body.reviewCount) or #embeds
	local heading =
		string.format("**ReviewDB** · %d review%s of %s", count, count == 1 and "" or "s", name_of(user_id))
	if #embeds == 0 then
		heading = heading .. "\nNo reviews yet. Write one with `/review`."
	elseif count > #embeds then
		heading = heading .. string.format("\nShowing the newest %d.", #embeds)
	end
	return { content = heading, embeds = astral.json.array(embeds) }
end

local function show_reviews(channel_id, user_id)
	request("/users/" .. user_id .. "/reviews?offset=0&limit=" .. PAGE_SIZE * 2, "GET", nil, function(response)
		if not response or not response.ok or type(response.json) ~= "table" then
			astral.messages.send_local(channel_id, server_message(response, "Failed to fetch ReviewDB reviews"))
			return
		end
		astral.messages.send_local(channel_id, reviews_message(user_id, response.json))
	end)
end

local function post_review(channel_id, user_id, comment)
	request("/users/" .. user_id .. "/reviews", "PUT", { userid = user_id, comment = comment }, function(response)
		local posted = response and response.ok
		astral.messages.send_local(
			channel_id,
			server_message(response, posted and "Posted your review" or "Failed to post your review")
		)
		if posted then
			show_reviews(channel_id, user_id)
		end
	end)
end

local function finish_sign_in(channel_id, location)
	astral.net.request({
		url = location .. "&clientMod=enmity",
		headers = { Accept = "application/json" },
		timeout_ms = 15000,
	}, function(response)
		local body = response and response.json
		if response and response.ok and type(body) == "table" and type(body.token) == "string" then
			astral.storage.set("token", body.token)
			astral.messages.send_local(channel_id, "Signed in to ReviewDB")
			return
		end
		astral.messages.send_local(channel_id, server_message(response, "Failed to sign in to ReviewDB"))
	end)
end

local function sign_in(channel_id)
	local url = "https://discord.com/oauth2/authorize?client_id="
		.. CLIENT_ID
		.. "&response_type=code&scope=identify&redirect_uri="
		.. encoded(AUTH_REDIRECT_URI)
	astral.ui.web_auth(url, AUTH_REDIRECT_URI, function(location, _, error)
		if type(location) ~= "string" or location:sub(1, #AUTH_REDIRECT_URI) ~= AUTH_REDIRECT_URI then
			if error then
				astral.messages.send_local(channel_id, "ReviewDB sign-in stopped: " .. tostring(error))
			end
			return
		end
		if location:find("[?&]error=") then
			astral.messages.send_local(channel_id, "ReviewDB sign-in was declined")
			return
		end
		finish_sign_in(channel_id, location)
	end)
end

astral.commands.register({
	name = "reviews",
	description = "Show someone's ReviewDB reviews",
	options = { { name = "user", description = "Whose reviews to show", type = "user", required = true } },
	execute = function(args, context)
		show_reviews(context.channel_id, tostring(args.user))
	end,
})

astral.commands.register({
	name = "review",
	description = "Write or replace your ReviewDB review of someone",
	options = {
		{ name = "user", description = "Who to review", type = "user", required = true },
		{ name = "text", description = "Your review", type = "string", required = true },
	},
	execute = function(args, context)
		if not token() then
			return "Sign in first with `/reviewdb-login`."
		end
		local comment = tostring(args.text or "")
		if comment == "" or #comment > COMMENT_MAX_LENGTH then
			return string.format("A review is 1 to %d characters.", COMMENT_MAX_LENGTH)
		end
		post_review(context.channel_id, tostring(args.user), comment)
	end,
})

astral.commands.register({
	name = "reviewdb-login",
	description = "Sign in to ReviewDB with Discord to write reviews",
	execute = function(_, context)
		sign_in(context.channel_id)
	end,
})

astral.commands.register({
	name = "reviewdb-logout",
	description = "Forget your ReviewDB sign-in on this device",
	execute = function()
		astral.storage.remove("token")
		return "Signed out of ReviewDB."
	end,
})

local function author_of(node)
	if node.user_id then
		return node.user_id
	end
	local message = node:message()
	return message and (message.author_id or (type(message.author) == "table" and message.author.id)) or nil
end

astral.ui.message_action("View Reviews", function(node)
	local author = author_of(node)
	if not author then
		astral.ui.toast("ReviewDB", "This message isn't cached yet")
		return
	end
	show_reviews(node.channel_id, author)
end, { icon = "res://ic_mana_star" })

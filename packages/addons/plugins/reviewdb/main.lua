local API_URL = "https://manti.vendicated.dev/api/reviewdb"
local LINK_COLOR = 0xFF00A8FC
local AUTH_REDIRECT_URI = API_URL .. "/auth"
local CLIENT_ID = "915703782174752809"
local WEBSITE_URL = "https://reviewdb.mantikafasi.dev"
local SUPPORT_SERVER_URL = "https://discord.gg/eWPBSbvznt"
local PAGE_SIZE = 50
local COMMENT_PREVIEW_LENGTH = 200
local SYSTEM_REVIEW = 3
local ADMIN = 1
local MONTHS = { "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec" }

local my_id
local reviewdb_user

local function t(key, variables)
	local text = astral.i18n.t(key)
	for name, value in pairs(variables or {}) do
		text = text:gsub("{" .. name .. "}", function()
			return tostring(value)
		end)
	end
	return text
end

local function plural_category(count, forms)
	if forms.few or forms.many then
		local tens, hundreds = count % 10, count % 100
		if tens == 1 and hundreds ~= 11 then
			return "one"
		elseif tens >= 2 and tens <= 4 and (hundreds < 12 or hundreds > 14) then
			return "few"
		end
		return forms.many and "many" or "other"
	end
	return count == 1 and "one" or "other"
end

local function count_text(count)
	local pattern = astral.i18n.t("REVIEW_COUNT")
	local forms = {}
	for name, form in pattern:gmatch("(%a+) {([^}]*)}") do
		forms[name] = form
	end
	local chosen = forms[plural_category(count, forms)] or forms.other or pattern
	return (chosen:gsub("#", tostring(count)))
end

local function date_text(seconds)
	local days = seconds // 86400
	local era_days = days + 719468
	local era = era_days // 146097
	local day_of_era = era_days - era * 146097
	local year_of_era = (day_of_era - day_of_era // 1460 + day_of_era // 36524 - day_of_era // 146096) // 365
	local day_of_year = day_of_era - (365 * year_of_era + year_of_era // 4 - year_of_era // 100)
	local month_index = (5 * day_of_year + 2) // 153
	local day = day_of_year - (153 * month_index + 2) // 5 + 1
	local month = month_index < 10 and month_index + 3 or month_index - 9
	local year = year_of_era + era * 400 + (month <= 2 and 1 or 0)
	return string.format("%s %d, %d", MONTHS[month], day, year)
end

local function token()
	local value = astral.storage.get("token")
	return type(value) == "string" and value ~= "" and value or nil
end

local function toast(text)
	astral.ui.toast(t("REVIEWDB"), text)
end

local function server_message(response, fallback)
	local body = response and response.json
	if type(body) == "table" and type(body.message) == "string" and body.message ~= "" then
		return body.message
	end
	if response and response.status == 429 then
		return t("RATE_LIMITED")
	end
	if not response or not response.status then
		return t("NETWORK_ERROR")
	end
	return fallback or t("REQUEST_FAILED", { status = response.status })
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
	}, function(response)
		local ok = response and response.ok
		if not ok then
			toast(server_message(response))
		end
		callback(ok and response.json or nil, response)
	end)
end

local function is_admin()
	return reviewdb_user and reviewdb_user.type == ADMIN
end

local function blocked(user_id)
	for _, id in ipairs(reviewdb_user and reviewdb_user.blockedUsers or {}) do
		if id == user_id then
			return true
		end
	end
	return false
end

local function permissions_for(review, profile_id)
	local vote = review.id ~= 0
	if not my_id then
		return { vote = vote }
	end
	local sender = type(review.sender) == "table" and review.sender.discordID
	local is_author = sender == my_id
	local own_profile = profile_id == my_id
	return {
		vote = vote,
		delete = own_profile or is_author or is_admin(),
		report = not is_author,
		block = own_profile and not is_author,
	}
end

local function change_vote(current, is_upvote)
	local step = is_upvote and 1 or -1
	if current == is_upvote then
		return nil, -step
	end
	return is_upvote, current == nil and step or step * 2
end

local function fetch_current_user(callback)
	if not token() then
		if callback then
			callback(nil)
		end
		return
	end
	request("/users", "POST", nil, function(user)
		if type(user) == "table" then
			reviewdb_user = user
			my_id = my_id or user.discordID
		end
		if callback then
			callback(user)
		end
	end)
end

local function notify_new_reviews()
	fetch_current_user(function(user)
		if type(user) ~= "table" or not astral.settings.get("notify_reviews", true) then
			return
		end
		local last = tonumber(astral.storage.get("last_review_id")) or 0
		local latest = tonumber(user.lastReviewID) or 0
		if last > 0 and last < latest then
			toast(t("NEW_REVIEWS"))
		end
		astral.storage.set("last_review_id", latest)
	end)
end

local function review_item(state, review)
	local sender = type(review.sender) == "table" and review.sender or {}
	local comment = tostring(review.comment or "")
	local long = utf8.len(comment) and utf8.len(comment) > COMMENT_PREVIEW_LENGTH
	local expanded = state.expanded[review.id]
	if long and not expanded then
		comment = comment:sub(1, utf8.offset(comment, COMMENT_PREVIEW_LENGTH + 1) - 1) .. "…"
	end
	local author_blocked = blocked(sender.discordID)
	local header = {
		{
			type = astral.SheetItemType.Text,
			text = tostring(sender.username or t("USER")),
			bold = true,
			size = 16,
			lines = 1,
		},
	}
	if author_blocked then
		header[#header + 1] = {
			type = astral.SheetItemType.Text,
			text = t("BLOCKED"),
			bold = true,
			size = 10,
			color = "#FFFFFF",
			background = "#F23F43",
			padding = 4,
			radius = 4,
		}
	end
	if not astral.settings.get("hide_timestamps", false) and tonumber(review.timestamp) then
		header[#header + 1] = {
			type = astral.SheetItemType.Text,
			text = date_text(math.floor(review.timestamp)),
			size = 12,
			color = astral.SheetColor.Muted,
		}
	end
	local body = {
		{ type = astral.SheetItemType.Row, spacing = 6, items = header },
		{ type = astral.SheetItemType.Text, text = comment },
	}
	if long then
		body[#body + 1] = {
			type = astral.SheetItemType.Text,
			text = expanded and t("SHOW_LESS") or t("READ_MORE"),
			bold = true,
			size = 14,
			color = astral.SheetColor.Link,
			action = "expand:" .. review.id,
		}
	end
	if permissions_for(review, state.target).vote then
		local vote = review.userVote
		body[#body + 1] = {
			type = astral.SheetItemType.Row,
			spacing = 6,
			items = {
				{
					type = astral.SheetItemType.Icon,
					name = "thumbs_up",
					size = 14,
					color = vote == true and "positive" or "muted",
					action = "up:" .. review.id,
				},
				{
					type = astral.SheetItemType.Text,
					text = tostring(review.score or 0),
					bold = true,
					size = 14,
					color = vote == true and "positive" or "muted",
					action = "up:" .. review.id,
				},
				{
					type = astral.SheetItemType.Icon,
					name = "thumbs_down",
					size = 14,
					color = vote == false and "danger" or "muted",
					action = "down:" .. review.id,
				},
			},
		}
	end
	return {
		type = astral.SheetItemType.Row,
		align = astral.SheetAlign.Top,
		spacing = 12,
		long_action = "actions:" .. review.id,
		items = {
			{ type = astral.SheetItemType.Image, url = tostring(sender.profilePhoto or ""), size = 40, round = true },
			{ type = astral.SheetItemType.Column, spacing = 4, items = body },
		},
	}
end

local function visible_reviews(state)
	local system, users = {}, {}
	local hide_blocked = astral.settings.get("hide_blocked_users", true)
	for _, review in ipairs(state.reviews) do
		if review.type == SYSTEM_REVIEW then
			if not astral.settings.get("hide_system_reviews", false) then
				system[#system + 1] = review
			end
		elseif not (hide_blocked and type(review.sender) == "table" and blocked(review.sender.discordID)) then
			users[#users + 1] = review
		end
	end
	return system, users
end

local function banned()
	return type(reviewdb_user) == "table" and type(reviewdb_user.banInfo) == "table"
end

local function composer_items(state, users)
	local items = {}
	if state.target == my_id or state.server or banned() then
		return items
	end
	if not token() then
		items[1] = {
			type = astral.SheetItemType.Buttons,
			buttons = { { label = t("SIGN_IN_TO_REVIEW"), action = "sign_in" } },
		}
		return items
	end
	local has_own = false
	for _, review in ipairs(users) do
		has_own = has_own or (type(review.sender) == "table" and review.sender.discordID == my_id)
	end
	items[#items + 1] =
		{ type = astral.SheetItemType.Heading, text = has_own and t("UPDATE_YOUR_REVIEW") or t("LEAVE_A_REVIEW") }
	if has_own then
		items[#items + 1] = {
			type = astral.SheetItemType.Text,
			text = t("UPDATE_REPLACES"),
			size = 13,
			color = astral.SheetColor.Muted,
		}
	end
	items[#items + 1] = {
		type = astral.SheetItemType.Input,
		placeholder = has_own and t("UPDATE_PLACEHOLDER", { name = state.name })
			or t("REVIEW_PLACEHOLDER", { name = state.name }),
		submit_label = has_own and t("UPDATE_REVIEW") or t("SUBMIT_REVIEW"),
		action = "submit",
	}
	return items
end

local function render(state)
	local system, users = visible_reviews(state)
	local items = {}
	for _, review in ipairs(system) do
		items[#items + 1] = {
			type = astral.SheetItemType.Row,
			align = astral.SheetAlign.Top,
			spacing = 10,
			background = "#265865F2",
			items = {
				{ type = astral.SheetItemType.Icon, name = "info", size = 16, color = astral.SheetColor.Link },
				{
					type = astral.SheetItemType.Column,
					spacing = 2,
					items = {
						{
							type = astral.SheetItemType.Text,
							text = tostring(review.sender and review.sender.username or ""),
							bold = true,
							size = 14,
						},
						{ type = astral.SheetItemType.Text, text = tostring(review.comment or ""), size = 14 },
					},
				},
			},
		}
	end
	if not state.loading and #users == 0 then
		items[#items + 1] = { type = astral.SheetItemType.Spacer, size = 12 }
		items[#items + 1] = {
			type = astral.SheetItemType.Text,
			text = t("NO_REVIEWS"),
			bold = true,
			size = 17,
			align = astral.SheetAlign.Center,
		}
		items[#items + 1] = {
			type = astral.SheetItemType.Text,
			text = t("NO_REVIEWS_SUBTITLE"),
			size = 14,
			color = astral.SheetColor.Muted,
			align = astral.SheetAlign.Center,
		}
	end
	for _, review in ipairs(users) do
		items[#items + 1] = review_item(state, review)
	end
	if state.has_next and not state.loading then
		items[#items + 1] =
			{ type = astral.SheetItemType.Buttons, buttons = { { label = t("LOAD_MORE"), action = "load_more" } } }
	end
	local title = state.server and t("SERVER_REVIEWS_FOR", { name = state.name })
		or t("REVIEWS_FOR", { name = state.name })
	return {
		title = title,
		subtitle = count_text(state.count or #users),
		expanded = true,
		header = composer_items(state, users),
		items = items,
	}
end

local function refresh(state)
	if state.sheet then
		state.sheet:update(render(state))
	end
end

local function with_votes(reviews, votes)
	local by_review = {}
	for _, vote in ipairs(votes or {}) do
		by_review[vote.reviewID] = vote.isUpvote
	end
	for _, review in ipairs(reviews) do
		review.userVote = by_review[review.id]
	end
	return reviews
end

local function load(state, offset)
	state.loading = true
	local path = "/users/" .. state.target .. "/reviews?offset=" .. offset .. "&limit=" .. PAGE_SIZE
	astral.net.request(
		{ url = API_URL .. path, headers = { Accept = "application/json" }, timeout_ms = 15000 },
		function(response)
			local body = response and response.ok and response.json
			if type(body) ~= "table" then
				state.loading = false
				toast(response and response.status == 429 and t("RATE_LIMITED") or t("FETCH_REVIEWS_FAILED"))
				refresh(state)
				return
			end
			local function finish(votes)
				local fetched = with_votes(type(body.reviews) == "table" and body.reviews or {}, votes)
				if offset == 0 then
					state.reviews = {}
				end
				for _, review in ipairs(fetched) do
					state.reviews[#state.reviews + 1] = review
				end
				state.count = tonumber(body.reviewCount)
				state.has_next = body.hasNextPage == true
				state.loading = false
				refresh(state)
			end
			if not token() then
				finish({})
				return
			end
			request("/users/" .. state.target .. "/reviews/votes", "GET", nil, function(votes)
				finish(type(votes) == "table" and votes.votes or {})
			end)
		end
	)
end

local function find_review(state, id)
	for _, review in ipairs(state.reviews) do
		if tostring(review.id) == id then
			return review
		end
	end
	return nil
end

local function vote(state, review, is_upvote)
	if type(review.sender) == "table" and review.sender.discordID == my_id then
		toast(t("OWN_REVIEW_VOTE"))
		return
	end
	if not token() then
		toast(t("AUTHORIZE_TO_VOTE"))
		return
	end
	local next_vote, delta = change_vote(review.userVote, is_upvote)
	local method = next_vote == nil and "DELETE" or "POST"
	local body = next_vote ~= nil and { isUpvote = next_vote } or nil
	request("/reviews/" .. review.id .. "/vote", method, body, function(result)
		if result == nil then
			return
		end
		review.userVote = next_vote
		review.score = (tonumber(review.score) or 0) + delta
		refresh(state)
	end)
end

local sign_in

local function delete_review(state, review)
	astral.ui.confirm({
		title = t("DELETE_REVIEW"),
		message = t("DELETE_REVIEW_BODY"),
		confirm = t("DELETE_REVIEW"),
		destructive = true,
	}, function(accepted)
		if not accepted then
			return
		end
		request("/users/" .. review.id .. "/reviews", "DELETE", { reviewid = review.id }, function(result)
			if type(result) == "table" and type(result.message) == "string" and result.message ~= "" then
				toast(result.message)
			end
			load(state, 0)
		end)
	end)
end

local function set_blocked(user_id, block, done)
	request("/blocks", "PATCH", { action = block and "block" or "unblock", discordId = user_id }, function(result)
		if result == nil then
			return
		end
		toast(block and t("USER_BLOCKED") or t("USER_UNBLOCKED"))
		if reviewdb_user then
			local list = {}
			for _, id in ipairs(reviewdb_user.blockedUsers or {}) do
				if id ~= user_id then
					list[#list + 1] = id
				end
			end
			if block then
				list[#list + 1] = user_id
			end
			reviewdb_user.blockedUsers = list
		end
		if done then
			done()
		end
	end)
end

local function review_actions(state, review)
	local allowed = permissions_for(review, state.target)
	local sender = type(review.sender) == "table" and review.sender or {}
	local author_blocked = blocked(sender.discordID)
	local buttons = {}
	if allowed.report then
		buttons[#buttons + 1] = { label = t("REPORT"), action = "report" }
	end
	if allowed.block then
		buttons[#buttons + 1] = { label = author_blocked and t("UNBLOCK_USER") or t("BLOCK_USER"), action = "block" }
	end
	if allowed.delete then
		buttons[#buttons + 1] = { label = t("DELETE_REVIEW"), action = "delete", style = astral.ButtonStyle.Danger }
	end
	if #buttons == 0 then
		return
	end
	local menu
	menu = astral.ui.sheet({
		title = tostring(sender.username or t("USER")),
		items = { { type = astral.SheetItemType.Buttons, direction = astral.Direction.Column, buttons = buttons } },
	}, function(action)
		if action == "closed" then
			return
		end
		menu:close()
		if action == "report" then
			request("/reports", "PUT", { reviewid = review.id }, function(result)
				if type(result) == "table" and type(result.message) == "string" then
					toast(result.message)
				end
			end)
		elseif action == "block" then
			set_blocked(sender.discordID, not author_blocked, function()
				refresh(state)
			end)
		elseif action == "delete" then
			delete_review(state, review)
		end
	end)
end

local function submit(state, comment)
	if not token() then
		toast(t("AUTHORIZE_TO_REVIEW"))
		return
	end
	request(
		"/users/" .. state.target .. "/reviews",
		"PUT",
		{ userid = state.target, comment = comment },
		function(result)
			if type(result) == "table" and type(result.message) == "string" and result.message ~= "" then
				toast(result.message)
			end
			if result ~= nil then
				load(state, 0)
			end
		end
	)
end

local function open_reviews(target, name, server)
	local state = { target = target, name = name, server = server, reviews = {}, expanded = {}, loading = true }
	state.sheet = astral.ui.sheet(render(state), function(action, value)
		if action == "closed" then
			state.sheet = nil
			return
		end
		local kind, id = action:match("^(%a+):(%-?%d+)$")
		local review = id and find_review(state, id)
		if kind == "expand" and review then
			state.expanded[review.id] = not state.expanded[review.id]
			refresh(state)
		elseif (kind == "up" or kind == "down") and review then
			vote(state, review, kind == "up")
		elseif kind == "actions" and review then
			review_actions(state, review)
		elseif action == "load_more" then
			load(state, #state.reviews)
		elseif action == "submit" then
			submit(state, value)
		elseif action == "sign_in" then
			sign_in(function()
				refresh(state)
			end)
		end
	end)
	load(state, 0)
end

local function user_name(user_id)
	local user = astral.discord.user(user_id)
	return user and (user.global_name or user.username) or t("USER")
end

local function register_account_buttons()
	local account = astral.settings.section(t("ACCOUNT"))
	local signed_in = token() ~= nil
	account:button("authorize", signed_in and t("REAUTHORIZE") or t("AUTHORIZE"), function()
		sign_in()
	end, { subtitle = signed_in and t("REAUTHORIZE_SUBTITLE") or t("AUTHORIZE_SUBTITLE") })
	account:button("view_my_reviews", t("VIEW_MY_REVIEWS"), function()
		if my_id then
			open_reviews(my_id, user_name(my_id), false)
		end
	end)
	account:button("manage_blocked_users", t("MANAGE_BLOCKED_USERS"), function()
		if not token() then
			toast(t("AUTHORIZE_TO_REVIEW"))
			return
		end
		request("/blocks", "GET", nil, function(users)
			local items = {}
			for _, user in ipairs(type(users) == "table" and users or {}) do
				items[#items + 1] = {
					type = astral.SheetItemType.Row,
					spacing = 12,
					items = {
						{
							type = astral.SheetItemType.Image,
							url = tostring(user.profilePhoto or ""),
							size = 32,
							round = true,
						},
						{ type = astral.SheetItemType.Text, text = tostring(user.username or ""), bold = true },
						{
							type = astral.SheetItemType.Buttons,
							buttons = { { label = t("UNBLOCK"), action = "unblock:" .. tostring(user.discordID) } },
						},
					},
				}
			end
			if #items == 0 then
				items[1] = {
					type = astral.SheetItemType.Text,
					text = t("NO_BLOCKED_USERS"),
					color = astral.SheetColor.Muted,
					align = astral.SheetAlign.Center,
				}
			end
			local blocked_sheet
			blocked_sheet = astral.ui.sheet({ title = t("BLOCKED_USERS"), items = items }, function(action)
				local user_id = action:match("^unblock:(%d+)$")
				if user_id then
					set_blocked(user_id, false, function()
						blocked_sheet:close()
					end)
				end
			end)
		end)
	end, { subtitle = t("MANAGE_BLOCKED_USERS_SUBTITLE") })
end

local function finish_sign_in(location, done)
	astral.net.request({
		url = location .. "&clientMod=enmity",
		headers = { Accept = "application/json" },
		timeout_ms = 15000,
	}, function(response)
		local body = response and response.json
		if not (response and response.ok) then
			toast(server_message(response, t("LOGIN_FAILED")))
			return
		end
		if type(body) ~= "table" or type(body.token) ~= "string" then
			toast(t("LOGIN_INVALID_PAYLOAD"))
			return
		end
		astral.storage.set("token", body.token)
		toast(t("LOGIN_SUCCESS"))
		register_account_buttons()
		fetch_current_user(function()
			if done then
				done()
			end
		end)
	end)
end

local function encoded(text)
	return (text:gsub("[^%w%-%._~]", function(character)
		return string.format("%%%02X", character:byte())
	end))
end

local function authorize_url()
	return "client_id=" .. CLIENT_ID .. "&response_type=code&scope=identify&redirect_uri=" .. encoded(AUTH_REDIRECT_URI)
end

local function sign_in_in_browser(done)
	local url = "https://discord.com/oauth2/authorize?" .. authorize_url()
	astral.ui.web_auth(url, AUTH_REDIRECT_URI, function(location, _, error)
		if type(location) ~= "string" or location:sub(1, #AUTH_REDIRECT_URI) ~= AUTH_REDIRECT_URI then
			if error and error ~= "cancelled" then
				toast(t("AUTHORIZATION_FAILED"))
			end
			return
		end
		if location:find("[?&]error=") then
			return
		end
		finish_sign_in(location, done)
	end)
end

function sign_in(done)
	astral.ui.confirm({
		title = t("AUTHORIZE"),
		icon = "StarIcon",
		subtitle = "manti.vendicated.dev",
		message = t("AUTHORIZE_CONFIRM"),
		details = {
			{ icon = "UserIcon", text = t("AUTHORIZE_DETAIL_IDENTITY") },
			{ icon = "LinkIcon", text = t("AUTHORIZE_DETAIL_APP") },
			{ icon = "LockIcon", text = t("AUTHORIZE_DETAIL_TOKEN") },
		},
		confirm = t("AUTHORIZE_CONFIRM_BUTTON"),
	}, function(accepted)
		if not accepted then
			return
		end
		astral.discord.request({
			method = "POST",
			path = "/oauth2/authorize?" .. authorize_url(),
			body = { permissions = "0", authorize = true, integration_type = 0 },
		}, function(response)
			local location = response and response.ok and type(response.json) == "table" and response.json.location
			if type(location) == "string" and location:sub(1, #AUTH_REDIRECT_URI) == AUTH_REDIRECT_URI then
				finish_sign_in(location, done)
				return
			end
			sign_in_in_browser(done)
		end)
	end)
end

local preferences = astral.settings.section(t("PREFERENCES"))
preferences:switch("notify_reviews", t("NOTIFY_REVIEWS"), true, { subtitle = t("NOTIFY_REVIEWS_SUBTITLE") })
preferences:switch("hide_timestamps", t("HIDE_TIMESTAMPS"), false)
preferences:switch(
	"hide_system_reviews",
	t("HIDE_SYSTEM_REVIEWS"),
	false,
	{ subtitle = t("HIDE_SYSTEM_REVIEWS_SUBTITLE") }
)
preferences:switch("hide_blocked_users", t("HIDE_BLOCKED_USERS"), true, { subtitle = t("HIDE_BLOCKED_USERS_SUBTITLE") })
register_account_buttons()
local links = astral.settings.section(t("LINKS"))
links:button("website", t("WEBSITE"), function()
	astral.ui.open(WEBSITE_URL)
end)
links:button("support_server", t("SUPPORT_SERVER"), function()
	astral.ui.open(SUPPORT_SERVER_URL)
end)

astral.ui.insert({ id = "Note^>" }, function(node)
	if not node.author_id then
		return nil
	end
	return {
		type = astral.InsertNodeType.View,
		margin = { 0, 0, 0, 16 },
		children = {
			{ type = astral.InsertNodeType.Text, text = t("REVIEWS_TITLE"), size = 16, weight = 500 },
			{
				type = astral.InsertNodeType.View,
				row = true,
				margin = { 0, 8, 0, 0 },
				action = "open_" .. node.author_id,
				children = {
					{
						type = astral.InsertNodeType.Image,
						icon = "res://ic_mana_star",
						width = 16,
						height = 16,
						margin = { 0, 0, 8, 0 },
					},
					{ type = astral.InsertNodeType.Text, text = t("VIEW_REVIEWS"), size = 16, color = LINK_COLOR },
				},
			},
		},
	}
end, function(_, action)
	local user_id = action:match("^open_(%d+)$")
	if user_id then
		open_reviews(user_id, user_name(user_id), false)
	end
end)

astral.ui.menu_action(t("SERVER_REVIEWS"), function(menu)
	open_reviews(menu.guild_id, menu.title, true)
end, {
	icon = "res://ic_mana_star",
	when = function(menu)
		return menu.guild_id ~= nil
	end,
})

astral.events.on(astral.Event.Gateway, function(event)
	if type(event.data) == "table" and type(event.data.user) == "table" then
		my_id = event.data.user.id
	end
	return nil
end, { "READY" })

notify_new_reviews()

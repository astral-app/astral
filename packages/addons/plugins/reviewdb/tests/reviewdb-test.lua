local menu
local buttons, switches, inserter, presser, gateway = {}, {}, nil, nil, nil
local requests, stored, toasts, sheets, auth = {}, {}, {}, {}, nil
local discord_requests = {}
local strings = {
	REVIEWDB = "ReviewDB",
	REVIEW_COUNT = "{count, plural, one {# review} other {# reviews}}",
	REVIEWS_FOR = "Reviews for @{name}",
	REVIEW_PLACEHOLDER = "Review @{name}",
	VIEW_REVIEWS = "View reviews",
	REVIEWS_TITLE = "Reviews",
	SUBMIT_REVIEW = "Submit review",
	LEAVE_A_REVIEW = "Leave a review",
	SIGN_IN_TO_REVIEW = "Sign in to leave a review",
	AUTHORIZE = "Authorize with ReviewDB",
	REAUTHORIZE = "Reauthorize with ReviewDB",
	LOGIN_SUCCESS = "Successfully authorized with ReviewDB.",
}
local function sheet_handle(spec, handler)
	local handle = { spec = spec, handler = handler, closed = false }
	function handle.update(_, next_spec)
		handle.spec = next_spec
	end
	function handle.close()
		handle.closed = true
	end
	sheets[#sheets + 1] = handle
	return handle
end
local Section = {}
Section.__index = Section
function Section.switch(_, key, _, default)
	switches[key] = default
	return default
end
function Section.button(_, key, title, callback)
	buttons[key] = { title = title, callback = callback }
end
astral = {
	log = function() end,
	i18n = {
		t = function(key)
			return strings[key] or key
		end,
	},
	settings = {
		section = function(title)
			return setmetatable({ title = title }, Section)
		end,
		get = function(key, fallback)
			if switches[key] ~= nil then
				return switches[key]
			end
			return fallback
		end,
	},
	storage = {
		get = function(key)
			return stored[key]
		end,
		set = function(key, value)
			stored[key] = value
		end,
	},
	net = {
		request = function(options, callback)
			requests[#requests + 1] = { options = options, callback = callback }
		end,
	},
	json = {
		encode = function(value)
			return "comment=" .. tostring(value.comment) .. ";vote=" .. tostring(value.isUpvote)
		end,
	},
	ui = {
		toast = function(_, text)
			toasts[#toasts + 1] = text
		end,
		sheet = sheet_handle,
		insert = function(selector, build, on_press)
			inserter = { selector = selector, build = build }
			presser = on_press
		end,
		menu_action = function(label, fn, options)
			menu = { label = label, fn = fn, options = options }
		end,
		web_auth = function(url, callback, fn)
			auth = { url = url, callback = callback, fn = fn }
		end,
		open = function() end,
		confirm = function(_, fn)
			if fn then
				fn(true)
			end
		end,
	},
	discord = {
		request = function(options, callback)
			discord_requests[#discord_requests + 1] = { options = options, callback = callback }
		end,
		user = function(id)
			return id == "42" and { username = "friend" } or nil
		end,
	},
	events = {
		on = function(_, fn)
			gateway = fn
		end,
	},
}
dofile(arg[1])

assert(menu.label == "SERVER_REVIEWS" and menu.options.icon == "res://ic_mana_star")
assert(menu.options.when({ title = "Main" }) == false)
assert(menu.options.when({ title = "The Botcave", guild_id = "5" }) == true)
local before = #sheets
menu.fn({ title = "The Botcave", guild_id = "5" })
assert(#sheets == before + 1)

assert(buttons.authorize.title == "Authorize with ReviewDB" and buttons.view_my_reviews and buttons.website)
assert(switches.notify_reviews == true and switches.hide_blocked_users == true and switches.hide_timestamps == false)
gateway({ type = "READY", data = { user = { id = "7" } } })

assert(inserter.selector.id == "Note^>")
assert(inserter.build({}) == nil)
local section = inserter.build({ author_id = "42" })
assert(section.children[1].text == "Reviews" and section.children[2].action == "open_42")

presser({}, "open_42")
local sheet = sheets[#sheets]
assert(sheet.spec.title == "Reviews for @friend")
local load = requests[#requests]
assert(load.options.url:find("/users/42/reviews?offset=0&limit=50", 1, true))
load.callback({
	ok = true,
	status = 200,
	json = {
		reviewCount = 2,
		hasNextPage = false,
		reviews = {
			{ id = 0, type = 3, comment = "Be nice", sender = { username = "Warning" } },
			{
				id = 9,
				type = 0,
				comment = "great",
				score = 2,
				timestamp = 1764028800,
				sender = { username = "pal", discordID = "5" },
			},
		},
	},
})
sheet = sheets[#sheets]
assert(sheet.spec.subtitle == "2 reviews")
local items = sheet.spec.items
assert(items[1].background and items[1].items[2].items[2].text == "Be nice")
local review = items[2]
assert(review.long_action == "actions:9" and review.items[1].round)
assert(review.items[2].items[1].items[2].text == "Nov 25, 2025")
assert(items[#items].buttons[1].label == "Sign in to leave a review")

sheet.handler("up:9", "")
assert(toasts[#toasts] == "AUTHORIZE_TO_VOTE")

sheet.handler("sign_in", "")
local authorize = discord_requests[#discord_requests]
assert(authorize.options.method == "POST" and authorize.options.path:find("/oauth2/authorize?client_id=", 1, true))
authorize.callback({ ok = false, status = 401 })
assert(auth.callback == "https://manti.vendicated.dev/api/reviewdb/auth")
auth.fn("https://manti.vendicated.dev/api/reviewdb/auth?code=abc")
requests[#requests].callback({ ok = true, status = 200, json = { token = "secret" } })
assert(stored.token == "secret" and buttons.authorize.title == "Reauthorize with ReviewDB")
requests[#requests].callback({ ok = true, status = 200, json = { discordID = "7", type = 0, lastReviewID = 3 } })

sheet.handler("up:9", "")
local voted = requests[#requests]
assert(voted.options.url:find("/reviews/9/vote", 1, true) and voted.options.method == "POST")
assert(voted.options.headers.Authorization == "secret")
voted.callback({ ok = true, status = 200, json = {} })
assert(sheets[#sheets].spec.items[2].items[2].items[3].items[2].text == "3")

sheet.handler("submit", "lovely person")
local put = requests[#requests]
assert(put.options.method == "PUT" and put.options.body == "comment=lovely person;vote=nil")
print("reviewdb ok")

local reauthorize_count = #discord_requests
buttons.authorize.callback()
local again = discord_requests[#discord_requests]
assert(#discord_requests == reauthorize_count + 1)
again.callback({
	ok = true,
	status = 200,
	json = { location = "https://manti.vendicated.dev/api/reviewdb/auth?code=fresh" },
})
assert(requests[#requests].options.url == "https://manti.vendicated.dev/api/reviewdb/auth?code=fresh&clientMod=enmity")

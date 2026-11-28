local interceptor, inserter, presser, composer, dispatched = nil, nil, nil, "", nil
local sticker_reply
local delayed = {}
astral = {
	log = function() end,
	net = {
		intercept = function(spec)
			interceptor = spec
		end,
	},
	base64 = {
		decode = function(text)
			return text
		end,
	},
	json = {
		decode = function(text)
			if text == "sticker_ctx" then
				return { location = "sticker_reply" }
			elseif text == "picker" then
				return { content = "", nonce = "5", sticker_ids = { "77" } }
			elseif text == "typed" then
				return { content = "\u{200B}hi", nonce = "6" }
			elseif text == '{"format_type":4}' then
				return { format_type = 4 }
			end
			return nil
		end,
		encode = function(value)
			return "content=" .. tostring(value.content) .. ";stickers=" .. table.concat(value.sticker_ids or {}, ",")
		end,
		array = function()
			return {}
		end,
	},
	discord = {
		request = function(_, fn)
			sticker_reply = fn
		end,
	},
	time = {
		after = function(delay, fn)
			if delay == 0 then
				fn()
			else
				delayed[#delayed + 1] = fn
			end
		end,
	},
	gateway = {
		dispatch = function(kind, data)
			dispatched = { kind = kind, data = data }
		end,
	},
	ui = {
		composer_text = function()
			return composer
		end,
		insert_text = function(text, options)
			composer = (options and options.replace) and text or composer .. text
		end,
		refresh = function() end,
		insert = function(selector, build, press)
			inserter = { selector = selector, build = build }
			presser = press
		end,
	},
}
dofile(arg[1])

assert(inserter.selector.id == "composer.attachments^^^^^^^^")
local answer = interceptor.request({
	id = 1,
	path = "/api/v9/channels/9/messages",
	headers = { ["X-Context-Properties"] = "sticker_ctx" },
	body = "picker",
})
assert(answer.status == 200 and dispatched.data.id == "5")
assert(composer == "")
assert(#delayed == 1)
delayed[1]()
assert(composer == "\u{200B}")
local preview = inserter.build({ channel_id = "9" })
assert(preview.justify == astral.Justify.End and preview.children[1].action == "remove_1")
local frame = preview.children[1]
assert(frame.width == 80 and frame.radius == 12)
local sticker = frame.children[1].children[1]
assert(sticker.width == 76 and sticker.icon:find("/stickers/77.webp", 1, true))
sticker_reply({ ok = true, body = '{"format_type":4}' })
sticker = inserter.build({ channel_id = "9" }).children[1].children[1].children[1]
assert(sticker.animated and sticker.icon:find("/stickers/77.gif", 1, true))

local rewritten = interceptor.request({ id = 2, path = "/api/v9/channels/9/messages", headers = {}, body = "typed" })
assert(rewritten.body == "content=hi;stickers=77")
assert(inserter.build({ channel_id = "9" }) == nil)
composer = ""
interceptor.response({ id = 2 }, { status = 500 })
assert(#inserter.build({ channel_id = "9" }).children == 1)
delayed[#delayed]()
assert(composer == "\u{200B}")

presser({ channel_id = "9" }, "remove_1")
assert(inserter.build({ channel_id = "9" }) == nil and composer == "")
print("sticker-preview ok")

local interceptor, inserted, dispatched
local function encode_message(value)
	return "encoded:" .. tostring(value.id)
end
astral = {
	log = function() end,
	settings = {},
	net = {
		intercept = function(spec)
			interceptor = spec
		end,
	},
	base64 = {
		decode = function(text)
			return text == "gif" and '{"location":"gif_reply"}' or (text and "{}" or nil)
		end,
	},
	json = {
		decode = function(text)
			if text == '{"location":"gif_reply"}' then
				return { location = "gif_reply" }
			elseif text == "{}" then
				return {}
			elseif text == "body" then
				return { content = "https://klipy.com/gifs/a", nonce = "77" }
			end
			return nil
		end,
		encode = encode_message,
	},
	ui = {
		insert_text = function(text)
			inserted = text
		end,
	},
	time = {
		after = function(_, fn)
			fn()
		end,
	},
	gateway = {
		dispatch = function(kind, data)
			dispatched = { kind = kind, data = data }
		end,
	},
}
dofile(arg[1])

assert(interceptor.request({ path = "/api/v9/channels/5/messages", headers = {}, body = "body" }) == nil)
assert(interceptor.request({
	path = "/api/v9/channels/5/messages",
	headers = { ["x-context-properties"] = "x" },
	body = "body",
}) == nil)
local answer = interceptor.request({
	path = "/api/v9/channels/5/messages",
	headers = { ["X-Context-Properties"] = "gif" },
	body = "body",
})
assert(answer.status == 200 and answer.body == "encoded:77")
assert(inserted == "https://klipy.com/gifs/a ")
assert(dispatched.kind == "MESSAGE_DELETE" and dispatched.data.id == "77" and dispatched.data.channel_id == "5")
print("gif-paste ok")

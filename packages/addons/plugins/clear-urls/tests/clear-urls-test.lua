local transform

astral = {
	settings = { switch = function() end, choice = function() end, number = function() end },
	ui = {
		decorate = function() end,
		relabel = function() end,
		style_text = function() end,
		reaction = function() end,
	},
	discord = { permission = {} },
}

dofile(arg[2])
astral.messages = {
	transform = function(callback)
		transform = callback
	end,
}
dofile(arg[1])

assert(transform)
assert(transform("see https://example.com/a?utm_source=x&id=7&fbclid=y") == "see https://example.com/a?id=7")
assert(transform("https://example.com/a?trk=mail&id=7") == "https://example.com/a?id=7")
assert(transform("https://example.com/a?_hsenc=mail&id=7") == "https://example.com/a?id=7")
assert(transform("https://example.com/a?aff_platform=share&id=7") == "https://example.com/a?id=7")
assert(transform("https://www.bing.com/search?cvid=1&q=astral") == "https://www.bing.com/search?q=astral")
assert(transform("https://m.aliexpress.com/item?algo_expid=1&id=7") == "https://m.aliexpress.com/item?id=7")
assert(transform("https://www.google.com/search?gs_lcp=1&q=astral") == "https://www.google.com/search?q=astral")
assert(transform("https://www.amazon.de/dp/1?node=1&th=4") == "https://www.amazon.de/dp/1?th=4")
assert(transform("https://www.amazon.de/dp/1?pd_rd_w=a&th=1") == "https://www.amazon.de/dp/1?th=1")
assert(transform("https://youtu.be/abc?si=123&t=4") == "https://youtu.be/abc?t=4")
assert(transform("https://example.com/?si=123") == nil)
assert(transform("(https://Example.com/Path).") == nil)
assert(transform("no links here") == nil)

print("clear-urls ok")

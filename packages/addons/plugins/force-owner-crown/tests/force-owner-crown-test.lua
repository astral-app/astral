local decorator
astral = {
	log = function() end,
	settings = {},
	i18n = {
		t = function(key)
			return key
		end,
	},
	ui = {
		decorate = function(_, fn)
			decorator = fn
		end,
	},
	discord = {
		permission = {},
		guild = function(id)
			return id == "g" and { id = "g", owner_id = "owner", roles = {} } or nil
		end,
	},
}
dofile(arg[2])
dofile(arg[1])

local crown = decorator({ user_id = "owner", guild_id = "g" })
assert(crown and #crown == 1 and crown[1].icon == "res://rn_ic_crown_16px" and crown[1].tint == 0xFFF0B232)
assert(decorator({ user_id = "member", guild_id = "g" }) == nil)
assert(decorator({ user_id = "owner" }) == nil)
print("force-owner-crown ok")

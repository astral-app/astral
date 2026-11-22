std = "lua54"
max_line_length = 120
globals = { "astral" }
files["plugins/*/tests/*.lua"] = { read_globals = { "arg" } }
exclude_files = { "sdk/types/*.lua", "vendor/**" }
files["packages/core/lua/prelude.lua"] = { globals = { "astral" } }
files["packages/core/tests/*.lua"] = { read_globals = { "arg" } }

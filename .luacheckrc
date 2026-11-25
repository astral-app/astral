std = "lua54"
max_line_length = 120
globals = { "astral" }
files["packages/addons/plugins/*/tests/*.lua"] = { read_globals = { "arg" } }
exclude_files = { "packages/sdk/library/*.lua", "vendor/**" }
files["packages/core/lua/prelude.lua"] = { globals = { "astral" } }
files["packages/core/tests/*.lua"] = { read_globals = { "arg" } }
files["packages/addons/plugins/shiki-codeblocks/lib/devicons.lua"] = { max_line_length = false }

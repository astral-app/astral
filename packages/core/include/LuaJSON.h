#pragma once

#include <nlohmann/json.hpp>

struct lua_State;

namespace astral::lua {
inline constexpr int kMaximumTableDepth = 8;

bool toJSON(lua_State *state, int index, nlohmann::json &out, int depth = 0);
void pushJSON(lua_State *state, const nlohmann::json &value, int depth = 0);
} // namespace astral::lua

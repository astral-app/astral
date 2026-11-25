#pragma once

#include <nlohmann/json.hpp>

#include "AstralRust.h"

struct lua_State;

extern "C" {
AstralRustString astral_lua_to_json(lua_State *state, int index, int depth);
void astral_lua_push_json(lua_State *state, const char *json, size_t length, int depth);
void astral_lua_push_array_metatable(lua_State *state);
}

namespace astral::lua {
inline constexpr int kMaximumTableDepth = 8;

inline bool toJSON(lua_State *state, int index, nlohmann::json &out, int depth = 0)
{
    auto text = rust::takeIfPresent(astral_lua_to_json(state, index, depth));
    if (!text) return false;
    auto parsed = nlohmann::json::parse(*text, nullptr, false);
    if (parsed.is_discarded()) return false;
    out = std::move(parsed);
    return true;
}

inline void pushJSON(lua_State *state, const nlohmann::json &value, int depth = 0)
{
    std::string text = value.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    astral_lua_push_json(state, text.data(), text.size(), depth);
}

inline void pushArrayMetatable(lua_State *state)
{
    astral_lua_push_array_metatable(state);
}
} // namespace astral::lua

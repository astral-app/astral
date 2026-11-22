#include "LuaJSON.h"

extern "C" {
#include "lua.h"
}

namespace astral::lua {
bool toJSON(lua_State *state, int index, nlohmann::json &out, int depth)
{
    index = lua_absindex(state, index);
    switch (lua_type(state, index)) {
    case LUA_TNIL:
        out = nullptr;
        return true;
    case LUA_TBOOLEAN:
        out = (bool)lua_toboolean(state, index);
        return true;
    case LUA_TNUMBER:
        if (lua_isinteger(state, index))
            out = (int64_t)lua_tointeger(state, index);
        else
            out = lua_tonumber(state, index);
        return true;
    case LUA_TSTRING: {
        size_t length = 0;
        const char *text = lua_tolstring(state, index, &length);
        out = std::string(text, length);
        return true;
    }
    case LUA_TTABLE: {
        if (depth > kMaximumTableDepth) return false;
        lua_Integer length = (lua_Integer)lua_rawlen(state, index);
        bool array = length > 0;
        lua_pushnil(state);
        while (array && lua_next(state, index)) {
            if (!lua_isinteger(state, -2) || lua_tointeger(state, -2) < 1 || lua_tointeger(state, -2) > length) array = false;
            lua_pop(state, 1);
            if (!array) lua_pop(state, 1);
        }
        if (array) {
            out = nlohmann::json::array();
            for (lua_Integer position = 1; position <= length; position++) {
                lua_rawgeti(state, index, position);
                nlohmann::json item;
                bool converted = toJSON(state, -1, item, depth + 1);
                lua_pop(state, 1);
                if (!converted) return false;
                out.push_back(item);
            }
            return true;
        }
        out = nlohmann::json::object();
        lua_pushnil(state);
        while (lua_next(state, index)) {
            if (lua_type(state, -2) != LUA_TSTRING) {
                lua_pop(state, 2);
                return false;
            }
            nlohmann::json item;
            if (!toJSON(state, -1, item, depth + 1)) {
                lua_pop(state, 2);
                return false;
            }
            out[lua_tostring(state, -2)] = item;
            lua_pop(state, 1);
        }
        return true;
    }
    default:
        return false;
    }
}

void pushJSON(lua_State *state, const nlohmann::json &value, int depth)
{
    if (depth > kMaximumTableDepth || value.is_null()) {
        lua_pushnil(state);
    } else if (value.is_boolean()) {
        lua_pushboolean(state, value.get<bool>());
    } else if (value.is_number_integer()) {
        lua_pushinteger(state, value.get<int64_t>());
    } else if (value.is_number()) {
        lua_pushnumber(state, value.get<double>());
    } else if (value.is_string()) {
        const std::string &text = value.get_ref<const std::string &>();
        lua_pushlstring(state, text.data(), text.size());
    } else if (value.is_array()) {
        lua_createtable(state, (int)value.size(), 0);
        lua_Integer position = 1;
        for (const auto &item : value) {
            pushJSON(state, item, depth + 1);
            lua_rawseti(state, -2, position++);
        }
    } else {
        lua_createtable(state, 0, (int)value.size());
        for (auto entry = value.begin(); entry != value.end(); ++entry) {
            pushJSON(state, entry.value(), depth + 1);
            lua_setfield(state, -2, entry.key().c_str());
        }
    }
}
} // namespace astral::lua

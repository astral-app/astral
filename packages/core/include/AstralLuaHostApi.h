#pragma once

#include <cstddef>

#include "AstralRust.h"

struct lua_State;

extern "C" {
typedef struct {
    int (*hasPermission)(void *plugin, const char *name, size_t length);
    AstralRustString (*identifier)(void *plugin);
} AstralLuaPluginCalls;

void astral_lua_api_configure(AstralLuaPluginCalls calls, const char *storagePath, size_t length);
void astral_lua_storage_remove_all(const char *identifier, size_t length);
int astral_lua_json_decode(lua_State *state);
int astral_lua_json_encode(lua_State *state);
int astral_lua_json_array(lua_State *state);
int astral_lua_time_now_ms(lua_State *state);
int astral_lua_time_iso(lua_State *state);
int astral_lua_storage_get(lua_State *state);
int astral_lua_storage_set(lua_State *state);
int astral_lua_storage_remove(lua_State *state);
int astral_lua_storage_keys(lua_State *state);
}

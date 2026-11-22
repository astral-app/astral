#include "LuaSandbox.h"

#include <cstdlib>

extern "C" {
#include "lauxlib.h"
#include "lua.h"
#include "lualib.h"
}

namespace astral::lua {
namespace {
void openSafeLibraries(lua_State *state)
{
    static const luaL_Reg libraries[] = {
        {LUA_GNAME, luaopen_base},       {LUA_TABLIBNAME, luaopen_table}, {LUA_STRLIBNAME, luaopen_string},
        {LUA_MATHLIBNAME, luaopen_math}, {LUA_UTF8LIBNAME, luaopen_utf8}, {LUA_COLIBNAME, luaopen_coroutine},
    };
    for (const auto &library : libraries) {
        luaL_requiref(state, library.name, library.func, 1);
        lua_pop(state, 1);
    }
    for (const char *name : {"dofile", "loadfile", "load", "require", "collectgarbage"}) {
        lua_pushnil(state);
        lua_setglobal(state, name);
    }
}
} // namespace

Sandbox::Sandbox(SandboxLimits limits) : limits(limits)
{
    luaState = lua_newstate(allocate, this);
    if (luaState) openSafeLibraries(luaState);
}

Sandbox::~Sandbox()
{
    if (luaState) lua_close(luaState);
}

void *Sandbox::allocate(void *user, void *pointer, size_t old, size_t size)
{
    Sandbox *sandbox = static_cast<Sandbox *>(user);
    size_t previous = pointer ? old : 0;
    if (size == 0) {
        sandbox->used -= previous;
        free(pointer);
        return nullptr;
    }
    if (sandbox->used - previous + size > sandbox->limits.memory) return nullptr;
    void *grown = realloc(pointer, size);
    if (grown) sandbox->used = sandbox->used - previous + size;
    return grown;
}

void Sandbox::spendBudget(lua_State *state, lua_Debug *)
{
    Sandbox *sandbox = nullptr;
    lua_getallocf(state, reinterpret_cast<void **>(&sandbox));
    if (sandbox && --sandbox->remaining <= 0) luaL_error(state, "astral: plugin exceeded its instruction budget");
}

std::string Sandbox::popError()
{
    const char *message = lua_tostring(luaState, -1);
    std::string error = message ? message : "unknown error";
    lua_pop(luaState, 1);
    return error;
}

std::optional<std::string> Sandbox::load(const std::string &source, const std::string &chunk)
{
    if (!luaState) return "out of memory";
    if (luaL_loadbufferx(luaState, source.data(), source.size(), ("=" + chunk).c_str(), "t") != LUA_OK) return popError();
    return std::nullopt;
}

std::optional<std::string> Sandbox::call(int arguments, int results)
{
    remaining = limits.budgetSlices;
    lua_sethook(luaState, spendBudget, LUA_MASKCOUNT, limits.sliceInstructions);
    int status = lua_pcall(luaState, arguments, results, 0);
    lua_sethook(luaState, nullptr, 0, 0);
    if (status == LUA_OK) return std::nullopt;
    return popError();
}

std::optional<std::string> Sandbox::run(const std::string &source, const std::string &chunk)
{
    if (auto error = load(source, chunk)) return error;
    return call(0, 0);
}
} // namespace astral::lua

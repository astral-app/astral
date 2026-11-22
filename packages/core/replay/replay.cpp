#include <cstdio>
#include <cstring>
#include <fstream>
#include <iterator>
#include <memory>
#include <sstream>
#include <string>
#include <vector>

#include "LuaSandbox.h"
#include "PluginPackage.h"

extern "C" {
#include "PhoenixWire.h"
#include "lauxlib.h"
#include "lua.h"
}

namespace {
struct Plugin {
    astral::PluginPackage package;
    std::unique_ptr<astral::lua::Sandbox> sandbox;
    std::vector<int> decorators;
    std::vector<std::pair<std::string, int>> relabelers;
};

std::vector<Plugin *> gPlugins;
unsigned long gDecorations, gRelabels, gErrors;

Plugin *Self(lua_State *state)
{
    return static_cast<Plugin *>(lua_touserdata(state, lua_upvalueindex(1)));
}

std::string ReadFile(const std::string &path)
{
    std::ifstream file(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>());
}

int Log(lua_State *state)
{
    std::string line;
    for (int index = 1; index <= lua_gettop(state); index++) {
        if (index > 1) line += " ";
        line += luaL_tolstring(state, index, nullptr);
        lua_pop(state, 1);
    }
    std::printf("log %s: %s\n", Self(state)->package.identifier.c_str(), line.c_str());
    return 0;
}

int SettingDefault(lua_State *state)
{
    lua_pushvalue(state, 3);
    return 1;
}

int SettingChoice(lua_State *state)
{
    lua_pushvalue(state, 4);
    return 1;
}

int SettingNumber(lua_State *state)
{
    lua_getfield(state, 3, "default");
    if (lua_isnil(state, -1)) {
        lua_pop(state, 1);
        lua_getfield(state, 3, "min");
    }
    return 1;
}

int Nothing(lua_State *)
{
    return 0;
}

int Translate(lua_State *state)
{
    std::string key = luaL_checkstring(state, 1);
    lua_pushstring(state, Self(state)->package.translate(key, {}).c_str());
    return 1;
}

int Decorate(lua_State *state)
{
    luaL_checktype(state, 2, LUA_TFUNCTION);
    lua_pushvalue(state, 2);
    Self(state)->decorators.push_back(luaL_ref(state, LUA_REGISTRYINDEX));
    return 0;
}

int Relabel(lua_State *state)
{
    std::string target = luaL_checkstring(state, 1);
    luaL_checktype(state, 2, LUA_TFUNCTION);
    lua_pushvalue(state, 2);
    Self(state)->relabelers.push_back({target, luaL_ref(state, LUA_REGISTRYINDEX)});
    return 0;
}

int Require(lua_State *state)
{
    Plugin *plugin = Self(state);
    std::string name = luaL_checkstring(state, 1);
    lua_getfield(state, LUA_REGISTRYINDEX, "astral.loaded");
    lua_getfield(state, -1, name.c_str());
    if (!lua_isnil(state, -1)) return 1;
    lua_pop(state, 1);
    auto module = plugin->package.modules.find(name);
    if (module == plugin->package.modules.end()) return luaL_error(state, "module '%s' not found", name.c_str());
    std::string chunk = "=" + plugin->package.identifier + "/" + name;
    if (luaL_loadbufferx(state, module->second.data(), module->second.size(), chunk.c_str(), "t") != LUA_OK) return lua_error(state);
    lua_pushstring(state, name.c_str());
    lua_call(state, 1, 1);
    if (lua_isnil(state, -1)) {
        lua_pop(state, 1);
        lua_pushboolean(state, 1);
    }
    lua_pushvalue(state, -1);
    lua_setfield(state, -3, name.c_str());
    return 1;
}

void Function(lua_State *state, Plugin *plugin, const char *name, lua_CFunction function)
{
    lua_pushlightuserdata(state, plugin);
    lua_pushcclosure(state, function, 1);
    lua_setfield(state, -2, name);
}

void Install(Plugin *plugin)
{
    lua_State *state = plugin->sandbox->state();
    lua_newtable(state);
    lua_setfield(state, LUA_REGISTRYINDEX, "astral.loaded");
    lua_pushlightuserdata(state, plugin);
    lua_pushcclosure(state, Require, 1);
    lua_setglobal(state, "require");
    lua_newtable(state);
    lua_pushstring(state, plugin->package.identifier.c_str());
    lua_setfield(state, -2, "id");
    Function(state, plugin, "log", Log);
    lua_newtable(state);
    Function(state, plugin, "switch", SettingDefault);
    Function(state, plugin, "choice", SettingChoice);
    Function(state, plugin, "number", SettingNumber);
    Function(state, plugin, "get", SettingDefault);
    Function(state, plugin, "set", Nothing);
    lua_setfield(state, -2, "settings");
    lua_newtable(state);
    Function(state, plugin, "t", Translate);
    lua_pushstring(state, plugin->package.locale.c_str());
    lua_setfield(state, -2, "locale");
    lua_setfield(state, -2, "i18n");
    lua_newtable(state);
    Function(state, plugin, "decorate", Decorate);
    Function(state, plugin, "relabel", Relabel);
    Function(state, plugin, "settings_backdrop", Nothing);
    lua_setfield(state, -2, "ui");
    lua_newtable(state);
    for (const char *name : {"get", "set", "remove", "keys"}) Function(state, plugin, name, Nothing);
    lua_setfield(state, -2, "storage");
    lua_newtable(state);
    for (const char *name : {"guild", "member", "channel_guild", "permissions", "message", "user", "nickname"})
        Function(state, plugin, name, Nothing);
    lua_newtable(state);
    const std::pair<const char *, int> permissions[] = {
        {"KICK_MEMBERS", 1},     {"BAN_MEMBERS", 2},     {"ADMINISTRATOR", 3},     {"MANAGE_CHANNELS", 4}, {"MANAGE_GUILD", 5},
        {"MANAGE_MESSAGES", 13}, {"MUTE_MEMBERS", 22},   {"DEAFEN_MEMBERS", 23},   {"MOVE_MEMBERS", 24},   {"MANAGE_ROLES", 28},
        {"MANAGE_WEBHOOKS", 29}, {"MANAGE_THREADS", 34}, {"MODERATE_MEMBERS", 40},
    };
    for (auto &permission : permissions) {
        lua_pushinteger(state, (lua_Integer)(1ll << permission.second));
        lua_setfield(state, -2, permission.first);
    }
    lua_setfield(state, -2, "permission");
    lua_setfield(state, -2, "discord");
    lua_setglobal(state, "astral");
}

bool Report(Plugin *plugin, const char *stage, const std::optional<std::string> &error)
{
    if (!error) return true;
    gErrors++;
    std::printf("error %s %s: %s\n", plugin->package.identifier.c_str(), stage, error->c_str());
    return false;
}

std::string Snowflake(uint64_t value)
{
    return value ? std::to_string(value) : std::string();
}

void SetField(lua_State *state, const char *name, const std::string &value)
{
    if (value.empty()) return;
    lua_pushstring(state, value.c_str());
    lua_setfield(state, -2, name);
}

size_t ReplayDecorate(const AstralDecorationContext *context, AstralTag *tags, size_t capacity)
{
    size_t count = 0;
    for (Plugin *plugin : gPlugins) {
        lua_State *state = plugin->sandbox->state();
        for (int decorator : plugin->decorators) {
            lua_rawgeti(state, LUA_REGISTRYINDEX, decorator);
            lua_createtable(state, 0, 3);
            SetField(state, "message_id", Snowflake(context->message_id));
            SetField(state, "user_id", Snowflake(context->user_id));
            SetField(state, "channel_id", Snowflake(context->channel_id));
            if (!Report(plugin, "decorate", plugin->sandbox->call(1, 1))) continue;
            gDecorations++;
            std::string texts;
            if (lua_istable(state, -1)) {
                lua_Integer length = luaL_len(state, -1);
                for (lua_Integer index = 1; index <= length && count < capacity; index++) {
                    lua_rawgeti(state, -1, index);
                    lua_getfield(state, -1, "text");
                    if (const char *text = lua_tostring(state, -1)) {
                        std::snprintf(tags[count].text, sizeof(tags[count].text), "%s", text);
                        tags[count].background = 0xFF5865F2;
                        tags[count].foreground = 0xFFFFFFFF;
                        count++;
                        texts += texts.empty() ? text : std::string(", ") + text;
                    }
                    lua_pop(state, 2);
                }
            }
            lua_pop(state, 1);
            std::printf("decorate %s message=%s user=%s -> [%s]\n", plugin->package.identifier.c_str(),
                        Snowflake(context->message_id).c_str(), Snowflake(context->user_id).c_str(), texts.c_str());
        }
    }
    return count;
}

const char *LabelTarget(AstralLabelKind kind)
{
    switch (kind) {
    case kAstralLabelChannel:
        return "channel.name";
    case kAstralLabelMember:
        return "member.name";
    case kAstralLabelComposer:
        return "composer.placeholder";
    case kAstralLabelProfile:
        return "profile.name";
    default:
        return "message.author";
    }
}

int ReplayRelabel(const AstralLabelContext *context, const char *text, size_t length, char *out, size_t capacity)
{
    std::string current(text, length), wanted = LabelTarget(context->kind);
    bool changed = false;
    for (Plugin *plugin : gPlugins) {
        lua_State *state = plugin->sandbox->state();
        for (auto &relabeler : plugin->relabelers) {
            if (relabeler.first != wanted) continue;
            lua_rawgeti(state, LUA_REGISTRYINDEX, relabeler.second);
            lua_createtable(state, 0, 4);
            SetField(state, "text", current);
            SetField(state, "message_id", Snowflake(context->message_id));
            SetField(state, "channel_id", Snowflake(context->channel_id));
            if (!Report(plugin, "relabel", plugin->sandbox->call(1, 1))) continue;
            gRelabels++;
            size_t size = 0;
            const char *result = lua_type(state, -1) == LUA_TSTRING ? lua_tolstring(state, -1, &size) : nullptr;
            if (result && size > 0 && size < capacity) {
                std::printf("relabel %s %s: \"%s\" -> \"%.*s\"\n", plugin->package.identifier.c_str(), wanted.c_str(), current.c_str(),
                            (int)size, result);
                current.assign(result, size);
                changed = true;
            }
            lua_pop(state, 1);
        }
    }
    if (!changed) return -1;
    return std::snprintf(out, capacity, "%s", current.c_str());
}

bool Load(Plugin *plugin, const std::string &directory, const std::string &prelude)
{
    auto package = astral::loadPluginPackage(directory, {"en-US"});
    if (!package) {
        std::printf("error %s: not a plugin package\n", directory.c_str());
        gErrors++;
        return false;
    }
    plugin->package = *package;
    plugin->sandbox = std::make_unique<astral::lua::Sandbox>();
    Install(plugin);
    if (!Report(plugin, "prelude", plugin->sandbox->run(prelude, "astral"))) return false;
    return Report(plugin, "main", plugin->sandbox->run(plugin->package.source, plugin->package.identifier + "/" + plugin->package.main));
}
} // namespace

int main(int argc, char **argv)
{
    if (argc < 4) {
        std::fprintf(stderr, "usage: replay <prelude.lua> <plugin folder>... -- <batch.bin>...\n");
        return 2;
    }
    std::string prelude = ReadFile(argv[1]);
    int index = 2;
    std::vector<std::unique_ptr<Plugin>> plugins;
    for (; index < argc && std::strcmp(argv[index], "--") != 0; index++) {
        auto plugin = std::make_unique<Plugin>();
        if (Load(plugin.get(), argv[index], prelude)) gPlugins.push_back(plugin.get());
        plugins.push_back(std::move(plugin));
    }
    astral_wire_set_decorator(ReplayDecorate);
    astral_wire_set_relabeler(ReplayRelabel);
    unsigned long batches = 0, rewritten = 0;
    for (index++; index < argc; index++) {
        std::string batch = ReadFile(argv[index]);
        if (batch.empty()) continue;
        batches++;
        AstralWireBuffer out;
        if (astral_settings_rewrite(reinterpret_cast<const uint8_t *>(batch.data()), batch.size(), &out)) {
            rewritten++;
            astral_wire_buffer_free(&out);
        }
    }
    std::printf("replayed %lu batches (%lu rewritten): %lu decorations, %lu relabels, %lu errors\n", batches, rewritten, gDecorations,
                gRelabels, gErrors);
    return gErrors ? 1 : 0;
}

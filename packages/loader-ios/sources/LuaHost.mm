#import "LuaHost.h"

#import "TomlDocument.h"

#include <map>
#include <mutex>
#include <set>
#include <sqlite3.h>
#include <string>
#include <vector>

#include "LoaderLog.h"
#include "LoaderSettings.h"
#import "AstralState.h"
#import "CrashReports.h"
#import "DevServer.h"
#import "DiscordCache.h"
#import "PluginSignature.h"

extern "C" {
#include "lauxlib.h"
#include "lua.h"
#include "lualib.h"
}

#include "AstralPrelude.h"

static const size_t kMemoryLimit = 8u << 20;
static const size_t kSourceLimit = 256u << 10;
static const size_t kModulesLimit = 1u << 20;
static const size_t kStorageLimit = 512u << 10;
static const int kBudgetSlices = 40;
static const int kSliceInstructions = 5000;
static const int kMaxErrors = 5;

struct LuaOption {
    std::string value;
    std::string title;
    std::string subtitle;
};

struct LuaSwitch {
    std::string key;
    std::string title;
    std::string subtitle;
    std::string section;
    bool fallback;
    std::vector<LuaOption> options;
    std::string fallbackChoice;
};

struct LuaPlugin {
    std::string identifier;
    std::string name;
    std::string version;
    std::string description;
    std::string icon;
    std::string iconSvg;
    std::vector<std::pair<std::string, std::string>> authors;
    std::string directory;
    bool bundled = false;
    bool signedByAstral = false;
    std::string source;
    lua_State *state = nullptr;
    size_t memory = 0;
    int budget = 0;
    int errors = 0;
    bool enabled = false;
    bool failed = false;
    std::string lastError;
    std::vector<LuaSwitch> switches;
    std::vector<std::pair<std::string, int>> decorators;
    std::map<std::string, std::string> strings;
    std::map<std::string, std::string> fallbackStrings;
    std::string locale;
    std::vector<std::pair<std::string, int>> relabelers;
    std::map<std::string, std::string> modules;
    std::set<std::string> permissions;
};

static int Translate(lua_State *state);

static std::recursive_mutex gMutex;
static std::vector<LuaPlugin *> gPlugins;
static loader::SettingsStore *gStore;
static NSArray<NSString *> *gDirectories;
static bool gStartEnabled;

static NSString *Native(const std::string &value)
{
    return [NSString stringWithUTF8String:value.c_str()] ?: @"";
}

static std::string SettingKey(const LuaPlugin *plugin, const std::string &key)
{
    return plugin->identifier + ".setting." + key;
}

static std::string StoredString(const std::string &key, const std::string &fallback)
{
    auto value = gStore->get("plugins", key);
    return value && value->is_string() ? value->get<std::string>() : fallback;
}

static bool StoredBool(const std::string &key, bool fallback)
{
    auto value = gStore->get("plugins", key);
    return value && value->is_boolean() ? value->get<bool>() : fallback;
}

static void Store(const std::string &key, const nlohmann::json &value)
{
    gStore->set("plugins", key, value);
    gStore->save();
}

static void *Allocate(void *user, void *pointer, size_t old, size_t size)
{
    LuaPlugin *plugin = static_cast<LuaPlugin *>(user);
    size_t previous = pointer ? old : 0;
    if (size == 0) {
        plugin->memory -= previous;
        free(pointer);
        return nullptr;
    }
    if (plugin->memory - previous + size > kMemoryLimit) return nullptr;
    void *grown = realloc(pointer, size);
    if (grown) plugin->memory = plugin->memory - previous + size;
    return grown;
}

static LuaPlugin *Self(lua_State *state)
{
    return static_cast<LuaPlugin *>(lua_touserdata(state, lua_upvalueindex(1)));
}

static void Budget(lua_State *state, lua_Debug *)
{
    LuaPlugin *plugin = nullptr;
    lua_getallocf(state, reinterpret_cast<void **>(&plugin));
    if (plugin && --plugin->budget <= 0) luaL_error(state, "astral: plugin exceeded its instruction budget");
}

static void MarkFailed(LuaPlugin *plugin)
{
    if (plugin->failed) return;
    plugin->failed = true;
    AstralRecordPluginFailure(Native(plugin->identifier), Native(plugin->name), Native(plugin->lastError));
}

static void Report(LuaPlugin *plugin, const char *stage)
{
    const char *message = lua_tostring(plugin->state, -1);
    plugin->lastError = std::string(stage) + ": " + (message ?: "unknown error");
    lua_pop(plugin->state, 1);
    loader::log(loader::LogLevel::Error, "plugin", plugin->identifier + " " + plugin->lastError);
    AstralDevAppendLog(Native(plugin->identifier + " error: " + plugin->lastError));
    if (++plugin->errors >= kMaxErrors) MarkFailed(plugin);
}

static bool Call(LuaPlugin *plugin, int arguments, int results, const char *stage)
{
    plugin->budget = kBudgetSlices;
    lua_sethook(plugin->state, Budget, LUA_MASKCOUNT, kSliceInstructions);
    int status = lua_pcall(plugin->state, arguments, results, 0);
    lua_sethook(plugin->state, nullptr, 0, 0);
    if (status == LUA_OK) return true;
    Report(plugin, stage);
    return false;
}

static int Log(lua_State *state)
{
    std::string line;
    for (int index = 1; index <= lua_gettop(state); index++) {
        if (index > 1) line += " ";
        line += luaL_tolstring(state, index, nullptr);
        lua_pop(state, 1);
    }
    loader::log(loader::LogLevel::Info, "plugin", Self(state)->identifier + ": " + line);
    AstralDevAppendLog(Native(Self(state)->identifier + ": " + line));
    return 0;
}

static int SettingsSwitch(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    std::string key = luaL_checkstring(state, 1);
    std::string title = luaL_checkstring(state, 2);
    bool fallback = lua_toboolean(state, 3);
    std::string subtitle, section;
    if (lua_istable(state, 4)) {
        lua_getfield(state, 4, "subtitle");
        lua_getfield(state, 4, "section");
        if (lua_isstring(state, -2)) subtitle = lua_tostring(state, -2);
        if (lua_isstring(state, -1)) section = lua_tostring(state, -1);
        lua_pop(state, 2);
    }
    if (key.empty() || key.size() > 48 || plugin->switches.size() >= 32) return luaL_error(state, "astral.settings.switch: invalid key");
    bool known = false;
    for (auto &entry : plugin->switches) known = known || entry.key == key;
    if (!known) plugin->switches.push_back({key, title.substr(0, 64), subtitle.substr(0, 120), section.substr(0, 48), fallback});
    lua_pushboolean(state, StoredBool(SettingKey(plugin, key), fallback));
    return 1;
}

static std::string FieldString(lua_State *state, int index, const char *name)
{
    lua_getfield(state, index, name);
    std::string value = lua_isstring(state, -1) ? lua_tostring(state, -1) : "";
    lua_pop(state, 1);
    return value;
}

static int SettingsChoice(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    std::string key = luaL_checkstring(state, 1);
    std::string title = luaL_checkstring(state, 2);
    luaL_checktype(state, 3, LUA_TTABLE);
    std::string fallback = luaL_checkstring(state, 4);
    LuaSwitch entry{key, title.substr(0, 64), "", "", false, {}, fallback};
    if (lua_istable(state, 5)) {
        entry.subtitle = FieldString(state, 5, "subtitle").substr(0, 120);
        entry.section = FieldString(state, 5, "section").substr(0, 48);
    }
    lua_Integer count = luaL_len(state, 3);
    for (lua_Integer index = 1; index <= count && entry.options.size() < 8; index++) {
        lua_rawgeti(state, 3, index);
        if (lua_istable(state, -1)) {
            LuaOption option{FieldString(state, -1, "value"), FieldString(state, -1, "title"), FieldString(state, -1, "subtitle")};
            if (!option.value.empty()) entry.options.push_back(option);
        }
        lua_pop(state, 1);
    }
    if (key.empty() || key.size() > 48 || entry.options.empty() || plugin->switches.size() >= 32) {
        return luaL_error(state, "astral.settings.choice: invalid choice");
    }
    bool known = false;
    for (auto &existing : plugin->switches) known = known || existing.key == key;
    if (!known) plugin->switches.push_back(entry);
    lua_pushstring(state, StoredString(SettingKey(plugin, key), fallback).c_str());
    return 1;
}

static int SettingsGet(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    std::string key = luaL_checkstring(state, 1);
    auto value = gStore->get("plugins", SettingKey(plugin, key));
    if (!value) {
        lua_pushvalue(state, 2);
    } else if (value->is_boolean()) {
        lua_pushboolean(state, value->get<bool>());
    } else if (value->is_number_integer()) {
        lua_pushinteger(state, value->get<lua_Integer>());
    } else if (value->is_number()) {
        lua_pushnumber(state, value->get<double>());
    } else if (value->is_string()) {
        lua_pushstring(state, value->get<std::string>().c_str());
    } else {
        lua_pushvalue(state, 2);
    }
    return 1;
}

static int SettingsSet(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    std::string key = SettingKey(plugin, luaL_checkstring(state, 1));
    switch (lua_type(state, 2)) {
    case LUA_TBOOLEAN:
        Store(key, (bool)lua_toboolean(state, 2));
        break;
    case LUA_TNUMBER:
        if (lua_isinteger(state, 2))
            Store(key, (int64_t)lua_tointeger(state, 2));
        else
            Store(key, lua_tonumber(state, 2));
        break;
    case LUA_TSTRING:
        Store(key, std::string(lua_tostring(state, 2)));
        break;
    default:
        return luaL_error(state, "astral.settings.set: unsupported value");
    }
    return 0;
}

static int Decorate(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    std::string prefix = luaL_checkstring(state, 1);
    luaL_checktype(state, 2, LUA_TFUNCTION);
    if (prefix != "message.author") return luaL_error(state, "astral.ui.decorate: unsupported target '%s'", prefix.c_str());
    lua_pushvalue(state, 2);
    plugin->decorators.push_back({prefix, luaL_ref(state, LUA_REGISTRYINDEX)});
    return 0;
}

static int Relabel(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    std::string target = luaL_checkstring(state, 1);
    luaL_checktype(state, 2, LUA_TFUNCTION);
    if (target != "message.author" && target != "channel.name" && target != "member.name" && target != "composer.placeholder") {
        return luaL_error(state, "astral.ui.relabel: unsupported target '%s'", target.c_str());
    }
    lua_pushvalue(state, 2);
    plugin->relabelers.push_back({target, luaL_ref(state, LUA_REGISTRYINDEX)});
    return 0;
}

static void PushObject(lua_State *state, id value, int depth)
{
    if (depth > 4 || !value || value == NSNull.null) {
        lua_pushnil(state);
    } else if ([value isKindOfClass:NSString.class]) {
        lua_pushstring(state, [value UTF8String]);
    } else if ([value isKindOfClass:NSNumber.class]) {
        if (strcmp([value objCType], @encode(BOOL)) == 0 || strcmp([value objCType], "c") == 0)
            lua_pushboolean(state, [value boolValue]);
        else if (CFNumberIsFloatType((__bridge CFNumberRef)value))
            lua_pushnumber(state, [value doubleValue]);
        else
            lua_pushinteger(state, [value longLongValue]);
    } else if ([value isKindOfClass:NSArray.class]) {
        lua_createtable(state, (int)[value count], 0);
        lua_Integer index = 1;
        for (id item in value) {
            PushObject(state, item, depth + 1);
            lua_rawseti(state, -2, index++);
        }
    } else if ([value isKindOfClass:NSDictionary.class]) {
        lua_createtable(state, 0, (int)[value count]);
        for (id key in value) {
            if (![key isKindOfClass:NSString.class]) continue;
            PushObject(state, value[key], depth + 1);
            lua_setfield(state, -2, [key UTF8String]);
        }
    } else {
        lua_pushnil(state);
    }
}

static void RequirePermission(lua_State *state, const char *permission)
{
    LuaPlugin *plugin = Self(state);
    if (!plugin->permissions.count(permission)) luaL_error(state, "permission '%s' is not declared in manifest.toml", permission);
}

static int Require(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    std::string name = luaL_checkstring(state, 1);
    lua_getfield(state, LUA_REGISTRYINDEX, "astral.loaded");
    lua_getfield(state, -1, name.c_str());
    if (!lua_isnil(state, -1)) return 1;
    lua_pop(state, 1);
    auto module = plugin->modules.find(name);
    if (module == plugin->modules.end()) return luaL_error(state, "module '%s' not found in %s", name.c_str(), plugin->identifier.c_str());
    std::string chunk = "=" + plugin->identifier + "/" + name;
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

static void InstallRequire(LuaPlugin *plugin)
{
    lua_newtable(plugin->state);
    lua_setfield(plugin->state, LUA_REGISTRYINDEX, "astral.loaded");
    lua_pushlightuserdata(plugin->state, plugin);
    lua_pushcclosure(plugin->state, Require, 1);
    lua_setglobal(plugin->state, "require");
}

static bool LuaToJSON(lua_State *state, int index, nlohmann::json &out, int depth)
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
        if (depth > 8) return false;
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
                bool converted = LuaToJSON(state, -1, item, depth + 1);
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
            if (!LuaToJSON(state, -1, item, depth + 1)) {
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

static void PushJSON(lua_State *state, const nlohmann::json &value, int depth)
{
    if (depth > 8 || value.is_null()) {
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
            PushJSON(state, item, depth + 1);
            lua_rawseti(state, -2, position++);
        }
    } else {
        lua_createtable(state, 0, (int)value.size());
        for (auto entry = value.begin(); entry != value.end(); ++entry) {
            PushJSON(state, entry.value(), depth + 1);
            lua_setfield(state, -2, entry.key().c_str());
        }
    }
}

static sqlite3 *StorageDatabase(void)
{
    static sqlite3 *database;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        NSString *path = [AstralStateDirectory() stringByAppendingPathComponent:@"plugin-storage.sqlite"];
        if (sqlite3_open_v2(path.fileSystemRepresentation, &database, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX,
                            NULL) != SQLITE_OK) {
            sqlite3_close(database);
            database = NULL;
            return;
        }
        sqlite3_exec(
            database,
            "PRAGMA journal_mode=WAL;"
            "CREATE TABLE IF NOT EXISTS storage (plugin TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY (plugin, key))",
            NULL, NULL, NULL);
    });
    return database;
}

static sqlite3_stmt *Prepare(lua_State *state, const char *sql)
{
    sqlite3 *database = StorageDatabase();
    sqlite3_stmt *statement = NULL;
    if (!database || sqlite3_prepare_v2(database, sql, -1, &statement, NULL) != SQLITE_OK)
        luaL_error(state, "astral.storage is unavailable");
    return statement;
}

static int StorageGet(lua_State *state)
{
    RequirePermission(state, "storage");
    LuaPlugin *plugin = Self(state);
    const char *key = luaL_checkstring(state, 1);
    sqlite3_stmt *statement = Prepare(state, "SELECT value FROM storage WHERE plugin = ? AND key = ?");
    sqlite3_bind_text(statement, 1, plugin->identifier.c_str(), -1, SQLITE_TRANSIENT);
    sqlite3_bind_text(statement, 2, key, -1, SQLITE_TRANSIENT);
    std::string text;
    bool found = sqlite3_step(statement) == SQLITE_ROW;
    if (found) text = reinterpret_cast<const char *>(sqlite3_column_text(statement, 0));
    sqlite3_finalize(statement);
    nlohmann::json value = found ? nlohmann::json::parse(text, nullptr, false) : nlohmann::json();
    if (!found || value.is_discarded())
        lua_pushvalue(state, 2);
    else
        PushJSON(state, value, 0);
    return 1;
}

static size_t StoredBytes(lua_State *state, LuaPlugin *plugin, const char *except)
{
    sqlite3_stmt *statement =
        Prepare(state, "SELECT COALESCE(SUM(LENGTH(key) + LENGTH(value)), 0) FROM storage WHERE plugin = ? AND key != ?");
    sqlite3_bind_text(statement, 1, plugin->identifier.c_str(), -1, SQLITE_TRANSIENT);
    sqlite3_bind_text(statement, 2, except, -1, SQLITE_TRANSIENT);
    size_t total = sqlite3_step(statement) == SQLITE_ROW ? (size_t)sqlite3_column_int64(statement, 0) : 0;
    sqlite3_finalize(statement);
    return total;
}

static void DeleteKey(lua_State *state, LuaPlugin *plugin, const char *key)
{
    sqlite3_stmt *statement = Prepare(state, "DELETE FROM storage WHERE plugin = ? AND key = ?");
    sqlite3_bind_text(statement, 1, plugin->identifier.c_str(), -1, SQLITE_TRANSIENT);
    sqlite3_bind_text(statement, 2, key, -1, SQLITE_TRANSIENT);
    sqlite3_step(statement);
    sqlite3_finalize(statement);
}

static int StorageSet(lua_State *state)
{
    RequirePermission(state, "storage");
    LuaPlugin *plugin = Self(state);
    const char *key = luaL_checkstring(state, 1);
    nlohmann::json value;
    if (!LuaToJSON(state, 2, value, 0)) return luaL_error(state, "astral.storage.set: '%s' holds a value that cannot be stored", key);
    if (value.is_null()) {
        DeleteKey(state, plugin, key);
        return 0;
    }
    std::string text = value.dump();
    if (StoredBytes(state, plugin, key) + strlen(key) + text.size() > kStorageLimit)
        return luaL_error(state, "astral.storage: %s would exceed %d KB", plugin->identifier.c_str(), (int)(kStorageLimit >> 10));
    sqlite3_stmt *statement = Prepare(
        state, "INSERT INTO storage (plugin, key, value) VALUES (?, ?, ?) ON CONFLICT (plugin, key) DO UPDATE SET value = excluded.value");
    sqlite3_bind_text(statement, 1, plugin->identifier.c_str(), -1, SQLITE_TRANSIENT);
    sqlite3_bind_text(statement, 2, key, -1, SQLITE_TRANSIENT);
    sqlite3_bind_text(statement, 3, text.c_str(), (int)text.size(), SQLITE_TRANSIENT);
    int result = sqlite3_step(statement);
    sqlite3_finalize(statement);
    if (result != SQLITE_DONE) return luaL_error(state, "astral.storage.set: could not save '%s'", key);
    return 0;
}

static int StorageRemove(lua_State *state)
{
    RequirePermission(state, "storage");
    DeleteKey(state, Self(state), luaL_checkstring(state, 1));
    return 0;
}

static int StorageKeys(lua_State *state)
{
    RequirePermission(state, "storage");
    LuaPlugin *plugin = Self(state);
    sqlite3_stmt *statement = Prepare(state, "SELECT key FROM storage WHERE plugin = ? ORDER BY key");
    sqlite3_bind_text(statement, 1, plugin->identifier.c_str(), -1, SQLITE_TRANSIENT);
    lua_newtable(state);
    lua_Integer position = 1;
    while (sqlite3_step(statement) == SQLITE_ROW) {
        lua_pushstring(state, reinterpret_cast<const char *>(sqlite3_column_text(statement, 0)));
        lua_rawseti(state, -2, position++);
    }
    sqlite3_finalize(statement);
    return 1;
}

void AstralLuaRemoveStorage(NSString *identifier)
{
    sqlite3 *database = StorageDatabase();
    sqlite3_stmt *statement = NULL;
    if (!database || sqlite3_prepare_v2(database, "DELETE FROM storage WHERE plugin = ?", -1, &statement, NULL) != SQLITE_OK) return;
    sqlite3_bind_text(statement, 1, identifier.UTF8String, -1, SQLITE_TRANSIENT);
    sqlite3_step(statement);
    sqlite3_finalize(statement);
}

static NSString *Argument(lua_State *state, int index)
{
    return [NSString stringWithUTF8String:luaL_checkstring(state, index)];
}

static int Guild(lua_State *state)
{
    RequirePermission(state, "discord.guilds");
    PushObject(state, AstralDiscordGuild(Argument(state, 1)), 0);
    return 1;
}

static int Member(lua_State *state)
{
    RequirePermission(state, "discord.members");
    PushObject(state, AstralDiscordMember(Argument(state, 1), Argument(state, 2)), 0);
    return 1;
}

static int ChannelGuild(lua_State *state)
{
    RequirePermission(state, "discord.guilds");
    PushObject(state, AstralDiscordChannelGuild(Argument(state, 1)), 0);
    return 1;
}

static NSString *OptionalArgument(lua_State *state, int index)
{
    const char *value = lua_tostring(state, index);
    return value ? [NSString stringWithUTF8String:value] : nil;
}

static int User(lua_State *state)
{
    RequirePermission(state, "discord.users");
    PushObject(state, AstralDiscordUser(Argument(state, 1)), 0);
    return 1;
}

static int Nickname(lua_State *state)
{
    RequirePermission(state, "discord.relationships");
    PushObject(state, AstralDiscordFriendNickname(Argument(state, 1)), 0);
    return 1;
}

static int Message(lua_State *state)
{
    RequirePermission(state, "discord.messages");
    PushObject(state, AstralDiscordMessage(Argument(state, 1), Argument(state, 2)), 0);
    return 1;
}

static int Permissions(lua_State *state)
{
    RequirePermission(state, "discord.permissions");
    NSNumber *permissions = AstralDiscordPermissions(Argument(state, 1), Argument(state, 2), OptionalArgument(state, 3));
    if (permissions)
        lua_pushinteger(state, (lua_Integer)permissions.unsignedLongLongValue);
    else
        lua_pushnil(state);
    return 1;
}

static void Function(lua_State *state, LuaPlugin *plugin, const char *name, lua_CFunction function)
{
    lua_pushlightuserdata(state, plugin);
    lua_pushcclosure(state, function, 1);
    lua_setfield(state, -2, name);
}

static void Constant(lua_State *state, const char *name, lua_Integer value)
{
    lua_pushinteger(state, value);
    lua_setfield(state, -2, name);
}

static void InstallAPI(LuaPlugin *plugin)
{
    lua_State *state = plugin->state;
    lua_newtable(state);
    Function(state, plugin, "log", Log);
    lua_pushstring(state, plugin->identifier.c_str());
    lua_setfield(state, -2, "id");
    lua_pushstring(state, "1");
    lua_setfield(state, -2, "api");

    lua_newtable(state);
    Function(state, plugin, "switch", SettingsSwitch);
    Function(state, plugin, "choice", SettingsChoice);
    Function(state, plugin, "get", SettingsGet);
    Function(state, plugin, "set", SettingsSet);
    lua_setfield(state, -2, "settings");

    lua_newtable(state);
    Function(state, plugin, "t", Translate);
    lua_pushstring(state, plugin->locale.c_str());
    lua_setfield(state, -2, "locale");
    lua_setfield(state, -2, "i18n");

    lua_newtable(state);
    Function(state, plugin, "decorate", Decorate);
    Function(state, plugin, "relabel", Relabel);
    lua_setfield(state, -2, "ui");

    lua_newtable(state);
    Function(state, plugin, "get", StorageGet);
    Function(state, plugin, "set", StorageSet);
    Function(state, plugin, "remove", StorageRemove);
    Function(state, plugin, "keys", StorageKeys);
    lua_setfield(state, -2, "storage");

    lua_newtable(state);
    Function(state, plugin, "guild", Guild);
    Function(state, plugin, "member", Member);
    Function(state, plugin, "channel_guild", ChannelGuild);
    Function(state, plugin, "permissions", Permissions);
    Function(state, plugin, "message", Message);
    Function(state, plugin, "user", User);
    Function(state, plugin, "nickname", Nickname);
    lua_newtable(state);
    Constant(state, "KICK_MEMBERS", 1ll << 1);
    Constant(state, "BAN_MEMBERS", 1ll << 2);
    Constant(state, "ADMINISTRATOR", 1ll << 3);
    Constant(state, "MANAGE_CHANNELS", 1ll << 4);
    Constant(state, "MANAGE_GUILD", 1ll << 5);
    Constant(state, "MANAGE_MESSAGES", 1ll << 13);
    Constant(state, "MUTE_MEMBERS", 1ll << 22);
    Constant(state, "DEAFEN_MEMBERS", 1ll << 23);
    Constant(state, "MOVE_MEMBERS", 1ll << 24);
    Constant(state, "MANAGE_ROLES", 1ll << 28);
    Constant(state, "MANAGE_WEBHOOKS", 1ll << 29);
    Constant(state, "MANAGE_THREADS", 1ll << 34);
    Constant(state, "MODERATE_MEMBERS", 1ll << 40);
    lua_setfield(state, -2, "permission");
    lua_setfield(state, -2, "discord");

    lua_setglobal(state, "astral");
}

static void OpenSafeLibraries(lua_State *state)
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

static bool RunPrelude(LuaPlugin *plugin)
{
    if (luaL_loadbufferx(plugin->state, kAstralPrelude, sizeof(kAstralPrelude) - 1, "=astral", "t") != LUA_OK) {
        Report(plugin, "prelude");
        return false;
    }
    return Call(plugin, 0, 0, "prelude");
}

static void Stop(LuaPlugin *plugin)
{
    if (plugin->state) lua_close(plugin->state);
    plugin->state = nullptr;
    plugin->memory = 0;
    plugin->switches.clear();
    plugin->decorators.clear();
    plugin->relabelers.clear();
}

static bool Runnable(LuaPlugin *plugin)
{
    return plugin->bundled || plugin->signedByAstral || gStore->getBoolean("astral", "developer-mode", false);
}

static bool Start(LuaPlugin *plugin)
{
    if (!Runnable(plugin)) {
        Stop(plugin);
        plugin->failed = true;
        plugin->lastError = "unsigned; turn on developer mode in astral's settings to run it";
        return false;
    }
    Stop(plugin);
    plugin->errors = 0;
    plugin->failed = false;
    plugin->lastError.clear();
    plugin->state = lua_newstate(Allocate, plugin);
    if (!plugin->state) return false;
    OpenSafeLibraries(plugin->state);
    InstallAPI(plugin);
    InstallRequire(plugin);
    if (!RunPrelude(plugin)) {
        MarkFailed(plugin);
        Stop(plugin);
        return false;
    }
    if (luaL_loadbufferx(plugin->state, plugin->source.data(), plugin->source.size(), ("=" + plugin->identifier).c_str(), "t") != LUA_OK) {
        Report(plugin, "load");
        MarkFailed(plugin);
        Stop(plugin);
        return false;
    }
    if (!Call(plugin, 0, 0, "start")) {
        MarkFailed(plugin);
        Stop(plugin);
        return false;
    }
    return true;
}

static bool ValidIdentifier(NSString *identifier)
{
    if (identifier.length < 3 || identifier.length > 80 || [identifier containsString:@".."]) return false;
    NSCharacterSet *allowed = [NSCharacterSet characterSetWithCharactersInString:@"abcdefghijklmnopqrstuvwxyz0123456789.-"];
    return [identifier rangeOfCharacterFromSet:allowed.invertedSet].location == NSNotFound;
}

static std::map<std::string, std::string> LoadStrings(NSString *path)
{
    std::map<std::string, std::string> strings;
    NSDictionary *table = AstralParseTOML([NSData dataWithContentsOfFile:path]);
    for (NSString *key in table) {
        if ([table[key] isKindOfClass:NSString.class]) strings[key.UTF8String] = [table[key] UTF8String];
    }
    return strings;
}

static NSString *BestLocale(NSArray<NSString *> *available)
{
    NSMutableArray<NSString *> *wanted = [NSMutableArray array];
    for (NSString *language in NSLocale.preferredLanguages) {
        [wanted addObject:language];
        NSString *base = [language componentsSeparatedByString:@"-"].firstObject;
        if (base) [wanted addObject:base];
    }
    [wanted addObjectsFromArray:@[ @"en-US", @"en-GB", @"en" ]];
    for (NSString *candidate in wanted) {
        for (NSString *locale in available) {
            if ([locale caseInsensitiveCompare:candidate] == NSOrderedSame) return locale;
        }
        for (NSString *locale in available) {
            if ([[locale componentsSeparatedByString:@"-"].firstObject caseInsensitiveCompare:candidate] == NSOrderedSame) return locale;
        }
    }
    return nil;
}

static void LoadTranslations(LuaPlugin *plugin, NSString *directory)
{
    NSString *folder = [directory stringByAppendingPathComponent:@"i18n"];
    NSMutableArray<NSString *> *available = [NSMutableArray array];
    for (NSString *entry in [NSFileManager.defaultManager contentsOfDirectoryAtPath:folder error:nil]) {
        if ([entry.pathExtension isEqualToString:@"toml"]) [available addObject:entry.stringByDeletingPathExtension];
    }
    NSString *locale = BestLocale(available);
    NSString *english = BestLocale([available filteredArrayUsingPredicate:[NSPredicate predicateWithFormat:@"SELF BEGINSWITH 'en'"]]);
    if (locale) {
        plugin->locale = locale.UTF8String;
        plugin->strings = LoadStrings([folder stringByAppendingPathComponent:[locale stringByAppendingPathExtension:@"toml"]]);
    }
    if (english)
        plugin->fallbackStrings = LoadStrings([folder stringByAppendingPathComponent:[english stringByAppendingPathExtension:@"toml"]]);
}

static int Translate(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    std::string key = luaL_checkstring(state, 1);
    auto found = plugin->strings.find(key);
    std::string text = found != plugin->strings.end() ? found->second : key;
    if (found == plugin->strings.end()) {
        auto fallback = plugin->fallbackStrings.find(key);
        if (fallback != plugin->fallbackStrings.end()) text = fallback->second;
    }
    if (lua_istable(state, 2)) {
        lua_pushnil(state);
        while (lua_next(state, 2)) {
            if (lua_type(state, -2) == LUA_TSTRING) {
                std::string token = std::string("{") + lua_tostring(state, -2) + "}";
                std::string value = luaL_tolstring(state, -1, nullptr);
                lua_pop(state, 1);
                for (size_t at = text.find(token); at != std::string::npos; at = text.find(token, at + value.size()))
                    text.replace(at, token.size(), value);
            }
            lua_pop(state, 1);
        }
    }
    lua_pushstring(state, text.c_str());
    return 1;
}

static bool LoadModules(LuaPlugin *plugin, NSString *directory, NSString *main)
{
    size_t total = 0;
    NSDirectoryEnumerator *walker = [NSFileManager.defaultManager enumeratorAtPath:directory];
    for (NSString *path in walker) {
        if (walker.level > 4 || [path hasPrefix:@"tests/"] || [path isEqualToString:@"tests"]) {
            if ([path isEqualToString:@"tests"]) [walker skipDescendants];
            continue;
        }
        if (![path.pathExtension isEqualToString:@"lua"] || [path isEqualToString:main]) continue;
        NSData *data = [NSData dataWithContentsOfFile:[directory stringByAppendingPathComponent:path]];
        if (!data) return false;
        total += data.length;
        if (total > kModulesLimit) return false;
        NSString *name = [path.stringByDeletingPathExtension stringByReplacingOccurrencesOfString:@"/" withString:@"."];
        plugin->modules[name.UTF8String] = std::string(static_cast<const char *>(data.bytes), data.length);
    }
    return true;
}

static LuaPlugin *Load(NSString *directory)
{
    NSData *manifestData = [NSData dataWithContentsOfFile:[directory stringByAppendingPathComponent:@"manifest.toml"]];
    NSDictionary *manifest = AstralParseTOML(manifestData);
    if (![manifest isKindOfClass:NSDictionary.class]) return nullptr;
    NSString *identifier = manifest[@"id"], *name = manifest[@"name"], *main = manifest[@"main"] ?: @"main.lua";
    if (![identifier isKindOfClass:NSString.class] || !ValidIdentifier(identifier) || ![name isKindOfClass:NSString.class] ||
        ![main isKindOfClass:NSString.class] || [main containsString:@"/"])
        return nullptr;
    NSData *source = [NSData dataWithContentsOfFile:[directory stringByAppendingPathComponent:main]];
    if (!source || source.length > kSourceLimit) return nullptr;
    LuaPlugin *plugin = new LuaPlugin();
    plugin->signedByAstral = AstralVerifyPluginDirectory(directory, main);
    LoadTranslations(plugin, directory);
    plugin->identifier = identifier.UTF8String;
    plugin->name = [name substringToIndex:MIN(name.length, (NSUInteger)48)].UTF8String;
    plugin->version = [manifest[@"version"] isKindOfClass:NSString.class] ? [manifest[@"version"] UTF8String] : "";
    plugin->description = [manifest[@"description"] isKindOfClass:NSString.class] ? [manifest[@"description"] UTF8String] : "";
    plugin->icon = [manifest[@"icon"] isKindOfClass:NSString.class] ? [manifest[@"icon"] UTF8String] : "";
    plugin->iconSvg = [manifest[@"iconSvg"] isKindOfClass:NSString.class] && [manifest[@"iconSvg"] length] < 16384
                          ? [manifest[@"iconSvg"] UTF8String]
                          : "";
    for (id author in [manifest[@"authors"] isKindOfClass:NSArray.class] ? manifest[@"authors"] : @[]) {
        NSString *authorName = [author isKindOfClass:NSDictionary.class] ? author[@"name"] : author;
        NSString *authorID =
            [author isKindOfClass:NSDictionary.class] && [author[@"id"] isKindOfClass:NSString.class] ? author[@"id"] : @"";
        if ([authorName isKindOfClass:NSString.class] && plugin->authors.size() < 12) {
            plugin->authors.push_back({authorName.UTF8String, authorID.UTF8String});
        }
    }
    plugin->directory = directory.UTF8String;
    plugin->source.assign(static_cast<const char *>(source.bytes), source.length);
    for (id permission in [manifest[@"permissions"] isKindOfClass:NSArray.class] ? manifest[@"permissions"] : @[]) {
        if ([permission isKindOfClass:NSString.class]) plugin->permissions.insert([permission UTF8String]);
    }
    if (!LoadModules(plugin, directory, main)) {
        delete plugin;
        return nullptr;
    }
    return plugin;
}

static void Scan(void)
{
    for (NSString *root in gDirectories) {
        bool bundled = [root hasPrefix:AstralResourceDirectory()];
        for (NSString *entry in
             [[NSFileManager.defaultManager contentsOfDirectoryAtPath:root error:nil] sortedArrayUsingSelector:@selector(compare:)]) {
            LuaPlugin *plugin = Load([root stringByAppendingPathComponent:entry]);
            if (!plugin) continue;
            bool duplicate = false;
            for (auto *existing : gPlugins) duplicate = duplicate || existing->identifier == plugin->identifier;
            if (duplicate) {
                delete plugin;
                continue;
            }
            plugin->bundled = bundled;
            gPlugins.push_back(plugin);
            plugin->enabled = StoredBool(plugin->identifier + ".enabled", !bundled);
            if (plugin->enabled && gStartEnabled) Start(plugin);
        }
    }
}

void AstralLuaHostStart(NSArray<NSString *> *directories, loader::SettingsStore *store, bool startEnabled)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (gStore) return;
    gStore = store;
    gDirectories = [directories copy];
    gStartEnabled = startEnabled;
    Scan();
}

void AstralLuaRescan(void)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (gStore) Scan();
}

NSArray<NSDictionary *> *AstralLuaPlugins(void)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    NSMutableArray *plugins = [NSMutableArray array];
    for (auto *plugin : gPlugins) {
        NSMutableArray *switches = [NSMutableArray array];
        for (auto &entry : plugin->switches) {
            NSMutableArray *options = [NSMutableArray array];
            for (auto &option : entry.options) {
                [options
                    addObject:@{@"value" : Native(option.value), @"title" : Native(option.title), @"subtitle" : Native(option.subtitle)}];
            }
            [switches addObject:@{
                @"key" : Native(entry.key),
                @"title" : Native(entry.title),
                @"subtitle" : Native(entry.subtitle),
                @"section" : Native(entry.section),
                @"on" : @(StoredBool(SettingKey(plugin, entry.key), entry.fallback)),
                @"options" : options,
                @"value" : Native(StoredString(SettingKey(plugin, entry.key), entry.fallbackChoice))
            }];
        }
        NSMutableArray *authors = [NSMutableArray array];
        for (auto &author : plugin->authors) [authors addObject:@{@"name" : Native(author.first), @"id" : Native(author.second)}];
        NSMutableArray *permissions = [NSMutableArray array];
        for (auto &permission : plugin->permissions) [permissions addObject:Native(permission)];
        [plugins addObject:@{
            @"permissions" : permissions,
            @"id" : Native(plugin->identifier),
            @"name" : Native(plugin->name),
            @"version" : Native(plugin->version),
            @"enabled" : @(plugin->enabled),
            @"running" : @(plugin->state != nullptr),
            @"failed" : @(plugin->failed),
            @"error" : Native(plugin->lastError),
            @"switches" : switches,
            @"description" : Native(plugin->description),
            @"icon" : Native(plugin->icon),
            @"iconSvg" : Native(plugin->iconSvg),
            @"authors" : authors,
            @"directory" : Native(plugin->directory),
            @"bundled" : @(plugin->bundled),
            @"signed" : @(plugin->bundled || plugin->signedByAstral)
        }];
    }
    return plugins;
}

static LuaPlugin *Find(NSString *pluginID)
{
    for (auto *plugin : gPlugins)
        if (Native(plugin->identifier) && [Native(plugin->identifier) isEqualToString:pluginID]) return plugin;
    return nullptr;
}

void AstralLuaSetEnabled(NSString *pluginID, BOOL enabled)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    LuaPlugin *plugin = Find(pluginID);
    if (!plugin) return;
    plugin->enabled = enabled;
    Store(plugin->identifier + ".enabled", (bool)enabled);
    if (enabled)
        Start(plugin);
    else
        Stop(plugin);
}

void AstralLuaResetSettings(NSString *pluginID)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    LuaPlugin *plugin = Find(pluginID);
    if (!plugin) return;
    for (auto &entry : plugin->switches) gStore->remove("plugins", SettingKey(plugin, entry.key));
    gStore->save();
    if (plugin->state) Start(plugin);
}

NSString *AstralLuaEval(NSString *pluginID, NSString *code)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    LuaPlugin *plugin = Find(pluginID);
    if (!plugin) return @"error: no such plugin";
    if (!plugin->state) return @"error: the plugin is not running";
    lua_State *state = plugin->state;
    int top = lua_gettop(state);
    std::string source = code.UTF8String ?: "";
    std::string expression = "return " + source;
    if (luaL_loadbufferx(state, expression.data(), expression.size(), "=eval", "t") != LUA_OK) {
        lua_pop(state, 1);
        if (luaL_loadbufferx(state, source.data(), source.size(), "=eval", "t") != LUA_OK) {
            NSString *message = Native(lua_tostring(state, -1) ?: "syntax error");
            lua_settop(state, top);
            return [@"error: " stringByAppendingString:message];
        }
    }
    plugin->budget = kBudgetSlices;
    lua_sethook(state, Budget, LUA_MASKCOUNT, kSliceInstructions);
    int status = lua_pcall(state, 0, LUA_MULTRET, 0);
    lua_sethook(state, nullptr, 0, 0);
    NSMutableArray *parts = [NSMutableArray array];
    for (int index = top + 1; index <= lua_gettop(state); index++) {
        [parts addObject:Native(luaL_tolstring(state, index, nullptr))];
        lua_pop(state, 1);
    }
    lua_settop(state, top);
    NSString *joined = [parts componentsJoinedByString:@"\t"];
    return status == LUA_OK ? joined : [@"error: " stringByAppendingString:joined];
}

void AstralLuaRemove(NSString *pluginID)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto iterator = gPlugins.begin(); iterator != gPlugins.end(); ++iterator) {
        if (![Native((*iterator)->identifier) isEqualToString:pluginID]) continue;
        Stop(*iterator);
        delete *iterator;
        gPlugins.erase(iterator);
        return;
    }
}

void AstralLuaSetChoice(NSString *pluginID, NSString *key, NSString *value)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    LuaPlugin *plugin = Find(pluginID);
    if (!plugin) return;
    Store(SettingKey(plugin, key.UTF8String), std::string(value.UTF8String));
    if (!plugin->state) return;
    lua_getglobal(plugin->state, "astral");
    lua_getfield(plugin->state, -1, "on_setting");
    lua_remove(plugin->state, -2);
    if (lua_isfunction(plugin->state, -1)) {
        lua_pushstring(plugin->state, key.UTF8String);
        lua_pushstring(plugin->state, value.UTF8String);
        Call(plugin, 2, 0, "on_setting");
    } else {
        lua_pop(plugin->state, 1);
    }
}

void AstralLuaSetSwitch(NSString *pluginID, NSString *key, BOOL on)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    LuaPlugin *plugin = Find(pluginID);
    if (!plugin) return;
    Store(SettingKey(plugin, key.UTF8String), (bool)on);
    if (!plugin->state) return;
    lua_getglobal(plugin->state, "astral");
    lua_getfield(plugin->state, -1, "on_setting");
    lua_remove(plugin->state, -2);
    if (lua_isfunction(plugin->state, -1)) {
        lua_pushstring(plugin->state, key.UTF8String);
        lua_pushboolean(plugin->state, on);
        Call(plugin, 2, 0, "on_setting");
    } else {
        lua_pop(plugin->state, 1);
    }
}

static std::string Snowflake(uint64_t value)
{
    return value ? std::to_string(value) : std::string();
}

static void CollectTags(LuaPlugin *plugin, AstralTag *tags, size_t capacity, size_t *count)
{
    lua_State *state = plugin->state;
    if (!lua_istable(state, -1)) {
        lua_pop(state, 1);
        return;
    }
    lua_Integer length = luaL_len(state, -1);
    for (lua_Integer index = 1; index <= length && *count < capacity; index++) {
        lua_rawgeti(state, -1, index);
        if (lua_istable(state, -1)) {
            lua_getfield(state, -1, "text");
            lua_getfield(state, -2, "color");
            lua_getfield(state, -3, "text_color");
            lua_getfield(state, -4, "colors");
            lua_getfield(state, -5, "shape");
            const char *text = lua_tostring(state, -5);
            if (text && *text) {
                AstralTag *tag = &tags[(*count)++];
                memset(tag, 0, sizeof(*tag));
                snprintf(tag->text, sizeof(tag->text), "%s", text);
                tag->background = lua_isinteger(state, -4) ? (uint32_t)lua_tointeger(state, -4) : 0xFF5865F2;
                tag->foreground = lua_isinteger(state, -3) ? (uint32_t)lua_tointeger(state, -3) : 0xFFFFFFFF;
                if (lua_istable(state, -2)) {
                    for (lua_Integer stop = 1; stop <= 3; stop++) {
                        lua_rawgeti(state, -2, stop);
                        if (lua_isinteger(state, -1)) tag->gradient[tag->gradientCount++] = (uint32_t)lua_tointeger(state, -1);
                        lua_pop(state, 1);
                    }
                }
                const char *shape = lua_tostring(state, -1);
                tag->shape = shape && strcmp(shape, "op") == 0 ? kAstralTagOriginalPoster : kAstralTagPill;
            }
            lua_pop(state, 5);
        }
        lua_pop(state, 1);
    }
    lua_pop(state, 1);
}

static std::string ResolveChannel(uint64_t channelID, const std::string &message)
{
    static std::string lastChannel;
    std::string channel = Snowflake(channelID);
    if (channel.empty() && !message.empty()) {
        NSString *found = AstralDiscordMessageChannel(Native(message));
        if (found) lastChannel = found.UTF8String;
        channel = lastChannel;
    }
    return channel;
}

static const char *LabelTarget(AstralLabelKind kind)
{
    switch (kind) {
    case kAstralLabelChannel:
        return "channel.name";
    case kAstralLabelMember:
        return "member.name";
    case kAstralLabelComposer:
        return "composer.placeholder";
    default:
        return "message.author";
    }
}

static const char *LabelKindName(AstralLabelKind kind)
{
    switch (kind) {
    case kAstralLabelReply:
        return "reply";
    case kAstralLabelChannel:
        return "channel";
    case kAstralLabelMember:
        return "member";
    case kAstralLabelComposer:
        return "composer";
    default:
        return "author";
    }
}

int AstralLuaRelabel(const AstralLabelContext *context, const char *text, size_t length, char *out, size_t capacity)
{
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) return -1;
    std::string message = Snowflake(context->message_id);
    std::string channel = ResolveChannel(context->channel_id, message);
    std::string target = Snowflake(context->target_id);
    NSString *guild = channel.empty() ? nil : AstralDiscordChannelGuild(Native(channel));
    std::string wanted = LabelTarget(context->kind);
    std::string current(text, length);
    bool changed = false;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (auto &relabeler : plugin->relabelers) {
            if (relabeler.first != wanted) continue;
            lua_State *state = plugin->state;
            lua_rawgeti(state, LUA_REGISTRYINDEX, relabeler.second);
            lua_createtable(state, 0, 6);
            lua_pushstring(state, LabelKindName(context->kind));
            lua_setfield(state, -2, "kind");
            lua_pushlstring(state, current.data(), current.size());
            lua_setfield(state, -2, "text");
            if (!message.empty()) {
                lua_pushstring(state, message.c_str());
                lua_setfield(state, -2, "message_id");
            }
            if (!channel.empty()) {
                lua_pushstring(state, channel.c_str());
                lua_setfield(state, -2, "channel_id");
            }
            if (context->kind == kAstralLabelMember && !target.empty()) {
                lua_pushstring(state, target.c_str());
                lua_setfield(state, -2, "user_id");
            }
            if (guild) {
                lua_pushstring(state, guild.UTF8String);
                lua_setfield(state, -2, "guild_id");
            }
            if (Call(plugin, 1, 1, "relabel")) {
                size_t size = 0;
                const char *result = lua_type(state, -1) == LUA_TSTRING ? lua_tolstring(state, -1, &size) : nullptr;
                if (result && size > 0 && size < capacity) {
                    current.assign(result, size);
                    changed = true;
                }
                lua_pop(state, 1);
            }
            if (plugin->failed) break;
        }
        if (plugin->failed) Stop(plugin);
    }
    if (!changed) return -1;
    return snprintf(out, capacity, "%s", current.c_str());
}

size_t AstralLuaDecorate(const AstralDecorationContext *context, AstralTag *tags, size_t capacity)
{
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) return 0;
    std::string user = Snowflake(context->user_id), message = Snowflake(context->message_id);
    std::string channel = ResolveChannel(context->channel_id, message);
    NSString *guild = channel.empty() ? nil : AstralDiscordChannelGuild(Native(channel));
    size_t count = 0;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (auto &decorator : plugin->decorators) {
            if (count >= capacity) break;
            lua_State *state = plugin->state;
            lua_rawgeti(state, LUA_REGISTRYINDEX, decorator.second);
            lua_createtable(state, 0, 4);
            lua_pushstring(state, message.c_str());
            lua_setfield(state, -2, "message_id");
            if (!user.empty()) {
                lua_pushstring(state, user.c_str());
                lua_setfield(state, -2, "user_id");
            }
            if (!channel.empty()) {
                lua_pushstring(state, channel.c_str());
                lua_setfield(state, -2, "channel_id");
            }
            if (guild) {
                lua_pushstring(state, guild.UTF8String);
                lua_setfield(state, -2, "guild_id");
            }
            if (Call(plugin, 1, 1, "decorate")) CollectTags(plugin, tags, capacity, &count);
            if (plugin->failed) break;
        }
        if (plugin->failed) Stop(plugin);
    }
    return count;
}

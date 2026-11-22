#import "LuaHost.h"

#include <algorithm>
#include <cmath>

#import "PhoenixWire.h"
#import "TomlDocument.h"

#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
#include <string>
#include <vector>

#include "AstralLog.h"
#include "AstralSettings.h"
#include "LuaJSON.h"
#include "LuaLocale.h"
#include "LuaSandbox.h"
#include "PluginPackage.h"
#include "PluginStorage.h"
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
    bool number = false;
    double minimum = 0;
    double maximum = 100;
    double step = 1;
    double fallbackNumber = 0;
};

struct LuaPlugin : astral::PluginPackage, astral::PluginHealth {
    bool bundled = false;
    bool signedByAstral = false;
    bool enabled = false;
    std::unique_ptr<astral::lua::Sandbox> sandbox;
    lua_State *state = nullptr;
    std::vector<LuaSwitch> switches;
    std::vector<std::pair<std::string, int>> decorators;
    std::vector<std::pair<std::string, int>> relabelers;
    std::optional<double> settingsBackdrop;
    std::vector<int> mentioners;
    std::vector<int> colorizers;
    std::vector<int> nameColorizers;
};

static int Translate(lua_State *state);

static std::recursive_mutex gMutex;
static std::vector<LuaPlugin *> gPlugins;
static astral::SettingsStore *gStore;
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

static double StoredNumber(const std::string &key, double fallback)
{
    auto value = gStore->get("plugins", key);
    return value && value->is_number() ? value->get<double>() : fallback;
}

static double SteppedNumber(const LuaSwitch &entry, double value)
{
    double clamped = std::min(entry.maximum, std::max(entry.minimum, value));
    double steps = std::round((clamped - entry.minimum) / entry.step);
    return std::min(entry.maximum, entry.minimum + steps * entry.step);
}

static void Store(const std::string &key, const nlohmann::json &value)
{
    gStore->set("plugins", key, value);
    gStore->save();
}

static LuaPlugin *Self(lua_State *state)
{
    return static_cast<LuaPlugin *>(lua_touserdata(state, lua_upvalueindex(1)));
}

static void RecordFailure(LuaPlugin *plugin)
{
    AstralRecordPluginFailure(Native(plugin->identifier), Native(plugin->name), Native(plugin->lastError));
}

static void MarkFailed(LuaPlugin *plugin)
{
    if (plugin->markFailed()) RecordFailure(plugin);
}

static void Report(LuaPlugin *plugin, const char *stage, const std::string &message)
{
    bool failedNow = plugin->recordError(stage, message, kMaxErrors);
    astral::log(astral::LogLevel::Error, "plugin", plugin->identifier + " " + plugin->lastError);
    AstralDevAppendLog(Native(plugin->identifier + " error: " + plugin->lastError));
    if (failedNow) RecordFailure(plugin);
}

static bool Call(LuaPlugin *plugin, int arguments, int results, const char *stage)
{
    auto error = plugin->sandbox->call(arguments, results);
    if (!error) return true;
    Report(plugin, stage, *error);
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
    astral::log(astral::LogLevel::Info, "plugin", Self(state)->identifier + ": " + line);
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

static double FieldNumber(lua_State *state, int index, const char *name, double fallback)
{
    lua_getfield(state, index, name);
    double value = lua_isnumber(state, -1) ? lua_tonumber(state, -1) : fallback;
    lua_pop(state, 1);
    return value;
}

static void PushNumber(lua_State *state, double value)
{
    if (value == std::floor(value) && std::fabs(value) < 9.0e15)
        lua_pushinteger(state, (lua_Integer)value);
    else
        lua_pushnumber(state, value);
}

static int SettingsNumber(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    std::string key = luaL_checkstring(state, 1);
    std::string title = luaL_checkstring(state, 2);
    luaL_checktype(state, 3, LUA_TTABLE);
    LuaSwitch entry{key, title.substr(0, 64), "", "", false, {}, ""};
    entry.number = true;
    entry.minimum = FieldNumber(state, 3, "min", 0);
    entry.maximum = FieldNumber(state, 3, "max", 100);
    entry.step = FieldNumber(state, 3, "step", 1);
    entry.subtitle = FieldString(state, 3, "subtitle").substr(0, 120);
    entry.section = FieldString(state, 3, "section").substr(0, 48);
    bool valid = !key.empty() && key.size() <= 48 && plugin->switches.size() < 32 && std::isfinite(entry.minimum) &&
                 std::isfinite(entry.maximum) && entry.maximum > entry.minimum && entry.step > 0 &&
                 (entry.maximum - entry.minimum) / entry.step <= 1000;
    if (!valid) return luaL_error(state, "astral.settings.number: invalid number");
    entry.fallbackNumber = SteppedNumber(entry, FieldNumber(state, 3, "default", entry.minimum));
    bool known = false;
    for (auto &existing : plugin->switches) known = known || existing.key == key;
    if (!known) plugin->switches.push_back(entry);
    PushNumber(state, SteppedNumber(entry, StoredNumber(SettingKey(plugin, key), entry.fallbackNumber)));
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

static int Mention(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    luaL_checktype(state, 1, LUA_TFUNCTION);
    lua_pushvalue(state, 1);
    plugin->mentioners.push_back(luaL_ref(state, LUA_REGISTRYINDEX));
    return 0;
}

static int MessageColor(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    luaL_checktype(state, 1, LUA_TFUNCTION);
    lua_pushvalue(state, 1);
    plugin->colorizers.push_back(luaL_ref(state, LUA_REGISTRYINDEX));
    return 0;
}

static int NameColor(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    luaL_checktype(state, 1, LUA_TFUNCTION);
    lua_pushvalue(state, 1);
    plugin->nameColorizers.push_back(luaL_ref(state, LUA_REGISTRYINDEX));
    return 0;
}

static uint32_t Opaque(lua_Integer value)
{
    uint32_t color = (uint32_t)value;
    return color >> 24 ? color : color | 0xFF000000u;
}

static void ApplySettingsBackdrop(void)
{
    std::optional<double> dim;
    for (auto &entry : gPlugins) {
        if (entry->state && entry->settingsBackdrop) dim = std::max(dim.value_or(0), *entry->settingsBackdrop);
    }
    astral_wire_set_settings_backdrop(dim.has_value(), dim.value_or(0));
}

static int SettingsBackdrop(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    if (lua_isnoneornil(state, 1)) {
        plugin->settingsBackdrop.reset();
    } else {
        luaL_checktype(state, 1, LUA_TTABLE);
        double dim = FieldNumber(state, 1, "dim", 55);
        if (!std::isfinite(dim) || dim < 0 || dim > 90)
            return luaL_error(state, "astral.ui.settings_backdrop: dim is a percentage from 0 to 90");
        plugin->settingsBackdrop = dim / 100;
    }
    ApplySettingsBackdrop();
    return 0;
}

static int Relabel(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    std::string target = luaL_checkstring(state, 1);
    luaL_checktype(state, 2, LUA_TFUNCTION);
    if (target != "message.author" && target != "channel.name" && target != "member.name" && target != "composer.placeholder" &&
        target != "profile.name") {
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

static astral::PluginStorage &Storage(void)
{
    static astral::PluginStorage *storage = new astral::PluginStorage(
        [AstralStateDirectory() stringByAppendingPathComponent:@"plugin-storage.sqlite"].fileSystemRepresentation, kStorageLimit);
    return *storage;
}

static astral::PluginStorage &AvailableStorage(lua_State *state)
{
    RequirePermission(state, "storage");
    if (!Storage().available()) luaL_error(state, "astral.storage is unavailable");
    return Storage();
}

static int StorageGet(lua_State *state)
{
    auto value = AvailableStorage(state).get(Self(state)->identifier, luaL_checkstring(state, 1));
    if (value)
        astral::lua::pushJSON(state, *value);
    else
        lua_pushvalue(state, 2);
    return 1;
}

static int StorageSet(lua_State *state)
{
    astral::PluginStorage &storage = AvailableStorage(state);
    LuaPlugin *plugin = Self(state);
    const char *key = luaL_checkstring(state, 1);
    nlohmann::json value;
    if (!astral::lua::toJSON(state, 2, value))
        return luaL_error(state, "astral.storage.set: '%s' holds a value that cannot be stored", key);
    switch (storage.set(plugin->identifier, key, value)) {
    case astral::StorageResult::Saved:
    case astral::StorageResult::Removed:
        return 0;
    case astral::StorageResult::OverLimit:
        return luaL_error(state, "astral.storage: %s would exceed %d KB", plugin->identifier.c_str(), (int)(kStorageLimit >> 10));
    case astral::StorageResult::Unavailable:
    case astral::StorageResult::Failed:
        return luaL_error(state, "astral.storage.set: could not save '%s'", key);
    }
}

static int StorageRemove(lua_State *state)
{
    AvailableStorage(state).remove(Self(state)->identifier, luaL_checkstring(state, 1));
    return 0;
}

static int StorageKeys(lua_State *state)
{
    auto keys = AvailableStorage(state).keys(Self(state)->identifier);
    lua_createtable(state, (int)keys.size(), 0);
    lua_Integer position = 1;
    for (const auto &key : keys) {
        lua_pushlstring(state, key.data(), key.size());
        lua_rawseti(state, -2, position++);
    }
    return 1;
}

void AstralLuaRemoveStorage(NSString *identifier)
{
    Storage().removeAll(identifier.UTF8String ?: "");
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
    Function(state, plugin, "number", SettingsNumber);
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
    Function(state, plugin, "settings_backdrop", SettingsBackdrop);
    Function(state, plugin, "mention", Mention);
    Function(state, plugin, "message_color", MessageColor);
    Function(state, plugin, "name_color", NameColor);
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

static bool RunPrelude(LuaPlugin *plugin)
{
    auto error = plugin->sandbox->load(std::string(kAstralPrelude, sizeof(kAstralPrelude) - 1), "astral");
    if (error) {
        Report(plugin, "prelude", *error);
        return false;
    }
    return Call(plugin, 0, 0, "prelude");
}

static void Stop(LuaPlugin *plugin)
{
    plugin->sandbox.reset();
    plugin->state = nullptr;
    plugin->switches.clear();
    plugin->decorators.clear();
    plugin->relabelers.clear();
    plugin->settingsBackdrop.reset();
    plugin->mentioners.clear();
    plugin->colorizers.clear();
    plugin->nameColorizers.clear();
    ApplySettingsBackdrop();
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
    plugin->reset();
    plugin->sandbox = std::make_unique<astral::lua::Sandbox>(astral::lua::SandboxLimits{kMemoryLimit, kBudgetSlices, kSliceInstructions});
    plugin->state = plugin->sandbox->state();
    if (!plugin->state) {
        Stop(plugin);
        return false;
    }
    InstallAPI(plugin);
    InstallRequire(plugin);
    if (!RunPrelude(plugin)) {
        MarkFailed(plugin);
        Stop(plugin);
        return false;
    }
    if (auto error = plugin->sandbox->load(plugin->source, plugin->identifier)) {
        Report(plugin, "load", *error);
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

static std::vector<std::string> PreferredLanguages(void)
{
    std::vector<std::string> languages;
    for (NSString *language in NSLocale.preferredLanguages) languages.push_back(language.UTF8String);
    return languages;
}

static int Translate(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    std::string key = luaL_checkstring(state, 1);
    std::vector<std::pair<std::string, std::string>> values;
    if (lua_istable(state, 2)) {
        lua_pushnil(state);
        while (lua_next(state, 2)) {
            if (lua_type(state, -2) == LUA_TSTRING) {
                std::string name = lua_tostring(state, -2);
                values.push_back({name, luaL_tolstring(state, -1, nullptr)});
                lua_pop(state, 1);
            }
            lua_pop(state, 1);
        }
    }
    std::string text = plugin->translate(key, values);
    lua_pushstring(state, text.c_str());
    return 1;
}

static LuaPlugin *Load(NSString *directory)
{
    auto package = astral::loadPluginPackage(directory.fileSystemRepresentation ?: "", PreferredLanguages(), {kSourceLimit, kModulesLimit});
    if (!package) return nullptr;
    LuaPlugin *plugin = new LuaPlugin();
    static_cast<astral::PluginPackage &>(*plugin) = std::move(*package);
    plugin->signedByAstral = AstralVerifyPluginDirectory(directory, Native(plugin->main));
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

void AstralLuaHostStart(NSArray<NSString *> *directories, astral::SettingsStore *store, bool startEnabled)
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
                @"value" : entry.number ? (id) @(SteppedNumber(entry, StoredNumber(SettingKey(plugin, entry.key), entry.fallbackNumber)))
                                        : (id)Native(StoredString(SettingKey(plugin, entry.key), entry.fallbackChoice)),
                @"number" : @(entry.number),
                @"min" : @(entry.minimum),
                @"max" : @(entry.maximum),
                @"step" : @(entry.step)
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
    if (plugin->sandbox->load("return " + source, "eval")) {
        if (auto error = plugin->sandbox->load(source, "eval")) {
            lua_settop(state, top);
            return [@"error: " stringByAppendingString:Native(*error)];
        }
    }
    if (auto error = plugin->sandbox->call(0, LUA_MULTRET)) {
        lua_settop(state, top);
        return [@"error: " stringByAppendingString:Native(*error)];
    }
    NSMutableArray *parts = [NSMutableArray array];
    for (int index = top + 1; index <= lua_gettop(state); index++) {
        [parts addObject:Native(luaL_tolstring(state, index, nullptr))];
        lua_pop(state, 1);
    }
    lua_settop(state, top);
    return [parts componentsJoinedByString:@"\t"];
}

static std::optional<nlohmann::json> KeptState(LuaPlugin *plugin)
{
    if (!plugin || !plugin->state) return std::nullopt;
    lua_State *state = plugin->state;
    int top = lua_gettop(state);
    lua_getglobal(state, "astral");
    if (lua_istable(state, -1)) lua_getfield(state, -1, "state");
    nlohmann::json kept;
    bool converted = lua_istable(state, -1) && astral::lua::toJSON(state, -1, kept);
    lua_settop(state, top);
    if (!converted) {
        AstralDevAppendLog(Native(plugin->identifier + ": astral.state holds values that cannot be kept; reloading without it"));
        return std::nullopt;
    }
    return kept;
}

static void RestoreState(LuaPlugin *plugin, const nlohmann::json &kept)
{
    if (!plugin->state) return;
    lua_State *state = plugin->state;
    int top = lua_gettop(state);
    lua_getglobal(state, "astral");
    if (!lua_istable(state, -1)) {
        lua_settop(state, top);
        return;
    }
    astral::lua::pushJSON(state, kept);
    lua_setfield(state, -2, "state");
    lua_getfield(state, -1, "on_reload");
    if (lua_isfunction(state, -1)) {
        lua_getfield(state, -2, "state");
        Call(plugin, 1, 0, "on_reload");
    }
    lua_settop(state, top);
}

void AstralLuaReload(NSString *pluginID)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    std::optional<nlohmann::json> kept = KeptState(Find(pluginID));
    AstralLuaRemove(pluginID);
    AstralLuaRescan();
    AstralLuaSetEnabled(pluginID, YES);
    LuaPlugin *plugin = Find(pluginID);
    if (plugin && kept) RestoreState(plugin, *kept);
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

void AstralLuaSetNumber(NSString *pluginID, NSString *key, double value)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    LuaPlugin *plugin = Find(pluginID);
    if (!plugin) return;
    const LuaSwitch *entry = nullptr;
    for (auto &candidate : plugin->switches) {
        if (candidate.number && candidate.key == key.UTF8String) entry = &candidate;
    }
    if (!entry) return;
    double stepped = SteppedNumber(*entry, value);
    if (StoredNumber(SettingKey(plugin, key.UTF8String), entry->fallbackNumber) == stepped) return;
    Store(SettingKey(plugin, key.UTF8String), stepped);
    if (!plugin->state) return;
    lua_getglobal(plugin->state, "astral");
    lua_getfield(plugin->state, -1, "on_setting");
    lua_remove(plugin->state, -2);
    if (lua_isfunction(plugin->state, -1)) {
        lua_pushstring(plugin->state, key.UTF8String);
        PushNumber(plugin->state, stepped);
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
    case kAstralLabelProfile:
        return "profile.name";
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
    case kAstralLabelProfile:
        return "profile";
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
            if ((context->kind == kAstralLabelMember || context->kind == kAstralLabelProfile) && !target.empty()) {
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

int AstralLuaMention(const AstralMentionContext *context, AstralMentionDecoration *decoration)
{
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) return 0;
    std::string message = Snowflake(context->message_id);
    std::string channel = ResolveChannel(context->channel_id, message);
    NSString *guild = channel.empty() ? nil : AstralDiscordChannelGuild(Native(channel));
    int decorated = 0;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (int mentioner : plugin->mentioners) {
            lua_State *state = plugin->state;
            lua_rawgeti(state, LUA_REGISTRYINDEX, mentioner);
            lua_createtable(state, 0, 5);
            lua_pushstring(state, context->text);
            lua_setfield(state, -2, "text");
            lua_pushinteger(state, context->index + 1);
            lua_setfield(state, -2, "index");
            lua_pushstring(state, message.c_str());
            lua_setfield(state, -2, "message_id");
            if (!channel.empty()) {
                lua_pushstring(state, channel.c_str());
                lua_setfield(state, -2, "channel_id");
            }
            if (guild) {
                lua_pushstring(state, guild.UTF8String);
                lua_setfield(state, -2, "guild_id");
            }
            if (Call(plugin, 1, 1, "mention")) {
                if (lua_istable(state, -1)) {
                    lua_getfield(state, -1, "after");
                    if (lua_toboolean(state, -1)) decoration->after = 1;
                    lua_pop(state, 1);
                    lua_getfield(state, -1, "color");
                    if (lua_isinteger(state, -1) && !decoration->color) decoration->color = Opaque(lua_tointeger(state, -1));
                    lua_pop(state, 1);
                    lua_getfield(state, -1, "image");
                    lua_getfield(state, -2, "round");
                    lua_getfield(state, -3, "hide_at");
                    const char *image = lua_tostring(state, -3);
                    if (image && *image && !decoration->image[0] &&
                        (strncmp(image, "https://", 8) == 0 || strncmp(image, "res://astral_tinted_", 20) == 0)) {
                        snprintf(decoration->image, sizeof(decoration->image), "%s", image);
                        decoration->round = lua_isnil(state, -2) ? 1 : lua_toboolean(state, -2);
                    }
                    if (lua_toboolean(state, -1)) decoration->hide_at = 1;
                    lua_pop(state, 3);
                    decorated = 1;
                }
                lua_pop(state, 1);
            }
            if (plugin->failed) break;
        }
        if (plugin->failed) Stop(plugin);
    }
    return decorated;
}

int AstralLuaMessageTextColor(uint64_t messageID, uint64_t channelID, uint32_t *color)
{
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) return 0;
    bool wanted = false;
    for (auto *plugin : gPlugins) wanted = wanted || (plugin->state && !plugin->failed && !plugin->colorizers.empty());
    if (!wanted) return 0;
    std::string message = Snowflake(messageID);
    std::string channel = ResolveChannel(channelID, message);
    if (channel.empty() || !AstralDiscordMessage(Native(channel), Native(message))) return -1;
    NSString *guild = AstralDiscordChannelGuild(Native(channel));
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (int colorizer : plugin->colorizers) {
            lua_State *state = plugin->state;
            lua_rawgeti(state, LUA_REGISTRYINDEX, colorizer);
            lua_createtable(state, 0, 3);
            lua_pushstring(state, message.c_str());
            lua_setfield(state, -2, "message_id");
            lua_pushstring(state, channel.c_str());
            lua_setfield(state, -2, "channel_id");
            if (guild) {
                lua_pushstring(state, guild.UTF8String);
                lua_setfield(state, -2, "guild_id");
            }
            if (Call(plugin, 1, 1, "message_color")) {
                bool found = lua_isinteger(state, -1);
                if (found) *color = (uint32_t)lua_tointeger(state, -1);
                lua_pop(state, 1);
                if (found) return 1;
            }
            if (plugin->failed) break;
        }
        if (plugin->failed) Stop(plugin);
    }
    return 0;
}

int AstralLuaNameColor(const AstralNameContext *context, uint32_t *color)
{
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) return 0;
    std::string channel = Snowflake(context->channel_id);
    NSString *guild = channel.empty() ? nil : AstralDiscordChannelGuild(Native(channel));
    if (!guild) return 0;
    std::string user = Snowflake(context->user_id);
    if (user.empty() && context->kind == kAstralNameTyping) {
        NSString *named = AstralDiscordMemberNamed(guild, Native(context->text));
        if (!named) return 0;
        user = named.UTF8String;
    }
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (int colorizer : plugin->nameColorizers) {
            lua_State *state = plugin->state;
            lua_rawgeti(state, LUA_REGISTRYINDEX, colorizer);
            lua_createtable(state, 0, 5);
            static const char *const kinds[] = {"avatar", "header", "voter", "typing"};
            lua_pushstring(state, kinds[context->kind]);
            lua_setfield(state, -2, "kind");
            lua_pushstring(state, context->text);
            lua_setfield(state, -2, "text");
            if (!user.empty()) {
                lua_pushstring(state, user.c_str());
                lua_setfield(state, -2, "user_id");
            }
            lua_pushstring(state, channel.c_str());
            lua_setfield(state, -2, "channel_id");
            lua_pushstring(state, guild.UTF8String);
            lua_setfield(state, -2, "guild_id");
            if (Call(plugin, 1, 1, "name_color")) {
                bool found = lua_isinteger(state, -1);
                if (found) *color = Opaque(lua_tointeger(state, -1));
                lua_pop(state, 1);
                if (found) return 1;
            }
            if (plugin->failed) break;
        }
        if (plugin->failed) Stop(plugin);
    }
    return 0;
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

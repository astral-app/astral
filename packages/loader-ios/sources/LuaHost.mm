#import "LuaHost.h"

#include <mutex>
#include <string>
#include <vector>

#include "LoaderLog.h"
#include "LoaderSettings.h"
#import "AstralState.h"
#import "DiscordCache.h"

extern "C" {
#include "lauxlib.h"
#include "lua.h"
#include "lualib.h"
}

static const size_t kMemoryLimit = 8u << 20;
static const size_t kSourceLimit = 256u << 10;
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
    std::vector<std::pair<std::string, std::string>> authors;
    std::string directory;
    bool bundled = false;
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
    std::vector<int> relabelers;
};

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

static void Report(LuaPlugin *plugin, const char *stage)
{
    const char *message = lua_tostring(plugin->state, -1);
    plugin->lastError = std::string(stage) + ": " + (message ?: "unknown error");
    lua_pop(plugin->state, 1);
    loader::log(loader::LogLevel::Error, "plugin", plugin->identifier + " " + plugin->lastError);
    if (++plugin->errors >= kMaxErrors) plugin->failed = true;
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
    if (target != "message.author") return luaL_error(state, "astral.ui.relabel: unsupported target '%s'", target.c_str());
    lua_pushvalue(state, 2);
    plugin->relabelers.push_back(luaL_ref(state, LUA_REGISTRYINDEX));
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

static NSString *Argument(lua_State *state, int index)
{
    return [NSString stringWithUTF8String:luaL_checkstring(state, index)];
}

static int Guild(lua_State *state)
{
    PushObject(state, AstralDiscordGuild(Argument(state, 1)), 0);
    return 1;
}

static int Member(lua_State *state)
{
    PushObject(state, AstralDiscordMember(Argument(state, 1), Argument(state, 2)), 0);
    return 1;
}

static int ChannelGuild(lua_State *state)
{
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
    PushObject(state, AstralDiscordUser(Argument(state, 1)), 0);
    return 1;
}

static int Nickname(lua_State *state)
{
    PushObject(state, AstralDiscordFriendNickname(Argument(state, 1)), 0);
    return 1;
}

static int Message(lua_State *state)
{
    PushObject(state, AstralDiscordMessage(Argument(state, 1), Argument(state, 2)), 0);
    return 1;
}

static int Permissions(lua_State *state)
{
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
    Function(state, plugin, "decorate", Decorate);
    Function(state, plugin, "relabel", Relabel);
    lua_setfield(state, -2, "ui");

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

static void Stop(LuaPlugin *plugin)
{
    if (plugin->state) lua_close(plugin->state);
    plugin->state = nullptr;
    plugin->memory = 0;
    plugin->switches.clear();
    plugin->decorators.clear();
    plugin->relabelers.clear();
}

static bool Start(LuaPlugin *plugin)
{
    Stop(plugin);
    plugin->errors = 0;
    plugin->failed = false;
    plugin->lastError.clear();
    plugin->state = lua_newstate(Allocate, plugin);
    if (!plugin->state) return false;
    OpenSafeLibraries(plugin->state);
    InstallAPI(plugin);
    if (luaL_loadbufferx(plugin->state, plugin->source.data(), plugin->source.size(), ("=" + plugin->identifier).c_str(), "t") != LUA_OK) {
        Report(plugin, "load");
        plugin->failed = true;
        Stop(plugin);
        return false;
    }
    if (!Call(plugin, 0, 0, "start")) {
        plugin->failed = true;
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

static LuaPlugin *Load(NSString *directory)
{
    NSData *manifestData = [NSData dataWithContentsOfFile:[directory stringByAppendingPathComponent:@"manifest.json"]];
    NSDictionary *manifest = manifestData ? [NSJSONSerialization JSONObjectWithData:manifestData options:0 error:nil] : nil;
    if (![manifest isKindOfClass:NSDictionary.class] || ![manifest[@"runtime"] isEqual:@"lua"]) return nullptr;
    NSString *identifier = manifest[@"id"], *name = manifest[@"name"], *main = manifest[@"main"] ?: @"main.lua";
    if (![identifier isKindOfClass:NSString.class] || !ValidIdentifier(identifier) || ![name isKindOfClass:NSString.class] ||
        ![main isKindOfClass:NSString.class] || [main containsString:@"/"])
        return nullptr;
    NSData *source = [NSData dataWithContentsOfFile:[directory stringByAppendingPathComponent:main]];
    if (!source || source.length > kSourceLimit) return nullptr;
    LuaPlugin *plugin = new LuaPlugin();
    plugin->identifier = identifier.UTF8String;
    plugin->name = [name substringToIndex:MIN(name.length, (NSUInteger)48)].UTF8String;
    plugin->version = [manifest[@"version"] isKindOfClass:NSString.class] ? [manifest[@"version"] UTF8String] : "";
    plugin->description = [manifest[@"description"] isKindOfClass:NSString.class] ? [manifest[@"description"] UTF8String] : "";
    plugin->icon = [manifest[@"icon"] isKindOfClass:NSString.class] ? [manifest[@"icon"] UTF8String] : "";
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
        [plugins addObject:@{
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
            @"authors" : authors,
            @"directory" : Native(plugin->directory),
            @"bundled" : @(plugin->bundled)
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

int AstralLuaRelabel(const AstralLabelContext *context, const char *text, size_t length, char *out, size_t capacity)
{
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) return -1;
    std::string message = Snowflake(context->message_id);
    std::string channel = ResolveChannel(context->channel_id, message);
    NSString *guild = channel.empty() ? nil : AstralDiscordChannelGuild(Native(channel));
    std::string current(text, length);
    bool changed = false;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (int relabeler : plugin->relabelers) {
            lua_State *state = plugin->state;
            lua_rawgeti(state, LUA_REGISTRYINDEX, relabeler);
            lua_createtable(state, 0, 5);
            lua_pushstring(state, context->kind == kAstralLabelReply ? "reply" : "author");
            lua_setfield(state, -2, "kind");
            lua_pushlstring(state, current.data(), current.size());
            lua_setfield(state, -2, "text");
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

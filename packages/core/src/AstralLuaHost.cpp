#include "AstralLuaHost.h"

#include <algorithm>
#include <atomic>
#include <cmath>
#include <cctype>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <vector>

#include "AstralLog.h"
#include "AstralSettings.h"
#include "LuaJSON.h"
#include "LuaLocale.h"
#include "LuaSandbox.h"
#include "PluginPackage.h"
#include "PluginStorage.h"

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

struct LuaTextSetting {
    std::string key;
    std::string title;
    std::string subtitle;
    std::string section;
    std::string fallback;
    size_t maximumLength = 0;
};

struct LuaEventCallback {
    std::string event;
    int reference;
};

struct LuaPlugin : astral::PluginPackage, astral::PluginHealth {
    bool bundled = false;
    bool signedByAstral = false;
    bool enabled = false;
    std::unique_ptr<astral::lua::Sandbox> sandbox;
    lua_State *state = nullptr;
    std::vector<LuaSwitch> switches;
    std::vector<LuaTextSetting> textSettings;
    std::vector<std::pair<std::string, int>> decorators;
    std::vector<std::pair<std::string, int>> relabelers;
    std::optional<double> settingsBackdrop;
    std::vector<int> mentioners;
    std::vector<int> colorizers;
    std::vector<int> nameColorizers;
    std::vector<int> typingDecorators;
    std::vector<int> reactionDecorators;
    std::vector<int> outgoingMessageTransforms;
    std::map<uint64_t, int> networkRequests;
    std::map<uint64_t, int> filePicks;
    std::map<uint64_t, LuaEventCallback> eventCallbacks;
};

static int Translate(lua_State *state);
static void RequirePermission(lua_State *state, const char *permission);

static std::recursive_mutex gMutex;
static std::vector<LuaPlugin *> gPlugins;
static astral::SettingsStore *gStore;
static std::vector<std::string> gDirectories;
static bool gStartEnabled;
static astral::LuaHostPlatform gPlatform;
static std::atomic<uint64_t> gNextRequest{1};
static std::string gCurrentUserID;

static void DeveloperLog(const std::string &line)
{
    if (gPlatform.developerLog) gPlatform.developerLog(line);
}

static void RefreshDecorations(void)
{
    if (gPlatform.refreshDecorations) gPlatform.refreshDecorations();
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
    if (gPlatform.recordFailure) gPlatform.recordFailure(plugin->identifier, plugin->name, plugin->lastError);
}

static void MarkFailed(LuaPlugin *plugin)
{
    if (plugin->markFailed()) RecordFailure(plugin);
}

static void Report(LuaPlugin *plugin, const char *stage, const std::string &message)
{
    bool failedNow = plugin->recordError(stage, message, kMaxErrors);
    astral::log(astral::LogLevel::Error, "plugin", plugin->identifier + " " + plugin->lastError);
    DeveloperLog(plugin->identifier + " error: " + plugin->lastError);
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
    DeveloperLog(Self(state)->identifier + ": " + line);
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
    if (!known) plugin->switches.push_back({key, title.substr(0, 64), subtitle.substr(0, 120), section.substr(0, 48), fallback, {}, {}});
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

static int SettingsText(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    std::string key = luaL_checkstring(state, 1);
    std::string title = luaL_checkstring(state, 2);
    std::string fallback = luaL_checkstring(state, 3);
    size_t maximumLength = 16384;
    LuaTextSetting entry{key, title.substr(0, 64), "", "", fallback, maximumLength};
    if (lua_istable(state, 4)) {
        entry.subtitle = FieldString(state, 4, "subtitle").substr(0, 120);
        entry.section = FieldString(state, 4, "section").substr(0, 48);
        double requestedMaximum = FieldNumber(state, 4, "max_length", static_cast<double>(maximumLength));
        if (!std::isfinite(requestedMaximum) || requestedMaximum < 1 || requestedMaximum > maximumLength ||
            std::floor(requestedMaximum) != requestedMaximum)
            return luaL_error(state, "astral.settings.text: max_length must be between 1 and 16384");
        entry.maximumLength = static_cast<size_t>(requestedMaximum);
    }
    if (key.empty() || key.size() > 48 || fallback.size() > entry.maximumLength || plugin->textSettings.size() >= 16)
        return luaL_error(state, "astral.settings.text: invalid text setting");
    bool known = false;
    for (auto &existing : plugin->textSettings) known = known || existing.key == key;
    for (auto &existing : plugin->switches) known = known || existing.key == key;
    if (!known) plugin->textSettings.push_back(entry);
    std::string value = StoredString(SettingKey(plugin, key), fallback);
    if (value.size() > entry.maximumLength) value = fallback;
    lua_pushlstring(state, value.data(), value.size());
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

static int ShowAlert(lua_State *state)
{
    RequirePermission(state, "ui.dialogs");
    std::string title = luaL_checkstring(state, 1);
    std::string message = luaL_checkstring(state, 2);
    if (title.size() > 120 || message.size() > 2048) return luaL_error(state, "astral.ui.alert: text exceeds its limit");
    if (!gPlatform.showAlert) return luaL_error(state, "astral.ui.alert: dialogs are unavailable on this build");
    try {
        gPlatform.showAlert(title, message);
    } catch (...) {
        return luaL_error(state, "astral.ui.alert: could not show the dialog");
    }
    return 0;
}

static int CopyText(lua_State *state)
{
    RequirePermission(state, "ui.clipboard");
    size_t length = 0;
    const char *value = luaL_checklstring(state, 1, &length);
    if (length > 16384) return luaL_error(state, "astral.ui.copy: text exceeds 16384 bytes");
    if (!gPlatform.writeClipboard) return luaL_error(state, "astral.ui.copy: clipboard is unavailable on this build");
    bool copied = false;
    try {
        copied = gPlatform.writeClipboard(std::string(value, length));
    } catch (...) {
    }
    lua_pushboolean(state, copied);
    return 1;
}

static int Haptic(lua_State *state)
{
    RequirePermission(state, "ui.haptics");
    std::string strength = luaL_checkstring(state, 1);
    if (strength != "light" && strength != "medium" && strength != "heavy")
        return luaL_error(state, "astral.ui.haptic: strength must be light, medium or heavy");
    if (!gPlatform.triggerHaptic) return luaL_error(state, "astral.ui.haptic: haptics are unavailable on this build");
    bool triggered = false;
    try {
        triggered = gPlatform.triggerHaptic(strength);
    } catch (...) {
    }
    lua_pushboolean(state, triggered);
    return 1;
}

static int PickFile(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "files.read");
    luaL_checktype(state, 1, LUA_TTABLE);
    luaL_checktype(state, 2, LUA_TFUNCTION);
    size_t count = lua_rawlen(state, 1);
    if (count == 0 || count > 16) return luaL_error(state, "astral.files.pick: provide between 1 and 16 file extensions");
    std::vector<std::string> extensions;
    extensions.reserve(count);
    for (size_t index = 1; index <= count; index++) {
        lua_rawgeti(state, 1, static_cast<lua_Integer>(index));
        std::string extension = luaL_checkstring(state, -1);
        lua_pop(state, 1);
        if (!extension.empty() && extension.front() == '.') extension.erase(extension.begin());
        if (extension.empty() || extension.size() > 16 || !std::all_of(extension.begin(), extension.end(), [](unsigned char character) {
                return (character >= 'a' && character <= 'z') || (character >= 'A' && character <= 'Z') ||
                       (character >= '0' && character <= '9') || character == '+' || character == '-';
            })) {
            return luaL_error(state, "astral.files.pick: invalid file extension");
        }
        std::transform(extension.begin(), extension.end(), extension.begin(),
                       [](unsigned char character) { return static_cast<char>(std::tolower(character)); });
        if (std::find(extensions.begin(), extensions.end(), extension) == extensions.end()) extensions.push_back(std::move(extension));
    }
    if (!gPlatform.pickFile) return luaL_error(state, "astral.files.pick: file selection is unavailable on this build");
    uint64_t request = gNextRequest.fetch_add(1);
    if (request == 0) request = gNextRequest.fetch_add(1);
    lua_pushvalue(state, 2);
    plugin->filePicks[request] = luaL_ref(state, LUA_REGISTRYINDEX);
    bool started = false;
    try {
        started = gPlatform.pickFile(request, extensions);
    } catch (...) {
    }
    if (!started) {
        luaL_unref(state, LUA_REGISTRYINDEX, plugin->filePicks[request]);
        plugin->filePicks.erase(request);
        return luaL_error(state, "astral.files.pick: could not open the file picker");
    }
    lua_pushinteger(state, static_cast<lua_Integer>(request));
    return 1;
}

static int CancelFilePick(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "files.read");
    lua_Integer value = luaL_checkinteger(state, 1);
    if (value <= 0) {
        lua_pushboolean(state, false);
        return 1;
    }
    uint64_t request = static_cast<uint64_t>(value);
    auto found = plugin->filePicks.find(request);
    if (found == plugin->filePicks.end()) {
        lua_pushboolean(state, false);
        return 1;
    }
    luaL_unref(state, LUA_REGISTRYINDEX, found->second);
    plugin->filePicks.erase(found);
    if (gPlatform.cancelFilePick) {
        try {
            gPlatform.cancelFilePick(request);
        } catch (...) {
        }
    }
    lua_pushboolean(state, true);
    return 1;
}

static int SubscribeEvent(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    std::string event = luaL_checkstring(state, 1);
    const char *permission = nullptr;
    if (event == "app.foreground" || event == "app.background") permission = "events.lifecycle";
    if (event == "message.create") permission = "events.messages";
    if (!permission) return luaL_error(state, "astral.events.on: unsupported event '%s'", event.c_str());
    RequirePermission(state, permission);
    luaL_checktype(state, 2, LUA_TFUNCTION);
    if (plugin->eventCallbacks.size() >= 32) return luaL_error(state, "astral.events.on: at most 32 subscriptions per plugin");
    uint64_t subscription = gNextRequest.fetch_add(1);
    if (subscription == 0) subscription = gNextRequest.fetch_add(1);
    lua_pushvalue(state, 2);
    plugin->eventCallbacks.emplace(subscription, LuaEventCallback{event, luaL_ref(state, LUA_REGISTRYINDEX)});
    lua_pushinteger(state, static_cast<lua_Integer>(subscription));
    return 1;
}

static int UnsubscribeEvent(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "events.lifecycle");
    lua_Integer value = luaL_checkinteger(state, 1);
    if (value <= 0) {
        lua_pushboolean(state, false);
        return 1;
    }
    auto found = plugin->eventCallbacks.find(static_cast<uint64_t>(value));
    if (found == plugin->eventCallbacks.end()) {
        lua_pushboolean(state, false);
        return 1;
    }
    luaL_unref(state, LUA_REGISTRYINDEX, found->second.reference);
    plugin->eventCallbacks.erase(found);
    lua_pushboolean(state, true);
    return 1;
}

static int RegisterOutgoingMessageTransform(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "discord.messages.send");
    luaL_checktype(state, 1, LUA_TFUNCTION);
    if (plugin->outgoingMessageTransforms.size() >= 16)
        return luaL_error(state, "astral.messages.transform: at most 16 transforms per plugin");
    lua_pushvalue(state, 1);
    plugin->outgoingMessageTransforms.push_back(luaL_ref(state, LUA_REGISTRYINDEX));
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

static int TypingDecorator(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    luaL_checktype(state, 1, LUA_TFUNCTION);
    lua_pushvalue(state, 1);
    plugin->typingDecorators.push_back(luaL_ref(state, LUA_REGISTRYINDEX));
    return 0;
}

static int ReactionDecorator(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    luaL_checktype(state, 1, LUA_TFUNCTION);
    lua_pushvalue(state, 1);
    plugin->reactionDecorators.push_back(luaL_ref(state, LUA_REGISTRYINDEX));
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

static void PushObject(lua_State *state, const std::optional<nlohmann::json> &value)
{
    if (value)
        astral::lua::pushJSON(state, *value);
    else
        lua_pushnil(state);
}

static void PushString(lua_State *state, const std::optional<std::string> &value)
{
    if (value)
        lua_pushlstring(state, value->data(), value->size());
    else
        lua_pushnil(state);
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
    static astral::PluginStorage *storage = new astral::PluginStorage(gPlatform.storagePath, kStorageLimit);
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

static std::string Argument(lua_State *state, int index)
{
    return luaL_checkstring(state, index);
}

static std::optional<std::string> OptionalArgument(lua_State *state, int index)
{
    const char *value = lua_tostring(state, index);
    return value ? std::optional<std::string>(value) : std::nullopt;
}

static int Guild(lua_State *state)
{
    RequirePermission(state, "discord.guilds");
    PushObject(state, gPlatform.guild ? gPlatform.guild(Argument(state, 1)) : std::nullopt);
    return 1;
}

static int Member(lua_State *state)
{
    RequirePermission(state, "discord.members");
    PushObject(state, gPlatform.member ? gPlatform.member(Argument(state, 1), Argument(state, 2)) : std::nullopt);
    return 1;
}

static int ChannelGuild(lua_State *state)
{
    RequirePermission(state, "discord.guilds");
    PushString(state, gPlatform.channelGuild ? gPlatform.channelGuild(Argument(state, 1)) : std::nullopt);
    return 1;
}

static int User(lua_State *state)
{
    RequirePermission(state, "discord.users");
    PushObject(state, gPlatform.user ? gPlatform.user(Argument(state, 1)) : std::nullopt);
    return 1;
}

static int Nickname(lua_State *state)
{
    RequirePermission(state, "discord.relationships");
    PushString(state, gPlatform.friendNickname ? gPlatform.friendNickname(Argument(state, 1)) : std::nullopt);
    return 1;
}

static int Message(lua_State *state)
{
    RequirePermission(state, "discord.messages");
    PushObject(state, gPlatform.message ? gPlatform.message(Argument(state, 1), Argument(state, 2)) : std::nullopt);
    return 1;
}

static bool gReactorsStale;

static int Reactors(lua_State *state)
{
    RequirePermission(state, "discord.messages");
    bool fresh = false;
    std::optional<nlohmann::json> users;
    if (gPlatform.reactors) {
        users = gPlatform.reactors(Argument(state, 1), Argument(state, 2), Argument(state, 3), lua_toboolean(state, 4),
                                   (uint64_t)std::max<lua_Integer>(0, luaL_optinteger(state, 5, 0)), fresh);
    }
    if (!fresh) gReactorsStale = true;
    PushObject(state, users);
    return 1;
}

static int Permissions(lua_State *state)
{
    RequirePermission(state, "discord.permissions");
    auto permissions =
        gPlatform.permissions ? gPlatform.permissions(Argument(state, 1), Argument(state, 2), OptionalArgument(state, 3)) : std::nullopt;
    if (permissions)
        lua_pushinteger(state, (lua_Integer)*permissions);
    else
        lua_pushnil(state);
    return 1;
}

static bool SafeHeaderName(const std::string &name)
{
    if (name.empty() || name.size() > 128) return false;
    constexpr std::string_view punctuation = "!#$%&'*+-.^_`|~";
    for (unsigned char character : name) {
        bool letter = (character >= 'a' && character <= 'z') || (character >= 'A' && character <= 'Z');
        bool digit = character >= '0' && character <= '9';
        if (!letter && !digit && punctuation.find((char)character) == std::string_view::npos) return false;
    }
    return true;
}

static bool ContainsControl(const std::string &value)
{
    return std::any_of(value.begin(), value.end(), [](unsigned char character) { return character <= 0x20 || character == 0x7f; });
}

static bool SafeHeaderValue(const std::string &value)
{
    return value.size() <= 8192 &&
           std::none_of(value.begin(), value.end(), [](unsigned char character) { return character < 0x20 || character == 0x7f; });
}

static bool ValidUTF8(const std::string &value)
{
    try {
        (void)nlohmann::json(value).dump();
        return true;
    } catch (...) {
        return false;
    }
}

enum class NetworkRequestFailure {
    none,
    invalidOptions,
    missingURL,
    invalidURL,
    invalidMethod,
    unsupportedMethod,
    invalidHeaders,
    invalidHeader,
    invalidHeaderValue,
    invalidBody,
    bodyForMethod,
    invalidTimeout,
    timeoutRange,
    requestStart,
};

static NetworkRequestFailure StartNetworkRequest(lua_State *state, LuaPlugin *plugin)
{
    nlohmann::json options;
    if (!astral::lua::toJSON(state, 1, options) || !options.is_object()) return NetworkRequestFailure::invalidOptions;
    if (!options.contains("url") || !options["url"].is_string()) return NetworkRequestFailure::missingURL;
    std::string url = options["url"].get<std::string>();
    size_t authorityEnd = url.find_first_of("/?#", 8);
    std::string authority = authorityEnd == std::string::npos ? url.substr(8) : url.substr(8, authorityEnd - 8);
    if (url.size() > 4096 || url.rfind("https://", 0) != 0 || authority.empty() || authority.find('@') != std::string::npos ||
        url.find('\\') != std::string::npos || ContainsControl(url) || !ValidUTF8(url))
        return NetworkRequestFailure::invalidURL;
    if (options.contains("method") && !options["method"].is_string()) return NetworkRequestFailure::invalidMethod;
    std::string method = options.contains("method") ? options["method"].get<std::string>() : "GET";
    std::transform(method.begin(), method.end(), method.begin(), [](unsigned char character) { return (char)std::toupper(character); });
    static const std::set<std::string> methods{"DELETE", "GET", "HEAD", "PATCH", "POST", "PUT"};
    if (!methods.count(method)) return NetworkRequestFailure::unsupportedMethod;
    options["method"] = method;
    if (options.contains("headers")) {
        if (!options["headers"].is_object() || options["headers"].size() > 32) return NetworkRequestFailure::invalidHeaders;
        for (auto entry = options["headers"].begin(); entry != options["headers"].end(); ++entry) {
            if (!SafeHeaderName(entry.key()) || !entry.value().is_string()) return NetworkRequestFailure::invalidHeader;
            const auto &value = entry.value().get_ref<const std::string &>();
            if (!SafeHeaderValue(value) || !ValidUTF8(value)) return NetworkRequestFailure::invalidHeaderValue;
        }
    } else {
        options["headers"] = nlohmann::json::object();
    }
    if (options.contains("body")) {
        if (!options["body"].is_string() || options["body"].get_ref<const std::string &>().size() > (256u << 10) ||
            !ValidUTF8(options["body"].get_ref<const std::string &>()))
            return NetworkRequestFailure::invalidBody;
        if (method == "GET" || method == "HEAD") return NetworkRequestFailure::bodyForMethod;
    }
    int timeout = 15000;
    if (options.contains("timeout_ms")) {
        if (!options["timeout_ms"].is_number_integer()) return NetworkRequestFailure::invalidTimeout;
        const auto &value = options["timeout_ms"];
        if (value.is_number_unsigned()) {
            uint64_t timeoutValue = value.get<uint64_t>();
            if (timeoutValue < 1000 || timeoutValue > 30000) return NetworkRequestFailure::timeoutRange;
            timeout = static_cast<int>(timeoutValue);
        } else {
            int64_t timeoutValue = value.get<int64_t>();
            if (timeoutValue < 1000 || timeoutValue > 30000) return NetworkRequestFailure::timeoutRange;
            timeout = static_cast<int>(timeoutValue);
        }
    }
    if (timeout < 1000 || timeout > 30000) return NetworkRequestFailure::timeoutRange;
    options["timeout_ms"] = timeout;
    uint64_t request = gNextRequest.fetch_add(1);
    if (request == 0) request = gNextRequest.fetch_add(1);
    lua_pushvalue(state, 2);
    plugin->networkRequests[request] = luaL_ref(state, LUA_REGISTRYINDEX);
    try {
        gPlatform.networkRequest(request, options);
    } catch (...) {
        luaL_unref(state, LUA_REGISTRYINDEX, plugin->networkRequests[request]);
        plugin->networkRequests.erase(request);
        return NetworkRequestFailure::requestStart;
    }
    lua_pushinteger(state, (lua_Integer)request);
    return NetworkRequestFailure::none;
}

static const char *NetworkRequestFailureMessage(NetworkRequestFailure failure)
{
    switch (failure) {
    case NetworkRequestFailure::none:
        return "";
    case NetworkRequestFailure::invalidOptions:
        return "astral.net.request: options must be an object";
    case NetworkRequestFailure::missingURL:
        return "astral.net.request: url is required";
    case NetworkRequestFailure::invalidURL:
        return "astral.net.request: url must be an https URL without credentials";
    case NetworkRequestFailure::invalidMethod:
        return "astral.net.request: method must be a string";
    case NetworkRequestFailure::unsupportedMethod:
        return "astral.net.request: unsupported method";
    case NetworkRequestFailure::invalidHeaders:
        return "astral.net.request: headers must be an object with at most 32 entries";
    case NetworkRequestFailure::invalidHeader:
        return "astral.net.request: invalid header";
    case NetworkRequestFailure::invalidHeaderValue:
        return "astral.net.request: invalid header value";
    case NetworkRequestFailure::invalidBody:
        return "astral.net.request: body must be a string no larger than 256 KiB";
    case NetworkRequestFailure::bodyForMethod:
        return "astral.net.request: GET and HEAD requests cannot have a body";
    case NetworkRequestFailure::invalidTimeout:
        return "astral.net.request: timeout_ms must be an integer";
    case NetworkRequestFailure::timeoutRange:
        return "astral.net.request: timeout_ms must be between 1000 and 30000";
    case NetworkRequestFailure::requestStart:
        return "astral.net.request: could not start the request";
    }
    return "astral.net.request: request failed";
}

static int NetworkRequest(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "network");
    if (!gPlatform.networkRequest) return luaL_error(state, "astral.net is unavailable on this client");
    luaL_checktype(state, 1, LUA_TTABLE);
    luaL_checktype(state, 2, LUA_TFUNCTION);
    NetworkRequestFailure failure = StartNetworkRequest(state, plugin);
    if (failure != NetworkRequestFailure::none) return luaL_error(state, "%s", NetworkRequestFailureMessage(failure));
    return 1;
}

static int CancelNetworkRequest(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "network");
    lua_Integer value = luaL_checkinteger(state, 1);
    if (value <= 0) {
        lua_pushboolean(state, false);
        return 1;
    }
    uint64_t request = (uint64_t)value;
    auto found = plugin->networkRequests.find(request);
    if (found == plugin->networkRequests.end()) {
        lua_pushboolean(state, false);
        return 1;
    }
    luaL_unref(state, LUA_REGISTRYINDEX, found->second);
    plugin->networkRequests.erase(found);
    if (gPlatform.cancelNetworkRequest) {
        try {
            gPlatform.cancelNetworkRequest(request);
        } catch (...) {
        }
    }
    lua_pushboolean(state, true);
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
    Function(state, plugin, "text", SettingsText);
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
    Function(state, plugin, "alert", ShowAlert);
    Function(state, plugin, "copy", CopyText);
    Function(state, plugin, "haptic", Haptic);
    Function(state, plugin, "settings_backdrop", SettingsBackdrop);
    Function(state, plugin, "mention", Mention);
    Function(state, plugin, "message_color", MessageColor);
    Function(state, plugin, "name_color", NameColor);
    Function(state, plugin, "typing", TypingDecorator);
    Function(state, plugin, "reaction", ReactionDecorator);
    lua_setfield(state, -2, "ui");

    lua_newtable(state);
    Function(state, plugin, "pick", PickFile);
    Function(state, plugin, "cancel", CancelFilePick);
    lua_setfield(state, -2, "files");

    lua_newtable(state);
    Function(state, plugin, "on", SubscribeEvent);
    Function(state, plugin, "off", UnsubscribeEvent);
    lua_setfield(state, -2, "events");

    lua_newtable(state);
    Function(state, plugin, "transform", RegisterOutgoingMessageTransform);
    lua_setfield(state, -2, "messages");

    lua_newtable(state);
    Function(state, plugin, "request", NetworkRequest);
    Function(state, plugin, "cancel", CancelNetworkRequest);
    lua_setfield(state, -2, "net");

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
    Function(state, plugin, "reactors", Reactors);
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
    auto requests = std::move(plugin->networkRequests);
    plugin->networkRequests.clear();
    for (const auto &[request, reference] : requests) {
        if (gPlatform.cancelNetworkRequest) {
            try {
                gPlatform.cancelNetworkRequest(request);
            } catch (...) {
            }
        }
        if (plugin->state) luaL_unref(plugin->state, LUA_REGISTRYINDEX, reference);
    }
    auto filePicks = std::move(plugin->filePicks);
    plugin->filePicks.clear();
    for (const auto &[request, reference] : filePicks) {
        if (gPlatform.cancelFilePick) {
            try {
                gPlatform.cancelFilePick(request);
            } catch (...) {
            }
        }
        if (plugin->state) luaL_unref(plugin->state, LUA_REGISTRYINDEX, reference);
    }
    auto eventCallbacks = std::move(plugin->eventCallbacks);
    plugin->eventCallbacks.clear();
    for (const auto &[subscription, callback] : eventCallbacks) {
        if (plugin->state) luaL_unref(plugin->state, LUA_REGISTRYINDEX, callback.reference);
    }
    auto outgoingMessageTransforms = std::move(plugin->outgoingMessageTransforms);
    plugin->outgoingMessageTransforms.clear();
    for (int reference : outgoingMessageTransforms) {
        if (plugin->state) luaL_unref(plugin->state, LUA_REGISTRYINDEX, reference);
    }
    plugin->sandbox.reset();
    plugin->state = nullptr;
    plugin->switches.clear();
    plugin->textSettings.clear();
    plugin->decorators.clear();
    plugin->relabelers.clear();
    plugin->settingsBackdrop.reset();
    plugin->mentioners.clear();
    plugin->colorizers.clear();
    plugin->nameColorizers.clear();
    plugin->typingDecorators.clear();
    plugin->reactionDecorators.clear();
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
    return gPlatform.preferredLanguages;
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

static LuaPlugin *Load(const std::string &directory)
{
    auto package = astral::loadPluginPackage(directory, PreferredLanguages(), {kSourceLimit, kModulesLimit});
    if (!package) return nullptr;
    LuaPlugin *plugin = new LuaPlugin();
    static_cast<astral::PluginPackage &>(*plugin) = std::move(*package);
    plugin->signedByAstral = gPlatform.verifySignature && gPlatform.verifySignature(directory, plugin->main);
    return plugin;
}

static std::vector<std::string> SortedEntries(const std::string &root)
{
    std::vector<std::string> entries;
    std::error_code error;
    for (const auto &entry : std::filesystem::directory_iterator(root, error)) entries.push_back(entry.path().filename().string());
    std::sort(entries.begin(), entries.end());
    return entries;
}

static void Scan(void)
{
    for (const auto &root : gDirectories) {
        bool bundled = !gPlatform.bundledDirectory.empty() && root.rfind(gPlatform.bundledDirectory, 0) == 0;
        for (const auto &entry : SortedEntries(root)) {
            LuaPlugin *plugin = Load(root + "/" + entry);
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

namespace astral::luahost {

void start(const std::vector<std::string> &directories, SettingsStore *store, bool startEnabled, LuaHostPlatform platform)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (gStore) return;
    gStore = store;
    gDirectories = directories;
    gStartEnabled = startEnabled;
    gPlatform = std::move(platform);
    Scan();
}

void rescan()
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (gStore) Scan();
}

void stop()
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto *plugin : gPlugins) Stop(plugin);
    for (auto *plugin : gPlugins) delete plugin;
    gPlugins.clear();
    gStore = nullptr;
    gDirectories.clear();
    gPlatform = {};
    gStartEnabled = false;
}

void removeStorage(const std::string &identifier)
{
    Storage().removeAll(identifier);
}

nlohmann::json plugins()
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    nlohmann::json plugins = nlohmann::json::array();
    for (auto *plugin : gPlugins) {
        nlohmann::json switches = nlohmann::json::array();
        for (auto &entry : plugin->switches) {
            nlohmann::json options = nlohmann::json::array();
            for (auto &option : entry.options)
                options.push_back({{"value", option.value}, {"title", option.title}, {"subtitle", option.subtitle}});
            nlohmann::json value =
                entry.number ? nlohmann::json(SteppedNumber(entry, StoredNumber(SettingKey(plugin, entry.key), entry.fallbackNumber)))
                             : nlohmann::json(StoredString(SettingKey(plugin, entry.key), entry.fallbackChoice));
            switches.push_back({{"key", entry.key},
                                {"title", entry.title},
                                {"subtitle", entry.subtitle},
                                {"section", entry.section},
                                {"on", StoredBool(SettingKey(plugin, entry.key), entry.fallback)},
                                {"options", options},
                                {"value", value},
                                {"number", entry.number},
                                {"min", entry.minimum},
                                {"max", entry.maximum},
                                {"step", entry.step}});
        }
        nlohmann::json textSettings = nlohmann::json::array();
        for (const auto &entry : plugin->textSettings) {
            std::string value = StoredString(SettingKey(plugin, entry.key), entry.fallback);
            if (value.size() > entry.maximumLength) value = entry.fallback;
            textSettings.push_back({{"key", entry.key},
                                    {"title", entry.title},
                                    {"subtitle", entry.subtitle},
                                    {"section", entry.section},
                                    {"value", value},
                                    {"maxLength", entry.maximumLength}});
        }
        nlohmann::json authors = nlohmann::json::array();
        for (auto &author : plugin->authors) authors.push_back({{"name", author.first}, {"id", author.second}});
        nlohmann::json permissions = nlohmann::json::array();
        for (auto &permission : plugin->permissions) permissions.push_back(permission);
        plugins.push_back({{"permissions", permissions},
                           {"id", plugin->identifier},
                           {"name", plugin->name},
                           {"version", plugin->version},
                           {"enabled", plugin->enabled},
                           {"running", plugin->state != nullptr},
                           {"failed", plugin->failed},
                           {"error", plugin->lastError},
                           {"switches", switches},
                           {"textSettings", textSettings},
                           {"description", plugin->description},
                           {"icon", plugin->icon},
                           {"iconSvg", plugin->iconSvg},
                           {"authors", authors},
                           {"directory", plugin->directory},
                           {"bundled", plugin->bundled},
                           {"signed", plugin->bundled || plugin->signedByAstral}});
    }
    return plugins;
}

} // namespace astral::luahost

static LuaPlugin *Find(const std::string &pluginID)
{
    for (auto *plugin : gPlugins)
        if (plugin->identifier == pluginID) return plugin;
    return nullptr;
}

namespace astral::luahost {

void setEnabled(const std::string &pluginID, bool enabled)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    LuaPlugin *plugin = Find(pluginID);
    if (!plugin) return;
    plugin->enabled = enabled;
    Store(plugin->identifier + ".enabled", enabled);
    if (enabled)
        Start(plugin);
    else
        Stop(plugin);
    RefreshDecorations();
}

void resetSettings(const std::string &pluginID)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    LuaPlugin *plugin = Find(pluginID);
    if (!plugin) return;
    for (auto &entry : plugin->switches) gStore->remove("plugins", SettingKey(plugin, entry.key));
    for (auto &entry : plugin->textSettings) gStore->remove("plugins", SettingKey(plugin, entry.key));
    gStore->save();
    if (plugin->state) Start(plugin);
}

void completeNetworkRequest(uint64_t request, const nlohmann::json &result)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto *plugin : gPlugins) {
        auto found = plugin->networkRequests.find(request);
        if (found == plugin->networkRequests.end()) continue;
        int reference = found->second;
        plugin->networkRequests.erase(found);
        if (!plugin->state || plugin->failed) return;
        nlohmann::json response = nlohmann::json::object();
        int status = 0;
        if (result.is_object() && result.contains("status") && result["status"].is_number_unsigned())
            status = static_cast<int>(std::min<uint64_t>(result["status"].get<uint64_t>(), 999));
        else if (result.is_object() && result.contains("status") && result["status"].is_number_integer())
            status = static_cast<int>(std::clamp<int64_t>(result["status"].get<int64_t>(), 0, 999));
        response["status"] = status;
        response["ok"] = result.is_object() && result.contains("ok") && result["ok"].is_boolean() && result["ok"].get<bool>();
        response["url"] = result.is_object() && result.contains("url") && result["url"].is_string()
                              ? result["url"].get<std::string>().substr(0, 4096)
                              : "";
        response["body"] = result.is_object() && result.contains("body") && result["body"].is_string()
                               ? result["body"].get<std::string>().substr(0, (1u << 20) + 1)
                               : "";
        nlohmann::json headers = nlohmann::json::object();
        if (result.is_object() && result.contains("headers") && result["headers"].is_object()) {
            size_t count = 0;
            for (auto entry = result["headers"].begin(); entry != result["headers"].end() && count < 64; ++entry) {
                if (SafeHeaderName(entry.key()) && entry.value().is_string() && SafeHeaderValue(entry.value().get<std::string>())) {
                    headers[entry.key()] = entry.value().get<std::string>().substr(0, 8192);
                    count++;
                }
            }
        }
        response["headers"] = std::move(headers);
        if (result.is_object() && result.contains("error") && result["error"].is_string())
            response["error"] = result["error"].get<std::string>().substr(0, 512);
        if (response["body"].get_ref<const std::string &>().size() > (1u << 20)) {
            response["body"] = "";
            response["ok"] = false;
            response["error"] = "response body exceeds 1 MiB";
        }
        lua_State *state = plugin->state;
        lua_rawgeti(state, LUA_REGISTRYINDEX, reference);
        luaL_unref(state, LUA_REGISTRYINDEX, reference);
        astral::lua::pushJSON(state, response);
        if (!Call(plugin, 1, 0, "network callback")) {
            MarkFailed(plugin);
            Stop(plugin);
        }
        return;
    }
}

void completeFilePick(uint64_t request, const std::optional<std::string> &name, const std::optional<std::string> &data,
                      const std::optional<std::string> &error)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto *plugin : gPlugins) {
        auto found = plugin->filePicks.find(request);
        if (found == plugin->filePicks.end()) continue;
        int reference = found->second;
        plugin->filePicks.erase(found);
        if (!plugin->state || plugin->failed) return;
        lua_State *state = plugin->state;
        lua_rawgeti(state, LUA_REGISTRYINDEX, reference);
        luaL_unref(state, LUA_REGISTRYINDEX, reference);
        if (name && data && data->size() <= (1u << 20)) {
            lua_createtable(state, 0, 2);
            lua_pushlstring(state, name->data(), name->size());
            lua_setfield(state, -2, "name");
            lua_pushlstring(state, data->data(), data->size());
            lua_setfield(state, -2, "data");
        } else {
            lua_pushnil(state);
            std::string reason = error.value_or(data && data->size() > (1u << 20) ? "file exceeds 1 MiB" : "cancelled");
            lua_pushlstring(state, reason.data(), std::min<size_t>(reason.size(), 256));
        }
        if (!Call(plugin, name && data && data->size() <= (1u << 20) ? 1 : 2, 0, "file picker callback")) {
            MarkFailed(plugin);
            Stop(plugin);
        }
        return;
    }
}

void emitEvent(const std::string &event)
{
    if (event != "app.foreground" && event != "app.background") return;
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        std::vector<uint64_t> subscriptions;
        for (const auto &[subscription, callback] : plugin->eventCallbacks)
            if (callback.event == event) subscriptions.push_back(subscription);
        for (uint64_t subscription : subscriptions) {
            auto found = plugin->eventCallbacks.find(subscription);
            if (found == plugin->eventCallbacks.end() || !plugin->state || plugin->failed) continue;
            int reference = found->second.reference;
            lua_State *state = plugin->state;
            lua_rawgeti(state, LUA_REGISTRYINDEX, reference);
            lua_pushlstring(state, event.data(), event.size());
            if (!Call(plugin, 1, 0, event.c_str())) {
                MarkFailed(plugin);
                Stop(plugin);
                break;
            }
        }
    }
}

static bool IsSnowflake(const std::string &value)
{
    return value.size() >= 15 && value.size() <= 20 &&
           std::all_of(value.begin(), value.end(), [](unsigned char character) { return character >= '0' && character <= '9'; });
}

static void EmitMessageCreate(const std::string &messageID, const std::string &channelID, const std::string &authorID, bool fromMe,
                              bool mentioned, bool direct, bool open)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        std::vector<uint64_t> subscriptions;
        for (const auto &[subscription, callback] : plugin->eventCallbacks)
            if (callback.event == "message.create") subscriptions.push_back(subscription);
        for (uint64_t subscription : subscriptions) {
            auto found = plugin->eventCallbacks.find(subscription);
            if (found == plugin->eventCallbacks.end() || !plugin->state || plugin->failed) continue;
            lua_State *state = plugin->state;
            lua_rawgeti(state, LUA_REGISTRYINDEX, found->second.reference);
            lua_createtable(state, 0, 7);
            lua_pushlstring(state, messageID.data(), messageID.size());
            lua_setfield(state, -2, "message_id");
            lua_pushlstring(state, channelID.data(), channelID.size());
            lua_setfield(state, -2, "channel_id");
            lua_pushlstring(state, authorID.data(), authorID.size());
            lua_setfield(state, -2, "author_id");
            lua_pushboolean(state, fromMe);
            lua_setfield(state, -2, "from_me");
            lua_pushboolean(state, mentioned);
            lua_setfield(state, -2, "mentioned");
            lua_pushboolean(state, direct);
            lua_setfield(state, -2, "direct");
            lua_pushboolean(state, open);
            lua_setfield(state, -2, "open");
            if (!Call(plugin, 1, 0, "message.create")) {
                MarkFailed(plugin);
                Stop(plugin);
                break;
            }
        }
    }
}

void observeGatewayMessage(std::string_view message)
{
    if (message.empty() || message.size() > (1u << 20)) return;
    nlohmann::json envelope = nlohmann::json::parse(message, nullptr, false);
    if (!envelope.is_object()) return;
    auto event = envelope.find("t");
    auto data = envelope.find("d");
    if (event == envelope.end() || !event->is_string() || data == envelope.end() || !data->is_object()) return;
    std::string type = event->get<std::string>();
    if (type == "READY") {
        auto user = data->find("user");
        if (user == data->end() || !user->is_object()) return;
        auto id = user->find("id");
        if (id == user->end() || !id->is_string()) return;
        std::string currentUser = id->get<std::string>();
        if (!IsSnowflake(currentUser)) return;
        std::lock_guard<std::recursive_mutex> lock(gMutex);
        gCurrentUserID = std::move(currentUser);
        return;
    }
    if (type != "MESSAGE_CREATE") return;
    auto channel = data->find("channel_id");
    auto author = data->find("author");
    if (channel == data->end() || !channel->is_string() || author == data->end() || !author->is_object()) return;
    auto authorID = author->find("id");
    if (authorID == author->end() || !authorID->is_string()) return;
    std::string channelID = channel->get<std::string>();
    std::string senderID = authorID->get<std::string>();
    if (!IsSnowflake(channelID) || !IsSnowflake(senderID)) return;
    std::string messageID;
    auto id = data->find("id");
    if (id != data->end() && id->is_string() && IsSnowflake(id->get<std::string>())) messageID = id->get<std::string>();
    std::string currentUser;
    {
        std::lock_guard<std::recursive_mutex> lock(gMutex);
        currentUser = gCurrentUserID;
    }
    bool mentioned = false;
    if (!currentUser.empty()) {
        auto mentions = data->find("mentions");
        if (mentions != data->end() && mentions->is_array()) {
            for (const auto &mention : *mentions) {
                if (mention.is_object() && mention.value("id", std::string()) == currentUser) {
                    mentioned = true;
                    break;
                }
            }
        }
    }
    auto guild = data->find("guild_id");
    bool direct = guild == data->end() || !guild->is_string();
    uint64_t selectedChannel = astral_wire_current_channel();
    bool open = selectedChannel != 0 && std::to_string(selectedChannel) == channelID;
    EmitMessageCreate(messageID, channelID, senderID, senderID == currentUser, mentioned, direct, open);
}

bool hasOutgoingMessageTransforms()
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto *plugin : gPlugins)
        if (plugin->state && !plugin->failed && !plugin->outgoingMessageTransforms.empty()) return true;
    return false;
}

std::optional<std::string> transformOutgoingMessage(const std::string &channel, const std::string &operation, const std::string &content)
{
    if (content.size() > 20000) return std::nullopt;
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    std::string transformed = content;
    bool changed = false;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed || plugin->outgoingMessageTransforms.empty()) continue;
        std::vector<int> references = plugin->outgoingMessageTransforms;
        lua_State *state = plugin->state;
        for (int reference : references) {
            if (!plugin->state || plugin->failed) break;
            int top = lua_gettop(state);
            lua_rawgeti(state, LUA_REGISTRYINDEX, reference);
            lua_pushlstring(state, transformed.data(), transformed.size());
            lua_createtable(state, 0, 2);
            lua_pushlstring(state, channel.data(), channel.size());
            lua_setfield(state, -2, "channel_id");
            lua_pushlstring(state, operation.data(), operation.size());
            lua_setfield(state, -2, "operation");
            if (!Call(plugin, 2, 1, "outgoing message transform")) {
                lua_settop(state, top);
                MarkFailed(plugin);
                Stop(plugin);
                break;
            }
            if (lua_type(state, -1) == LUA_TSTRING) {
                size_t length = 0;
                const char *value = lua_tolstring(state, -1, &length);
                if (value && length <= 20000) {
                    std::string replacement(value, length);
                    changed = changed || replacement != transformed;
                    transformed = std::move(replacement);
                }
            }
            lua_settop(state, top);
        }
    }
    if (!changed) return std::nullopt;
    try {
        nlohmann::json validation = transformed;
        validation.dump();
    } catch (const nlohmann::json::exception &) {
        return std::nullopt;
    }
    return transformed;
}

std::string eval(const std::string &pluginID, const std::string &source)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    LuaPlugin *plugin = Find(pluginID);
    if (!plugin) return "error: no such plugin";
    if (!plugin->state) return "error: the plugin is not running";
    lua_State *state = plugin->state;
    int top = lua_gettop(state);
    if (plugin->sandbox->load("return " + source, "eval")) {
        if (auto error = plugin->sandbox->load(source, "eval")) {
            lua_settop(state, top);
            return "error: " + *error;
        }
    }
    if (auto error = plugin->sandbox->call(0, LUA_MULTRET)) {
        lua_settop(state, top);
        return "error: " + *error;
    }
    std::string joined;
    for (int index = top + 1; index <= lua_gettop(state); index++) {
        if (!joined.empty() || index > top + 1) joined += "\t";
        joined += luaL_tolstring(state, index, nullptr);
        lua_pop(state, 1);
    }
    lua_settop(state, top);
    return joined;
}

} // namespace astral::luahost

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
        DeveloperLog(plugin->identifier + ": astral.state holds values that cannot be kept; reloading without it");
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

namespace astral::luahost {

void remove(const std::string &pluginID)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto iterator = gPlugins.begin(); iterator != gPlugins.end(); ++iterator) {
        if ((*iterator)->identifier != pluginID) continue;
        Stop(*iterator);
        delete *iterator;
        gPlugins.erase(iterator);
        RefreshDecorations();
        return;
    }
}

void reload(const std::string &pluginID)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    std::optional<nlohmann::json> kept = KeptState(Find(pluginID));
    remove(pluginID);
    rescan();
    setEnabled(pluginID, true);
    LuaPlugin *plugin = Find(pluginID);
    if (plugin && kept) RestoreState(plugin, *kept);
}

} // namespace astral::luahost

static void NotifySetting(LuaPlugin *plugin, const std::string &key, const std::function<void(lua_State *)> &pushValue)
{
    if (!plugin->state) return;
    lua_getglobal(plugin->state, "astral");
    lua_getfield(plugin->state, -1, "on_setting");
    lua_remove(plugin->state, -2);
    if (lua_isfunction(plugin->state, -1)) {
        lua_pushstring(plugin->state, key.c_str());
        pushValue(plugin->state);
        Call(plugin, 2, 0, "on_setting");
    } else {
        lua_pop(plugin->state, 1);
    }
}

namespace astral::luahost {

void setChoice(const std::string &pluginID, const std::string &key, const std::string &value)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    LuaPlugin *plugin = Find(pluginID);
    if (!plugin) return;
    Store(SettingKey(plugin, key), value);
    NotifySetting(plugin, key, [&](lua_State *state) { lua_pushstring(state, value.c_str()); });
}

void setNumber(const std::string &pluginID, const std::string &key, double value)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    LuaPlugin *plugin = Find(pluginID);
    if (!plugin) return;
    const LuaSwitch *entry = nullptr;
    for (auto &candidate : plugin->switches) {
        if (candidate.number && candidate.key == key) entry = &candidate;
    }
    if (!entry) return;
    double stepped = SteppedNumber(*entry, value);
    if (StoredNumber(SettingKey(plugin, key), entry->fallbackNumber) == stepped) return;
    Store(SettingKey(plugin, key), stepped);
    NotifySetting(plugin, key, [&](lua_State *state) { PushNumber(state, stepped); });
}

void setSwitch(const std::string &pluginID, const std::string &key, bool on)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    LuaPlugin *plugin = Find(pluginID);
    if (!plugin) return;
    Store(SettingKey(plugin, key), on);
    NotifySetting(plugin, key, [&](lua_State *state) { lua_pushboolean(state, on); });
}

void setText(const std::string &pluginID, const std::string &key, const std::string &value)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    LuaPlugin *plugin = Find(pluginID);
    if (!plugin || !plugin->state) return;
    const LuaTextSetting *setting = nullptr;
    for (const auto &candidate : plugin->textSettings)
        if (candidate.key == key) setting = &candidate;
    if (!setting || value.size() > setting->maximumLength) return;
    if (StoredString(SettingKey(plugin, key), setting->fallback) == value) return;
    Store(SettingKey(plugin, key), value);
    NotifySetting(plugin, key, [&](lua_State *state) { lua_pushlstring(state, value.data(), value.size()); });
}

} // namespace astral::luahost

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
        auto found = gPlatform.messageChannel ? gPlatform.messageChannel(message) : std::nullopt;
        if (found) lastChannel = *found;
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
    auto guild = channel.empty() || !gPlatform.channelGuild ? std::nullopt : gPlatform.channelGuild(channel);
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
                lua_pushstring(state, guild->c_str());
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
    auto guild = channel.empty() || !gPlatform.channelGuild ? std::nullopt : gPlatform.channelGuild(channel);
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
                lua_pushstring(state, guild->c_str());
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
    if (channel.empty() || !gPlatform.message || !gPlatform.message(channel, message)) return -1;
    auto guild = gPlatform.channelGuild ? gPlatform.channelGuild(channel) : std::nullopt;
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
                lua_pushstring(state, guild->c_str());
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

int AstralLuaTyping(const AstralNameContext *context, AstralMentionDecoration *decoration)
{
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) return 0;
    bool wanted = false;
    for (auto *plugin : gPlugins) wanted = wanted || (plugin->state && !plugin->failed && !plugin->typingDecorators.empty());
    if (!wanted) return 0;
    std::string channel = Snowflake(context->channel_id);
    auto guild = channel.empty() || !gPlatform.channelGuild ? std::nullopt : gPlatform.channelGuild(channel);
    auto user = guild && gPlatform.memberNamed ? gPlatform.memberNamed(*guild, context->text) : std::nullopt;
    if (!user) return 0;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (int decorator : plugin->typingDecorators) {
            lua_State *state = plugin->state;
            lua_rawgeti(state, LUA_REGISTRYINDEX, decorator);
            lua_createtable(state, 0, 4);
            lua_pushstring(state, context->text);
            lua_setfield(state, -2, "text");
            lua_pushstring(state, user->c_str());
            lua_setfield(state, -2, "user_id");
            lua_pushstring(state, channel.c_str());
            lua_setfield(state, -2, "channel_id");
            lua_pushstring(state, guild->c_str());
            lua_setfield(state, -2, "guild_id");
            if (Call(plugin, 1, 1, "typing")) {
                if (lua_istable(state, -1)) {
                    lua_getfield(state, -1, "image");
                    lua_getfield(state, -2, "round");
                    const char *image = lua_tostring(state, -2);
                    if (image && strncmp(image, "https://", 8) == 0) {
                        snprintf(decoration->image, sizeof(decoration->image), "%s", image);
                        decoration->round = lua_isnil(state, -1) ? 1 : lua_toboolean(state, -1);
                    }
                    lua_pop(state, 2);
                }
                lua_pop(state, 1);
                if (decoration->image[0]) return 1;
            }
            if (plugin->failed) break;
        }
        if (plugin->failed) Stop(plugin);
    }
    return 0;
}

static uint32_t ReadReactionImages(lua_State *state, AstralReactionDecoration *decoration)
{
    lua_getfield(state, -1, "images");
    if (lua_istable(state, -1)) {
        lua_Integer length = (lua_Integer)lua_rawlen(state, -1);
        for (lua_Integer index = 1; index <= length && decoration->image_count < kAstralReactionFaces; index++) {
            lua_rawgeti(state, -1, index);
            const char *image = lua_tostring(state, -1);
            if (image && strncmp(image, "https://", 8) == 0)
                snprintf(decoration->images[decoration->image_count++], sizeof(decoration->images[0]), "%s", image);
            lua_pop(state, 1);
        }
    }
    lua_pop(state, 1);
    lua_getfield(state, -1, "total");
    if (lua_isinteger(state, -1)) decoration->total = (uint32_t)std::max<lua_Integer>(0, lua_tointeger(state, -1));
    lua_pop(state, 1);
    return decoration->image_count;
}

int AstralLuaReaction(const AstralReactionContext *context, AstralReactionDecoration *decoration)
{
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) return 0;
    std::string channel = Snowflake(context->channel_id);
    std::string message = Snowflake(context->message_id);
    if (channel.empty() || message.empty()) return 0;
    auto guild = gPlatform.channelGuild ? gPlatform.channelGuild(channel) : std::nullopt;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (int decorator : plugin->reactionDecorators) {
            lua_State *state = plugin->state;
            gReactorsStale = false;
            lua_rawgeti(state, LUA_REGISTRYINDEX, decorator);
            lua_createtable(state, 0, 6);
            lua_pushstring(state, channel.c_str());
            lua_setfield(state, -2, "channel_id");
            lua_pushstring(state, message.c_str());
            lua_setfield(state, -2, "message_id");
            if (guild) {
                lua_pushstring(state, guild->c_str());
                lua_setfield(state, -2, "guild_id");
            }
            lua_pushstring(state, context->emoji);
            lua_setfield(state, -2, "emoji");
            lua_pushboolean(state, context->burst);
            lua_setfield(state, -2, "burst");
            lua_pushinteger(state, context->count);
            lua_setfield(state, -2, "count");
            if (Call(plugin, 1, 1, "reaction")) {
                if (lua_istable(state, -1)) ReadReactionImages(state, decoration);
                lua_pop(state, 1);
                decoration->stale = gReactorsStale;
                if (decoration->image_count) return 1;
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
    auto guild = channel.empty() || !gPlatform.channelGuild ? std::nullopt : gPlatform.channelGuild(channel);
    if (!guild) return 0;
    std::string user = Snowflake(context->user_id);
    if (user.empty() && context->kind == kAstralNameTyping) {
        auto named = gPlatform.memberNamed ? gPlatform.memberNamed(*guild, context->text) : std::nullopt;
        if (!named) return 0;
        user = *named;
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
            lua_pushstring(state, guild->c_str());
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
    auto guild = channel.empty() || !gPlatform.channelGuild ? std::nullopt : gPlatform.channelGuild(channel);
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
                lua_pushstring(state, guild->c_str());
                lua_setfield(state, -2, "guild_id");
            }
            if (Call(plugin, 1, 1, "decorate")) CollectTags(plugin, tags, capacity, &count);
            if (plugin->failed) break;
        }
        if (plugin->failed) Stop(plugin);
    }
    return count;
}

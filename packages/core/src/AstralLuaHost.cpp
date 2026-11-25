#include "AstralLuaHost.h"
#include "AstralPluginTrace.h"
#include "AstralSwatch.h"

#include <algorithm>
#include <atomic>
#include <cmath>
#include <cctype>
#include <charconv>
#include <chrono>
#include <condition_variable>
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
#include <thread>
#include <vector>

#include "AstralLog.h"
#include "AstralNetworkPolicy.h"
#include "PhoenixProperties.h"
#include "AstralJSON.h"
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
static const size_t kMaximumChoiceOptions = 64;

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
    std::string icon = {};
    size_t order = 0;
};

struct LuaTextSetting {
    std::string key;
    std::string title;
    std::string subtitle;
    std::string section;
    std::string fallback;
    size_t maximumLength = 0;
    bool color = false;
    std::string icon = {};
    size_t order = 0;
};

struct LuaSettingsButton {
    std::string key;
    std::string title;
    std::string subtitle;
    std::string section;
    std::string icon;
    std::string detail;
    size_t order = 0;
    int reference = LUA_NOREF;
};

struct LuaNodePatch {
    std::string prefix;
    std::string type;
    int reference;
};

struct LuaNodeInsert {
    uint64_t identifier;
    std::string prefix;
    std::string type;
    int build;
    int press;
};

struct LuaEventCallback {
    std::string event;
    int reference;
    std::set<std::string> types;
};

struct LuaMessageAction {
    uint64_t identifier;
    std::string label;
    std::string icon = {};
    int reference;
    int when = LUA_NOREF;
};

struct LuaHttpHook {
    std::string method;
    std::string pattern;
    int request = LUA_NOREF;
    int response = LUA_NOREF;
};

struct LuaCommand {
    std::string name;
    std::string description;
    nlohmann::json options = nlohmann::json::array();
    int reference = LUA_NOREF;
};

struct LuaTimer {
    int reference = LUA_NOREF;
    int64_t interval = 0;
};

struct LuaPlugin : astral::PluginPackage, astral::PluginHealth {
    bool bundled = false;
    bool signedByAstral = false;
    bool enabled = false;
    std::unique_ptr<astral::lua::Sandbox> sandbox;
    lua_State *state = nullptr;
    std::vector<LuaSwitch> switches;
    std::vector<LuaSettingsButton> settingsButtons;
    std::vector<LuaTextSetting> textSettings;
    size_t settingCount = 0;
    std::vector<std::pair<std::string, int>> decorators;
    std::vector<std::pair<std::string, int>> relabelers;
    std::optional<double> settingsBackdrop;
    std::vector<int> mentioners;
    std::vector<int> colorizers;
    std::vector<int> nameColorizers;
    std::vector<int> typingDecorators;
    std::vector<int> reactionDecorators;
    std::vector<int> outgoingMessageTransforms;
    std::vector<int> mediaSaveHandlers;
    std::vector<int> composerDecorators;
    std::vector<int> messageBackgroundDecorators;
    std::vector<LuaNodePatch> nodePatches;
    std::vector<LuaNodeInsert> nodeInserts;
    std::vector<LuaMessageAction> messageActions;
    std::map<uint64_t, int> networkRequests;
    std::map<uint64_t, LuaTimer> timers;
    std::map<uint64_t, int> webAuthRequests;
    std::map<uint64_t, int> confirmRequests;
    std::map<uint64_t, int> sheets;
    std::map<uint64_t, int> filePicks;
    std::map<uint64_t, LuaEventCallback> eventCallbacks;
    std::vector<LuaHttpHook> httpHooks;
    std::vector<std::pair<std::string, int>> imageRewriters;
    std::vector<int> textTransforms;
    std::vector<LuaCommand> commands;
};

static int Translate(lua_State *state);
static void RequirePermission(lua_State *state, const char *permission);
static bool ContainsControl(const std::string &value);
static bool ValidUTF8(const std::string &value);

static std::recursive_mutex gMutex;
static std::vector<LuaPlugin *> gPlugins;
static astral::SettingsStore *gStore;
static std::vector<std::string> gDirectories;
static bool gStartEnabled;
static astral::LuaHostPlatform gPlatform;
static std::atomic<uint64_t> gNextRequest{1};
static std::atomic<uint64_t> gNextMessageAction{1};
static uint64_t gMenuChannel;
struct DisplayedText {
    std::string text;
    std::string note;
};
static std::map<uint64_t, DisplayedText> gDisplayedTexts;
static constexpr size_t kMaxDisplayedTexts = 512;
static constexpr size_t kMaxDisplayedText = 8000;
static uint64_t gMenuMessage;
static std::string gCurrentUserID;

static void DeveloperLog(const std::string &line)
{
    if (gPlatform.developerLog) gPlatform.developerLog(line);
}

static void RefreshDecorations(void)
{
    if (gPlatform.refreshDecorations) gPlatform.refreshDecorations();
}

static std::atomic<bool> gRedrawAfterBusy{false};

static void RedrawAfterBusy(void)
{
    if (gRedrawAfterBusy.exchange(true)) return;
    std::thread([] {
        std::this_thread::sleep_for(std::chrono::milliseconds(250));
        gRedrawAfterBusy = false;
        RefreshDecorations();
    }).detach();
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
    astral::trace::enter(plugin->identifier);
    auto error = plugin->sandbox->call(arguments, results);
    astral::trace::leave();
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

constexpr size_t kMaxTimersPerPlugin = 64;
constexpr int64_t kMaxTimerDelay = 24 * 60 * 60 * 1000;
constexpr int64_t kMinTimerInterval = 16;

static std::mutex gTimerLock;
static std::condition_variable gTimerWake;
static std::multimap<std::chrono::steady_clock::time_point, uint64_t> gTimerQueue;
static std::once_flag gTimerThread;

static void FireTimer(uint64_t identifier);

static void TimerLoop()
{
    std::unique_lock<std::mutex> lock(gTimerLock);
    for (;;) {
        if (gTimerQueue.empty()) {
            gTimerWake.wait(lock);
            continue;
        }
        auto next = gTimerQueue.begin()->first;
        if (std::chrono::steady_clock::now() < next) {
            gTimerWake.wait_until(lock, next);
            continue;
        }
        std::vector<uint64_t> due;
        auto now = std::chrono::steady_clock::now();
        while (!gTimerQueue.empty() && gTimerQueue.begin()->first <= now) {
            due.push_back(gTimerQueue.begin()->second);
            gTimerQueue.erase(gTimerQueue.begin());
        }
        lock.unlock();
        for (uint64_t identifier : due) FireTimer(identifier);
        lock.lock();
    }
}

static void ScheduleTimer(uint64_t identifier, int64_t delay)
{
    std::call_once(gTimerThread, [] { std::thread(TimerLoop).detach(); });
    std::lock_guard<std::mutex> lock(gTimerLock);
    gTimerQueue.emplace(std::chrono::steady_clock::now() + std::chrono::milliseconds(delay), identifier);
    gTimerWake.notify_one();
}

struct HttpHold {
    std::mutex mutex;
    std::condition_variable ready;
    bool done = false;
    LuaPlugin *owner = nullptr;
    std::optional<astral::luahost::HttpResponse> response;
    std::optional<nlohmann::json> changes;
};

static std::mutex gHoldMutex;
static std::map<uint64_t, std::shared_ptr<HttpHold>> gHttpHolds;
static constexpr const char *kHoldRequest = "hold";
static constexpr auto kMaximumHold = std::chrono::minutes(10);
static constexpr size_t kMaximumHttpBody = 4u << 20;

static bool HiddenHeader(const std::string &name)
{
    return name == "authorization" || name == "cookie" || name == "set-cookie";
}

static std::string LowerCase(std::string value)
{
    for (char &character : value) character = static_cast<char>(tolower(static_cast<unsigned char>(character)));
    return value;
}

static void FinishHold(uint64_t exchange, std::optional<astral::luahost::HttpResponse> response, std::optional<nlohmann::json> changes)
{
    std::shared_ptr<HttpHold> hold;
    {
        std::lock_guard<std::mutex> lock(gHoldMutex);
        auto found = gHttpHolds.find(exchange);
        if (found == gHttpHolds.end()) return;
        hold = found->second;
        gHttpHolds.erase(found);
    }
    std::lock_guard<std::mutex> lock(hold->mutex);
    hold->response = std::move(response);
    hold->changes = std::move(changes);
    hold->done = true;
    hold->ready.notify_all();
}

static void ReleaseHttpHooks(LuaPlugin *plugin)
{
    for (const auto &hook : plugin->httpHooks) {
        if (!plugin->state) continue;
        if (hook.request != LUA_NOREF) luaL_unref(plugin->state, LUA_REGISTRYINDEX, hook.request);
        if (hook.response != LUA_NOREF) luaL_unref(plugin->state, LUA_REGISTRYINDEX, hook.response);
    }
    plugin->httpHooks.clear();
    std::vector<uint64_t> owned;
    {
        std::lock_guard<std::mutex> lock(gHoldMutex);
        for (const auto &[exchange, hold] : gHttpHolds)
            if (hold->owner == plugin) owned.push_back(exchange);
    }
    for (uint64_t exchange : owned) FinishHold(exchange, std::nullopt, std::nullopt);
}

static bool PatternMatches(lua_State *state, const std::string &text, const std::string &pattern)
{
    int top = lua_gettop(state);
    lua_getglobal(state, "string");
    if (!lua_istable(state, -1)) {
        lua_settop(state, top);
        return false;
    }
    lua_getfield(state, -1, "find");
    lua_pushlstring(state, text.data(), text.size());
    lua_pushlstring(state, pattern.data(), pattern.size());
    bool matched = lua_pcall(state, 2, 1, 0) == LUA_OK && !lua_isnil(state, -1);
    lua_settop(state, top);
    return matched;
}

static bool HookMatches(LuaPlugin *plugin, const LuaHttpHook &hook, const std::string &method, const std::string &path)
{
    if (!hook.method.empty() && hook.method != method) return false;
    return PatternMatches(plugin->state, path, hook.pattern);
}

static void PushHeaders(lua_State *state, const std::vector<std::pair<std::string, std::string>> &headers)
{
    lua_createtable(state, 0, static_cast<int>(headers.size()));
    for (const auto &[name, value] : headers) {
        if (HiddenHeader(name)) continue;
        lua_getfield(state, -1, name.c_str());
        std::string joined = lua_isstring(state, -1) ? std::string(lua_tostring(state, -1)) + ", " + value : value;
        lua_pop(state, 1);
        lua_pushlstring(state, joined.data(), joined.size());
        lua_setfield(state, -2, name.c_str());
    }
}

static void PushHttpRequest(lua_State *state, uint64_t exchange, const astral::luahost::HttpRequest &request)
{
    lua_createtable(state, 0, 6);
    lua_pushstring(state, std::to_string(exchange).c_str());
    lua_setfield(state, -2, "id");
    lua_pushlstring(state, request.method.data(), request.method.size());
    lua_setfield(state, -2, "method");
    lua_pushlstring(state, request.url.data(), request.url.size());
    lua_setfield(state, -2, "url");
    lua_pushlstring(state, request.path.data(), request.path.size());
    lua_setfield(state, -2, "path");
    PushHeaders(state, request.headers);
    lua_setfield(state, -2, "headers");
    lua_pushlstring(state, request.body.data(), request.body.size());
    lua_setfield(state, -2, "body");
}

static std::vector<std::pair<std::string, std::string>> HeadersFromJSON(const nlohmann::json &value,
                                                                        const std::vector<std::pair<std::string, std::string>> &previous)
{
    std::vector<std::pair<std::string, std::string>> headers;
    for (const auto &[name, header] : previous)
        if (HiddenHeader(name)) headers.emplace_back(name, header);
    for (const auto &[name, header] : value.items()) {
        std::string lower = LowerCase(name);
        if (HiddenHeader(lower) || !header.is_string() || ContainsControl(lower) || ContainsControl(header.get<std::string>())) continue;
        headers.emplace_back(lower, header.get<std::string>());
    }
    return headers;
}

static std::string UrlHost(const std::string &url)
{
    size_t start = url.find("://");
    if (start == std::string::npos) return "";
    start += 3;
    size_t end = url.find_first_of("/?#:", start);
    return LowerCase(url.substr(start, end == std::string::npos ? std::string::npos : end - start));
}

static std::string UrlPath(const std::string &url)
{
    size_t start = url.find("://");
    start = start == std::string::npos ? 0 : url.find('/', start + 3);
    if (start == std::string::npos) return "/";
    size_t end = url.find_first_of("?#", start);
    return url.substr(start, end == std::string::npos ? std::string::npos : end - start);
}

static void ApplyRequestChanges(astral::luahost::HttpRequest &request, const nlohmann::json &changes)
{
    if (changes.contains("method") && changes["method"].is_string()) request.method = changes["method"].get<std::string>();
    if (changes.contains("url") && changes["url"].is_string()) {
        std::string url = changes["url"].get<std::string>();
        if (url.rfind("https://", 0) == 0 && astral::isDiscordHost(UrlHost(url))) {
            request.url = url;
            request.path = UrlPath(url);
        }
    }
    if (changes.contains("headers") && changes["headers"].is_object())
        request.headers = HeadersFromJSON(changes["headers"], request.headers);
    if (changes.contains("body") && changes["body"].is_string() &&
        changes["body"].get_ref<const std::string &>().size() <= kMaximumHttpBody)
        request.body = changes["body"].get<std::string>();
}

static astral::luahost::HttpResponse ResponseFromJSON(const nlohmann::json &value)
{
    astral::luahost::HttpResponse response;
    response.status = value.value("status", 200);
    if (response.status < 100 || response.status > 599) response.status = 200;
    if (value.contains("headers") && value["headers"].is_object()) response.headers = HeadersFromJSON(value["headers"], {});
    if (value.contains("body") && value["body"].is_string()) response.body = value["body"].get<std::string>();
    return response;
}

static int RegisterHttpIntercept(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "net.discord");
    luaL_checktype(state, 1, LUA_TTABLE);
    if (plugin->httpHooks.size() >= 16) return luaL_error(state, "astral.net.intercept: at most 16 intercepts per plugin");
    LuaHttpHook hook;
    lua_getfield(state, 1, "path");
    if (!lua_isstring(state, -1)) return luaL_error(state, "astral.net.intercept: path must be a Lua pattern");
    hook.pattern = lua_tostring(state, -1);
    lua_getfield(state, 1, "method");
    if (lua_isstring(state, -1)) hook.method = lua_tostring(state, -1);
    lua_pop(state, 2);
    lua_getfield(state, 1, "request");
    if (lua_isfunction(state, -1))
        hook.request = luaL_ref(state, LUA_REGISTRYINDEX);
    else
        lua_pop(state, 1);
    lua_getfield(state, 1, "response");
    if (lua_isfunction(state, -1))
        hook.response = luaL_ref(state, LUA_REGISTRYINDEX);
    else
        lua_pop(state, 1);
    if (hook.request == LUA_NOREF && hook.response == LUA_NOREF)
        return luaL_error(state, "astral.net.intercept: give a request or response function");
    plugin->httpHooks.push_back(std::move(hook));
    return 0;
}

static uint64_t ExchangeArgument(lua_State *state, int index)
{
    const char *value = luaL_checkstring(state, index);
    char *end = nullptr;
    unsigned long long parsed = strtoull(value, &end, 10);
    if (!parsed || !end || *end) luaL_error(state, "astral.net: '%s' is not a held request", value);
    return parsed;
}

static int ReleaseHeldRequest(lua_State *state)
{
    RequirePermission(state, "net.discord");
    uint64_t exchange = ExchangeArgument(state, 1);
    std::optional<nlohmann::json> changes;
    if (lua_istable(state, 2)) {
        nlohmann::json value;
        if (astral::lua::toJSON(state, 2, value) && value.is_object()) changes = std::move(value);
    }
    FinishHold(exchange, std::nullopt, std::move(changes));
    return 0;
}

static int RespondToHeldRequest(lua_State *state)
{
    RequirePermission(state, "net.discord");
    uint64_t exchange = ExchangeArgument(state, 1);
    nlohmann::json value;
    if (!astral::lua::toJSON(state, 2, value) || !value.is_object()) return luaL_error(state, "astral.net.respond: give a response table");
    FinishHold(exchange, ResponseFromJSON(value), std::nullopt);
    return 0;
}

static int DecodeJSON(lua_State *state)
{
    size_t length = 0;
    const char *text = luaL_checklstring(state, 1, &length);
    if (length > kMaximumHttpBody) return luaL_error(state, "astral.json.decode: text exceeds 4 MB");
    nlohmann::json value = nlohmann::json::parse(std::string_view(text, length), nullptr, false);
    if (value.is_discarded()) return 0;
    astral::lua::pushJSON(state, value);
    return 1;
}

static int MarkJSONArray(lua_State *state)
{
    if (lua_isnoneornil(state, 1)) lua_newtable(state);
    luaL_checktype(state, 1, LUA_TTABLE);
    lua_settop(state, 1);
    astral::lua::pushArrayMetatable(state);
    lua_setmetatable(state, 1);
    return 1;
}

static int EncodeJSON(lua_State *state)
{
    nlohmann::json value;
    if (!astral::lua::toJSON(state, 1, value)) return luaL_error(state, "astral.json.encode: value is not JSON-compatible");
    int indent = -1;
    if (lua_istable(state, 2)) {
        lua_getfield(state, 2, "indent");
        if (lua_isinteger(state, -1)) indent = static_cast<int>(std::clamp<lua_Integer>(lua_tointeger(state, -1), 0, 8));
        lua_pop(state, 1);
    }
    std::string text = value.dump(indent, ' ', false, nlohmann::json::error_handler_t::replace);
    lua_pushlstring(state, text.data(), text.size());
    return 1;
}

static void ReleaseTimers(LuaPlugin *plugin)
{
    auto timers = std::move(plugin->timers);
    plugin->timers.clear();
    for (const auto &[identifier, timer] : timers)
        if (plugin->state) luaL_unref(plugin->state, LUA_REGISTRYINDEX, timer.reference);
}

static int TimeNowMilliseconds(lua_State *state)
{
    auto now = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::system_clock::now().time_since_epoch()).count();
    lua_pushinteger(state, static_cast<lua_Integer>(now));
    return 1;
}

static int AddTimer(lua_State *state, int64_t interval)
{
    LuaPlugin *plugin = Self(state);
    lua_Integer delay = luaL_checkinteger(state, 1);
    luaL_checktype(state, 2, LUA_TFUNCTION);
    if (delay < 0 || delay > kMaxTimerDelay) return luaL_error(state, "astral.time: delay is 0 to 86400000 milliseconds");
    if (plugin->timers.size() >= kMaxTimersPerPlugin) return luaL_error(state, "astral.time: at most 64 timers per plugin");
    lua_pushvalue(state, 2);
    int reference = luaL_ref(state, LUA_REGISTRYINDEX);
    uint64_t identifier = gNextRequest++;
    plugin->timers[identifier] = LuaTimer{reference, interval ? std::max<int64_t>(delay, kMinTimerInterval) : 0};
    ScheduleTimer(identifier, interval ? std::max<int64_t>(delay, kMinTimerInterval) : delay);
    lua_pushinteger(state, static_cast<lua_Integer>(identifier));
    return 1;
}

static int TimeAfter(lua_State *state)
{
    return AddTimer(state, 0);
}

static int TimeEvery(lua_State *state)
{
    return AddTimer(state, 1);
}

static int TimeCancel(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    auto found = plugin->timers.find(static_cast<uint64_t>(luaL_checkinteger(state, 1)));
    if (found == plugin->timers.end()) return 0;
    luaL_unref(state, LUA_REGISTRYINDEX, found->second.reference);
    plugin->timers.erase(found);
    return 0;
}

static int SettingsSwitch(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    std::string key = luaL_checkstring(state, 1);
    std::string title = luaL_checkstring(state, 2);
    bool fallback = lua_toboolean(state, 3);
    std::string subtitle, section, icon;
    if (lua_istable(state, 4)) {
        lua_getfield(state, 4, "subtitle");
        lua_getfield(state, 4, "section");
        lua_getfield(state, 4, "icon");
        if (lua_isstring(state, -3)) subtitle = lua_tostring(state, -3);
        if (lua_isstring(state, -2)) section = lua_tostring(state, -2);
        if (lua_isstring(state, -1)) icon = lua_tostring(state, -1);
        lua_pop(state, 3);
    }
    if (key.empty() || key.size() > 48 || plugin->switches.size() >= 32) return luaL_error(state, "astral.settings.switch: invalid key");
    bool known = false;
    for (auto &entry : plugin->switches) known = known || entry.key == key;
    if (!known) {
        LuaSwitch entry{key, title.substr(0, 64), subtitle.substr(0, 120), section.substr(0, 48), fallback, {}, {}};
        entry.icon = icon.substr(0, 64);
        entry.order = plugin->settingCount++;
        plugin->switches.push_back(std::move(entry));
    }
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

static int SettingsButton(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    std::string key = luaL_checkstring(state, 1);
    std::string title = luaL_checkstring(state, 2);
    luaL_checktype(state, 3, LUA_TFUNCTION);
    if (key.empty() || key.size() > 48) return luaL_error(state, "astral.settings.button: invalid key");
    LuaSettingsButton *button = nullptr;
    for (auto &entry : plugin->settingsButtons)
        if (entry.key == key) button = &entry;
    if (!button) {
        if (plugin->settingsButtons.size() >= 16) return luaL_error(state, "astral.settings.button: at most 16 buttons per plugin");
        plugin->settingsButtons.push_back({});
        button = &plugin->settingsButtons.back();
        button->key = key;
        button->order = plugin->settingCount++;
    } else if (button->reference != LUA_NOREF) {
        luaL_unref(state, LUA_REGISTRYINDEX, button->reference);
    }
    button->title = title.substr(0, 64);
    if (lua_istable(state, 4)) {
        button->subtitle = FieldString(state, 4, "subtitle").substr(0, 120);
        button->section = FieldString(state, 4, "section").substr(0, 48);
        button->icon = FieldString(state, 4, "icon").substr(0, 64);
        button->detail = FieldString(state, 4, "detail").substr(0, 32);
    }
    lua_pushvalue(state, 3);
    button->reference = luaL_ref(state, LUA_REGISTRYINDEX);
    return 0;
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
        entry.icon = FieldString(state, 5, "icon").substr(0, 64);
    }
    lua_Integer count = luaL_len(state, 3);
    if (count <= 0 || count > static_cast<lua_Integer>(kMaximumChoiceOptions)) {
        return luaL_error(state, "astral.settings.choice: provide between 1 and %d choices", static_cast<int>(kMaximumChoiceOptions));
    }
    for (lua_Integer index = 1; index <= count; index++) {
        lua_rawgeti(state, 3, index);
        if (lua_istable(state, -1)) {
            LuaOption option{FieldString(state, -1, "value"), FieldString(state, -1, "title"), FieldString(state, -1, "subtitle")};
            if (option.value.size() > 128 || option.title.size() > 96 || option.subtitle.size() > 120) {
                lua_pop(state, 1);
                return luaL_error(state, "astral.settings.choice: option text exceeds its limit");
            }
            if (!option.value.empty()) entry.options.push_back(std::move(option));
        }
        lua_pop(state, 1);
    }
    if (key.empty() || key.size() > 48 || entry.options.empty() || plugin->switches.size() >= 32) {
        return luaL_error(state, "astral.settings.choice: invalid choice");
    }
    bool known = false;
    for (auto &existing : plugin->switches) known = known || existing.key == key;
    if (!known) {
        entry.order = plugin->settingCount++;
        plugin->switches.push_back(entry);
    }
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
    entry.icon = FieldString(state, 3, "icon").substr(0, 64);
    bool valid = !key.empty() && key.size() <= 48 && plugin->switches.size() < 32 && std::isfinite(entry.minimum) &&
                 std::isfinite(entry.maximum) && entry.maximum > entry.minimum && entry.step > 0 &&
                 (entry.maximum - entry.minimum) / entry.step <= 1000;
    if (!valid) return luaL_error(state, "astral.settings.number: invalid number");
    entry.fallbackNumber = SteppedNumber(entry, FieldNumber(state, 3, "default", entry.minimum));
    bool known = false;
    for (auto &existing : plugin->switches) known = known || existing.key == key;
    if (!known) {
        entry.order = plugin->settingCount++;
        plugin->switches.push_back(entry);
    }
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
        entry.icon = FieldString(state, 4, "icon").substr(0, 64);
        lua_getfield(state, 4, "color");
        entry.color = lua_toboolean(state, -1);
        lua_pop(state, 1);
        double requestedMaximum = FieldNumber(state, 4, "max_length", static_cast<double>(maximumLength));
        if (!std::isfinite(requestedMaximum) || requestedMaximum < 1 || requestedMaximum > maximumLength ||
            std::floor(requestedMaximum) != requestedMaximum)
            return luaL_error(state, "astral.settings.text: max_length must be between 1 and 16384");
        entry.maximumLength = static_cast<size_t>(requestedMaximum);
    }
    if (entry.color) entry.maximumLength = 9;
    if (key.empty() || key.size() > 48 || fallback.size() > entry.maximumLength || plugin->textSettings.size() >= 16)
        return luaL_error(state, "astral.settings.text: invalid text setting");
    if (entry.color && !astral::settings::isColorText(fallback))
        return luaL_error(state, "astral.settings.color: default must be empty, #RRGGBB or #AARRGGBB");
    bool known = false;
    for (auto &existing : plugin->textSettings) known = known || existing.key == key;
    for (auto &existing : plugin->switches) known = known || existing.key == key;
    if (!known) {
        entry.order = plugin->settingCount++;
        plugin->textSettings.push_back(entry);
    }
    std::string value = StoredString(SettingKey(plugin, key), fallback);
    if (value.size() > entry.maximumLength || (entry.color && !astral::settings::isColorText(value))) value = fallback;
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
    RefreshDecorations();
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

static std::string ConfirmText(lua_State *state, const char *field, size_t limit, const std::string &fallback)
{
    lua_getfield(state, 1, field);
    std::string value = fallback;
    if (lua_isstring(state, -1)) value = lua_tostring(state, -1);
    lua_pop(state, 1);
    if (value.size() > limit) luaL_error(state, "astral.ui.confirm: %s exceeds %d bytes", field, static_cast<int>(limit));
    return value;
}

static int Confirm(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "ui.dialogs");
    luaL_checktype(state, 1, LUA_TTABLE);
    luaL_checktype(state, 2, LUA_TFUNCTION);
    nlohmann::json spec = {{"title", ConfirmText(state, "title", 120, "")},
                           {"message", ConfirmText(state, "message", 2048, "")},
                           {"image", ConfirmText(state, "image", 2048, "")},
                           {"confirm", ConfirmText(state, "confirm", 40, "OK")},
                           {"cancel", ConfirmText(state, "cancel", 40, "Cancel")}};
    lua_getfield(state, 1, "destructive");
    spec["destructive"] = static_cast<bool>(lua_toboolean(state, -1));
    lua_pop(state, 1);
    const std::string image = spec["image"];
    if (spec["title"].get<std::string>().empty()) return luaL_error(state, "astral.ui.confirm: give a title");
    if (!image.empty() && image.rfind("https://", 0) != 0) return luaL_error(state, "astral.ui.confirm: image must be an https URL");
    if (plugin->confirmRequests.size() >= 4) return luaL_error(state, "astral.ui.confirm: at most 4 open dialogs per plugin");
    if (!gPlatform.confirm) return luaL_error(state, "astral.ui.confirm: dialogs are unavailable on this build");
    uint64_t request = gNextRequest.fetch_add(1);
    if (!request) request = gNextRequest.fetch_add(1);
    lua_pushvalue(state, 2);
    plugin->confirmRequests[request] = luaL_ref(state, LUA_REGISTRYINDEX);
    bool shown = false;
    try {
        shown = gPlatform.confirm(request, spec);
    } catch (...) {
    }
    if (!shown) {
        luaL_unref(state, LUA_REGISTRYINDEX, plugin->confirmRequests[request]);
        plugin->confirmRequests.erase(request);
        return luaL_error(state, "astral.ui.confirm: could not show the dialog");
    }
    lua_pushinteger(state, static_cast<lua_Integer>(request));
    return 1;
}

static constexpr size_t kMaximumSheetSpec = 256 * 1024;

static std::optional<nlohmann::json> SheetSpec(lua_State *state, int index, const char *function)
{
    luaL_checktype(state, index, LUA_TTABLE);
    nlohmann::json spec;
    if (!astral::lua::toJSON(state, index, spec) || !spec.is_object()) {
        luaL_error(state, "%s: give a sheet table", function);
        return std::nullopt;
    }
    if (spec.dump().size() > kMaximumSheetSpec) {
        luaL_error(state, "%s: the sheet exceeds 256 KiB", function);
        return std::nullopt;
    }
    return spec;
}

static uint64_t SheetId(lua_State *state, LuaPlugin *plugin, const char *function)
{
    lua_Integer value = luaL_checkinteger(state, 1);
    uint64_t sheet = value > 0 ? static_cast<uint64_t>(value) : 0;
    if (!plugin->sheets.count(sheet)) luaL_error(state, "%s: that sheet is not open", function);
    return sheet;
}

static int OpenSheet(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "ui.dialogs");
    auto spec = SheetSpec(state, 1, "astral.ui.sheet");
    luaL_checktype(state, 2, LUA_TFUNCTION);
    if (plugin->sheets.size() >= 4) return luaL_error(state, "astral.ui.sheet: at most 4 open sheets per plugin");
    if (!gPlatform.openSheet) return luaL_error(state, "astral.ui.sheet: sheets are unavailable on this build");
    uint64_t sheet = gNextRequest.fetch_add(1);
    if (!sheet) sheet = gNextRequest.fetch_add(1);
    lua_pushvalue(state, 2);
    plugin->sheets[sheet] = luaL_ref(state, LUA_REGISTRYINDEX);
    bool shown = false;
    try {
        shown = gPlatform.openSheet(sheet, *spec);
    } catch (...) {
    }
    if (!shown) {
        luaL_unref(state, LUA_REGISTRYINDEX, plugin->sheets[sheet]);
        plugin->sheets.erase(sheet);
        return luaL_error(state, "astral.ui.sheet: could not show the sheet");
    }
    lua_pushinteger(state, static_cast<lua_Integer>(sheet));
    return 1;
}

static int UpdateSheet(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    uint64_t sheet = SheetId(state, plugin, "astral.ui.sheet_update");
    auto spec = SheetSpec(state, 2, "astral.ui.sheet_update");
    if (gPlatform.updateSheet) gPlatform.updateSheet(sheet, *spec);
    return 0;
}

static int CloseSheet(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    uint64_t sheet = SheetId(state, plugin, "astral.ui.sheet_close");
    if (gPlatform.closeSheet) gPlatform.closeSheet(sheet);
    return 0;
}

static int ShowToast(lua_State *state)
{
    RequirePermission(state, "ui.toasts");
    std::string title = luaL_checkstring(state, 1);
    std::string message = luaL_checkstring(state, 2);
    if (title.size() > 120 || message.size() > 2048) return luaL_error(state, "astral.ui.toast: text exceeds its limit");
    if (!gPlatform.showToast) return luaL_error(state, "astral.ui.toast: notifications are unavailable on this build");
    try {
        gPlatform.showToast(title, message);
    } catch (...) {
        return luaL_error(state, "astral.ui.toast: could not show the notification");
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

static uint64_t MessageArgument(lua_State *state, int index);

static int ScrollToMessage(lua_State *state)
{
    RequirePermission(state, "ui.navigate");
    uint64_t message = MessageArgument(state, 1);
    bool animated = true;
    std::string align = "visible";
    if (lua_istable(state, 2)) {
        lua_getfield(state, 2, "animated");
        if (!lua_isnil(state, -1)) animated = lua_toboolean(state, -1);
        lua_getfield(state, 2, "align");
        if (lua_isstring(state, -1)) align = lua_tostring(state, -1);
        lua_pop(state, 2);
    }
    bool queued = gPlatform.scrollToMessage && gPlatform.scrollToMessage(message, animated, align);
    lua_pushboolean(state, queued);
    return 1;
}

static int FocusComposer(lua_State *state)
{
    RequirePermission(state, "ui.navigate");
    bool focus = lua_isnoneornil(state, 1) || lua_toboolean(state, 1);
    bool queued = gPlatform.focusComposer && gPlatform.focusComposer(focus);
    lua_pushboolean(state, queued);
    return 1;
}

static bool SnowflakeText(const std::string &value)
{
    return !value.empty() && value.size() <= 20 && value.find_first_not_of("0123456789") == std::string::npos;
}

static int OpenChannel(lua_State *state)
{
    RequirePermission(state, "ui.navigate");
    std::string channel = luaL_checkstring(state, 1);
    std::string message = luaL_optstring(state, 2, "");
    if (!SnowflakeText(channel)) return luaL_error(state, "astral.ui.open_channel: '%s' is not a channel id", channel.c_str());
    if (!message.empty() && !SnowflakeText(message))
        return luaL_error(state, "astral.ui.open_channel: '%s' is not a message id", message.c_str());
    auto guild = gPlatform.channelGuild ? gPlatform.channelGuild(channel) : std::nullopt;
    std::string link = "https://discord.com/channels/" + (guild && SnowflakeText(*guild) ? *guild : std::string("@me")) + "/" + channel;
    if (!message.empty()) link += "/" + message;
    bool opened = gPlatform.openURL && gPlatform.openURL(link);
    lua_pushboolean(state, opened);
    return 1;
}

static int PressNode(lua_State *state)
{
    RequirePermission(state, "ui.press");
    size_t length = 0;
    const char *identifier = luaL_checklstring(state, 1, &length);
    if (length == 0 || length > 128) return luaL_error(state, "astral.ui.press: identifiers are 1 to 128 characters");
    std::string wanted(identifier, length);
    bool prefix = !wanted.empty() && wanted.back() == '*';
    if (prefix) wanted.pop_back();
    if (ContainsControl(wanted)) return luaL_error(state, "astral.ui.press: identifier has control characters");
    lua_pushboolean(state, gPlatform.pressNode && gPlatform.pressNode(wanted, prefix));
    return 1;
}

static int RegisterImageRewrite(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "ui.images");
    std::string prefix = luaL_checkstring(state, 1);
    luaL_checktype(state, 2, LUA_TFUNCTION);
    if (prefix.rfind("http", 0) != 0) return luaL_error(state, "astral.ui.image_rewrite: the prefix must start with http");
    if (plugin->imageRewriters.size() >= 16) return luaL_error(state, "astral.ui.image_rewrite: at most 16 rewriters per plugin");
    lua_pushvalue(state, 2);
    plugin->imageRewriters.emplace_back(prefix, luaL_ref(state, LUA_REGISTRYINDEX));
    return 0;
}

static int RegisterTextTransform(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "ui.text");
    luaL_checktype(state, 1, LUA_TFUNCTION);
    if (plugin->textTransforms.size() >= 8) return luaL_error(state, "astral.ui.text_transform: at most 8 transforms per plugin");
    lua_pushvalue(state, 1);
    plugin->textTransforms.push_back(luaL_ref(state, LUA_REGISTRYINDEX));
    if (gPlatform.refreshDecorations) gPlatform.refreshDecorations();
    return 0;
}

static constexpr const char *kCommandApplication = "1000000000000000001";

static int OptionType(const std::string &type)
{
    static const std::map<std::string, int> types = {{"string", 3},  {"integer", 4}, {"boolean", 5}, {"user", 6},
                                                     {"channel", 7}, {"role", 8},    {"number", 10}};
    auto found = types.find(type);
    return found == types.end() ? 0 : found->second;
}

static bool CommandName(const std::string &name)
{
    if (name.empty() || name.size() > 32) return false;
    for (char character : name)
        if (!(islower(static_cast<unsigned char>(character)) || isdigit(static_cast<unsigned char>(character)) || character == '-' ||
              character == '_'))
            return false;
    return true;
}

static int RegisterCommand(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "commands");
    luaL_checktype(state, 1, LUA_TTABLE);
    if (plugin->commands.size() >= 16) return luaL_error(state, "astral.commands.register: at most 16 commands per plugin");
    LuaCommand command;
    lua_getfield(state, 1, "name");
    command.name = lua_isstring(state, -1) ? lua_tostring(state, -1) : "";
    lua_getfield(state, 1, "description");
    command.description = lua_isstring(state, -1) ? lua_tostring(state, -1) : "";
    lua_pop(state, 2);
    if (!CommandName(command.name))
        return luaL_error(state, "astral.commands.register: names are 1 to 32 lower-case letters, digits, - or _");
    if (command.description.empty() || command.description.size() > 100)
        return luaL_error(state, "astral.commands.register: descriptions are 1 to 100 characters");
    lua_getfield(state, 1, "options");
    if (lua_istable(state, -1)) {
        lua_Integer count = luaL_len(state, -1);
        if (count > 25) return luaL_error(state, "astral.commands.register: at most 25 options");
        for (lua_Integer index = 1; index <= count; index++) {
            lua_rawgeti(state, -1, index);
            lua_getfield(state, -1, "name");
            std::string name = lua_isstring(state, -1) ? lua_tostring(state, -1) : "";
            lua_getfield(state, -2, "description");
            std::string description = lua_isstring(state, -1) ? lua_tostring(state, -1) : name;
            lua_getfield(state, -3, "type");
            int type = OptionType(lua_isstring(state, -1) ? lua_tostring(state, -1) : "string");
            lua_getfield(state, -4, "required");
            bool required = lua_toboolean(state, -1);
            lua_pop(state, 5);
            if (!CommandName(name) || !type)
                return luaL_error(state, "astral.commands.register: option %d needs a valid name and type", (int)index);
            command.options.push_back(
                {{"name", name}, {"description", description.substr(0, 100)}, {"type", type}, {"required", required}});
        }
    }
    lua_pop(state, 1);
    lua_getfield(state, 1, "execute");
    if (!lua_isfunction(state, -1)) return luaL_error(state, "astral.commands.register: execute must be a function");
    command.reference = luaL_ref(state, LUA_REGISTRYINDEX);
    for (auto &existing : plugin->commands)
        if (existing.name == command.name) {
            luaL_unref(state, LUA_REGISTRYINDEX, existing.reference);
            existing = std::move(command);
            return 0;
        }
    plugin->commands.push_back(std::move(command));
    return 0;
}

static std::string GatewayTimestamp(int64_t milliseconds);

static int InsertComposerText(lua_State *state)
{
    RequirePermission(state, "ui.composer");
    size_t length = 0;
    const char *text = luaL_checklstring(state, 1, &length);
    bool replace = false;
    if (lua_istable(state, 2)) {
        lua_getfield(state, 2, "replace");
        replace = lua_toboolean(state, -1);
        lua_pop(state, 1);
    }
    if ((length == 0 && !replace) || length > 4000) return luaL_error(state, "astral.ui.insert_text: text is 1 to 4000 bytes");
    lua_pushboolean(state, gPlatform.insertComposerText && gPlatform.insertComposerText(std::string(text, length), replace));
    return 1;
}

static int ComposerText(lua_State *state)
{
    RequirePermission(state, "ui.composer");
    static char text[8192];
    size_t length = astral_wire_composer_text(text, sizeof text);
    lua_pushlstring(state, text, length);
    return 1;
}

static int KeepAlive(lua_State *state)
{
    RequirePermission(state, "background");
    bool on = lua_toboolean(state, 1);
    lua_pushboolean(state, gPlatform.keepAlive && gPlatform.keepAlive(on));
    return 1;
}

static int RefreshDecorationsNow(lua_State *)
{
    if (gPlatform.refreshDecorations) gPlatform.refreshDecorations();
    return 0;
}

static int TimeISO(lua_State *state)
{
    int64_t now = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::system_clock::now().time_since_epoch()).count();
    int64_t milliseconds = lua_isinteger(state, 1) ? lua_tointeger(state, 1) : now;
    std::string iso = GatewayTimestamp(milliseconds).substr(0, 16);
    iso[10] = ' ';
    iso += " UTC";
    lua_pushstring(state, iso.c_str());
    return 1;
}

static int RenderImage(lua_State *state)
{
    nlohmann::json spec;
    if (!lua_istable(state, 1) || !astral::lua::toJSON(state, 1, spec) || !spec.is_object())
        return luaL_error(state, "astral.image.render: give a layout table");
    double width = spec.value("width", 0.0), height = spec.value("height", 0.0), scale = spec.value("scale", 2.0);
    if (width < 1 || height < 1 || width > 2048 || height > 4096 || scale < 1 || scale > 3)
        return luaL_error(state, "astral.image.render: width up to 2048, height up to 4096, scale 1 to 3");
    if (!spec.contains("items") || !spec["items"].is_array() || spec["items"].size() > 2000)
        return luaL_error(state, "astral.image.render: items must be a list of at most 2000 shapes");
    if (!gPlatform.renderImage) return 0;
    std::optional<std::string> png;
    try {
        png = gPlatform.renderImage(spec);
    } catch (...) {
    }
    if (!png) return 0;
    lua_pushlstring(state, png->data(), png->size());
    return 1;
}

static int DiagnosticEnvironment(lua_State *state)
{
    RequirePermission(state, "diagnostics");
    nlohmann::json environment = gPlatform.environment ? gPlatform.environment() : nlohmann::json::object();
    if (!environment.is_object()) environment = nlohmann::json::object();
    nlohmann::json plugins = nlohmann::json::array();
    for (auto *plugin : gPlugins)
        plugins.push_back({{"id", plugin->identifier},
                           {"name", plugin->name},
                           {"version", plugin->version},
                           {"enabled", plugin->enabled},
                           {"running", plugin->state != nullptr && !plugin->failed},
                           {"failed", plugin->failed}});
    environment["plugin_list"] = plugins;
    astral::lua::pushJSON(state, environment);
    return 1;
}

static int PasteText(lua_State *state)
{
    RequirePermission(state, "ui.clipboard");
    if (!gPlatform.readClipboard) return luaL_error(state, "astral.ui.paste: clipboard is unavailable on this build");
    std::optional<std::string> value;
    try {
        value = gPlatform.readClipboard();
    } catch (...) {
    }
    if (!value) return 0;
    lua_pushlstring(state, value->data(), std::min<size_t>(value->size(), 16384));
    return 1;
}

static bool OpenableURL(const std::string &url)
{
    return url.size() <= 2048 && (url.rfind("https://", 0) == 0 || url.rfind("discord://", 0) == 0);
}

static int OpenURL(lua_State *state)
{
    std::string url = luaL_checkstring(state, 1);
    if (!OpenableURL(url)) return luaL_error(state, "astral.ui.open: only https:// and discord:// links up to 2048 bytes");
    if (!gPlatform.openURL) return luaL_error(state, "astral.ui.open: opening links is unavailable on this build");
    bool opened = false;
    try {
        opened = gPlatform.openURL(url);
    } catch (...) {
    }
    lua_pushboolean(state, opened);
    return 1;
}

static int Notify(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "ui.notifications");
    std::string title = luaL_checkstring(state, 1);
    std::string body = luaL_checkstring(state, 2);
    if (title.size() > 120 || body.size() > 2048) return luaL_error(state, "astral.ui.notify: text exceeds its limit");
    double delay = 0;
    std::string channel, message;
    if (lua_istable(state, 3)) {
        lua_getfield(state, 3, "delay");
        delay = lua_isnumber(state, -1) ? lua_tonumber(state, -1) : 0;
        lua_pop(state, 1);
        lua_getfield(state, 3, "channel_id");
        if (lua_isstring(state, -1)) channel = lua_tostring(state, -1);
        lua_pop(state, 1);
        lua_getfield(state, 3, "message_id");
        if (lua_isstring(state, -1)) message = lua_tostring(state, -1);
        lua_pop(state, 1);
    }
    if ((!channel.empty() && !SnowflakeText(channel)) || (!message.empty() && (channel.empty() || !SnowflakeText(message))))
        return luaL_error(state, "astral.ui.notify: channel_id and message_id must be Discord ids");
    std::string link;
    if (!channel.empty()) {
        auto guild = gPlatform.channelGuild ? gPlatform.channelGuild(channel) : std::nullopt;
        link = "https://discord.com/channels/" + (guild && SnowflakeText(*guild) ? *guild : std::string("@me")) + "/" + channel;
        if (!message.empty()) link += "/" + message;
    }
    if (!std::isfinite(delay) || delay < 0 || delay > 7 * 24 * 60 * 60)
        return luaL_error(state, "astral.ui.notify: delay is 0 to 604800 seconds");
    if (!gPlatform.notify) return luaL_error(state, "astral.ui.notify: notifications are unavailable on this build");
    std::string identifier = "astral." + plugin->identifier + "." + std::to_string(gNextRequest++);
    bool scheduled = false;
    try {
        scheduled = gPlatform.notify(identifier, title, body, delay, link);
    } catch (...) {
    }
    lua_pushboolean(state, scheduled);
    return 1;
}

static const std::set<std::string> kSoundExtensions = {".mp3", ".m4a", ".wav", ".caf", ".aac"};

static std::optional<std::string> PluginSoundPath(const LuaPlugin *plugin, const std::string &relative)
{
    std::filesystem::path requested(relative);
    if (relative.empty() || requested.is_absolute()) return std::nullopt;
    for (const auto &part : requested)
        if (part == "..") return std::nullopt;
    std::string extension = requested.extension().string();
    std::transform(extension.begin(), extension.end(), extension.begin(), [](unsigned char c) { return std::tolower(c); });
    if (!kSoundExtensions.count(extension)) return std::nullopt;
    std::error_code error;
    std::filesystem::path full = std::filesystem::path(plugin->directory) / requested;
    if (!std::filesystem::is_regular_file(full, error)) return std::nullopt;
    return full.string();
}

static int Speak(lua_State *state)
{
    RequirePermission(state, "audio");
    std::string text = luaL_checkstring(state, 1);
    if (text.empty() || text.size() > 4096) return luaL_error(state, "astral.audio.speak: text is 1 to 4096 bytes");
    std::string language;
    double rate = 0.5;
    if (lua_istable(state, 2)) {
        lua_getfield(state, 2, "language");
        if (lua_isstring(state, -1)) language = lua_tostring(state, -1);
        lua_pop(state, 1);
        lua_getfield(state, 2, "rate");
        if (lua_isnumber(state, -1)) rate = lua_tonumber(state, -1);
        lua_pop(state, 1);
    }
    if (!std::isfinite(rate) || rate < 0 || rate > 1) return luaL_error(state, "astral.audio.speak: rate is 0 to 1");
    if (language.size() > 35) return luaL_error(state, "astral.audio.speak: language is a BCP 47 tag");
    if (!gPlatform.speak) return luaL_error(state, "astral.audio.speak: speech is unavailable on this build");
    bool spoken = false;
    try {
        spoken = gPlatform.speak(text, language, rate);
    } catch (...) {
    }
    lua_pushboolean(state, spoken);
    return 1;
}

static int PlaySound(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "audio");
    std::string file = luaL_checkstring(state, 1);
    double volume = 1;
    if (lua_istable(state, 2)) {
        lua_getfield(state, 2, "volume");
        if (lua_isnumber(state, -1)) volume = lua_tonumber(state, -1);
        lua_pop(state, 1);
    }
    if (!std::isfinite(volume) || volume < 0 || volume > 1) return luaL_error(state, "astral.audio.play: volume is 0 to 1");
    auto path = PluginSoundPath(plugin, file);
    if (!path) return luaL_error(state, "astral.audio.play: %s is not an mp3, m4a, wav, caf or aac file in the plugin", file.c_str());
    if (!gPlatform.playSound) return luaL_error(state, "astral.audio.play: sound is unavailable on this build");
    bool played = false;
    try {
        played = gPlatform.playSound(*path, volume);
    } catch (...) {
    }
    lua_pushboolean(state, played);
    return 1;
}

static int StopAudio(lua_State *state)
{
    RequirePermission(state, "audio");
    if (gPlatform.stopAudio) {
        try {
            gPlatform.stopAudio();
        } catch (...) {
        }
    }
    return 0;
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

static bool ValidAuthorizationURL(const std::string &url, size_t maximumLength)
{
    if (url.size() > maximumLength || url.rfind("https://", 0) != 0 || url.find('\\') != std::string::npos || ContainsControl(url) ||
        !ValidUTF8(url))
        return false;
    size_t authorityEnd = url.find_first_of("/?#", 8);
    std::string authority = authorityEnd == std::string::npos ? url.substr(8) : url.substr(8, authorityEnd - 8);
    return !authority.empty() && authority.find('@') == std::string::npos;
}

static int HexValue(char character)
{
    if (character >= '0' && character <= '9') return character - '0';
    if (character >= 'a' && character <= 'f') return character - 'a' + 10;
    if (character >= 'A' && character <= 'F') return character - 'A' + 10;
    return -1;
}

static std::string PercentDecoded(const std::string &text)
{
    std::string decoded;
    for (size_t index = 0; index < text.size(); index++) {
        int high = index + 2 < text.size() && text[index] == '%' ? HexValue(text[index + 1]) : -1;
        int low = high >= 0 ? HexValue(text[index + 2]) : -1;
        if (low >= 0) {
            decoded += static_cast<char>(high * 16 + low);
            index += 2;
        } else {
            decoded += text[index] == '+' ? ' ' : text[index];
        }
    }
    return decoded;
}

static std::string RedirectURI(const std::string &url)
{
    size_t query = url.find('?');
    if (query == std::string::npos) return {};
    size_t end = url.find('#', query);
    std::string parameters = url.substr(query + 1, end == std::string::npos ? std::string::npos : end - query - 1);
    size_t start = 0;
    while (start <= parameters.size()) {
        size_t next = parameters.find('&', start);
        std::string pair = parameters.substr(start, next == std::string::npos ? std::string::npos : next - start);
        if (pair.rfind("redirect_uri=", 0) == 0) return PercentDecoded(pair.substr(13));
        if (next == std::string::npos) break;
        start = next + 1;
    }
    return {};
}

static int WebAuth(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "ui.web_auth");
    std::string url = luaL_checkstring(state, 1);
    std::string callbackURL = luaL_checkstring(state, 2);
    luaL_checktype(state, 3, LUA_TFUNCTION);
    if (!ValidAuthorizationURL(url, 4096) || !ValidAuthorizationURL(callbackURL, 1024))
        return luaL_error(state, "astral.ui.web_auth: URLs must be valid HTTPS URLs without credentials");
    size_t urlAuthorityEnd = url.find_first_of("/?#", 8);
    size_t callbackAuthorityEnd = callbackURL.find_first_of("/?#", 8);
    std::string urlAuthority = urlAuthorityEnd == std::string::npos ? url.substr(8) : url.substr(8, urlAuthorityEnd - 8);
    std::string callbackAuthority =
        callbackAuthorityEnd == std::string::npos ? callbackURL.substr(8) : callbackURL.substr(8, callbackAuthorityEnd - 8);
    bool redirectsToCallback = RedirectURI(url).rfind(callbackURL, 0) == 0;
    if ((urlAuthority != callbackAuthority && !redirectsToCallback) || callbackURL.find_first_of("?#") != std::string::npos)
        return luaL_error(state, "astral.ui.web_auth: callback must be a path on the authorization URL's HTTPS host");
    for (auto *pending : gPlugins) {
        if (!pending->webAuthRequests.empty()) return luaL_error(state, "astral.ui.web_auth: another authorization flow is already active");
    }
    if (!gPlatform.webAuth) return luaL_error(state, "astral.ui.web_auth: browser authorization is unavailable on this client");
    uint64_t request = gNextRequest.fetch_add(1);
    if (request == 0) request = gNextRequest.fetch_add(1);
    lua_pushvalue(state, 3);
    plugin->webAuthRequests[request] = luaL_ref(state, LUA_REGISTRYINDEX);
    bool started = false;
    try {
        started = gPlatform.webAuth(request, url, callbackURL);
    } catch (...) {
    }
    if (!started) {
        luaL_unref(state, LUA_REGISTRYINDEX, plugin->webAuthRequests[request]);
        plugin->webAuthRequests.erase(request);
        return luaL_error(state, "astral.ui.web_auth: could not open the authorization window");
    }
    lua_pushinteger(state, static_cast<lua_Integer>(request));
    return 1;
}

static int CancelWebAuth(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "ui.web_auth");
    lua_Integer value = luaL_checkinteger(state, 1);
    if (value <= 0) {
        lua_pushboolean(state, false);
        return 1;
    }
    uint64_t request = static_cast<uint64_t>(value);
    auto found = plugin->webAuthRequests.find(request);
    if (found == plugin->webAuthRequests.end()) {
        lua_pushboolean(state, false);
        return 1;
    }
    luaL_unref(state, LUA_REGISTRYINDEX, found->second);
    plugin->webAuthRequests.erase(found);
    if (gPlatform.cancelWebAuth) {
        try {
            gPlatform.cancelWebAuth(request);
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
    if (event == "message.filter") permission = "events.messages.filter";
    if (event == "gateway") permission = "events.gateway";
    if (!permission) return luaL_error(state, "astral.events.on: unsupported event '%s'", event.c_str());
    RequirePermission(state, permission);
    luaL_checktype(state, 2, LUA_TFUNCTION);
    if (plugin->eventCallbacks.size() >= 32) return luaL_error(state, "astral.events.on: at most 32 subscriptions per plugin");
    std::set<std::string> types;
    if (event == "gateway" && lua_istable(state, 3)) {
        lua_Integer count = luaL_len(state, 3);
        if (count > 64) return luaL_error(state, "astral.events.on: at most 64 gateway event types");
        for (lua_Integer index = 1; index <= count; index++) {
            lua_rawgeti(state, 3, index);
            size_t length = 0;
            const char *type = lua_tolstring(state, -1, &length);
            if (!type || length == 0 || length > 64) return luaL_error(state, "astral.events.on: gateway event types are strings");
            types.emplace(type, length);
            lua_pop(state, 1);
        }
    }
    uint64_t subscription = gNextRequest.fetch_add(1);
    if (subscription == 0) subscription = gNextRequest.fetch_add(1);
    lua_pushvalue(state, 2);
    plugin->eventCallbacks.emplace(subscription, LuaEventCallback{event, luaL_ref(state, LUA_REGISTRYINDEX), std::move(types)});
    lua_pushinteger(state, static_cast<lua_Integer>(subscription));
    return 1;
}

static int UnsubscribeEvent(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
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
    if (found->second.event == "app.foreground" || found->second.event == "app.background")
        RequirePermission(state, "events.lifecycle");
    else if (found->second.event == "message.create")
        RequirePermission(state, "events.messages");
    else if (found->second.event == "message.filter")
        RequirePermission(state, "events.messages.filter");
    else if (found->second.event == "gateway")
        RequirePermission(state, "events.gateway");
    luaL_unref(state, LUA_REGISTRYINDEX, found->second.reference);
    plugin->eventCallbacks.erase(found);
    lua_pushboolean(state, true);
    return 1;
}

static int DispatchGatewayEvent(lua_State *state)
{
    RequirePermission(state, "messages.local");
    size_t length = 0;
    const char *type = luaL_checklstring(state, 1, &length);
    if (length == 0 || length > 64) return luaL_error(state, "astral.gateway.dispatch: event types are 1 to 64 characters");
    for (size_t index = 0; index < length; index++)
        if (!(isupper((unsigned char)type[index]) || type[index] == '_'))
            return luaL_error(state, "astral.gateway.dispatch: '%s' is not a gateway event type", type);
    nlohmann::json data;
    if (!astral::lua::toJSON(state, 2, data)) return luaL_error(state, "astral.gateway.dispatch: data must be JSON-compatible");
    nlohmann::json envelope = {{"op", 0}, {"t", std::string(type, length)}, {"s", nullptr}, {"d", std::move(data)}};
    std::string message = envelope.dump();
    if (message.size() > (1u << 20)) return luaL_error(state, "astral.gateway.dispatch: event exceeds 1 MB");
    lua_pushboolean(state, gPlatform.dispatchGateway && gPlatform.dispatchGateway(message));
    return 1;
}

static std::string GatewayTimestamp(int64_t milliseconds)
{
    int64_t seconds = milliseconds / 1000;
    int64_t days = seconds / 86400, rest = seconds % 86400;
    int64_t z = days + 719468, era = z / 146097, doe = z - era * 146097;
    int64_t yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    int64_t doy = doe - (365 * yoe + yoe / 4 - yoe / 100), mp = (5 * doy + 2) / 153;
    int64_t day = doy - (153 * mp + 2) / 5 + 1, month = mp < 10 ? mp + 3 : mp - 9, year = yoe + era * 400 + (month <= 2 ? 1 : 0);
    char text[48];
    snprintf(text, sizeof text, "%04lld-%02lld-%02lldT%02lld:%02lld:%02lld.%03lld000+00:00", (long long)year, (long long)month,
             (long long)day, (long long)(rest / 3600), (long long)(rest % 3600 / 60), (long long)(rest % 60),
             (long long)(milliseconds % 1000));
    return text;
}

static std::optional<std::string> SendLocalMessageJSON(const std::string &channel, const nlohmann::json &message);

static int SendLocalMessage(lua_State *state)
{
    RequirePermission(state, "messages.local");
    std::string channel = luaL_checkstring(state, 1);
    if (!SnowflakeText(channel)) return luaL_error(state, "astral.messages.send_local: '%s' is not a channel id", channel.c_str());
    nlohmann::json message = nlohmann::json::object();
    if (lua_isstring(state, 2))
        message["content"] = lua_tostring(state, 2);
    else if (!lua_istable(state, 2) || !astral::lua::toJSON(state, 2, message) || !message.is_object())
        return luaL_error(state, "astral.messages.send_local: give text or a message table");
    auto id = SendLocalMessageJSON(channel, message);
    if (!id) return 0;
    lua_pushlstring(state, id->data(), id->size());
    return 1;
}

static std::optional<std::string> SendLocalMessageJSON(const std::string &channel, const nlohmann::json &message)
{
    int64_t now = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::system_clock::now().time_since_epoch()).count();
    static std::atomic<uint64_t> sequence{0};
    uint64_t snowflake = (static_cast<uint64_t>(now - 1420070400000LL) << 22) | (sequence.fetch_add(1) & 0xFFF);
    nlohmann::json author = {{"id", "1"},   {"username", "astral"}, {"global_name", "astral"}, {"discriminator", "0"},
                             {"bot", true}, {"public_flags", 0}};
    nlohmann::json data = {{"id", std::to_string(snowflake)},
                           {"channel_id", channel},
                           {"type", 0},
                           {"content", ""},
                           {"flags", 64},
                           {"timestamp", GatewayTimestamp(now)},
                           {"author", author},
                           {"tts", false},
                           {"pinned", false},
                           {"mention_everyone", false},
                           {"mentions", nlohmann::json::array()},
                           {"mention_roles", nlohmann::json::array()},
                           {"attachments", nlohmann::json::array()},
                           {"embeds", nlohmann::json::array()},
                           {"components", nlohmann::json::array()}};
    if (gPlatform.channelGuild) {
        auto guild = gPlatform.channelGuild(channel);
        if (guild && SnowflakeText(*guild)) data["guild_id"] = *guild;
    }
    for (const auto &[key, value] : message.items())
        if (key != "channel_id" && key != "id") data[key] = value;
    nlohmann::json envelope = {{"op", 0}, {"t", "MESSAGE_CREATE"}, {"s", nullptr}, {"d", data}};
    std::string text = envelope.dump();
    if (text.size() > (1u << 20) || !gPlatform.dispatchGateway || !gPlatform.dispatchGateway(text)) return std::nullopt;
    return data["id"].get<std::string>();
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

static int RegisterMediaSaveHandler(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "media.save");
    luaL_checktype(state, 1, LUA_TFUNCTION);
    if (plugin->mediaSaveHandlers.size() >= 4) return luaL_error(state, "astral.media.on_save: at most 4 handlers per plugin");
    lua_pushvalue(state, 1);
    plugin->mediaSaveHandlers.push_back(luaL_ref(state, LUA_REGISTRYINDEX));
    return 0;
}

static int RegisterComposerDecorator(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "ui.composer");
    luaL_checktype(state, 1, LUA_TFUNCTION);
    if (!plugin->composerDecorators.empty()) return luaL_error(state, "astral.ui.composer: at most one formatter per plugin");
    lua_pushvalue(state, 1);
    plugin->composerDecorators.push_back(luaL_ref(state, LUA_REGISTRYINDEX));
    return 0;
}

static int RegisterMessageBackground(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "ui.message_background");
    luaL_checktype(state, 1, LUA_TFUNCTION);
    if (!plugin->messageBackgroundDecorators.empty())
        return luaL_error(state, "astral.ui.message_background: at most one decorator per plugin");
    lua_pushvalue(state, 1);
    plugin->messageBackgroundDecorators.push_back(luaL_ref(state, LUA_REGISTRYINDEX));
    return 0;
}

static void SyncPatchPrefixes()
{
    std::vector<std::string> prefixes;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (const auto &patch : plugin->nodePatches)
            if (std::find(prefixes.begin(), prefixes.end(), patch.prefix) == prefixes.end()) prefixes.push_back(patch.prefix);
    }
    std::vector<const char *> pointers;
    for (const auto &prefix : prefixes) pointers.push_back(prefix.c_str());
    astral_wire_set_patch_prefixes(pointers.data(), pointers.size());
}

static std::atomic<uint64_t> gNextInsert{1};

static void SyncInsertPrefixes()
{
    std::vector<std::string> prefixes;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (const auto &insert : plugin->nodeInserts)
            if (std::find(prefixes.begin(), prefixes.end(), insert.prefix) == prefixes.end()) prefixes.push_back(insert.prefix);
    }
    std::vector<const char *> pointers;
    for (const auto &prefix : prefixes) pointers.push_back(prefix.c_str());
    astral_wire_set_insert_prefixes(pointers.data(), pointers.size());
}

static int RegisterNodeInsert(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "ui.insert");
    std::string prefix = luaL_checkstring(state, 1);
    std::string type = luaL_optstring(state, 2, "");
    luaL_checktype(state, 3, LUA_TFUNCTION);
    if (prefix.size() < 3 || prefix.size() >= 96) return luaL_error(state, "astral.ui.insert: id prefix must be 3 to 95 characters");
    if (plugin->nodeInserts.size() >= 16) return luaL_error(state, "astral.ui.insert: at most 16 inserts per plugin");
    lua_pushvalue(state, 3);
    int build = luaL_ref(state, LUA_REGISTRYINDEX);
    int press = LUA_NOREF;
    if (lua_isfunction(state, 4)) {
        lua_pushvalue(state, 4);
        press = luaL_ref(state, LUA_REGISTRYINDEX);
    }
    plugin->nodeInserts.push_back({gNextInsert.fetch_add(1), prefix, type, build, press});
    SyncInsertPrefixes();
    return 0;
}

static int RegisterNodePatch(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "ui.patch");
    std::string prefix = luaL_checkstring(state, 1);
    std::string type = luaL_optstring(state, 2, "");
    luaL_checktype(state, 3, LUA_TFUNCTION);
    if (prefix.size() < 3 || prefix.size() >= 96) return luaL_error(state, "astral.ui.patch: id prefix must be 3 to 95 characters");
    if (plugin->nodePatches.size() >= 16) return luaL_error(state, "astral.ui.patch: at most 16 patches per plugin");
    lua_pushvalue(state, 3);
    plugin->nodePatches.push_back({prefix, type, luaL_ref(state, LUA_REGISTRYINDEX)});
    SyncPatchPrefixes();
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

static uint64_t MessageArgument(lua_State *state, int index)
{
    const char *value = luaL_checkstring(state, index);
    char *end = nullptr;
    unsigned long long parsed = strtoull(value, &end, 10);
    if (!parsed || !end || *end) luaL_error(state, "astral.messages: '%s' is not a message id", value);
    return parsed;
}

static int SetMessageText(lua_State *state)
{
    RequirePermission(state, "messages.display");
    uint64_t message = MessageArgument(state, 2);
    if (lua_isnoneornil(state, 3)) {
        gDisplayedTexts.erase(message);
    } else {
        size_t length = 0;
        const char *text = luaL_checklstring(state, 3, &length);
        if (!length || length > kMaxDisplayedText)
            return luaL_error(state, "astral.messages.set_text: text must be 1 to %d bytes", (int)kMaxDisplayedText);
        std::string note;
        if (lua_istable(state, 4)) {
            lua_getfield(state, 4, "note");
            if (lua_isstring(state, -1)) note = std::string(lua_tostring(state, -1)).substr(0, 48);
            lua_pop(state, 1);
        }
        if (gDisplayedTexts.size() >= kMaxDisplayedTexts && !gDisplayedTexts.count(message)) gDisplayedTexts.erase(gDisplayedTexts.begin());
        gDisplayedTexts[message] = {std::string(text, length), note};
    }
    RefreshDecorations();
    return 0;
}

static int GetMessageText(lua_State *state)
{
    RequirePermission(state, "messages.display");
    auto found = gDisplayedTexts.find(MessageArgument(state, 1));
    if (found == gDisplayedTexts.end()) {
        lua_pushnil(state);
        return 1;
    }
    lua_pushlstring(state, found->second.text.data(), found->second.text.size());
    return 1;
}

static int MessageAction(lua_State *state)
{
    LuaPlugin *plugin = Self(state);
    RequirePermission(state, "ui.message_actions");
    size_t length = 0;
    const char *label = luaL_checklstring(state, 1, &length);
    luaL_checktype(state, 2, LUA_TFUNCTION);
    if (!length || length > 24) return luaL_error(state, "astral.ui.message_action: label must be 1 to 24 bytes");
    if (plugin->messageActions.size() >= 4) return luaL_error(state, "astral.ui.message_action: at most 4 actions per plugin");
    std::string icon = "res://ic_mana_puzzle_piece";
    if (lua_istable(state, 3)) {
        lua_getfield(state, 3, "icon");
        if (lua_isstring(state, -1)) {
            std::string chosen = lua_tostring(state, -1);
            if (chosen.rfind("res://", 0) == 0 && chosen.size() < 120) icon = chosen;
        }
        lua_pop(state, 1);
    }
    int when = LUA_NOREF;
    if (lua_istable(state, 3)) {
        lua_getfield(state, 3, "when");
        if (lua_isfunction(state, -1))
            when = luaL_ref(state, LUA_REGISTRYINDEX);
        else
            lua_pop(state, 1);
    }
    uint64_t identifier = gNextMessageAction.fetch_add(1);
    if (!identifier) identifier = gNextMessageAction.fetch_add(1);
    lua_pushvalue(state, 2);
    plugin->messageActions.push_back({identifier, std::string(label, length), icon, luaL_ref(state, LUA_REGISTRYINDEX), when});
    lua_pushinteger(state, static_cast<lua_Integer>(identifier));
    return 1;
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
        double dim = FieldNumber(state, 1, "dim", 0);
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
    auto compiled = plugin->signedByAstral ? plugin->moduleBytecode.find(name) : plugin->moduleBytecode.end();
    bool binary = compiled != plugin->moduleBytecode.end();
    const std::string &code = binary ? compiled->second : module->second;
    if (luaL_loadbufferx(state, code.data(), code.size(), chunk.c_str(), binary ? "b" : "t") != LUA_OK) return lua_error(state);
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

static int Relationship(lua_State *state)
{
    RequirePermission(state, "discord.relationships");
    auto type = gPlatform.relationship ? gPlatform.relationship(Argument(state, 1)) : std::nullopt;
    static const char *const names[] = {nullptr, "friend", "blocked", "incoming", "outgoing"};
    if (!type || *type < 1 || *type > 4) {
        lua_pushnil(state);
        return 1;
    }
    lua_pushstring(state, names[*type]);
    return 1;
}

static int Message(lua_State *state)
{
    RequirePermission(state, "discord.messages");
    auto message = gPlatform.message ? gPlatform.message(Argument(state, 1), Argument(state, 2)) : std::nullopt;
    if (message && !Self(state)->permissions.count("discord.messages.content")) message->erase("content");
    PushObject(state, message);
    return 1;
}

static int RawMessage(lua_State *state)
{
    RequirePermission(state, "discord.messages");
    RequirePermission(state, "discord.messages.content");
    auto message = gPlatform.rawMessage ? gPlatform.rawMessage(Argument(state, 1), Argument(state, 2)) : std::nullopt;
    PushObject(state, message);
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
    Function(state, plugin, "button", SettingsButton);
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
    Function(state, plugin, "speak", Speak);
    Function(state, plugin, "play", PlaySound);
    Function(state, plugin, "stop", StopAudio);
    lua_setfield(state, -2, "audio");

    lua_newtable(state);
    Function(state, plugin, "now_ms", TimeNowMilliseconds);
    Function(state, plugin, "after", TimeAfter);
    Function(state, plugin, "every", TimeEvery);
    Function(state, plugin, "cancel", TimeCancel);
    Function(state, plugin, "iso", TimeISO);
    lua_setfield(state, -2, "time");

    lua_newtable(state);
    Function(state, plugin, "decorate", Decorate);
    Function(state, plugin, "relabel", Relabel);
    Function(state, plugin, "alert", ShowAlert);
    Function(state, plugin, "confirm", Confirm);
    Function(state, plugin, "sheet", OpenSheet);
    Function(state, plugin, "sheet_update", UpdateSheet);
    Function(state, plugin, "sheet_close", CloseSheet);
    Function(state, plugin, "toast", ShowToast);
    Function(state, plugin, "copy", CopyText);
    Function(state, plugin, "paste", PasteText);
    Function(state, plugin, "open", OpenURL);
    Function(state, plugin, "open_channel", OpenChannel);
    Function(state, plugin, "scroll_to", ScrollToMessage);
    Function(state, plugin, "focus_composer", FocusComposer);
    Function(state, plugin, "press", PressNode);
    Function(state, plugin, "insert_text", InsertComposerText);
    Function(state, plugin, "refresh", RefreshDecorationsNow);
    Function(state, plugin, "composer_text", ComposerText);
    Function(state, plugin, "image_rewrite", RegisterImageRewrite);
    Function(state, plugin, "text_transform", RegisterTextTransform);
    Function(state, plugin, "notify", Notify);
    Function(state, plugin, "composer", RegisterComposerDecorator);
    Function(state, plugin, "message_background", RegisterMessageBackground);
    Function(state, plugin, "patch", RegisterNodePatch);
    Function(state, plugin, "insert", RegisterNodeInsert);
    Function(state, plugin, "haptic", Haptic);
    Function(state, plugin, "web_auth", WebAuth);
    Function(state, plugin, "cancel_web_auth", CancelWebAuth);
    Function(state, plugin, "settings_backdrop", SettingsBackdrop);
    Function(state, plugin, "mention", Mention);
    Function(state, plugin, "message_color", MessageColor);
    Function(state, plugin, "message_action", MessageAction);
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
    Function(state, plugin, "dispatch", DispatchGatewayEvent);
    lua_setfield(state, -2, "gateway");

    lua_newtable(state);
    Function(state, plugin, "register", RegisterCommand);
    lua_setfield(state, -2, "commands");

    lua_newtable(state);
    Function(state, plugin, "render", RenderImage);
    lua_setfield(state, -2, "image");

    lua_newtable(state);
    Function(state, plugin, "keep_alive", KeepAlive);
    lua_setfield(state, -2, "background");

    lua_newtable(state);
    Function(state, plugin, "environment", DiagnosticEnvironment);
    lua_setfield(state, -2, "diagnostics");

    lua_newtable(state);
    Function(state, plugin, "decode", DecodeJSON);
    Function(state, plugin, "encode", EncodeJSON);
    Function(state, plugin, "array", MarkJSONArray);
    lua_setfield(state, -2, "json");

    lua_newtable(state);
    Function(state, plugin, "transform", RegisterOutgoingMessageTransform);
    Function(state, plugin, "set_text", SetMessageText);
    Function(state, plugin, "get_text", GetMessageText);
    Function(state, plugin, "send_local", SendLocalMessage);
    lua_setfield(state, -2, "messages");

    lua_newtable(state);
    Function(state, plugin, "on_save", RegisterMediaSaveHandler);
    lua_setfield(state, -2, "media");

    lua_newtable(state);
    Function(state, plugin, "request", NetworkRequest);
    Function(state, plugin, "intercept", RegisterHttpIntercept);
    Function(state, plugin, "release", ReleaseHeldRequest);
    Function(state, plugin, "respond", RespondToHeldRequest);
    lua_pushstring(state, kHoldRequest);
    lua_setfield(state, -2, "HOLD");
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
    Function(state, plugin, "raw_message", RawMessage);
    Function(state, plugin, "reactors", Reactors);
    Function(state, plugin, "user", User);
    Function(state, plugin, "nickname", Nickname);
    Function(state, plugin, "relationship", Relationship);
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
    auto error =
        plugin->sandbox->loadBytecode(std::string(reinterpret_cast<const char *>(kAstralPrelude), sizeof(kAstralPrelude)), "astral");
    if (error) {
        Report(plugin, "prelude", *error);
        return false;
    }
    return Call(plugin, 0, 0, "prelude");
}

static void TraceRunningPlugins()
{
    std::vector<std::string> running;
    for (auto *plugin : gPlugins)
        if (plugin->state && !plugin->failed) running.push_back(plugin->identifier);
    astral::trace::setRunning(running);
}

static void Stop(LuaPlugin *plugin)
{
    ReleaseTimers(plugin);
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
    auto sheets = std::move(plugin->sheets);
    plugin->sheets.clear();
    for (const auto &[sheet, reference] : sheets) {
        if (gPlatform.closeSheet) {
            try {
                gPlatform.closeSheet(sheet);
            } catch (...) {
            }
        }
        if (plugin->state) luaL_unref(plugin->state, LUA_REGISTRYINDEX, reference);
    }
    auto confirmRequests = std::move(plugin->confirmRequests);
    plugin->confirmRequests.clear();
    for (const auto &[request, reference] : confirmRequests)
        if (plugin->state) luaL_unref(plugin->state, LUA_REGISTRYINDEX, reference);
    auto webAuthRequests = std::move(plugin->webAuthRequests);
    plugin->webAuthRequests.clear();
    for (const auto &[request, reference] : webAuthRequests) {
        if (gPlatform.cancelWebAuth) {
            try {
                gPlatform.cancelWebAuth(request);
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
    ReleaseHttpHooks(plugin);
    for (const auto &[prefix, reference] : plugin->imageRewriters)
        if (plugin->state) luaL_unref(plugin->state, LUA_REGISTRYINDEX, reference);
    plugin->imageRewriters.clear();
    for (const auto &command : plugin->commands)
        if (plugin->state) luaL_unref(plugin->state, LUA_REGISTRYINDEX, command.reference);
    plugin->commands.clear();
    for (int reference : plugin->textTransforms)
        if (plugin->state) luaL_unref(plugin->state, LUA_REGISTRYINDEX, reference);
    plugin->textTransforms.clear();
    auto eventCallbacks = std::move(plugin->eventCallbacks);
    plugin->eventCallbacks.clear();
    for (const auto &[subscription, callback] : eventCallbacks) {
        if (plugin->state) luaL_unref(plugin->state, LUA_REGISTRYINDEX, callback.reference);
    }
    auto messageActions = std::move(plugin->messageActions);
    plugin->messageActions.clear();
    for (const auto &action : messageActions) {
        if (plugin->state) luaL_unref(plugin->state, LUA_REGISTRYINDEX, action.reference);
    }
    auto outgoingMessageTransforms = std::move(plugin->outgoingMessageTransforms);
    plugin->outgoingMessageTransforms.clear();
    for (int reference : outgoingMessageTransforms) {
        if (plugin->state) luaL_unref(plugin->state, LUA_REGISTRYINDEX, reference);
    }
    auto mediaSaveHandlers = std::move(plugin->mediaSaveHandlers);
    plugin->mediaSaveHandlers.clear();
    for (int reference : mediaSaveHandlers) {
        if (plugin->state) luaL_unref(plugin->state, LUA_REGISTRYINDEX, reference);
    }
    plugin->sandbox.reset();
    plugin->state = nullptr;
    plugin->switches.clear();
    plugin->textSettings.clear();
    plugin->settingsButtons.clear();
    plugin->settingCount = 0;
    plugin->decorators.clear();
    plugin->relabelers.clear();
    plugin->settingsBackdrop.reset();
    plugin->mentioners.clear();
    plugin->colorizers.clear();
    plugin->nameColorizers.clear();
    plugin->typingDecorators.clear();
    plugin->reactionDecorators.clear();
    plugin->composerDecorators.clear();
    plugin->messageBackgroundDecorators.clear();
    plugin->nodePatches.clear();
    SyncPatchPrefixes();
    plugin->nodeInserts.clear();
    SyncInsertPrefixes();
    ApplySettingsBackdrop();
    TraceRunningPlugins();
}

static void FireTimer(uint64_t identifier)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto *plugin : gPlugins) {
        auto found = plugin->timers.find(identifier);
        if (found == plugin->timers.end()) continue;
        LuaTimer timer = found->second;
        if (!timer.interval) plugin->timers.erase(found);
        if (!plugin->state || plugin->failed) return;
        lua_State *state = plugin->state;
        int top = lua_gettop(state);
        lua_rawgeti(state, LUA_REGISTRYINDEX, timer.reference);
        if (!timer.interval) luaL_unref(state, LUA_REGISTRYINDEX, timer.reference);
        if (!Call(plugin, 0, 0, "timer")) {
            lua_settop(state, top);
            MarkFailed(plugin);
            Stop(plugin);
            return;
        }
        lua_settop(state, top);
        if (timer.interval && plugin->timers.count(identifier)) ScheduleTimer(identifier, timer.interval);
        return;
    }
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
    bool compiled = plugin->signedByAstral && !plugin->mainBytecode.empty();
    auto loaded = compiled ? plugin->sandbox->loadBytecode(plugin->mainBytecode, plugin->identifier)
                           : plugin->sandbox->load(plugin->source, plugin->identifier);
    if (auto error = loaded) {
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
    TraceRunningPlugins();
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
            plugin->enabled = StoredBool(plugin->identifier + ".enabled", !bundled || plugin->enabledByDefault);
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
    RefreshDecorations();
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
                                {"step", entry.step},
                                {"icon", entry.icon},
                                {"order", entry.order}});
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
                                    {"maxLength", entry.maximumLength},
                                    {"color", entry.color},
                                    {"icon", entry.icon},
                                    {"order", entry.order}});
        }
        nlohmann::json settingsButtons = nlohmann::json::array();
        for (const auto &entry : plugin->settingsButtons)
            settingsButtons.push_back({{"key", entry.key},
                                       {"title", entry.title},
                                       {"subtitle", entry.subtitle},
                                       {"section", entry.section},
                                       {"icon", entry.icon},
                                       {"detail", entry.detail},
                                       {"button", true},
                                       {"order", entry.order}});
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
                           {"settingsButtons", settingsButtons},
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
        if (response["body"].get_ref<const std::string &>().size() <= (64u << 10)) {
            nlohmann::json parsed = astral::parseBoundedJSON(response["body"].get_ref<const std::string &>(), 64);
            if (!parsed.is_discarded()) response["json"] = std::move(parsed);
        }
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

void sheetEvent(uint64_t sheet, const std::string &action, const std::string &value, bool closed)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto *plugin : gPlugins) {
        auto found = plugin->sheets.find(sheet);
        if (found == plugin->sheets.end()) continue;
        int reference = found->second;
        if (closed) plugin->sheets.erase(found);
        if (!plugin->state || plugin->failed) return;
        lua_State *state = plugin->state;
        lua_rawgeti(state, LUA_REGISTRYINDEX, reference);
        if (closed) luaL_unref(state, LUA_REGISTRYINDEX, reference);
        lua_pushlstring(state, closed ? "closed" : action.data(), closed ? 6 : action.size());
        lua_pushlstring(state, value.data(), value.size());
        if (!Call(plugin, 2, 0, "sheet")) {
            MarkFailed(plugin);
            Stop(plugin);
        }
        return;
    }
}

void completeConfirm(uint64_t request, bool accepted)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto *plugin : gPlugins) {
        auto found = plugin->confirmRequests.find(request);
        if (found == plugin->confirmRequests.end()) continue;
        int reference = found->second;
        plugin->confirmRequests.erase(found);
        if (!plugin->state || plugin->failed) return;
        lua_State *state = plugin->state;
        lua_rawgeti(state, LUA_REGISTRYINDEX, reference);
        luaL_unref(state, LUA_REGISTRYINDEX, reference);
        lua_pushboolean(state, accepted);
        if (!Call(plugin, 1, 0, "confirm")) {
            MarkFailed(plugin);
            Stop(plugin);
        }
        return;
    }
}

void completeWebAuth(uint64_t request, const std::optional<std::string> &url, const std::optional<std::string> &cookies,
                     const std::optional<std::string> &error)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    std::optional<std::string> resultURL = url;
    std::optional<std::string> resultCookies = cookies;
    std::optional<std::string> resultError = error;
    if (resultURL && resultURL->size() > 4096) {
        resultURL.reset();
        resultCookies.reset();
        resultError = "authorization callback URL exceeds 4096 bytes";
    }
    if (resultCookies && resultCookies->size() > 8192) {
        resultCookies.reset();
        resultError = "authorization cookies exceed 8192 bytes";
    }
    if (resultError && resultError->size() > 512) resultError->resize(512);
    for (auto *plugin : gPlugins) {
        auto found = plugin->webAuthRequests.find(request);
        if (found == plugin->webAuthRequests.end()) continue;
        int reference = found->second;
        plugin->webAuthRequests.erase(found);
        if (!plugin->state || plugin->failed) return;
        lua_State *state = plugin->state;
        lua_rawgeti(state, LUA_REGISTRYINDEX, reference);
        luaL_unref(state, LUA_REGISTRYINDEX, reference);
        if (resultURL)
            lua_pushlstring(state, resultURL->data(), resultURL->size());
        else
            lua_pushnil(state);
        if (resultCookies)
            lua_pushlstring(state, resultCookies->data(), resultCookies->size());
        else
            lua_pushnil(state);
        if (resultError)
            lua_pushlstring(state, resultError->data(), resultError->size());
        else
            lua_pushnil(state);
        if (!Call(plugin, 3, 0, "web authorization callback")) {
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

static bool FilterMessageCreate(const std::string &messageID, const std::string &channelID, const std::string &authorID,
                                const std::string &referencedAuthorID, const std::string &content, bool fromMe, bool direct, bool open,
                                bool edited)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        std::vector<uint64_t> subscriptions;
        for (const auto &[subscription, callback] : plugin->eventCallbacks)
            if (callback.event == "message.filter") subscriptions.push_back(subscription);
        for (uint64_t subscription : subscriptions) {
            auto found = plugin->eventCallbacks.find(subscription);
            if (found == plugin->eventCallbacks.end() || !plugin->state || plugin->failed) continue;
            lua_State *state = plugin->state;
            lua_rawgeti(state, LUA_REGISTRYINDEX, found->second.reference);
            lua_createtable(state, 0, 9);
            lua_pushlstring(state, messageID.data(), messageID.size());
            lua_setfield(state, -2, "message_id");
            lua_pushlstring(state, channelID.data(), channelID.size());
            lua_setfield(state, -2, "channel_id");
            lua_pushlstring(state, authorID.data(), authorID.size());
            lua_setfield(state, -2, "author_id");
            lua_pushboolean(state, edited);
            lua_setfield(state, -2, "edited");
            lua_pushlstring(state, referencedAuthorID.data(), referencedAuthorID.size());
            lua_setfield(state, -2, "referenced_author_id");
            lua_pushlstring(state, content.data(), content.size());
            lua_setfield(state, -2, "content");
            lua_pushboolean(state, fromMe);
            lua_setfield(state, -2, "from_me");
            lua_pushboolean(state, direct);
            lua_setfield(state, -2, "direct");
            lua_pushboolean(state, open);
            lua_setfield(state, -2, "open");
            if (!Call(plugin, 1, 1, "message.filter")) {
                MarkFailed(plugin);
                Stop(plugin);
                break;
            }
            bool filtered = lua_isboolean(state, -1) && lua_toboolean(state, -1);
            lua_pop(state, 1);
            if (filtered) return true;
        }
    }
    return false;
}

struct MessageMentions {
    bool user = false;
    bool everyone = false;
    bool role = false;
};

static bool MentionsMyRole(const nlohmann::json &data, const std::string &guild, const std::string &currentUser)
{
    auto roles = data.find("mention_roles");
    if (roles == data.end() || !roles->is_array() || roles->empty() || guild.empty() || !gPlatform.member) return false;
    auto member = gPlatform.member(guild, currentUser);
    if (!member || !member->is_object()) return false;
    auto mine = member->find("roles");
    if (mine == member->end() || !mine->is_array()) return false;
    for (const auto &role : *roles)
        for (const auto &held : *mine)
            if (role.is_string() && held.is_string() && role == held) return true;
    return false;
}

static void EmitMessageCreate(const std::string &messageID, const std::string &channelID, const std::string &authorID,
                              const std::string &guildID, bool fromMe, const MessageMentions &mentions, bool direct, bool open)
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
            lua_createtable(state, 0, 10);
            lua_pushlstring(state, messageID.data(), messageID.size());
            lua_setfield(state, -2, "message_id");
            if (!guildID.empty()) {
                lua_pushlstring(state, guildID.data(), guildID.size());
                lua_setfield(state, -2, "guild_id");
            }
            lua_pushboolean(state, mentions.everyone);
            lua_setfield(state, -2, "everyone");
            lua_pushboolean(state, mentions.role);
            lua_setfield(state, -2, "role_mentioned");
            lua_pushlstring(state, channelID.data(), channelID.size());
            lua_setfield(state, -2, "channel_id");
            lua_pushlstring(state, authorID.data(), authorID.size());
            lua_setfield(state, -2, "author_id");
            lua_pushboolean(state, fromMe);
            lua_setfield(state, -2, "from_me");
            lua_pushboolean(state, mentions.user);
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

enum class GatewayVerdict { Keep, Changed, Drop };

static bool WantsGatewayType(const LuaEventCallback &callback, const std::string &type)
{
    return callback.event == "gateway" && (callback.types.empty() || callback.types.count(type));
}

static GatewayVerdict RunGatewaySubscribers(nlohmann::json &envelope)
{
    auto event = envelope.find("t");
    if (event == envelope.end() || !event->is_string()) return GatewayVerdict::Keep;
    std::string type = event->get<std::string>();
    GatewayVerdict verdict = GatewayVerdict::Keep;
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        std::vector<uint64_t> subscriptions;
        for (const auto &[subscription, callback] : plugin->eventCallbacks)
            if (WantsGatewayType(callback, type)) subscriptions.push_back(subscription);
        for (uint64_t subscription : subscriptions) {
            auto found = plugin->eventCallbacks.find(subscription);
            if (found == plugin->eventCallbacks.end() || !plugin->state || plugin->failed) continue;
            lua_State *state = plugin->state;
            lua_rawgeti(state, LUA_REGISTRYINDEX, found->second.reference);
            lua_createtable(state, 0, 4);
            lua_pushlstring(state, type.data(), type.size());
            lua_setfield(state, -2, "type");
            if (envelope.contains("s") && envelope["s"].is_number_integer()) {
                lua_pushinteger(state, envelope["s"].get<lua_Integer>());
                lua_setfield(state, -2, "seq");
            }
            astral::lua::pushJSON(state, envelope.value("d", nlohmann::json()));
            lua_setfield(state, -2, "data");
            if (!Call(plugin, 1, 1, "gateway")) {
                MarkFailed(plugin);
                Stop(plugin);
                break;
            }
            if (lua_isboolean(state, -1) && !lua_toboolean(state, -1)) {
                lua_pop(state, 1);
                envelope["t"] = "ASTRAL_FILTERED";
                return GatewayVerdict::Drop;
            }
            if (lua_istable(state, -1)) {
                nlohmann::json replaced;
                if (astral::lua::toJSON(state, -1, replaced)) {
                    envelope["d"] = std::move(replaced);
                    verdict = GatewayVerdict::Changed;
                }
            }
            lua_pop(state, 1);
        }
    }
    return verdict;
}

bool needsGatewayEvents()
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (const auto &[subscription, callback] : plugin->eventCallbacks)
            if (callback.event == "gateway" || callback.event == "message.create" || callback.event == "message.filter") return true;
        if (plugin->permissions.count("messages.local")) return true;
    }
    return false;
}

static std::optional<std::string> processKnownGatewayEvents(nlohmann::json &envelope);

static std::string CommandID(const std::string &plugin, const std::string &name)
{
    uint64_t hash = 1469598103934665603ull;
    for (char character : plugin + "/" + name) hash = (hash ^ static_cast<uint8_t>(character)) * 1099511628211ull;
    return std::to_string((hash >> 2) | (1ull << 60));
}

static bool HasCommands()
{
    for (auto *plugin : gPlugins)
        if (plugin->state && !plugin->failed && !plugin->commands.empty()) return true;
    return false;
}

static bool IsCommandIndex(const std::string &method, const std::string &path)
{
    static const std::string suffix = "/application-command-index";
    return method == "GET" && path.size() > suffix.size() && path.compare(path.size() - suffix.size(), suffix.size(), suffix) == 0;
}

static bool IsInteraction(const std::string &method, const std::string &path)
{
    static const std::string suffix = "/interactions";
    return method == "POST" && path.size() >= suffix.size() && path.compare(path.size() - suffix.size(), suffix.size(), suffix) == 0;
}

static bool InjectCommands(std::string &body)
{
    nlohmann::json index = nlohmann::json::parse(body, nullptr, false);
    if (!index.is_object()) return false;
    if (!index.contains("applications") || !index["applications"].is_array()) index["applications"] = nlohmann::json::array();
    if (!index.contains("application_commands") || !index["application_commands"].is_array())
        index["application_commands"] = nlohmann::json::array();
    index["applications"].push_back({{"id", kCommandApplication},
                                     {"name", "astral"},
                                     {"description", "Commands from astral plugins"},
                                     {"icon", nullptr},
                                     {"bot_id", kCommandApplication},
                                     {"flags", "0"}});
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (const auto &command : plugin->commands)
            index["application_commands"].push_back({{"id", CommandID(plugin->identifier, command.name)},
                                                     {"application_id", kCommandApplication},
                                                     {"version", "1"},
                                                     {"type", 1},
                                                     {"name", command.name},
                                                     {"description", command.description},
                                                     {"options", command.options},
                                                     {"contexts", {0, 1, 2}},
                                                     {"integration_types", {0, 1}},
                                                     {"dm_permission", true}});
    }
    body = index.dump();
    return true;
}

static std::optional<nlohmann::json> InteractionPayload(const HttpRequest &request)
{
    std::string contentType;
    for (const auto &[name, value] : request.headers)
        if (name == "content-type") contentType = value;
    if (contentType.find("multipart/form-data") == std::string::npos) {
        nlohmann::json payload = nlohmann::json::parse(request.body, nullptr, false);
        if (payload.is_object()) return payload;
        return std::nullopt;
    }
    size_t field = request.body.find("name=\"payload_json\"");
    if (field == std::string::npos) return std::nullopt;
    size_t start = request.body.find("\r\n\r\n", field);
    if (start == std::string::npos) return std::nullopt;
    start += 4;
    size_t end = request.body.find("\r\n--", start);
    nlohmann::json payload =
        nlohmann::json::parse(request.body.substr(start, end == std::string::npos ? std::string::npos : end - start), nullptr, false);
    if (payload.is_object()) return payload;
    return std::nullopt;
}

static void PushOptionValues(lua_State *state, const nlohmann::json &options)
{
    lua_createtable(state, 0, 4);
    if (!options.is_array()) return;
    for (const auto &option : options) {
        if (!option.is_object() || !option.contains("name") || !option["name"].is_string()) continue;
        astral::lua::pushJSON(state, option.value("value", nlohmann::json()));
        lua_setfield(state, -2, option["name"].get<std::string>().c_str());
    }
}

static std::string MultipartMessage(const nlohmann::json &payload, const std::string &filename, const std::string &type,
                                    const std::string &data, std::string &boundary)
{
    boundary = "astral" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count());
    std::string body = "--" + boundary +
                       "\r\nContent-Disposition: form-data; name=\"payload_json\"\r\nContent-Type: application/json\r\n\r\n" +
                       payload.dump() + "\r\n";
    body += "--" + boundary + "\r\nContent-Disposition: form-data; name=\"files[0]\"; filename=\"" + filename +
            "\"\r\nContent-Type: " + type + "\r\n\r\n" + data + "\r\n--" + boundary + "--\r\n";
    return body;
}

static std::optional<HttpDecision> HandleInteraction(HttpRequest &request)
{
    auto payload = InteractionPayload(request);
    if (!payload || payload->value("application_id", std::string()) != kCommandApplication) return std::nullopt;
    const nlohmann::json data = payload->value("data", nlohmann::json::object());
    std::string name = data.value("name", std::string());
    std::string channel = payload->value("channel_id", std::string());
    HttpDecision decision;
    decision.response = HttpResponse{204, {}, ""};
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (const auto &command : plugin->commands) {
            if (command.name != name) continue;
            lua_State *state = plugin->state;
            int top = lua_gettop(state);
            lua_rawgeti(state, LUA_REGISTRYINDEX, command.reference);
            PushOptionValues(state, data.value("options", nlohmann::json::array()));
            lua_createtable(state, 0, 2);
            lua_pushstring(state, channel.c_str());
            lua_setfield(state, -2, "channel_id");
            if (payload->contains("guild_id") && (*payload)["guild_id"].is_string()) {
                lua_pushstring(state, (*payload)["guild_id"].get<std::string>().c_str());
                lua_setfield(state, -2, "guild_id");
            }
            if (!Call(plugin, 2, 1, "command")) {
                lua_settop(state, top);
                MarkFailed(plugin);
                Stop(plugin);
                return decision;
            }
            nlohmann::json reply;
            bool hasReply = !lua_isnil(state, -1) && astral::lua::toJSON(state, -1, reply);
            lua_settop(state, top);
            std::string nonce = payload->value("nonce", std::string());
            std::string interaction = CommandID("interaction", std::to_string(gNextRequest.fetch_add(1)));
            if (gPlatform.dispatchGateway) {
                nlohmann::json created = {
                    {"op", 0}, {"t", "INTERACTION_CREATE"}, {"s", nullptr}, {"d", {{"id", interaction}, {"nonce", nonce}}}};
                nlohmann::json succeeded = {
                    {"op", 0}, {"t", "INTERACTION_SUCCESS"}, {"s", nullptr}, {"d", {{"id", interaction}, {"nonce", nonce}}}};
                gPlatform.dispatchGateway(created.dump());
                gPlatform.dispatchGateway(succeeded.dump());
            }
            bool sends = hasReply && reply.is_object() && reply.contains("send") && reply["send"].is_object() && SnowflakeText(channel);
            if ((!hasReply || sends) && gPlatform.dispatchGateway && !nonce.empty()) {
                nlohmann::json placeholder = {
                    {"op", 0}, {"t", "MESSAGE_DELETE"}, {"s", nullptr}, {"d", {{"id", nonce}, {"channel_id", channel}}}};
                gPlatform.dispatchGateway(placeholder.dump());
            }
            if (!hasReply) return decision;
            if (reply.is_string()) reply = {{"content", reply}};
            if (sends) {
                nlohmann::json message = reply["send"];
                nlohmann::json file = message.value("file", nlohmann::json());
                message.erase("file");
                if (!nonce.empty()) message["nonce"] = nonce;
                request.method = "POST";
                request.url = "https://discord.com/api/v9/channels/" + channel + "/messages";
                request.path = UrlPath(request.url);
                std::vector<std::pair<std::string, std::string>> headers;
                for (const auto &[header, value] : request.headers)
                    if (header != "content-type" && header != "content-length") headers.emplace_back(header, value);
                if (file.is_object() && file.contains("data") && file["data"].is_string()) {
                    std::string filename = file.value("name", std::string("file.png"));
                    nlohmann::json attachment = {{"id", 0}, {"filename", filename}};
                    if (file.contains("description") && file["description"].is_string())
                        attachment["description"] = file["description"].get<std::string>().substr(0, 1024);
                    message["attachments"] = nlohmann::json::array({attachment});
                    std::string boundary;
                    request.body = MultipartMessage(message, filename, file.value("content_type", std::string("image/png")),
                                                    file["data"].get<std::string>(), boundary);
                    headers.emplace_back("content-type", "multipart/form-data; boundary=" + boundary);
                } else {
                    request.body = message.dump();
                    headers.emplace_back("content-type", "application/json");
                }
                request.headers = std::move(headers);
                decision.response.reset();
                decision.changed = true;
                return decision;
            }
            if (reply.is_object() && SnowflakeText(channel)) {
                nlohmann::json local = reply;
                local.erase("send");
                local["nonce"] = nonce;
                local["type"] = 20;
                local["application_id"] = kCommandApplication;
                nlohmann::json user = {{"id", gCurrentUserID}};
                if (gPlatform.user && !gCurrentUserID.empty()) {
                    auto cached = gPlatform.user(gCurrentUserID);
                    if (cached && cached->is_object()) user = *cached;
                }
                local["interaction_metadata"] = {{"id", interaction}, {"type", 2}, {"name", name}, {"user", user}};
                SendLocalMessageJSON(channel, local);
            }
            return decision;
        }
    }
    return decision;
}

bool wantsHttp(const std::string &method, const std::string &host, const std::string &path)
{
    if (!astral::isDiscordHost(host)) return false;
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if ((IsCommandIndex(method, path) || IsInteraction(method, path)) && HasCommands()) return true;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (const auto &hook : plugin->httpHooks)
            if (HookMatches(plugin, hook, method, path)) return true;
    }
    return false;
}

HttpDecision interceptHttpRequest(HttpRequest &request)
{
    HttpDecision decision;
    if (request.body.size() > kMaximumHttpBody) return decision;
    std::unique_lock<std::recursive_mutex> lock(gMutex);
    if (IsInteraction(request.method, request.path) && HasCommands()) {
        auto handled = HandleInteraction(request);
        if (handled) return *handled;
    }
    std::vector<LuaPlugin *> plugins(gPlugins.begin(), gPlugins.end());
    for (auto *plugin : plugins) {
        size_t hooks = plugin->httpHooks.size();
        for (size_t index = 0; index < hooks; index++) {
            if (!plugin->state || plugin->failed || index >= plugin->httpHooks.size()) break;
            const LuaHttpHook hook = plugin->httpHooks[index];
            if (hook.request == LUA_NOREF || !HookMatches(plugin, hook, request.method, request.path)) continue;
            uint64_t exchange = gNextRequest.fetch_add(1);
            lua_State *state = plugin->state;
            lua_rawgeti(state, LUA_REGISTRYINDEX, hook.request);
            PushHttpRequest(state, exchange, request);
            if (!Call(plugin, 1, 1, "net.intercept request")) {
                MarkFailed(plugin);
                Stop(plugin);
                break;
            }
            std::optional<nlohmann::json> changes;
            bool hold = lua_isstring(state, -1) && std::string(lua_tostring(state, -1)) == kHoldRequest;
            if (lua_istable(state, -1)) {
                nlohmann::json value;
                if (astral::lua::toJSON(state, -1, value) && value.is_object()) changes = std::move(value);
            }
            lua_pop(state, 1);
            if (hold) {
                auto pending = std::make_shared<HttpHold>();
                pending->owner = plugin;
                {
                    std::lock_guard<std::mutex> holdLock(gHoldMutex);
                    gHttpHolds[exchange] = pending;
                }
                lock.unlock();
                {
                    std::unique_lock<std::mutex> waiting(pending->mutex);
                    if (!pending->ready.wait_for(waiting, kMaximumHold, [&] { return pending->done; })) {
                        waiting.unlock();
                        FinishHold(exchange, std::nullopt, std::nullopt);
                        waiting.lock();
                    }
                    changes = pending->changes;
                    if (pending->response) {
                        decision.response = pending->response;
                        return decision;
                    }
                }
                lock.lock();
            }
            if (!changes) continue;
            if (changes->contains("status")) {
                decision.response = ResponseFromJSON(*changes);
                return decision;
            }
            ApplyRequestChanges(request, *changes);
            decision.changed = true;
        }
    }
    return decision;
}

bool interceptHttpResponse(const HttpRequest &request, HttpResponse &response)
{
    if (response.body.size() > kMaximumHttpBody) return false;
    bool changed = false;
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (IsCommandIndex(request.method, request.path) && response.status == 200 && HasCommands()) changed = InjectCommands(response.body);
    for (auto *plugin : gPlugins) {
        for (size_t index = 0; index < plugin->httpHooks.size(); index++) {
            if (!plugin->state || plugin->failed) break;
            const LuaHttpHook hook = plugin->httpHooks[index];
            if (hook.response == LUA_NOREF || !HookMatches(plugin, hook, request.method, request.path)) continue;
            lua_State *state = plugin->state;
            lua_rawgeti(state, LUA_REGISTRYINDEX, hook.response);
            PushHttpRequest(state, 0, request);
            lua_createtable(state, 0, 3);
            lua_pushinteger(state, response.status);
            lua_setfield(state, -2, "status");
            PushHeaders(state, response.headers);
            lua_setfield(state, -2, "headers");
            lua_pushlstring(state, response.body.data(), response.body.size());
            lua_setfield(state, -2, "body");
            if (!Call(plugin, 2, 1, "net.intercept response")) {
                MarkFailed(plugin);
                Stop(plugin);
                break;
            }
            if (lua_istable(state, -1)) {
                nlohmann::json value;
                if (astral::lua::toJSON(state, -1, value) && value.is_object()) {
                    if (value.contains("status") && value["status"].is_number_integer()) {
                        int status = value["status"].get<int>();
                        if (status >= 100 && status <= 599) response.status = status;
                    }
                    if (value.contains("headers") && value["headers"].is_object())
                        response.headers = HeadersFromJSON(value["headers"], response.headers);
                    if (value.contains("body") && value["body"].is_string()) response.body = value["body"].get<std::string>();
                    changed = true;
                }
            }
            lua_pop(state, 1);
        }
    }
    return changed;
}

std::optional<std::string> processGatewayMessage(std::string_view message)
{
    if (message.empty() || message.size() > (8u << 20)) return std::nullopt;
    nlohmann::json envelope = nlohmann::json::parse(message, nullptr, false);
    if (!envelope.is_object()) return std::nullopt;
    GatewayVerdict verdict = RunGatewaySubscribers(envelope);
    if (verdict == GatewayVerdict::Drop) return envelope.dump();
    auto known = processKnownGatewayEvents(envelope);
    if (known) return known;
    if (verdict == GatewayVerdict::Changed) return envelope.dump();
    return std::nullopt;
}

static std::optional<std::string> processKnownGatewayEvents(nlohmann::json &envelope)
{
    auto event = envelope.find("t");
    auto data = envelope.find("d");
    if (event == envelope.end() || !event->is_string() || data == envelope.end() || !data->is_object()) return std::nullopt;
    std::string type = event->get<std::string>();
    if (type == "READY") {
        auto user = data->find("user");
        if (user == data->end() || !user->is_object()) return std::nullopt;
        auto id = user->find("id");
        if (id == user->end() || !id->is_string()) return std::nullopt;
        std::string currentUser = id->get<std::string>();
        if (!IsSnowflake(currentUser)) return std::nullopt;
        std::lock_guard<std::recursive_mutex> lock(gMutex);
        gCurrentUserID = std::move(currentUser);
        return std::nullopt;
    }
    bool edited = type == "MESSAGE_UPDATE";
    if (type != "MESSAGE_CREATE" && !edited) return std::nullopt;
    auto channel = data->find("channel_id");
    auto author = data->find("author");
    if (channel == data->end() || !channel->is_string() || author == data->end() || !author->is_object()) return std::nullopt;
    auto authorID = author->find("id");
    if (authorID == author->end() || !authorID->is_string()) return std::nullopt;
    std::string channelID = channel->get<std::string>();
    std::string senderID = authorID->get<std::string>();
    if (!IsSnowflake(channelID) || !IsSnowflake(senderID)) return std::nullopt;
    std::string messageID;
    auto id = data->find("id");
    if (id != data->end() && id->is_string() && IsSnowflake(id->get<std::string>())) messageID = id->get<std::string>();
    std::string content;
    auto contentField = data->find("content");
    if (contentField != data->end() && contentField->is_string()) content = contentField->get<std::string>();
    std::string currentUser;
    {
        std::lock_guard<std::recursive_mutex> lock(gMutex);
        currentUser = gCurrentUserID;
    }
    std::string referencedAuthorID;
    auto referenced = data->find("referenced_message");
    if (referenced != data->end() && referenced->is_object()) {
        auto referencedAuthor = referenced->find("author");
        if (referencedAuthor != referenced->end() && referencedAuthor->is_object()) {
            auto referencedID = referencedAuthor->find("id");
            if (referencedID != referencedAuthor->end() && referencedID->is_string() && IsSnowflake(referencedID->get<std::string>()))
                referencedAuthorID = referencedID->get<std::string>();
        }
    }
    MessageMentions mentions;
    bool &mentioned = mentions.user;
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
    std::string guildID = direct ? std::string() : guild->get<std::string>();
    auto everyone = data->find("mention_everyone");
    mentions.everyone = everyone != data->end() && everyone->is_boolean() && everyone->get<bool>();
    mentions.role = !currentUser.empty() && MentionsMyRole(*data, guildID, currentUser);
    uint64_t selectedChannel = astral_wire_current_channel();
    bool open = selectedChannel != 0 && std::to_string(selectedChannel) == channelID;
    bool fromMe = senderID == currentUser;
    if (edited) {
        if (fromMe || messageID.empty() ||
            !FilterMessageCreate(messageID, channelID, senderID, referencedAuthorID, content, fromMe, direct, open, true))
            return std::nullopt;
        nlohmann::json removed = {{"id", messageID}, {"channel_id", channelID}};
        if (!guildID.empty()) removed["guild_id"] = guildID;
        envelope["t"] = "MESSAGE_DELETE";
        envelope["d"] = removed;
        return envelope.dump();
    }
    if (!fromMe && FilterMessageCreate(messageID, channelID, senderID, referencedAuthorID, content, fromMe, direct, open, false)) {
        envelope["t"] = "ASTRAL_FILTERED";
        return envelope.dump();
    }
    EmitMessageCreate(messageID, channelID, senderID, guildID, fromMe, mentions, direct, open);
    return std::nullopt;
}

struct PendingMediaSave {
    MediaSave save;
    std::vector<std::pair<std::string, size_t>> handlers;
    size_t next = 0;
    std::function<void(std::optional<std::string>)> done;
};

static std::map<uint64_t, PendingMediaSave> gMediaSaves;
static uint64_t gNextMediaSave = 1;

static void AdvanceMediaSave(uint64_t id);

static void FinishMediaSave(uint64_t id, std::optional<std::string> url)
{
    auto node = gMediaSaves.extract(id);
    if (node.empty()) return;
    node.mapped().done(std::move(url));
}

static bool IsSaveURL(const std::string &url)
{
    return url.size() <= 4096 && url.rfind("https://", 0) == 0;
}

static int ReplyMediaSave(lua_State *state)
{
    auto id = static_cast<uint64_t>(lua_tointeger(state, lua_upvalueindex(1)));
    auto step = static_cast<size_t>(lua_tointeger(state, lua_upvalueindex(2)));
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    auto pending = gMediaSaves.find(id);
    if (pending == gMediaSaves.end() || pending->second.next != step) return 0;
    if (lua_type(state, 1) == LUA_TSTRING) {
        std::string url = lua_tostring(state, 1);
        if (IsSaveURL(url)) {
            FinishMediaSave(id, url);
            return 0;
        }
    }
    AdvanceMediaSave(id);
    return 0;
}

static void PushMediaSave(lua_State *state, const MediaSave &save)
{
    lua_createtable(state, 0, 4);
    lua_pushlstring(state, save.url.data(), save.url.size());
    lua_setfield(state, -2, "url");
    lua_pushlstring(state, save.kind.data(), save.kind.size());
    lua_setfield(state, -2, "kind");
    if (!save.channelID.empty()) {
        lua_pushlstring(state, save.channelID.data(), save.channelID.size());
        lua_setfield(state, -2, "channel_id");
    }
    if (!save.messageID.empty()) {
        lua_pushlstring(state, save.messageID.data(), save.messageID.size());
        lua_setfield(state, -2, "message_id");
    }
}

static void AdvanceMediaSave(uint64_t id)
{
    while (true) {
        auto pending = gMediaSaves.find(id);
        if (pending == gMediaSaves.end()) return;
        if (pending->second.next >= pending->second.handlers.size()) {
            FinishMediaSave(id, std::nullopt);
            return;
        }
        auto [pluginID, index] = pending->second.handlers[pending->second.next++];
        size_t step = pending->second.next;
        LuaPlugin *plugin = Find(pluginID);
        if (!plugin || !plugin->state || plugin->failed || index >= plugin->mediaSaveHandlers.size()) continue;
        lua_State *state = plugin->state;
        int top = lua_gettop(state);
        lua_rawgeti(state, LUA_REGISTRYINDEX, plugin->mediaSaveHandlers[index]);
        PushMediaSave(state, pending->second.save);
        lua_pushinteger(state, static_cast<lua_Integer>(id));
        lua_pushinteger(state, static_cast<lua_Integer>(step));
        lua_pushcclosure(state, ReplyMediaSave, 2);
        if (!Call(plugin, 2, 0, "media save")) {
            lua_settop(state, top);
            MarkFailed(plugin);
            Stop(plugin);
            auto again = gMediaSaves.find(id);
            if (again != gMediaSaves.end() && again->second.next == step) continue;
            return;
        }
        lua_settop(state, top);
        return;
    }
}

bool hasMediaSaveHandlers()
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto *plugin : gPlugins)
        if (plugin->state && !plugin->failed && !plugin->mediaSaveHandlers.empty()) return true;
    return false;
}

uint64_t resolveMediaSave(const MediaSave &save, std::function<void(std::optional<std::string>)> done)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    PendingMediaSave pending{save, {}, 0, std::move(done)};
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (size_t index = 0; index < plugin->mediaSaveHandlers.size(); index++) pending.handlers.emplace_back(plugin->identifier, index);
    }
    uint64_t id = gNextMediaSave++;
    gMediaSaves.emplace(id, std::move(pending));
    AdvanceMediaSave(id);
    return id;
}

void cancelMediaSave(uint64_t id)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    gMediaSaves.erase(id);
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

nlohmann::json composerStyles(std::string_view text)
{
    nlohmann::json result = nlohmann::json::array();
    if (text.empty() || text.size() > 8192 || !ValidUTF8(std::string(text))) return result;
    static const std::set<std::string> styles = {"bold",   "italic",   "underline", "strike",   "code",    "spoiler",
                                                 "marker", "heading1", "heading2",  "heading3", "subtext", "quote"};
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed || plugin->composerDecorators.empty()) continue;
        lua_State *state = plugin->state;
        for (int reference : plugin->composerDecorators) {
            int top = lua_gettop(state);
            lua_rawgeti(state, LUA_REGISTRYINDEX, reference);
            lua_pushlstring(state, text.data(), text.size());
            if (!Call(plugin, 1, 1, "composer formatting")) {
                lua_settop(state, top);
                MarkFailed(plugin);
                Stop(plugin);
                break;
            }
            nlohmann::json formatted;
            if (astral::lua::toJSON(state, -1, formatted) && formatted.is_array()) {
                for (size_t index = 0; index < formatted.size() && index < 128 && result.size() < 256; index++) {
                    const auto &run = formatted[index];
                    if (!run.is_object()) continue;
                    auto start = run.find("start");
                    auto length = run.find("length");
                    auto style = run.find("style");
                    if (start == run.end() || length == run.end() || style == run.end() || !start->is_number_integer() ||
                        !length->is_number_integer() || !style->is_string())
                        continue;
                    int64_t byteStart = start->get<int64_t>();
                    int64_t byteLength = length->get<int64_t>();
                    std::string name = style->get<std::string>();
                    if (byteStart < 1 || byteLength < 1 || byteStart > static_cast<int64_t>(text.size()) ||
                        byteLength > static_cast<int64_t>(text.size()) - byteStart + 1 || !styles.contains(name))
                        continue;
                    size_t first = static_cast<size_t>(byteStart - 1);
                    size_t end = first + static_cast<size_t>(byteLength);
                    if ((first > 0 && (static_cast<uint8_t>(text[first]) & 0xc0) == 0x80) ||
                        (end < text.size() && (static_cast<uint8_t>(text[end]) & 0xc0) == 0x80))
                        continue;
                    result.push_back({{"start", byteStart}, {"length", byteLength}, {"style", std::move(name)}});
                }
            }
            lua_settop(state, top);
            if (result.size() >= 256) return result;
        }
    }
    return result;
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
    astral::trace::enter(plugin->identifier);
    auto evaluated = plugin->sandbox->call(0, LUA_MULTRET);
    astral::trace::leave();
    if (evaluated) {
        lua_settop(state, top);
        return "error: " + *evaluated;
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
    RefreshDecorations();
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

void pressSettingsButton(const std::string &pluginID, const std::string &key)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    LuaPlugin *plugin = Find(pluginID);
    if (!plugin || !plugin->state || plugin->failed) return;
    for (const auto &button : plugin->settingsButtons) {
        if (button.key != key || button.reference == LUA_NOREF) continue;
        lua_rawgeti(plugin->state, LUA_REGISTRYINDEX, button.reference);
        if (!Call(plugin, 0, 0, "settings button")) {
            MarkFailed(plugin);
            Stop(plugin);
        }
        return;
    }
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
    if (setting->color && !astral::settings::isColorText(value)) return;
    if (StoredString(SettingKey(plugin, key), setting->fallback) == value) return;
    Store(SettingKey(plugin, key), value);
    NotifySetting(plugin, key, [&](lua_State *state) { lua_pushlstring(state, value.data(), value.size()); });
}

} // namespace astral::luahost

static std::string Snowflake(uint64_t value)
{
    return value ? std::to_string(value) : std::string();
}

static std::optional<std::string> PluginIconURI(const LuaPlugin *plugin, const std::string &icon)
{
    if (icon.size() >= 500) return std::nullopt;
    if (icon.rfind("res://", 0) == 0 || icon.rfind("https://", 0) == 0) return icon;
    std::filesystem::path requested(icon);
    if (icon.empty() || requested.is_absolute()) return std::nullopt;
    for (const auto &part : requested)
        if (part == "..") return std::nullopt;
    std::string extension = requested.extension().string();
    if (extension != ".png" && extension != ".webp") return std::nullopt;
    std::error_code error;
    std::filesystem::path full = std::filesystem::path(plugin->directory) / requested;
    if (!std::filesystem::is_regular_file(full, error)) return std::nullopt;
    return "file://" + full.string();
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
            lua_getfield(state, -1, "icon");
            auto icon = lua_isstring(state, -1) ? PluginIconURI(plugin, lua_tostring(state, -1)) : std::nullopt;
            lua_pop(state, 1);
            if (icon) {
                AstralTag *tag = &tags[(*count)++];
                memset(tag, 0, sizeof(*tag));
                tag->shape = kAstralTagIcon;
                snprintf(tag->icon, sizeof(tag->icon), "%s", icon->c_str());
                lua_getfield(state, -1, "tint");
                if (lua_isinteger(state, -1)) tag->tint = (uint32_t)lua_tointeger(state, -1);
                lua_pop(state, 1);
                lua_getfield(state, -1, "text");
                if (lua_isstring(state, -1)) snprintf(tag->text, sizeof(tag->text), "%s", lua_tostring(state, -1));
                lua_pop(state, 2);
                continue;
            }
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

static std::string MessageAuthor(const std::string &channel, const std::string &message)
{
    if (channel.empty() || message.empty() || !gPlatform.message) return "";
    auto record = gPlatform.message(channel, message);
    if (!record || !record->is_object()) return "";
    const auto &author = record->value("author", nlohmann::json::object());
    std::string user = author.is_object() ? author.value("id", std::string()) : std::string();
    return SnowflakeText(user) ? user : "";
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
    if (!lock.owns_lock()) {
        RedrawAfterBusy();
        return -1;
    }
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
    if (!lock.owns_lock()) {
        RedrawAfterBusy();
        return 0;
    }
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
    if (!lock.owns_lock()) {
        RedrawAfterBusy();
        return 0;
    }
    bool wanted = false;
    bool wantsReplyContext = false;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed || plugin->colorizers.empty()) continue;
        wanted = true;
        wantsReplyContext = wantsReplyContext || plugin->permissions.count("discord.messages");
    }
    if (!wanted) return 0;
    std::string message = Snowflake(messageID);
    std::string channel = ResolveChannel(channelID, message);
    if (channel.empty() || !gPlatform.message) return -1;
    auto messageRecord = gPlatform.message(channel, message);
    if (!messageRecord) return -1;
    bool repliesToCurrentUser = false;
    if (wantsReplyContext && !gCurrentUserID.empty() && messageRecord->is_object()) {
        const auto &author = messageRecord->value("author", nlohmann::json::object());
        const auto authorID = author.is_object() ? author.value("id", std::string()) : std::string();
        const auto &reference = messageRecord->value("reference", nlohmann::json::object());
        if (!authorID.empty() && authorID != gCurrentUserID && reference.is_object()) {
            const auto referencedChannel = reference.value("channel_id", std::string());
            const auto referencedMessage = reference.value("message_id", std::string());
            if (!referencedChannel.empty() && !referencedMessage.empty()) {
                auto target = gPlatform.message(referencedChannel, referencedMessage);
                if (target && target->is_object()) {
                    const auto &targetAuthor = target->value("author", nlohmann::json::object());
                    repliesToCurrentUser = targetAuthor.is_object() && targetAuthor.value("id", std::string()) == gCurrentUserID;
                }
            }
        }
    }
    auto guild = gPlatform.channelGuild ? gPlatform.channelGuild(channel) : std::nullopt;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (int colorizer : plugin->colorizers) {
            lua_State *state = plugin->state;
            lua_rawgeti(state, LUA_REGISTRYINDEX, colorizer);
            lua_createtable(state, 0, 4);
            lua_pushstring(state, message.c_str());
            lua_setfield(state, -2, "message_id");
            lua_pushstring(state, channel.c_str());
            lua_setfield(state, -2, "channel_id");
            if (plugin->permissions.count("discord.messages")) {
                lua_pushboolean(state, repliesToCurrentUser);
                lua_setfield(state, -2, "reply_to_me");
            }
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

static bool PatchColor(lua_State *state, const char *field, uint32_t *color)
{
    lua_getfield(state, -1, field);
    bool valid = lua_isinteger(state, -1) && lua_tointeger(state, -1) >= 0 && static_cast<uint64_t>(lua_tointeger(state, -1)) <= UINT32_MAX;
    if (valid) *color = static_cast<uint32_t>(lua_tointeger(state, -1));
    lua_pop(state, 1);
    return valid;
}

static bool PatchNumber(lua_State *state, const char *field, double minimum, double maximum, double *value)
{
    lua_getfield(state, -1, field);
    bool valid = lua_isnumber(state, -1) && std::isfinite(lua_tonumber(state, -1)) && lua_tonumber(state, -1) >= minimum &&
                 lua_tonumber(state, -1) <= maximum;
    if (valid) *value = lua_tonumber(state, -1);
    lua_pop(state, 1);
    return valid;
}

static bool PatchQuad(lua_State *state, const char *field, float quad[4])
{
    lua_getfield(state, -1, field);
    bool valid = false;
    if (lua_isnumber(state, -1)) {
        double value = lua_tonumber(state, -1);
        valid = std::isfinite(value) && value >= -64 && value <= 64;
        for (int index = 0; index < 4 && valid; index++) quad[index] = static_cast<float>(value);
    } else if (lua_istable(state, -1) && luaL_len(state, -1) == 4) {
        valid = true;
        for (int index = 0; index < 4; index++) {
            lua_rawgeti(state, -1, index + 1);
            double value = lua_isnumber(state, -1) ? lua_tonumber(state, -1) : NAN;
            valid = valid && std::isfinite(value) && value >= -64 && value <= 64;
            quad[index] = static_cast<float>(value);
            lua_pop(state, 1);
        }
    }
    lua_pop(state, 1);
    return valid;
}

static void ReadNodePatch(lua_State *state, AstralNodePatch *patch)
{
    if (PatchColor(state, "background", &patch->background)) patch->fields |= AstralPatchBackground;
    if (PatchNumber(state, "radius", 0, 64, &patch->radius)) patch->fields |= AstralPatchRadius;
    if (PatchNumber(state, "opacity", 0, 1, &patch->opacity)) patch->fields |= AstralPatchOpacity;
    if (PatchQuad(state, "padding", patch->padding)) patch->fields |= AstralPatchPadding;
    if (PatchQuad(state, "margin", patch->margin)) patch->fields |= AstralPatchMargin;
    if (PatchColor(state, "border_color", &patch->borderColor) && PatchNumber(state, "border_width", 0, 8, &patch->borderWidth))
        patch->fields |= AstralPatchBorder;
    if (PatchColor(state, "text_color", &patch->textColor)) patch->fields |= AstralPatchTextColor;
    if (PatchNumber(state, "text_scale", 0.5, 3, &patch->textScale)) patch->fields |= AstralPatchTextScale;
    lua_getfield(state, -1, "hidden");
    if (lua_toboolean(state, -1)) patch->fields |= AstralPatchHidden;
    lua_pop(state, 1);
}

static void PushSnowflakeField(lua_State *state, const char *field, uint64_t value)
{
    if (!value) return;
    lua_pushstring(state, std::to_string(value).c_str());
    lua_setfield(state, -2, field);
}

static void PushTargetAuthor(lua_State *state, const AstralPatchTarget *target)
{
    if (target->author_id || !target->message_id) {
        PushSnowflakeField(state, "author_id", target->author_id);
        return;
    }
    std::string message = std::to_string(target->message_id);
    std::string channel = target->channel_id ? std::to_string(target->channel_id) : ResolveChannel(0, message);
    std::string author = MessageAuthor(channel, message);
    if (author.empty()) return;
    lua_pushstring(state, author.c_str());
    lua_setfield(state, -2, "author_id");
}

static void PushTargetChannel(lua_State *state, const AstralPatchTarget *target)
{
    if (target->channel_id) {
        PushSnowflakeField(state, "channel_id", target->channel_id);
        return;
    }
    if (!target->message_id || !gPlatform.messageChannel) return;
    auto channel = gPlatform.messageChannel(std::to_string(target->message_id));
    if (!channel || channel->empty()) return;
    lua_pushstring(state, channel->c_str());
    lua_setfield(state, -2, "channel_id");
}

namespace {

struct SpanReader {
    const uint8_t *bytes;
    size_t length;
    size_t offset = 0;
    bool failed = false;

    bool need(size_t count)
    {
        if (failed || length - offset < count) failed = true;
        return !failed;
    }
    uint8_t u8() { return need(1) ? bytes[offset++] : 0; }
    uint16_t u16()
    {
        uint16_t value = 0;
        if (need(2)) memcpy(&value, bytes + offset, 2), offset += 2;
        return value;
    }
    uint32_t u32()
    {
        uint32_t value = 0;
        if (need(4)) memcpy(&value, bytes + offset, 4), offset += 4;
        return value;
    }
    void skip(uint8_t tag, int depth = 0)
    {
        if (depth > 8) {
            failed = true;
            return;
        }
        switch (tag) {
        case 0:
        case 1:
        case 2:
            return;
        case 3:
            if (need(4)) offset += 4;
            return;
        case 4:
        case 5:
            if (need(8)) offset += 8;
            return;
        case 6: {
            uint32_t size = u32();
            if (need(size)) offset += size;
            return;
        }
        case 7: {
            uint32_t count = u32();
            for (uint32_t index = 0; index < count && !failed; index++) skip(u8(), depth + 1);
            return;
        }
        case 8: {
            uint32_t count = u32();
            for (uint32_t index = 0; index < count && !failed; index++) {
                u16();
                skip(u8(), depth + 1);
            }
            return;
        }
        case 9:
            if (need(16)) offset += 16;
            return;
        case 11: {
            uint32_t count = u32();
            if (need((size_t)count * 4)) offset += (size_t)count * 4;
            return;
        }
        default:
            failed = true;
        }
    }
};

struct SpanWriter {
    std::vector<uint8_t> bytes;
    void u8(uint8_t value) { bytes.push_back(value); }
    void u16(uint16_t value) { bytes.insert(bytes.end(), reinterpret_cast<uint8_t *>(&value), reinterpret_cast<uint8_t *>(&value) + 2); }
    void u32(uint32_t value) { bytes.insert(bytes.end(), reinterpret_cast<uint8_t *>(&value), reinterpret_cast<uint8_t *>(&value) + 4); }
    void raw(const void *data, size_t size)
    {
        bytes.insert(bytes.end(), static_cast<const uint8_t *>(data), static_cast<const uint8_t *>(data) + size);
    }
    void string(uint16_t key, const std::string &value)
    {
        u16(key);
        u8(6);
        u32(static_cast<uint32_t>(value.size()));
        raw(value.data(), value.size());
    }
    void number(uint16_t key, double value)
    {
        u16(key);
        u8(5);
        raw(&value, 8);
    }
    void integer(uint16_t key, int32_t value)
    {
        u16(key);
        u8(3);
        raw(&value, 4);
    }
    void color(uint16_t key, uint32_t value)
    {
        int64_t wide = value;
        u16(key);
        u8(4);
        raw(&wide, 8);
    }
    void boolean(uint16_t key, bool value)
    {
        u16(key);
        u8(value ? 2 : 1);
    }
};

enum SpanKind { kSpanString, kSpanColor, kSpanInteger, kSpanNumber, kSpanBool };

struct SpanField {
    const char *name;
    uint16_t key;
    SpanKind kind;
};

std::vector<SpanField> SpanFields()
{
    return {{"text", P(text), kSpanString},          {"link", P(link), kSpanString},
            {"font", P(font), kSpanString},          {"color", P(color), kSpanColor},
            {"weight", P(weight), kSpanInteger},     {"bg", P(bg), kSpanInteger},
            {"bg_radius", P(bgRadius), kSpanNumber}, {"scale", P(scale), kSpanNumber},
            {"advance", P(advance), kSpanNumber},    {"bold", P(bold), kSpanBool},
            {"strike", P(strike), kSpanBool},        {"underline", P(underline), kSpanBool}};
}

bool PushSpans(lua_State *state, const uint8_t *bytes, size_t length)
{
    SpanReader reader{bytes, length};
    if (reader.u8() != 7) return false;
    uint32_t count = reader.u32();
    if (count > 512) return false;
    auto fields = SpanFields();
    lua_createtable(state, static_cast<int>(count), 0);
    for (uint32_t index = 0; index < count && !reader.failed; index++) {
        if (reader.u8() != 8) return false;
        uint32_t entries = reader.u32();
        lua_createtable(state, 0, 4);
        std::string extra;
        uint32_t extraCount = 0;
        for (uint32_t entry = 0; entry < entries && !reader.failed; entry++) {
            size_t start = reader.offset;
            uint16_t key = reader.u16();
            uint8_t tag = reader.u8();
            const SpanField *field = nullptr;
            for (const auto &candidate : fields)
                if (candidate.key == key) field = &candidate;
            size_t value = reader.offset;
            reader.skip(tag);
            if (reader.failed) return false;
            if (field && tag == 6 && field->kind == kSpanString) {
                lua_pushlstring(state, reinterpret_cast<const char *>(bytes + value + 4), reader.offset - value - 4);
            } else if (field && tag == 4 && field->kind == kSpanColor) {
                int64_t wide = 0;
                memcpy(&wide, bytes + value, 8);
                lua_pushinteger(state, static_cast<lua_Integer>(static_cast<uint32_t>(wide)));
            } else if (field && tag == 3 && field->kind == kSpanInteger) {
                int32_t integer = 0;
                memcpy(&integer, bytes + value, 4);
                lua_pushinteger(state, field->key == P(bg) ? static_cast<lua_Integer>(static_cast<uint32_t>(integer)) : integer);
            } else if (field && tag == 5 && field->kind == kSpanNumber) {
                double number = 0;
                memcpy(&number, bytes + value, 8);
                lua_pushnumber(state, number);
            } else if (field && (tag == 1 || tag == 2) && field->kind == kSpanBool) {
                lua_pushboolean(state, tag == 2);
            } else {
                extra.append(reinterpret_cast<const char *>(bytes + start), reader.offset - start);
                extraCount++;
                continue;
            }
            lua_setfield(state, -2, field->name);
        }
        if (extraCount) {
            lua_pushlstring(state, extra.data(), extra.size());
            lua_setfield(state, -2, "_raw");
            lua_pushinteger(state, extraCount);
            lua_setfield(state, -2, "_raw_count");
        }
        lua_rawseti(state, -2, static_cast<lua_Integer>(index) + 1);
    }
    return !reader.failed;
}

bool EncodeSpans(lua_State *state, int index, std::vector<uint8_t> &out)
{
    index = lua_absindex(state, index);
    if (!lua_istable(state, index)) return false;
    lua_Integer count = luaL_len(state, index);
    if (count < 0 || count > 512) return false;
    auto fields = SpanFields();
    SpanWriter writer;
    writer.u8(7);
    writer.u32(static_cast<uint32_t>(count));
    for (lua_Integer position = 1; position <= count; position++) {
        lua_rawgeti(state, index, position);
        if (!lua_istable(state, -1)) {
            lua_pop(state, 1);
            return false;
        }
        SpanWriter entries;
        uint32_t entryCount = 0;
        for (const auto &field : fields) {
            lua_getfield(state, -1, field.name);
            if (!lua_isnil(state, -1)) {
                switch (field.kind) {
                case kSpanString:
                    if (lua_type(state, -1) == LUA_TSTRING) {
                        size_t length = 0;
                        const char *text = lua_tolstring(state, -1, &length);
                        if (length <= 8192) entries.string(field.key, std::string(text, length)), entryCount++;
                    }
                    break;
                case kSpanColor:
                    lua_pop(state, 1);
                    {
                        uint32_t color = 0;
                        if (PatchColor(state, field.name, &color)) entries.color(field.key, color), entryCount++;
                    }
                    lua_pushnil(state);
                    break;
                case kSpanInteger:
                    if (lua_isinteger(state, -1)) entries.integer(field.key, static_cast<int32_t>(lua_tointeger(state, -1))), entryCount++;
                    break;
                case kSpanNumber:
                    if (lua_isnumber(state, -1) && std::isfinite(lua_tonumber(state, -1)))
                        entries.number(field.key, lua_tonumber(state, -1)), entryCount++;
                    break;
                case kSpanBool:
                    entries.boolean(field.key, lua_toboolean(state, -1)), entryCount++;
                    break;
                }
            }
            lua_pop(state, 1);
        }
        lua_getfield(state, -1, "_raw");
        lua_getfield(state, -2, "_raw_count");
        if (lua_type(state, -2) == LUA_TSTRING && lua_isinteger(state, -1)) {
            size_t length = 0;
            const char *raw = lua_tolstring(state, -2, &length);
            entries.raw(raw, length);
            entryCount += static_cast<uint32_t>(lua_tointeger(state, -1));
        }
        lua_pop(state, 3);
        writer.u8(8);
        writer.u32(entryCount);
        writer.raw(entries.bytes.data(), entries.bytes.size());
    }
    out = std::move(writer.bytes);
    return true;
}

} // namespace

int AstralLuaSpanTransform(uint64_t messageID, const uint8_t *spans, size_t length, uint8_t **transformed, size_t *transformedLength)
{
    if (!spans || !transformed || !transformedLength) return 0;
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) {
        RedrawAfterBusy();
        return -1;
    }
    std::vector<uint8_t> current(spans, spans + length);
    bool changed = false;
    std::string message = std::to_string(messageID);
    std::string author;
    if (uint64_t authorID = astral_wire_message_author(messageID)) author = std::to_string(authorID);
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (size_t index = 0; index < plugin->textTransforms.size(); index++) {
            lua_State *state = plugin->state;
            int top = lua_gettop(state);
            lua_rawgeti(state, LUA_REGISTRYINDEX, plugin->textTransforms[index]);
            if (!PushSpans(state, current.data(), current.size())) {
                lua_settop(state, top);
                return changed ? 1 : 0;
            }
            lua_createtable(state, 0, 2);
            lua_pushlstring(state, message.data(), message.size());
            lua_setfield(state, -2, "message_id");
            if (!author.empty()) {
                lua_pushlstring(state, author.data(), author.size());
                lua_setfield(state, -2, "author_id");
            }
            if (!Call(plugin, 2, 1, "text transform")) {
                lua_settop(state, top);
                MarkFailed(plugin);
                Stop(plugin);
                break;
            }
            std::vector<uint8_t> next;
            if (lua_istable(state, -1) && EncodeSpans(state, -1, next) && next != current) {
                current = std::move(next);
                changed = true;
            }
            lua_settop(state, top);
        }
    }
    if (!changed) return 0;
    *transformed = static_cast<uint8_t *>(malloc(current.size()));
    if (!*transformed) return 0;
    memcpy(*transformed, current.data(), current.size());
    *transformedLength = current.size();
    return 1;
}

int AstralLuaImageRewrite(const char *uri, char *rewritten, size_t capacity)
{
    if (!uri || !rewritten || capacity == 0) return 0;
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) {
        RedrawAfterBusy();
        return 0;
    }
    std::string current(uri);
    bool changed = false;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (size_t index = 0; index < plugin->imageRewriters.size(); index++) {
            const auto [prefix, reference] = plugin->imageRewriters[index];
            if (current.rfind(prefix, 0) != 0) continue;
            lua_State *state = plugin->state;
            lua_rawgeti(state, LUA_REGISTRYINDEX, reference);
            lua_pushlstring(state, current.data(), current.size());
            if (!Call(plugin, 1, 1, "image rewrite")) {
                MarkFailed(plugin);
                Stop(plugin);
                break;
            }
            size_t length = 0;
            const char *next = lua_isstring(state, -1) ? lua_tolstring(state, -1, &length) : nullptr;
            if (next && length > 0 && length < capacity && strncmp(next, "http", 4) == 0 && !ContainsControl(std::string(next, length))) {
                current.assign(next, length);
                changed = true;
            }
            lua_pop(state, 1);
        }
    }
    if (!changed) return 0;
    memcpy(rewritten, current.c_str(), current.size() + 1);
    return 1;
}

int AstralLuaPatchNode(const AstralPatchTarget *target, AstralNodePatch *patch)
{
    if (!target || !target->identifier || !target->type || !patch) return 0;
    const char *identifier = target->identifier;
    const char *type = target->type;
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) {
        RedrawAfterBusy();
        return -1;
    }
    std::string_view id(identifier);
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (const auto &entry : plugin->nodePatches) {
            if (id.rfind(entry.prefix, 0) != 0 || (!entry.type.empty() && entry.type != type)) continue;
            lua_State *state = plugin->state;
            int top = lua_gettop(state);
            lua_rawgeti(state, LUA_REGISTRYINDEX, entry.reference);
            lua_createtable(state, 0, 6);
            lua_pushstring(state, identifier);
            lua_setfield(state, -2, "id");
            lua_pushstring(state, identifier + entry.prefix.size());
            lua_setfield(state, -2, "key");
            lua_pushstring(state, type);
            lua_setfield(state, -2, "type");
            PushSnowflakeField(state, "message_id", target->message_id);
            PushTargetAuthor(state, target);
            PushTargetChannel(state, target);
            if (!Call(plugin, 1, 1, "node patch")) {
                lua_settop(state, top);
                MarkFailed(plugin);
                Stop(plugin);
                break;
            }
            if (lua_istable(state, -1)) ReadNodePatch(state, patch);
            lua_settop(state, top);
        }
    }
    return patch->fields ? 1 : 0;
}

static void PushPatchTarget(lua_State *state, const AstralPatchTarget *target, size_t prefixLength)
{
    lua_createtable(state, 0, 6);
    lua_pushstring(state, target->identifier);
    lua_setfield(state, -2, "id");
    lua_pushstring(state, target->identifier + prefixLength);
    lua_setfield(state, -2, "key");
    lua_pushstring(state, target->type);
    lua_setfield(state, -2, "type");
    PushSnowflakeField(state, "message_id", target->message_id);
    PushTargetAuthor(state, target);
    PushTargetChannel(state, target);
}

static void CopyField(lua_State *state, const char *field, char *out, size_t capacity)
{
    lua_getfield(state, -1, field);
    if (lua_isstring(state, -1)) snprintf(out, capacity, "%s", lua_tostring(state, -1));
    lua_pop(state, 1);
}

static void ReadInsertStyle(lua_State *state, AstralInsertNode *node)
{
    double number = 0;
    if (PatchColor(state, "color", &node->color)) node->fields |= node->kind == AstralInsertImage ? AstralInsertTint : AstralInsertColor;
    if (PatchColor(state, "background", &node->background)) node->fields |= AstralInsertBackground;
    if (PatchNumber(state, "radius", 0, 64, &node->radius)) node->fields |= AstralInsertRadius;
    if (PatchQuad(state, "padding", node->padding)) node->fields |= AstralInsertPadding;
    if (PatchQuad(state, "margin", node->margin)) node->fields |= AstralInsertMargin;
    if (PatchNumber(state, "size", 6, 48, &number)) {
        node->fontSize = number;
        node->fields |= AstralInsertFontSize;
    }
    if (PatchNumber(state, "width", 0, 1000, &node->width)) node->fields |= AstralInsertWidth;
    if (PatchNumber(state, "height", 0, 1000, &node->height)) node->fields |= AstralInsertHeight;
    lua_getfield(state, -1, "row");
    if (lua_toboolean(state, -1)) node->fields |= AstralInsertRow;
    lua_pop(state, 1);
    lua_getfield(state, -1, "fill");
    if (lua_toboolean(state, -1)) {
        node->fields |= AstralInsertFill;
        node->fields &= ~AstralInsertWidth;
    }
    lua_pop(state, 1);
    lua_getfield(state, -1, "justify");
    if (lua_isstring(state, -1)) {
        std::string justify = lua_tostring(state, -1);
        if (justify == "start" || justify == "center" || justify == "end") {
            std::snprintf(node->justify, sizeof(node->justify), "%s", justify.c_str());
            node->fields |= AstralInsertJustify;
        }
    }
    lua_pop(state, 1);
    lua_getfield(state, -1, "bold");
    if (lua_toboolean(state, -1)) node->fields |= AstralInsertBold;
    lua_pop(state, 1);
}

static void ReadInsertNode(lua_State *state, const LuaPlugin *plugin, const LuaNodeInsert &entry, const char *key, AstralInsertNode *nodes,
                           size_t capacity, size_t *count, int parent)
{
    if (!lua_istable(state, -1) || *count >= capacity) return;
    AstralInsertNode *node = &nodes[*count];
    memset(node, 0, sizeof(*node));
    char kind[16] = {0};
    CopyField(state, "type", kind, sizeof(kind));
    node->kind = std::strcmp(kind, "text") == 0 ? AstralInsertText : std::strcmp(kind, "image") == 0 ? AstralInsertImage : AstralInsertView;
    node->parent = parent;
    CopyField(state, "text", node->text, sizeof(node->text));
    char icon[256] = {0};
    CopyField(state, "icon", icon, sizeof(icon));
    if (icon[0]) {
        auto uri = PluginIconURI(plugin, icon);
        if (uri && uri->size() < sizeof(node->uri)) snprintf(node->uri, sizeof(node->uri), "%s", uri->c_str());
    }
    char action[32] = {0};
    CopyField(state, "action", action, sizeof(action));
    if (action[0] && entry.press != LUA_NOREF)
        snprintf(node->tap, sizeof(node->tap), "astral.tap.%llu:%s:%s", static_cast<unsigned long long>(entry.identifier), action, key);
    if (std::strlen(node->tap) >= sizeof(node->tap) - 1) node->tap[0] = 0;
    ReadInsertStyle(state, node);
    int index = static_cast<int>((*count)++);
    lua_getfield(state, -1, "children");
    if (lua_istable(state, -1)) {
        lua_Integer children = luaL_len(state, -1);
        for (lua_Integer child = 1; child <= children && *count < capacity; child++) {
            lua_rawgeti(state, -1, child);
            ReadInsertNode(state, plugin, entry, key, nodes, capacity, count, index);
            lua_pop(state, 1);
        }
    }
    lua_pop(state, 1);
}

int AstralLuaInsertNodes(const AstralPatchTarget *target, AstralInsertNode *nodes, size_t capacity, int *atStart)
{
    if (!target || !target->identifier || !target->type || !nodes || !atStart) return 0;
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) {
        RedrawAfterBusy();
        return -1;
    }
    std::string_view id(target->identifier);
    size_t count = 0;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (const auto &entry : plugin->nodeInserts) {
            if (id.rfind(entry.prefix, 0) != 0 || (!entry.type.empty() && entry.type != target->type)) continue;
            lua_State *state = plugin->state;
            int top = lua_gettop(state);
            lua_rawgeti(state, LUA_REGISTRYINDEX, entry.build);
            PushPatchTarget(state, target, entry.prefix.size());
            if (!Call(plugin, 1, 1, "node insert")) {
                lua_settop(state, top);
                MarkFailed(plugin);
                Stop(plugin);
                break;
            }
            const char *key = target->identifier + entry.prefix.size();
            if (lua_istable(state, -1)) {
                lua_getfield(state, -1, "position");
                if (lua_isstring(state, -1) && std::strcmp(lua_tostring(state, -1), "start") == 0) *atStart = 1;
                lua_pop(state, 1);
                lua_getfield(state, -1, "type");
                bool single = !lua_isnil(state, -1);
                lua_pop(state, 1);
                if (single) {
                    ReadInsertNode(state, plugin, entry, key, nodes, capacity, &count, -1);
                } else {
                    lua_Integer items = luaL_len(state, -1);
                    for (lua_Integer item = 1; item <= items && count < capacity; item++) {
                        lua_rawgeti(state, -1, item);
                        ReadInsertNode(state, plugin, entry, key, nodes, capacity, &count, -1);
                        lua_pop(state, 1);
                    }
                }
            }
            lua_settop(state, top);
        }
    }
    return static_cast<int>(count);
}

int AstralLuaMessageBackground(const AstralMessageBackgroundContext *context, AstralMessageBackgroundStyle *style)
{
    if (!context || !style) return 0;
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) {
        RedrawAfterBusy();
        return -1;
    }
    bool wanted = false;
    for (auto *plugin : gPlugins) {
        if (plugin->state && !plugin->failed && !plugin->messageBackgroundDecorators.empty()) wanted = true;
    }
    if (!wanted) return 0;
    std::string message = Snowflake(context->message_id);
    std::string user = Snowflake(context->user_id);
    std::string channel = Snowflake(context->channel_id);
    if (user.empty()) user = MessageAuthor(channel.empty() ? ResolveChannel(0, message) : channel, message);
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (int decorator : plugin->messageBackgroundDecorators) {
            lua_State *state = plugin->state;
            int top = lua_gettop(state);
            lua_rawgeti(state, LUA_REGISTRYINDEX, decorator);
            lua_createtable(state, 0, 3);
            if (!message.empty()) {
                lua_pushstring(state, message.c_str());
                lua_setfield(state, -2, "message_id");
            }
            if (!user.empty()) {
                lua_pushstring(state, user.c_str());
                lua_setfield(state, -2, "author_id");
            }
            if (!channel.empty()) {
                lua_pushstring(state, channel.c_str());
                lua_setfield(state, -2, "channel_id");
            }
            if (!Call(plugin, 1, 1, "message background")) {
                lua_settop(state, top);
                MarkFailed(plugin);
                Stop(plugin);
                break;
            }
            bool found = false;
            if (lua_istable(state, -1)) {
                lua_getfield(state, -1, "color");
                bool validColor = lua_isinteger(state, -1) && lua_tointeger(state, -1) >= 0 &&
                                  static_cast<uint64_t>(lua_tointeger(state, -1)) <= UINT32_MAX;
                uint32_t color = validColor ? static_cast<uint32_t>(lua_tointeger(state, -1)) : 0;
                lua_pop(state, 1);
                lua_getfield(state, -1, "radius");
                double radius = lua_isnumber(state, -1) ? lua_tonumber(state, -1) : -1;
                lua_pop(state, 1);
                lua_getfield(state, -1, "bubble");
                bool bubble = lua_toboolean(state, -1);
                lua_pop(state, 1);
                lua_getfield(state, -1, "name");
                bool nameInside = lua_isstring(state, -1) && std::strcmp(lua_tostring(state, -1), "inside") == 0;
                lua_pop(state, 1);
                lua_getfield(state, -1, "time");
                const char *time = lua_isstring(state, -1) ? lua_tostring(state, -1) : "";
                int timeStyle = std::strcmp(time, "inline") == 0   ? AstralTimeInline
                                : std::strcmp(time, "hidden") == 0 ? AstralTimeHidden
                                                                   : AstralTimeHeader;
                lua_pop(state, 1);
                lua_getfield(state, -1, "reply");
                const char *reply = lua_isstring(state, -1) ? lua_tostring(state, -1) : "";
                int replyStyle = std::strcmp(reply, "inside") == 0   ? AstralReplyInside
                                 : std::strcmp(reply, "bubble") == 0 ? AstralReplyBubble
                                                                     : AstralReplyAbove;
                lua_pop(state, 1);
                if (validColor && std::isfinite(radius) && radius >= 0 && radius <= 32) {
                    style->color = color;
                    style->radius = radius;
                    style->bubble = bubble;
                    style->reply = replyStyle;
                    style->nameInside = nameInside;
                    style->time = timeStyle;
                    found = true;
                }
            }
            lua_settop(state, top);
            if (found) return 1;
        }
        if (plugin->failed) Stop(plugin);
    }
    return 0;
}

int AstralLuaTyping(const AstralNameContext *context, AstralMentionDecoration *decoration)
{
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) {
        RedrawAfterBusy();
        return 0;
    }
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
    if (!lock.owns_lock()) {
        RedrawAfterBusy();
        return 0;
    }
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
    if (!lock.owns_lock()) {
        RedrawAfterBusy();
        return 0;
    }
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
    if (!lock.owns_lock()) {
        RedrawAfterBusy();
        return 0;
    }
    std::string user = Snowflake(context->user_id), message = Snowflake(context->message_id);
    std::string channel = ResolveChannel(context->channel_id, message);
    if (user.empty()) user = MessageAuthor(channel, message);
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

namespace astral::luahost {

void setMenuMessage(uint64_t channelID, uint64_t messageID)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    gMenuChannel = channelID;
    gMenuMessage = messageID;
}

bool pressInsertedNode(std::string_view identifier)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    constexpr std::string_view prefix = "astral.tap.";
    if (identifier.rfind(prefix, 0) != 0) return false;
    identifier.remove_prefix(prefix.size());
    size_t firstColon = identifier.find(':');
    size_t secondColon = firstColon == std::string_view::npos ? firstColon : identifier.find(':', firstColon + 1);
    if (secondColon == std::string_view::npos) return false;
    uint64_t registration = 0;
    auto [end, error] = std::from_chars(identifier.data(), identifier.data() + firstColon, registration);
    if (error != std::errc() || end != identifier.data() + firstColon) return false;
    std::string action(identifier.substr(firstColon + 1, secondColon - firstColon - 1));
    std::string key(identifier.substr(secondColon + 1));
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (const auto &entry : plugin->nodeInserts) {
            if (entry.identifier != registration || entry.press == LUA_NOREF) continue;
            std::string full = entry.prefix + key;
            uint64_t message = astral_wire_message_of(full.c_str());
            AstralPatchTarget target = {full.c_str(), entry.type.c_str(), entry.prefix.size(), message, 0, astral_wire_current_channel()};
            lua_State *state = plugin->state;
            int top = lua_gettop(state);
            lua_rawgeti(state, LUA_REGISTRYINDEX, entry.press);
            PushPatchTarget(state, &target, entry.prefix.size());
            lua_pushstring(state, action.c_str());
            if (!Call(plugin, 2, 0, "insert press")) {
                lua_settop(state, top);
                MarkFailed(plugin);
                Stop(plugin);
                return true;
            }
            lua_settop(state, top);
            return true;
        }
    }
    return false;
}

bool pressMessageAction(std::string_view identifier)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    constexpr std::string_view prefix = "astral.action.";
    if (identifier.rfind(prefix, 0) != 0) return false;
    identifier.remove_prefix(prefix.size());
    uint64_t actionID = 0;
    auto [end, error] = std::from_chars(identifier.data(), identifier.data() + identifier.size(), actionID);
    if (error != std::errc() || end != identifier.data() + identifier.size() || !actionID) return false;
    uint64_t channelID = gMenuChannel, messageID = gMenuMessage;
    if (!channelID || !messageID) {
        if (gPlatform.developerLog)
            gPlatform.developerLog("Failed to press message action " + std::to_string(actionID) + ": no menu message");
        return false;
    }
    LuaPlugin *owner = nullptr;
    const LuaMessageAction *selected = nullptr;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        auto found = std::find_if(plugin->messageActions.begin(), plugin->messageActions.end(),
                                  [actionID](const LuaMessageAction &action) { return action.identifier == actionID; });
        if (found != plugin->messageActions.end()) {
            owner = plugin;
            selected = &*found;
            break;
        }
    }
    if (!owner || !selected) return false;
    std::string channel = std::to_string(channelID);
    std::string message = std::to_string(messageID);
    auto record = gPlatform.message ? gPlatform.message(channel, message) : std::nullopt;
    if (!record && gPlatform.developerLog)
        gPlatform.developerLog("Message action " + std::to_string(actionID) + " found no cached message " + message);
    auto guild = gPlatform.channelGuild ? gPlatform.channelGuild(channel) : std::nullopt;
    std::string user;
    if (record && record->is_object()) {
        const auto &author = record->value("author", nlohmann::json::object());
        if (author.is_object()) user = author.value("id", std::string());
    }
    lua_State *state = owner->state;
    lua_rawgeti(state, LUA_REGISTRYINDEX, selected->reference);
    lua_createtable(state, 0, 4);
    lua_pushstring(state, message.c_str());
    lua_setfield(state, -2, "message_id");
    lua_pushstring(state, channel.c_str());
    lua_setfield(state, -2, "channel_id");
    if (!user.empty()) {
        lua_pushstring(state, user.c_str());
        lua_setfield(state, -2, "user_id");
    }
    if (guild) {
        lua_pushstring(state, guild->c_str());
        lua_setfield(state, -2, "guild_id");
    }
    if (!Call(owner, 1, 0, "message.action")) {
        MarkFailed(owner);
        Stop(owner);
    }
    return true;
}

}

static bool ActionShows(LuaPlugin *plugin, const LuaMessageAction &action)
{
    if (action.when == LUA_NOREF) return true;
    if (!gMenuMessage) return false;
    std::string message = std::to_string(gMenuMessage);
    std::optional<std::string> channel = gMenuChannel ? std::optional<std::string>(std::to_string(gMenuChannel)) : std::nullopt;
    if (!channel && gPlatform.messageChannel) channel = gPlatform.messageChannel(message);
    if (!channel || channel->empty()) return false;
    lua_State *state = plugin->state;
    int top = lua_gettop(state);
    lua_rawgeti(state, LUA_REGISTRYINDEX, action.when);
    lua_createtable(state, 0, 2);
    lua_pushstring(state, message.c_str());
    lua_setfield(state, -2, "message_id");
    lua_pushstring(state, channel->c_str());
    lua_setfield(state, -2, "channel_id");
    if (!Call(plugin, 1, 1, "message.action when")) {
        lua_settop(state, top);
        return false;
    }
    bool shows = lua_toboolean(state, -1);
    lua_settop(state, top);
    return shows;
}

size_t AstralLuaMessageMenu(AstralMenuEntry *entries, size_t capacity)
{
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) {
        RedrawAfterBusy();
        return 0;
    }
    size_t count = 0;
    for (auto *plugin : gPlugins) {
        if (!plugin->state || plugin->failed) continue;
        for (const auto &action : plugin->messageActions) {
            if (count >= capacity) return count;
            if (!ActionShows(plugin, action)) continue;
            AstralMenuEntry *entry = &entries[count++];
            snprintf(entry->action_id, sizeof(entry->action_id), "astral.action.%llu", static_cast<unsigned long long>(action.identifier));
            snprintf(entry->label, sizeof(entry->label), "%s", action.label.c_str());
            snprintf(entry->icon, sizeof(entry->icon), "%s", action.icon.c_str());
        }
    }
    return count;
}

int AstralLuaMessageText(uint64_t messageID, char *text, size_t textCapacity, char *note, size_t noteCapacity)
{
    std::unique_lock<std::recursive_mutex> lock(gMutex, std::try_to_lock);
    if (!lock.owns_lock()) {
        RedrawAfterBusy();
        return 0;
    }
    auto found = gDisplayedTexts.find(messageID);
    if (found == gDisplayedTexts.end()) return 0;
    snprintf(text, textCapacity, "%s", found->second.text.c_str());
    snprintf(note, noteCapacity, "%s", found->second.note.c_str());
    return 1;
}

int AstralLuaSendTransform(uint64_t channelID, const char *text, char *rewritten, size_t capacity)
{
    auto changed = astral::luahost::transformOutgoingMessage(Snowflake(channelID), "send", text);
    if (!changed || changed->size() >= capacity) return 0;
    snprintf(rewritten, capacity, "%s", changed->c_str());
    return 1;
}

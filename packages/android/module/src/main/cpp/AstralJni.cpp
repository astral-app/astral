#include <android/log.h>
#include <jni.h>

#include <filesystem>
#include <fstream>
#include <algorithm>
#include <atomic>
#include <cstdint>
#include <ctime>
#include <optional>
#include <memory>
#include <map>
#include <mutex>
#include <string>
#include <string_view>
#include <stdexcept>
#include <thread>
#include <vector>

#include "AstralDevServer.h"
#include "AstralAndroidBackup.h"
#include "AstralAndroidMarketplace.h"
#include "AstralExitRecord.h"
#include "AstralPluginTrace.h"
#include "AstralLaunchGuard.h"
#include "AstralDiscordCache.h"
#include "AstralFiles.h"
#include "AstralLuaHost.h"
#include "AstralNetworkPolicy.h"
#include "AstralPrivacy.h"
#include "AstralPacks.h"
#include "AstralSignatures.h"
#include "AstralTOML.h"
#include "AstralThemes.h"
#include "AstralReactors.h"
#include "AstralRequestSink.h"
#include "AstralSession.h"
#include "AstralSettingsNavigator.h"
#include "AstralSettingsPages.h"
#include "AstralSettings.h"
#include "LuaLocale.h"
#include "PhoenixProperties.h"
#include "PhoenixWire.h"

#define ASTRAL_EXPORT __attribute__((visibility("default")))

namespace {

constexpr jsize kMaxPropertyNames = 2048;

std::unique_ptr<astral::SettingsStore> gSettings;
std::unique_ptr<astral::Packs> gPacks;
std::unique_ptr<astral::Themes> gThemes;
std::unique_ptr<astral::Signatures> gSignatures;
std::unique_ptr<astral::android::Marketplace> gMarketplace;
std::string gStateDirectory;
std::string gReportsPath;
nlohmann::json gEnvironment = nlohmann::json::object();
bool gRecovering = false;
unsigned gImageGeneration = 0;
std::mutex gWireLock;
std::mutex gNetworkLock;
std::mutex gReportsLock;
JavaVM *gVM = nullptr;
jclass gBridge = nullptr;
jmethodID gDeliver = nullptr;
jmethodID gFetch = nullptr;
jmethodID gRequest = nullptr;
jmethodID gCancelRequest = nullptr;
jmethodID gExportBackup = nullptr;
jmethodID gImportBackup = nullptr;
jmethodID gOpenReport = nullptr;
jmethodID gRefreshEmoji = nullptr;
jmethodID gShowAlert = nullptr;
jmethodID gShowToast = nullptr;
jmethodID gTriggerHaptic = nullptr;
jmethodID gCopyText = nullptr;
jmethodID gPickFile = nullptr;
jmethodID gCancelFilePick = nullptr;
jmethodID gOpenWebAuth = nullptr;
jmethodID gCancelWebAuth = nullptr;
jmethodID gEditText = nullptr;
jmethodID gRestartGatewaySession = nullptr;
jmethodID gPasteText = nullptr;
jmethodID gOpenURL = nullptr;
std::mutex gTextEditLock;
std::map<uint64_t, std::function<void(std::optional<std::string>)>> gTextEditCallbacks;
std::atomic<uint64_t> gNextTextEditRequest{1};

std::atomic<bool> gRefreshDecorations{false};
std::atomic<uint64_t> gBlockedRequests{0};
void deliver(const std::vector<uint8_t> &bytes);
void refreshImages();
void applyTheme(const std::string &identifier);
std::string sha256(const std::string &data);
void startNetworkRequest(uint64_t request, const nlohmann::json &options);
void cancelNetworkRequest(uint64_t request);
JNIEnv *currentEnvironment(bool &attached);
std::optional<std::string> pasteFromBridge();
jstring toJava(JNIEnv *env, std::string_view value);
void callBridge(jmethodID method);
void callBridge(jmethodID method, const std::string &argument);
void callBridge(jmethodID method, const std::string &first, const std::string &second);
void callBridge(jmethodID method, jlong request);
bool callBridge(jmethodID method, jlong request, const std::string &argument);
bool callBridge(jmethodID method, jlong request, const std::string &first, const std::string &second);
bool callBridge(jmethodID method, jlong request, const std::string &title, const std::string &value, jint maximumLength);

std::string reportText(const nlohmann::json &report, const char *key)
{
    auto value = report.find(key);
    if (value != report.end() && value->is_string()) return value->get<std::string>();
    if (std::string(key) == "detail") {
        auto legacy = report.find("error");
        if (legacy != report.end() && legacy->is_string()) return legacy->get<std::string>();
    }
    return "";
}

nlohmann::json reportEnvironment()
{
    nlohmann::json environment = nlohmann::json::object();
    for (const char *key : {"device", "architecture", "abi", "system", "locale", "astral", "discord", "phoenix", "engine"}) {
        auto value = gEnvironment.find(key);
        if (value != gEnvironment.end() && value->is_string()) environment[key] = astral::sanitizeDiagnosticText(value->get<std::string>());
    }
    environment["schema"] = 2;
    environment["platform"] = "android";
    environment["install_mode"] = "injected";
    nlohmann::json plugins = nlohmann::json::array();
    for (const auto &plugin : astral::luahost::plugins()) {
        if (plugin.value("running", false)) plugins.push_back(astral::sanitizeDiagnosticText(plugin.value("id", "")));
    }
    environment["plugins"] = std::move(plugins);
    return environment;
}

nlohmann::json normalizedReports(const nlohmann::json &saved)
{
    nlohmann::json reports = nlohmann::json::array();
    if (!saved.is_array()) return reports;
    for (const auto &report : saved) {
        if (!report.is_object()) continue;
        std::string kind = reportText(report, "kind");
        if (kind.empty()) kind = "plugin";
        if (kind != "plugin" && kind != "crash" && kind != "hang") continue;
        nlohmann::json sanitized = {{"schema", 2}, {"kind", kind}};
        for (const char *key : {"id", "plugin", "title"}) {
            auto value = report.find(key);
            if (value != report.end() && value->is_string()) sanitized[key] = astral::sanitizeDiagnosticText(value->get<std::string>());
        }
        std::string detail = reportText(report, "detail");
        if (!detail.empty()) sanitized["detail"] = astral::sanitizeDiagnosticText(detail);
        auto time = report.find("time");
        if (time != report.end() && time->is_number()) sanitized["time"] = *time;
        auto fingerprint = report.find("fingerprint");
        if (fingerprint != report.end() && fingerprint->is_string()) sanitized["fingerprint"] = fingerprint->get<std::string>();
        auto reason = report.find("reason");
        if (reason != report.end() && reason->is_number_integer()) sanitized["reason"] = *reason;
        auto savedEnvironment = report.find("environment");
        nlohmann::json environment = savedEnvironment != report.end() ? *savedEnvironment : nlohmann::json::object();
        if (!environment.is_object()) environment = nlohmann::json::object();
        nlohmann::json safeEnvironment = {{"schema", 2}, {"platform", "android"}, {"install_mode", "injected"}};
        for (const char *key : {"device", "architecture", "abi", "system", "locale", "astral", "discord", "phoenix", "engine"}) {
            auto value = environment.find(key);
            if (value != environment.end() && value->is_string())
                safeEnvironment[key] = astral::sanitizeDiagnosticText(value->get<std::string>());
        }
        auto plugins = environment.find("plugins");
        if (plugins != environment.end() && plugins->is_array()) {
            safeEnvironment["plugins"] = nlohmann::json::array();
            for (const auto &identifier : *plugins) {
                if (identifier.is_string())
                    safeEnvironment["plugins"].push_back(astral::sanitizeDiagnosticText(identifier.get<std::string>()));
            }
        }
        sanitized["environment"] = std::move(safeEnvironment);
        reports.push_back(std::move(sanitized));
        if (reports.size() == 20) break;
    }
    return reports;
}

nlohmann::json readReports()
{
    auto saved = astral::readTextFile(gReportsPath);
    if (!saved) return nlohmann::json::array();
    nlohmann::json parsed = nlohmann::json::parse(*saved, nullptr, false);
    return normalizedReports(parsed);
}

void saveReports(const nlohmann::json &reports)
{
    astral::createDirectories(std::filesystem::path(gReportsPath).parent_path().string());
    if (!astral::writeTextFileAtomically(gReportsPath, reports.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace)))
        __android_log_print(ANDROID_LOG_ERROR, "astral", "failed to persist plugin reports");
}

nlohmann::json pluginReports()
{
    std::lock_guard<std::mutex> lock(gReportsLock);
    nlohmann::json reports = readReports();
    for (const auto &plugin : astral::luahost::plugins()) {
        if (!plugin.value("failed", false)) continue;
        std::string identifier = plugin.value("id", "");
        std::string failure = astral::sanitizeDiagnosticText(plugin.value("error", ""));
        std::string title = astral::sanitizeDiagnosticText(plugin.value("name", plugin.value("id", "Plugin")));
        bool exists = std::any_of(reports.begin(), reports.end(), [&](const nlohmann::json &report) {
            return report.is_object() && reportText(report, "id") == identifier &&
                   (reportText(report, "detail") == failure || reportText(report, "error") == failure);
        });
        if (!exists) {
            reports.insert(reports.begin(), {{"schema", 2},
                                             {"kind", "plugin"},
                                             {"title", title + " stopped after errors"},
                                             {"id", astral::sanitizeDiagnosticText(identifier)},
                                             {"detail", failure},
                                             {"environment", reportEnvironment()},
                                             {"time", static_cast<int64_t>(std::time(nullptr))}});
        }
    }
    if (reports.size() > 20) reports.erase(reports.begin() + 20, reports.end());
    saveReports(reports);
    return reports;
}

void recordPluginFailure(const std::string &identifier, const std::string &name, const std::string &error)
{
    std::string failure = astral::sanitizeDiagnosticText(error);
    std::string title = astral::sanitizeDiagnosticText(name) + " stopped after errors";
    std::string safeIdentifier = astral::sanitizeDiagnosticText(identifier);
    __android_log_print(ANDROID_LOG_ERROR, "astral", "plugin %s (%s) failed: %s", safeIdentifier.c_str(), title.c_str(), failure.c_str());
    std::lock_guard<std::mutex> lock(gReportsLock);
    nlohmann::json reports = readReports();
    bool exists = std::any_of(reports.begin(), reports.end(), [&](const nlohmann::json &report) {
        return report.is_object() && reportText(report, "id") == safeIdentifier &&
               (reportText(report, "detail") == failure || reportText(report, "error") == failure);
    });
    if (exists) return;
    reports.insert(reports.begin(), {{"schema", 2},
                                     {"kind", "plugin"},
                                     {"title", title},
                                     {"id", safeIdentifier},
                                     {"detail", failure},
                                     {"environment", reportEnvironment()},
                                     {"time", static_cast<int64_t>(std::time(nullptr))}});
    if (reports.size() > 20) reports.erase(reports.begin() + 20, reports.end());
    saveReports(reports);
}

void recordProcessExitReport(const std::string &kind, int64_t timestamp, int reason, const std::string &description,
                             const std::string &trace)
{
    if ((kind != "crash" && kind != "hang") || timestamp <= 0) return;
    std::string detail = description;
    if (!trace.empty()) detail += (detail.empty() ? "" : "\n") + trace;
    detail = astral::sanitizeDiagnosticText(detail);
    if (detail.empty()) detail = "No system diagnostic details were available.";
    std::lock_guard<std::mutex> lock(gReportsLock);
    nlohmann::json reports = readReports();
    bool exists = std::any_of(reports.begin(), reports.end(), [&](const nlohmann::json &report) {
        auto savedReason = report.find("reason");
        return report.is_object() && reportText(report, "kind") == kind && report.contains("time") && report["time"] == timestamp &&
               savedReason != report.end() && savedReason->is_number_integer() && savedReason->get<int>() == reason;
    });
    if (exists) return;
    std::string title = kind == "hang" ? "Discord stopped responding" : "Discord process crashed";
    nlohmann::json report = {
        {"schema", 2}, {"kind", kind}, {"detail", detail}, {"reason", reason}, {"environment", reportEnvironment()}, {"time", timestamp}};
    auto session = astral::trace::previous();
    if (session && session->started > 0 && timestamp >= session->started) {
        nlohmann::json plugins = nlohmann::json::array();
        for (const auto &plugin : session->running) plugins.push_back(astral::sanitizeDiagnosticText(plugin));
        report["environment"]["plugins"] = std::move(plugins);
        if (!session->active.empty()) {
            report["plugin"] = astral::sanitizeDiagnosticText(session->active);
            title += kind == "hang" ? " while " + session->active + " was running" : " in " + session->active;
        }
    }
    report["title"] = title;
    reports.insert(reports.begin(), std::move(report));
    if (reports.size() > 20) reports.erase(reports.begin() + 20, reports.end());
    saveReports(reports);
}

std::string plain(JNIEnv *env, jstring value)
{
    if (!value) return {};
    const char *utf = env->GetStringUTFChars(value, nullptr);
    std::string result = utf ? utf : "";
    if (utf) env->ReleaseStringUTFChars(value, utf);
    return result;
}

std::vector<std::string> strings(JNIEnv *env, jobjectArray array)
{
    std::vector<std::string> values;
    if (!array) return values;
    jsize count = env->GetArrayLength(array);
    for (jsize index = 0; index < count && index < kMaxPropertyNames; index++) {
        auto element = static_cast<jstring>(env->GetObjectArrayElement(array, index));
        values.push_back(plain(env, element));
        env->DeleteLocalRef(element);
    }
    return values;
}

size_t resolveProperties(const std::vector<std::string> &names)
{
    if (names.empty()) return 0;
    std::vector<const char *> pointers;
    pointers.reserve(names.size());
    for (const auto &name : names) pointers.push_back(name.c_str());
    const char *missing = nullptr;
    size_t resolved = astral_wire_resolve_properties(pointers.data(), pointers.size(), &missing);
    if (missing) __android_log_print(ANDROID_LOG_WARN, "astral", "Phoenix has no property %s; astral keeps its defaults", missing);
    return resolved;
}

void developerLog(const std::string &line)
{
    __android_log_print(ANDROID_LOG_INFO, "astral", "%s", line.c_str());
    astral::dev::appendLog(line);
}

std::optional<astral::dev::Response> developerRoute(const astral::dev::Request &request)
{
    if (request.method == "GET" && request.route == "/themes") {
        auto identifiers = nlohmann::json::array();
        for (const auto &theme : gThemes->list()) identifiers.push_back(theme["id"]);
        return astral::dev::Response{200, {{"active", gThemes->active()}, {"themes", identifiers}}};
    }
    if (request.method == "POST" && request.route == "/unbound-theme") {
        if (auto error = gThemes->importUnbound(request.body, "Imported")) return astral::dev::Response{400, {{"error", *error}}};
        applyTheme(gThemes->active());
        astral::navigator::refresh();
        return astral::dev::Response{200, {{"active", gThemes->active()}}};
    }
    if (request.method == "POST" && (request.route == "/theme" || request.route == "/icon-pack" || request.route == "/emoji-pack")) {
        if (!request.json.is_object() || !request.json.contains("id") || !request.json["id"].is_string())
            return astral::dev::Response{400, {{"error", "id is required"}}};
        auto identifier = request.json["id"].get<std::string>();
        if (request.route == "/theme")
            applyTheme(identifier);
        else if (request.route == "/icon-pack") {
            if (!gPacks->selectIcons(identifier)) return astral::dev::Response{400, {{"error", "no such icon pack"}}};
            refreshImages();
        } else if (!gPacks->selectEmoji(identifier))
            return astral::dev::Response{400, {{"error", "no such emoji pack"}}};
        astral::navigator::refresh();
        return astral::dev::Response{200, {{"active", request.route == "/theme" ? gThemes->active() : identifier}}};
    }
    if (request.method != "POST" || request.route != "/reload") return std::nullopt;
    size_t reloaded = 0;
    for (const auto &plugin : astral::luahost::plugins()) {
        if (!plugin.value("enabled", false)) continue;
        astral::luahost::reload(plugin.value("id", ""));
        reloaded++;
    }
    gPacks->invalidate();
    refreshImages();
    applyTheme(gThemes->active());
    astral::navigator::refresh();
    return astral::dev::Response{200, {{"plugins", reloaded}, {"theme", gThemes->active()}}};
}

void startDeveloperServer(const std::string &state)
{
    astral::dev::Hooks hooks;
    hooks.enabled = [] { return gSettings && gSettings->getBoolean("astral", "developer-mode", false); };
    hooks.routes = developerRoute;
    hooks.status = [] {
        AstralWireStats stats = astral_wire_stats();
        return nlohmann::json{{"wire",
                               {{"channel", std::to_string(astral_wire_current_channel())},
                                {"authors", stats.authors},
                                {"withUser", stats.with_user},
                                {"withChannel", stats.with_channel},
                                {"tagged", stats.tagged},
                                {"pages", astral_page_template_status()},
                                {"navigators", astral_page_navigator_count()},
                                {"depth", astral_page_depth()}}}};
    };
    hooks.version = ASTRAL_VERSION;
    hooks.stateDirectory = state;
    hooks.settings = gSettings.get();
    astral::dev::start(std::move(hooks));
}

astral::LuaHostPlatform platform(const std::string &state, const std::string &resources, std::vector<std::string> languages)
{
    astral::LuaHostPlatform platform;
    platform.guild = astral::discord::guild;
    platform.member = astral::discord::member;
    platform.memberNamed = astral::discord::memberNamed;
    platform.channelGuild = astral::discord::channelGuild;
    platform.user = astral::discord::user;
    platform.friendNickname = astral::discord::friendNickname;
    platform.messageChannel = astral::discord::messageChannel;
    platform.message = astral::discord::message;
    platform.permissions = astral::discord::permissions;
    platform.networkRequest = startNetworkRequest;
    platform.cancelNetworkRequest = cancelNetworkRequest;
    platform.showAlert = [](const std::string &title, const std::string &message) { callBridge(gShowAlert, title, message); };
    platform.showToast = [](const std::string &title, const std::string &message) { callBridge(gShowToast, title, message); };
    platform.webAuth = [](uint64_t request, const std::string &url, const std::string &callbackURL) {
        if (!gOpenWebAuth) return false;
        return callBridge(gOpenWebAuth, static_cast<jlong>(request), url, callbackURL);
    };
    platform.cancelWebAuth = [](uint64_t request) {
        if (gCancelWebAuth) callBridge(gCancelWebAuth, static_cast<jlong>(request));
    };
    platform.triggerHaptic = [](const std::string &strength) {
        if (!gTriggerHaptic) return false;
        callBridge(gTriggerHaptic, strength);
        return true;
    };
    platform.writeClipboard = [](const std::string &value) {
        if (!gCopyText) return false;
        callBridge(gCopyText, value);
        return true;
    };
    platform.readClipboard = []() -> std::optional<std::string> { return pasteFromBridge(); };
    platform.openURL = [](const std::string &url) {
        if (!gOpenURL) return false;
        callBridge(gOpenURL, url);
        return true;
    };
    platform.pickFile = [](uint64_t request, const std::vector<std::string> &extensions) {
        std::string encoded;
        for (const auto &extension : extensions) {
            if (!encoded.empty()) encoded += '\n';
            encoded += extension;
        }
        if (!gPickFile) return false;
        return callBridge(gPickFile, static_cast<jlong>(request), encoded);
    };
    platform.cancelFilePick = [](uint64_t request) {
        if (gCancelFilePick) callBridge(gCancelFilePick, static_cast<jlong>(request));
    };
    platform.developerLog = developerLog;
    platform.recordFailure = recordPluginFailure;
    platform.reactors = astral::reactors::lookup;
    platform.refreshDecorations = [] {
        gRefreshDecorations = true;
        if (gRefreshEmoji) callBridge(gRefreshEmoji);
    };
    platform.preferredLanguages = std::move(languages);
    platform.storagePath = state + "/plugin-storage.sqlite";
    platform.verifySignature = [](const std::string &directory, const std::string &main) {
        return gSignatures->directory(directory, main);
    };
    platform.bundledDirectory = resources;
    return platform;
}

std::vector<std::string> gCategoryStrings;

void installCategory()
{
    nlohmann::json category = astral::pages::category();
    nlohmann::json rows = category.value("rows", nlohmann::json::array());
    gCategoryStrings.clear();
    gCategoryStrings.reserve(rows.size() * 3 + 1);
    std::vector<AstralRowSpec> specs;
    for (const auto &row : rows) {
        gCategoryStrings.push_back(row.value("key", ""));
        const char *key = gCategoryStrings.back().c_str();
        gCategoryStrings.push_back(row.value("title", ""));
        const char *title = gCategoryStrings.back().c_str();
        gCategoryStrings.push_back(row.value("icon", ""));
        const char *icon = gCategoryStrings.back().c_str();
        specs.push_back({key, title, nullptr, nullptr, icon, kAstralRowLink, 0, 0, 0});
    }
    gCategoryStrings.push_back(category.value("title", "astral"));
    astral_wire_set_category(gCategoryStrings.back().c_str(), specs.data(), specs.size());
}

void refreshCatalogIfDue(bool force)
{
    astral::pages::refreshCatalogIfDue(static_cast<int64_t>(time(nullptr)), force, [] { astral::navigator::refresh(); });
}

void configurePages(const std::string &discordVersion)
{
    astral::SettingsPagesPlatform pages;
    pages.settings = gSettings.get();
    pages.crashReports = pluginReports;
    pages.restartGatewaySession = [] { callBridge(gRestartGatewaySession); };
    pages.openReport = [](size_t index) {
        nlohmann::json reports = pluginReports();
        if (index >= reports.size()) return;
        const auto &report = reports[index];
        std::string content = reportText(report, "title");
        if (content.empty()) content = "Plugin failure";
        content += " (" + reportText(report, "id") + ")\n\n" + reportText(report, "detail");
        nlohmann::json environment = report.value("environment", nlohmann::json::object());
        if (environment.is_object()) {
            for (auto item = environment.begin(); item != environment.end(); ++item) {
                content += "\n" + item.key() + ": ";
                content += item.value().is_string() ? item.value().get<std::string>() : item.value().dump();
            }
        }
        callBridge(gOpenReport, content);
    };
    pages.themes = [] { return gThemes->list(); };
    pages.activeTheme = [] { return gThemes->active(); };
    pages.applyTheme = applyTheme;
    pages.iconPacks = [] { return gPacks->icons(); };
    pages.activeIconPack = [] { return gPacks->activeIcons(); };
    pages.selectIconPack = [](const std::string &identifier) {
        if (gPacks->selectIcons(identifier)) refreshImages();
    };
    pages.emojiPacks = [] { return gPacks->emoji(); };
    pages.activeEmojiPack = [] { return gPacks->activeEmoji(); };
    pages.selectEmojiPack = [](const std::string &identifier) {
        if (gPacks->selectEmoji(identifier)) callBridge(gRefreshEmoji);
    };
    pages.emojiAnimated = [] { return gPacks->animated(); };
    pages.setEmojiAnimated = [](bool animated) {
        gPacks->setAnimated(animated);
        callBridge(gRefreshEmoji);
    };
    pages.blockedRequests = [] { return gBlockedRequests.load(); };
    pages.catalog = [] { return gMarketplace ? gMarketplace->catalog() : nlohmann::json::object(); };
    pages.catalogState = [] { return gMarketplace ? gMarketplace->catalogState() : "idle"; };
    pages.catalogError = [] { return gMarketplace ? gMarketplace->catalogError() : ""; };
    pages.catalogURL = [] { return gMarketplace ? gMarketplace->catalogURL() : ""; };
    pages.installStatus = [](const std::string &identifier) {
        return gMarketplace ? gMarketplace->installStatus(identifier) : std::optional<std::string>();
    };
    pages.installPackage = [](const nlohmann::json &package, const std::string &kind, const std::function<void()> &changed) {
        if (gMarketplace) gMarketplace->installPackage(package, kind, changed);
    };
    pages.refreshCatalog = [](const std::function<void()> &changed) {
        if (gMarketplace) gMarketplace->refreshCatalog(changed);
    };
    pages.authorAvatar = [](const std::string &userID, size_t index) {
        if (gMarketplace) return gMarketplace->authorAvatar(userID, index);
        return "https://cdn.discordapp.com/embed/avatars/" + std::to_string(index % 6) + ".png";
    };
    pages.pluginIconSVG = [](const nlohmann::json &plugin) -> std::optional<std::string> {
        std::string identifier = plugin.value("id", "");
        if (plugin.value("iconSvg", "").empty() || !astral::lua::validPluginIdentifier(identifier)) return std::nullopt;
        return "res://astral_plugin_" + identifier;
    };
    pages.editText = [](const std::string &title, const std::string &value, size_t maximumLength,
                        const std::function<void(std::optional<std::string>)> &completion) {
        uint64_t request = gNextTextEditRequest.fetch_add(1);
        if (request == 0) request = gNextTextEditRequest.fetch_add(1);
        {
            std::lock_guard<std::mutex> lock(gTextEditLock);
            gTextEditCallbacks.emplace(request, completion);
        }
        if (!callBridge(gEditText, static_cast<jlong>(request), title, value, static_cast<jint>(maximumLength))) {
            {
                std::lock_guard<std::mutex> lock(gTextEditLock);
                gTextEditCallbacks.erase(request);
            }
            completion(std::nullopt);
        }
    };
    pages.version = ASTRAL_VERSION;
    pages.context = [discordVersion] { return astral::SettingsPageContext{gRecovering, discordVersion, "ready"}; };
    pages.exportBackup = [] { callBridge(gExportBackup); };
    pages.importBackup = [](const std::function<void()> &) { callBridge(gImportBackup); };
    pages.softRestart = [] {
        gPacks->invalidate();
        refreshImages();
        applyTheme(gThemes->active());
        callBridge(gRefreshEmoji);
        for (const auto &plugin : astral::luahost::plugins())
            if (plugin.value("enabled", false)) astral::luahost::reload(plugin.value("id", ""));
    };
    pages.notify = [](const std::string &title, const std::string &message) { callBridge(gShowToast, title, message); };
    astral::pages::configure(std::move(pages));
    astral::navigator::configure([](std::vector<uint8_t> bytes) { deliver(bytes); },
                                 [](const std::function<void()> &work) {
                                     std::lock_guard<std::mutex> lock(gWireLock);
                                     work();
                                 });
    refreshCatalogIfDue(true);
}

void installDecorators()
{
    astral_wire_set_decorator(AstralLuaDecorate);
    astral_wire_set_message_background_decorator(AstralLuaMessageBackground);
    astral_wire_set_message_menu_provider(AstralLuaMessageMenu);
    astral_wire_set_message_text_provider(AstralLuaMessageText);
    astral_wire_set_send_transform(AstralLuaSendTransform);
    astral_wire_set_relabeler(AstralLuaRelabel);
    astral_wire_set_mention_decorator(AstralLuaMention);
    astral_wire_set_message_text_colorizer(AstralLuaMessageTextColor);
    astral_wire_set_name_colorizer(AstralLuaNameColor);
    astral_wire_set_typing_decorator(AstralLuaTyping);
    astral_wire_set_reaction_decorator(AstralLuaReaction);
}

void append(std::vector<uint8_t> &merged, AstralWireBuffer &buffer)
{
    if (!buffer.bytes || buffer.length < 2) {
        astral_wire_buffer_free(&buffer);
        return;
    }
    if (merged.empty())
        merged.assign(buffer.bytes, buffer.bytes + buffer.length);
    else
        merged.insert(merged.end(), buffer.bytes + 1, buffer.bytes + buffer.length);
    astral_wire_buffer_free(&buffer);
}

std::vector<uint8_t> pendingUpdates()
{
    std::lock_guard<std::mutex> lock(gWireLock);
    std::vector<uint8_t> merged;
    AstralWireBuffer buffer;
    if (astral_wire_retry_mentions(&buffer)) append(merged, buffer);
    if (astral_wire_retry_reactions(&buffer)) append(merged, buffer);
    if (gRefreshDecorations.exchange(false)) {
        if (astral_wire_refresh_reactions(&buffer)) append(merged, buffer);
        if (astral_wire_refresh_message_texts(&buffer)) append(merged, buffer);
    }
    return merged;
}

jbyteArray bytesOf(JNIEnv *env, const std::vector<uint8_t> &bytes)
{
    if (bytes.empty()) return nullptr;
    jbyteArray array = env->NewByteArray((jsize)bytes.size());
    if (array) env->SetByteArrayRegion(array, 0, (jsize)bytes.size(), reinterpret_cast<const jbyte *>(bytes.data()));
    return array;
}

void deliver(const std::vector<uint8_t> &bytes)
{
    if (bytes.empty() || !gVM || !gBridge || !gDeliver) return;
    JNIEnv *env = nullptr;
    bool attached = false;
    if (gVM->GetEnv(reinterpret_cast<void **>(&env), JNI_VERSION_1_6) != JNI_OK) {
        if (gVM->AttachCurrentThread(&env, nullptr) != JNI_OK) return;
        attached = true;
    }
    jbyteArray array = bytesOf(env, bytes);
    if (array) {
        env->CallStaticVoidMethod(gBridge, gDeliver, array);
        if (env->ExceptionCheck()) env->ExceptionClear();
        env->DeleteLocalRef(array);
    }
    if (attached) gVM->DetachCurrentThread();
}

void refreshImages()
{
    AstralWireBuffer buffer = {};
    std::vector<uint8_t> bytes;
    {
        std::lock_guard<std::mutex> lock(gWireLock);
        astral_wire_refresh_images(&buffer, ++gImageGeneration);
        append(bytes, buffer);
    }
    deliver(bytes);
}

int resolveEmoji(const char *codepoints, char *url, size_t capacity)
{
    if (!gPacks || !codepoints || !url) return 0;
    auto resolved = gPacks->emojiURL(codepoints);
    if (resolved.empty() || resolved.size() >= capacity) return 0;
    memcpy(url, resolved.c_str(), resolved.size() + 1);
    return 1;
}

void applyTheme(const std::string &identifier)
{
    auto style = gThemes->select(identifier);
    AstralWireBuffer buffer = {};
    std::vector<uint8_t> bytes;
    {
        std::lock_guard<std::mutex> lock(gWireLock);
        style.apply();
        astral_wire_recolor(&buffer);
        append(bytes, buffer);
    }
    deliver(bytes);
}

JNIEnv *currentEnvironment(bool &attached)
{
    JNIEnv *env = nullptr;
    attached = false;
    if (!gVM) return nullptr;
    if (gVM->GetEnv(reinterpret_cast<void **>(&env), JNI_VERSION_1_6) == JNI_OK) return env;
    if (gVM->AttachCurrentThread(&env, nullptr) != JNI_OK) return nullptr;
    attached = true;
    return env;
}

void appendUtf8(std::string &result, uint32_t codepoint)
{
    if (codepoint <= 0x7f) {
        result.push_back(static_cast<char>(codepoint));
    } else if (codepoint <= 0x7ff) {
        result.push_back(static_cast<char>(0xc0 | (codepoint >> 6)));
        result.push_back(static_cast<char>(0x80 | (codepoint & 0x3f)));
    } else if (codepoint <= 0xffff) {
        result.push_back(static_cast<char>(0xe0 | (codepoint >> 12)));
        result.push_back(static_cast<char>(0x80 | ((codepoint >> 6) & 0x3f)));
        result.push_back(static_cast<char>(0x80 | (codepoint & 0x3f)));
    } else {
        result.push_back(static_cast<char>(0xf0 | (codepoint >> 18)));
        result.push_back(static_cast<char>(0x80 | ((codepoint >> 12) & 0x3f)));
        result.push_back(static_cast<char>(0x80 | ((codepoint >> 6) & 0x3f)));
        result.push_back(static_cast<char>(0x80 | (codepoint & 0x3f)));
    }
}

std::string fromJava(JNIEnv *env, jstring value)
{
    if (!value) return {};
    jsize length = env->GetStringLength(value);
    const jchar *characters = env->GetStringChars(value, nullptr);
    if (!characters) return {};
    std::string result;
    for (jsize index = 0; index < length; index++) {
        uint32_t codepoint = characters[index];
        if (codepoint >= 0xd800 && codepoint <= 0xdbff && index + 1 < length && characters[index + 1] >= 0xdc00 &&
            characters[index + 1] <= 0xdfff) {
            codepoint = 0x10000 + ((codepoint - 0xd800) << 10) + (characters[++index] - 0xdc00);
        } else if (codepoint >= 0xd800 && codepoint <= 0xdfff) {
            codepoint = 0xfffd;
        }
        appendUtf8(result, codepoint);
    }
    env->ReleaseStringChars(value, characters);
    return result;
}

std::optional<std::string> pasteFromBridge()
{
    if (!gVM || !gBridge || !gPasteText) return std::nullopt;
    bool attached = false;
    JNIEnv *env = currentEnvironment(attached);
    if (!env) return std::nullopt;
    auto value = static_cast<jstring>(env->CallStaticObjectMethod(gBridge, gPasteText));
    std::optional<std::string> result;
    if (env->ExceptionCheck())
        env->ExceptionClear();
    else if (value)
        result = fromJava(env, value);
    if (value) env->DeleteLocalRef(value);
    if (attached) gVM->DetachCurrentThread();
    return result;
}

jstring toJava(JNIEnv *env, std::string_view value)
{
    std::vector<jchar> characters;
    characters.reserve(value.size());
    for (size_t index = 0; index < value.size();) {
        uint8_t first = static_cast<uint8_t>(value[index]);
        uint32_t codepoint = 0xfffd;
        size_t width = 1;
        if (first <= 0x7f) {
            codepoint = first;
        } else if ((first & 0xe0) == 0xc0 && index + 1 < value.size()) {
            uint8_t second = static_cast<uint8_t>(value[index + 1]);
            if ((second & 0xc0) == 0x80) {
                codepoint = ((first & 0x1f) << 6) | (second & 0x3f);
                width = 2;
            }
        } else if ((first & 0xf0) == 0xe0 && index + 2 < value.size()) {
            uint8_t second = static_cast<uint8_t>(value[index + 1]);
            uint8_t third = static_cast<uint8_t>(value[index + 2]);
            if ((second & 0xc0) == 0x80 && (third & 0xc0) == 0x80) {
                codepoint = ((first & 0x0f) << 12) | ((second & 0x3f) << 6) | (third & 0x3f);
                width = 3;
            }
        } else if ((first & 0xf8) == 0xf0 && index + 3 < value.size()) {
            uint8_t second = static_cast<uint8_t>(value[index + 1]);
            uint8_t third = static_cast<uint8_t>(value[index + 2]);
            uint8_t fourth = static_cast<uint8_t>(value[index + 3]);
            if ((second & 0xc0) == 0x80 && (third & 0xc0) == 0x80 && (fourth & 0xc0) == 0x80) {
                codepoint = ((first & 0x07) << 18) | ((second & 0x3f) << 12) | ((third & 0x3f) << 6) | (fourth & 0x3f);
                width = 4;
            }
        }
        if (codepoint > 0x10ffff || (codepoint >= 0xd800 && codepoint <= 0xdfff)) codepoint = 0xfffd;
        if (codepoint <= 0xffff) {
            characters.push_back(static_cast<jchar>(codepoint));
        } else {
            codepoint -= 0x10000;
            characters.push_back(static_cast<jchar>(0xd800 | (codepoint >> 10)));
            characters.push_back(static_cast<jchar>(0xdc00 | (codepoint & 0x3ff)));
        }
        index += width;
    }
    static const jchar empty = 0;
    return env->NewString(characters.empty() ? &empty : characters.data(), static_cast<jsize>(characters.size()));
}

void startNetworkRequest(uint64_t request, const nlohmann::json &options)
{
    bool attached = false;
    JNIEnv *env = currentEnvironment(attached);
    if (!env) throw std::runtime_error("could not attach the network request thread");
    bool failed = false;
    {
        std::lock_guard<std::mutex> lock(gNetworkLock);
        jstring encoded = toJava(env, options.dump());
        if (!gBridge || !gRequest || !encoded) {
            failed = true;
        } else {
            env->CallStaticVoidMethod(gBridge, gRequest, static_cast<jlong>(request), encoded);
            failed = env->ExceptionCheck();
        }
        if (env->ExceptionCheck()) env->ExceptionClear();
        if (encoded) env->DeleteLocalRef(encoded);
    }
    if (attached) gVM->DetachCurrentThread();
    if (failed) throw std::runtime_error("could not start the network request");
}

void cancelNetworkRequest(uint64_t request)
{
    bool attached = false;
    JNIEnv *env = currentEnvironment(attached);
    if (!env) throw std::runtime_error("could not attach the network cancellation thread");
    bool failed = false;
    {
        std::lock_guard<std::mutex> lock(gNetworkLock);
        if (!gBridge || !gCancelRequest) {
            failed = true;
        } else {
            env->CallStaticVoidMethod(gBridge, gCancelRequest, static_cast<jlong>(request));
            failed = env->ExceptionCheck();
        }
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
    if (attached) gVM->DetachCurrentThread();
    if (failed) throw std::runtime_error("could not cancel the network request");
}

astral::reactors::FetchResult fetch(const std::string &url, const std::string &token)
{
    astral::reactors::FetchResult result;
    bool attached = false;
    JNIEnv *env = currentEnvironment(attached);
    if (!env || !gBridge || !gFetch) return result;
    jstring jurl = env->NewStringUTF(url.c_str());
    jstring jtoken = env->NewStringUTF(token.c_str());
    auto reply = static_cast<jbyteArray>(env->CallStaticObjectMethod(gBridge, gFetch, jurl, jtoken));
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        reply = nullptr;
    }
    if (reply) {
        jsize length = env->GetArrayLength(reply);
        if (length >= 4) {
            jbyte status[4]{};
            env->GetByteArrayRegion(reply, 0, 4, status);
            if (!env->ExceptionCheck()) {
                result.status = ((static_cast<uint8_t>(status[0]) << 24) | (static_cast<uint8_t>(status[1]) << 16) |
                                 (static_cast<uint8_t>(status[2]) << 8) | static_cast<uint8_t>(status[3]));
                result.body.resize(static_cast<size_t>(length - 4));
                if (length > 4) env->GetByteArrayRegion(reply, 4, length - 4, reinterpret_cast<jbyte *>(result.body.data()));
            }
            if (env->ExceptionCheck()) {
                env->ExceptionClear();
                result = {};
            }
        }
        env->DeleteLocalRef(reply);
    }
    env->DeleteLocalRef(jurl);
    env->DeleteLocalRef(jtoken);
    if (attached) gVM->DetachCurrentThread();
    return result;
}

jstring text(JNIEnv *env, const std::string &value)
{
    return env->NewStringUTF(value.c_str());
}

void callBridge(jmethodID method)
{
    bool attached = false;
    JNIEnv *env = currentEnvironment(attached);
    if (!env) return;
    {
        std::lock_guard<std::mutex> lock(gNetworkLock);
        if (gBridge && method) env->CallStaticVoidMethod(gBridge, method);
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
    if (attached) gVM->DetachCurrentThread();
}

void callBridge(jmethodID method, const std::string &argument)
{
    bool attached = false;
    JNIEnv *env = currentEnvironment(attached);
    if (!env) return;
    {
        std::lock_guard<std::mutex> lock(gNetworkLock);
        jstring value = toJava(env, argument);
        if (gBridge && method && value) env->CallStaticVoidMethod(gBridge, method, value);
        if (env->ExceptionCheck()) env->ExceptionClear();
        if (value) env->DeleteLocalRef(value);
    }
    if (attached) gVM->DetachCurrentThread();
}

void callBridge(jmethodID method, const std::string &first, const std::string &second)
{
    bool attached = false;
    JNIEnv *env = currentEnvironment(attached);
    if (!env) return;
    {
        std::lock_guard<std::mutex> lock(gNetworkLock);
        jstring firstValue = toJava(env, first);
        jstring secondValue = toJava(env, second);
        if (gBridge && method && firstValue && secondValue) env->CallStaticVoidMethod(gBridge, method, firstValue, secondValue);
        if (env->ExceptionCheck()) env->ExceptionClear();
        if (firstValue) env->DeleteLocalRef(firstValue);
        if (secondValue) env->DeleteLocalRef(secondValue);
    }
    if (attached) gVM->DetachCurrentThread();
}

void callBridge(jmethodID method, jlong request)
{
    bool attached = false;
    JNIEnv *env = currentEnvironment(attached);
    if (!env) return;
    {
        std::lock_guard<std::mutex> lock(gNetworkLock);
        if (gBridge && method) env->CallStaticVoidMethod(gBridge, method, request);
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
    if (attached) gVM->DetachCurrentThread();
}

bool callBridge(jmethodID method, jlong request, const std::string &argument)
{
    bool attached = false;
    JNIEnv *env = currentEnvironment(attached);
    if (!env) return false;
    bool succeeded = false;
    {
        std::lock_guard<std::mutex> lock(gNetworkLock);
        jstring value = toJava(env, argument);
        if (gBridge && method && value) {
            env->CallStaticVoidMethod(gBridge, method, request, value);
            succeeded = !env->ExceptionCheck();
        }
        if (env->ExceptionCheck()) env->ExceptionClear();
        if (value) env->DeleteLocalRef(value);
    }
    if (attached) gVM->DetachCurrentThread();
    return succeeded;
}

bool callBridge(jmethodID method, jlong request, const std::string &first, const std::string &second)
{
    bool attached = false;
    JNIEnv *env = currentEnvironment(attached);
    if (!env) return false;
    bool succeeded = false;
    {
        std::lock_guard<std::mutex> lock(gNetworkLock);
        jstring firstValue = toJava(env, first);
        jstring secondValue = toJava(env, second);
        if (gBridge && method && firstValue && secondValue) {
            env->CallStaticVoidMethod(gBridge, method, request, firstValue, secondValue);
            succeeded = !env->ExceptionCheck();
        }
        if (env->ExceptionCheck()) env->ExceptionClear();
        if (firstValue) env->DeleteLocalRef(firstValue);
        if (secondValue) env->DeleteLocalRef(secondValue);
    }
    if (attached) gVM->DetachCurrentThread();
    return succeeded;
}

bool callBridge(jmethodID method, jlong request, const std::string &title, const std::string &value, jint maximumLength)
{
    bool attached = false;
    JNIEnv *env = currentEnvironment(attached);
    if (!env) return false;
    bool succeeded = false;
    {
        std::lock_guard<std::mutex> lock(gNetworkLock);
        jstring titleValue = toJava(env, title);
        jstring textValue = toJava(env, value);
        if (gBridge && method && titleValue && textValue) {
            env->CallStaticVoidMethod(gBridge, method, request, titleValue, textValue, maximumLength);
            succeeded = !env->ExceptionCheck();
        }
        if (env->ExceptionCheck()) env->ExceptionClear();
        if (titleValue) env->DeleteLocalRef(titleValue);
        if (textValue) env->DeleteLocalRef(textValue);
    }
    if (attached) gVM->DetachCurrentThread();
    return succeeded;
}

std::string sha256(const std::string &data)
{
    if (data.size() > (2u << 20)) return "";
    bool attached = false;
    JNIEnv *env = currentEnvironment(attached);
    if (!env) return "";
    std::string result;
    bool frame = env->PushLocalFrame(8) >= 0;
    if (frame) do {
            jclass type = env->FindClass("java/security/MessageDigest");
            if (!type || env->ExceptionCheck()) break;
            jstring algorithm = env->NewStringUTF("SHA-256");
            if (!algorithm || env->ExceptionCheck()) break;
            auto factory = env->GetStaticMethodID(type, "getInstance", "(Ljava/lang/String;)Ljava/security/MessageDigest;");
            if (!factory || env->ExceptionCheck()) break;
            auto digest = env->GetMethodID(type, "digest", "([B)[B");
            if (!digest || env->ExceptionCheck()) break;
            auto instance = env->CallStaticObjectMethod(type, factory, algorithm);
            if (!instance || env->ExceptionCheck()) break;
            jbyteArray input = env->NewByteArray(static_cast<jsize>(data.size()));
            if (!input || env->ExceptionCheck()) break;
            env->SetByteArrayRegion(input, 0, static_cast<jsize>(data.size()), reinterpret_cast<const jbyte *>(data.data()));
            if (env->ExceptionCheck()) break;
            auto output = static_cast<jbyteArray>(env->CallObjectMethod(instance, digest, input));
            if (!output || env->ExceptionCheck() || env->GetArrayLength(output) != 32) break;
            uint8_t bytes[32];
            env->GetByteArrayRegion(output, 0, 32, reinterpret_cast<jbyte *>(bytes));
            if (env->ExceptionCheck()) break;
            const char *digits = "0123456789abcdef";
            for (auto byte : bytes) {
                result += digits[byte >> 4];
                result += digits[byte & 15];
            }
        } while (false);
    if (env->ExceptionCheck()) env->ExceptionClear();
    if (frame) env->PopLocalFrame(nullptr);
    if (attached) gVM->DetachCurrentThread();
    return result;
}

} // namespace

extern "C" {

ASTRAL_EXPORT JNIEXPORT jint JNICALL Java_app_astral_android_Native_initialize(JNIEnv *env, jclass, jobjectArray names,
                                                                               jstring dataDirectory, jstring resourceDirectory,
                                                                               jobjectArray languages, jstring discordVersion,
                                                                               jstring environment)
{
    size_t resolved = resolveProperties(strings(env, names));
    installCategory();
    std::string data = plain(env, dataDirectory);
    std::string resources = plain(env, resourceDirectory);
    std::string state = data + "/files/astral";
    gEnvironment = nlohmann::json::parse(plain(env, environment), nullptr, false);
    if (!gEnvironment.is_object()) gEnvironment = nlohmann::json::object();
    gEnvironment["astral"] = ASTRAL_VERSION;
    gEnvironment["discord"] = plain(env, discordVersion);
    gEnvironment["phoenix"] = plain(env, discordVersion);
    gEnvironment["engine"] = "Phoenix";
    gStateDirectory = state;
    gReportsPath = state + "/reports.json";
    std::error_code error;
    std::filesystem::create_directories(state + "/plugins", error);
    auto previousExit = astral::ExitRecord(state).take(false);
    astral::trace::open(state, static_cast<int64_t>(time(nullptr)));
    auto launch = astral::LaunchGuard(state).begin(previousExit);
    gRecovering = launch.enterRecovery;
    astral::discord::setDataDirectory(data + "/files");
    gSettings = std::make_unique<astral::SettingsStore>(state + "/settings.json");
    gSignatures = std::make_unique<astral::Signatures>(ASTRAL_SIGNING_KEY, sha256);
    gPacks = std::make_unique<astral::Packs>(*gSettings, state, resources);
    gThemes = std::make_unique<astral::Themes>(*gSettings, state, resources);
    gMarketplace = std::make_unique<astral::android::Marketplace>(*gSettings, *gThemes, *gSignatures, state, resources, fetch, applyTheme);
    applyTheme(gThemes->active());
    astral_wire_set_emoji_resolver(resolveEmoji);
    bool safeMode = gSettings->getBoolean("astral", "safe-mode", false);
    astral::luahost::start({state + "/plugins", resources + "/plugins"}, gSettings.get(), !safeMode && !gRecovering,
                           platform(state, resources, strings(env, languages)));
    installDecorators();
    configurePages(plain(env, discordVersion));
    startDeveloperServer(state);
    __android_log_print(ANDROID_LOG_INFO, "astral", "astral %s resolved %zu Phoenix properties; %zu plugins", ASTRAL_VERSION, resolved,
                        astral::luahost::plugins().size());
    return (jint)resolved;
}

ASTRAL_EXPORT JNIEXPORT void JNICALL Java_app_astral_android_Native_markVisible(JNIEnv *, jclass)
{
    if (!gStateDirectory.empty()) astral::LaunchGuard(gStateDirectory).markShown();
}

ASTRAL_EXPORT JNIEXPORT void JNICALL Java_app_astral_android_Native_markHidden(JNIEnv *, jclass)
{
    if (gStateDirectory.empty()) return;
    astral::LaunchGuard(gStateDirectory).markHidden();
    astral::ExitRecord(gStateDirectory).markEnded();
}

ASTRAL_EXPORT JNIEXPORT void JNICALL Java_app_astral_android_Native_markReady(JNIEnv *, jclass)
{
    if (gStateDirectory.empty()) return;
    astral::LaunchGuard(gStateDirectory).succeed();
    astral::ExitRecord(gStateDirectory).clear();
}

ASTRAL_EXPORT JNIEXPORT jbyteArray JNICALL Java_app_astral_android_Native_rewrite(JNIEnv *env, jclass, jbyteArray ops, jint length)
{
    if (!ops || length <= 0 || length > env->GetArrayLength(ops)) return nullptr;
    std::vector<uint8_t> bytes((size_t)length);
    env->GetByteArrayRegion(ops, 0, length, reinterpret_cast<jbyte *>(bytes.data()));
    AstralWireBuffer rewritten;
    {
        std::lock_guard<std::mutex> lock(gWireLock);
        if (!astral_settings_rewrite(bytes.data(), bytes.size(), &rewritten)) {
            astral_wire_note_identifiers(bytes.data(), bytes.size());
            return nullptr;
        }
        astral_wire_note_identifiers(rewritten.bytes, rewritten.length);
    }
    jbyteArray result = env->NewByteArray((jsize)rewritten.length);
    if (result) env->SetByteArrayRegion(result, 0, (jsize)rewritten.length, reinterpret_cast<const jbyte *>(rewritten.bytes));
    astral_wire_buffer_free(&rewritten);
    return result;
}

ASTRAL_EXPORT JNIEXPORT jstring JNICALL Java_app_astral_android_Native_plugins(JNIEnv *env, jclass)
{
    return text(env, astral::luahost::plugins().dump());
}

ASTRAL_EXPORT JNIEXPORT void JNICALL Java_app_astral_android_Native_setEnabled(JNIEnv *env, jclass, jstring identifier, jboolean enabled)
{
    astral::luahost::setEnabled(plain(env, identifier), enabled);
}

ASTRAL_EXPORT JNIEXPORT jstring JNICALL Java_app_astral_android_Native_eval(JNIEnv *env, jclass, jstring identifier, jstring code)
{
    return text(env, astral::luahost::eval(plain(env, identifier), plain(env, code)));
}

ASTRAL_EXPORT JNIEXPORT jboolean JNICALL Java_app_astral_android_Native_press(JNIEnv *, jclass, jint node)
{
    char identifier[128];
    {
        std::lock_guard<std::mutex> lock(gWireLock);
        if (!astral_wire_identifier(node, identifier, sizeof(identifier))) return JNI_FALSE;
    }
    if (!astral::navigator::handles(identifier)) return JNI_FALSE;
    astral::navigator::tap(identifier);
    return JNI_TRUE;
}

ASTRAL_EXPORT JNIEXPORT jboolean JNICALL Java_app_astral_android_Native_pressMessageAction(JNIEnv *, jclass, jint node)
{
    char identifier[128];
    {
        std::lock_guard<std::mutex> lock(gWireLock);
        if (!astral_wire_identifier(node, identifier, sizeof(identifier))) {
            return JNI_FALSE;
        }
    }
    bool pressed = astral::luahost::pressMessageAction(identifier);
    return pressed ? JNI_TRUE : JNI_FALSE;
}

ASTRAL_EXPORT JNIEXPORT jbyteArray JNICALL Java_app_astral_android_Native_composerEvent(JNIEnv *env, jclass, jbyteArray event)
{
    if (!event) return nullptr;
    jsize size = env->GetArrayLength(event);
    if (size <= 0 || size > 65536) return nullptr;
    std::vector<uint8_t> bytes(static_cast<size_t>(size));
    env->GetByteArrayRegion(event, 0, size, reinterpret_cast<jbyte *>(bytes.data()));
    AstralWireBuffer before;
    {
        std::lock_guard<std::mutex> lock(gWireLock);
        if (!astral_wire_composer_event(bytes.data(), bytes.size(), &before)) return nullptr;
    }
    jbyteArray result = env->NewByteArray(static_cast<jsize>(before.length));
    if (result) env->SetByteArrayRegion(result, 0, static_cast<jsize>(before.length), reinterpret_cast<const jbyte *>(before.bytes));
    astral_wire_buffer_free(&before);
    return result;
}

ASTRAL_EXPORT JNIEXPORT void JNICALL Java_app_astral_android_Native_touchMessage(JNIEnv *, jclass, jint node, jlong row)
{
    uint64_t message = 0;
    uint64_t channel = 0;
    {
        std::lock_guard<std::mutex> lock(gWireLock);
        message = row >= 0 ? astral_wire_message_for_row(node, row) : astral_wire_message_for_node(node);
        channel = astral_wire_current_channel();
    }
    if (message) astral::luahost::setMenuMessage(channel, message);
}

ASTRAL_EXPORT JNIEXPORT jbyteArray JNICALL Java_app_astral_android_Native_pendingUpdates(JNIEnv *env, jclass)
{
    return bytesOf(env, pendingUpdates());
}

ASTRAL_EXPORT JNIEXPORT jboolean JNICALL Java_app_astral_android_Native_tweensActive(JNIEnv *, jclass)
{
    std::lock_guard<std::mutex> lock(gWireLock);
    return astral_wire_tweens_active() ? JNI_TRUE : JNI_FALSE;
}

ASTRAL_EXPORT JNIEXPORT jbyteArray JNICALL Java_app_astral_android_Native_tweenFrame(JNIEnv *env, jclass, jdouble nowMs)
{
    std::vector<uint8_t> bytes;
    {
        std::lock_guard<std::mutex> lock(gWireLock);
        AstralWireBuffer buffer;
        if (astral_wire_tween_frame(&buffer, nowMs)) append(bytes, buffer);
    }
    return bytesOf(env, bytes);
}

ASTRAL_EXPORT JNIEXPORT jboolean JNICALL Java_app_astral_android_Native_slide(JNIEnv *env, jclass, jint node, jbyteArray payload)
{
    if (!payload) return JNI_FALSE;
    jsize size = env->GetArrayLength(payload);
    if (size <= 0 || size > 4096) return JNI_FALSE;
    std::vector<uint8_t> bytes(static_cast<size_t>(size));
    env->GetByteArrayRegion(payload, 0, size, reinterpret_cast<jbyte *>(bytes.data()));
    char identifier[128];
    double position = 0;
    {
        std::lock_guard<std::mutex> lock(gWireLock);
        if (!astral_wire_identifier(node, identifier, sizeof(identifier)) ||
            !astral_wire_event_value(bytes.data(), bytes.size(), &position))
            return JNI_FALSE;
    }
    if (std::string(identifier).rfind("astral.slider.", 0) != 0) return JNI_FALSE;
    astral::navigator::slidePosition(identifier, position);
    return JNI_TRUE;
}

ASTRAL_EXPORT JNIEXPORT jboolean JNICALL Java_app_astral_android_Native_back(JNIEnv *, jclass)
{
    return astral::navigator::back();
}

ASTRAL_EXPORT JNIEXPORT jboolean JNICALL Java_app_astral_android_Native_pageOpen(JNIEnv *, jclass)
{
    return astral::navigator::open();
}

ASTRAL_EXPORT JNIEXPORT void JNICALL Java_app_astral_android_Native_attachBridge(JNIEnv *env, jclass, jclass bridge)
{
    jclass reference = static_cast<jclass>(env->NewGlobalRef(bridge));
    if (!reference) return;
    jmethodID deliverMethod = env->GetStaticMethodID(reference, "deliver", "([B)V");
    jmethodID fetchMethod = env->GetStaticMethodID(reference, "fetch", "(Ljava/lang/String;Ljava/lang/String;)[B");
    jmethodID requestMethod = env->GetStaticMethodID(reference, "request", "(JLjava/lang/String;)V");
    jmethodID cancelMethod = env->GetStaticMethodID(reference, "cancelRequest", "(J)V");
    jmethodID exportBackupMethod = env->GetStaticMethodID(reference, "exportBackup", "()V");
    jmethodID importBackupMethod = env->GetStaticMethodID(reference, "importBackup", "()V");
    jmethodID openReportMethod = env->GetStaticMethodID(reference, "openReport", "(Ljava/lang/String;)V");
    jmethodID refreshEmojiMethod = env->GetStaticMethodID(reference, "refreshEmoji", "()V");
    jmethodID showAlertMethod = env->GetStaticMethodID(reference, "showAlert", "(Ljava/lang/String;Ljava/lang/String;)V");
    jmethodID showToastMethod = env->GetStaticMethodID(reference, "showToast", "(Ljava/lang/String;Ljava/lang/String;)V");
    jmethodID triggerHapticMethod = env->GetStaticMethodID(reference, "triggerHaptic", "(Ljava/lang/String;)V");
    jmethodID copyTextMethod = env->GetStaticMethodID(reference, "copyText", "(Ljava/lang/String;)V");
    jmethodID pickFileMethod = env->GetStaticMethodID(reference, "pickFile", "(JLjava/lang/String;)V");
    jmethodID cancelFilePickMethod = env->GetStaticMethodID(reference, "cancelFilePick", "(J)V");
    jmethodID openWebAuthMethod = env->GetStaticMethodID(reference, "openWebAuth", "(JLjava/lang/String;Ljava/lang/String;)V");
    jmethodID cancelWebAuthMethod = env->GetStaticMethodID(reference, "cancelWebAuth", "(J)V");
    jmethodID editTextMethod = env->GetStaticMethodID(reference, "editText", "(JLjava/lang/String;Ljava/lang/String;I)V");
    jmethodID restartGatewaySessionMethod = env->GetStaticMethodID(reference, "restartGatewaySession", "()V");
    jmethodID pasteTextMethod = env->GetStaticMethodID(reference, "pasteText", "()Ljava/lang/String;");
    jmethodID openURLMethod = env->GetStaticMethodID(reference, "openURL", "(Ljava/lang/String;)V");
    if (env->ExceptionCheck() || !deliverMethod || !fetchMethod || !requestMethod || !cancelMethod || !exportBackupMethod ||
        !importBackupMethod || !openReportMethod || !refreshEmojiMethod || !showAlertMethod || !triggerHapticMethod || !copyTextMethod ||
        !pickFileMethod || !cancelFilePickMethod || !showToastMethod || !openWebAuthMethod || !cancelWebAuthMethod || !editTextMethod ||
        !restartGatewaySessionMethod || !pasteTextMethod || !openURLMethod) {
        env->ExceptionClear();
        env->DeleteGlobalRef(reference);
        return;
    }
    {
        std::lock_guard<std::mutex> lock(gNetworkLock);
        if (gBridge) env->DeleteGlobalRef(gBridge);
        gBridge = reference;
        gDeliver = deliverMethod;
        gFetch = fetchMethod;
        gRequest = requestMethod;
        gCancelRequest = cancelMethod;
        gExportBackup = exportBackupMethod;
        gImportBackup = importBackupMethod;
        gOpenReport = openReportMethod;
        gRefreshEmoji = refreshEmojiMethod;
        gShowAlert = showAlertMethod;
        gShowToast = showToastMethod;
        gTriggerHaptic = triggerHapticMethod;
        gCopyText = copyTextMethod;
        gPickFile = pickFileMethod;
        gCancelFilePick = cancelFilePickMethod;
        gOpenWebAuth = openWebAuthMethod;
        gCancelWebAuth = cancelWebAuthMethod;
        gEditText = editTextMethod;
        gRestartGatewaySession = restartGatewaySessionMethod;
        gPasteText = pasteTextMethod;
        gOpenURL = openURLMethod;
    }
    astral::reactors::setFetcher(fetch);
}

ASTRAL_EXPORT JNIEXPORT void JNICALL Java_app_astral_android_Native_completeRequest(JNIEnv *env, jclass, jlong request, jint status,
                                                                                    jboolean ok, jstring url, jstring headers, jstring body,
                                                                                    jstring error)
{
    if (request <= 0) return;
    nlohmann::json response = {{"status", std::clamp(static_cast<int>(status), 0, 999)},
                               {"ok", ok == JNI_TRUE},
                               {"url", fromJava(env, url)},
                               {"body", fromJava(env, body)}};
    try {
        std::string encodedHeaders = fromJava(env, headers);
        if (encodedHeaders.size() <= (512u << 10)) {
            auto parsedHeaders = nlohmann::json::parse(encodedHeaders);
            if (parsedHeaders.is_object()) response["headers"] = std::move(parsedHeaders);
        }
    } catch (...) {
    }
    if (error) response["error"] = fromJava(env, error);
    astral::luahost::completeNetworkRequest(static_cast<uint64_t>(request), response);
}

ASTRAL_EXPORT JNIEXPORT void JNICALL Java_app_astral_android_Native_completeWebAuth(JNIEnv *env, jclass, jlong request, jstring url,
                                                                                    jstring cookies, jstring error)
{
    if (request <= 0) return;
    std::optional<std::string> resultURL;
    std::optional<std::string> resultCookies;
    std::optional<std::string> failure;
    if (url) resultURL = fromJava(env, url);
    if (cookies) resultCookies = fromJava(env, cookies);
    if (error) failure = fromJava(env, error);
    astral::luahost::completeWebAuth(static_cast<uint64_t>(request), resultURL, resultCookies, failure);
}

ASTRAL_EXPORT JNIEXPORT void JNICALL Java_app_astral_android_Native_completeFilePick(JNIEnv *env, jclass, jlong request, jstring name,
                                                                                     jbyteArray data, jstring error)
{
    if (request <= 0) return;
    std::optional<std::string> fileName;
    std::optional<std::string> contents;
    std::optional<std::string> failure;
    if (name) fileName = fromJava(env, name);
    if (data) {
        jsize length = env->GetArrayLength(data);
        if (length > (1 << 20)) {
            failure = "file exceeds 1 MiB";
        } else {
            contents.emplace(static_cast<size_t>(length), '\0');
            if (length > 0) env->GetByteArrayRegion(data, 0, length, reinterpret_cast<jbyte *>(contents->data()));
            if (env->ExceptionCheck()) {
                env->ExceptionClear();
                contents.reset();
                failure = "file could not be read";
            }
        }
    }
    if (error) failure = fromJava(env, error);
    astral::luahost::completeFilePick(static_cast<uint64_t>(request), fileName, contents, failure);
}

ASTRAL_EXPORT JNIEXPORT void JNICALL Java_app_astral_android_Native_completeTextEdit(JNIEnv *env, jclass, jlong request, jstring value)
{
    if (request <= 0) return;
    std::function<void(std::optional<std::string>)> completion;
    {
        std::lock_guard<std::mutex> lock(gTextEditLock);
        auto found = gTextEditCallbacks.find(static_cast<uint64_t>(request));
        if (found == gTextEditCallbacks.end()) return;
        completion = std::move(found->second);
        gTextEditCallbacks.erase(found);
    }
    if (value)
        completion(plain(env, value));
    else
        completion(std::nullopt);
}

ASTRAL_EXPORT JNIEXPORT void JNICALL Java_app_astral_android_Native_emitEvent(JNIEnv *env, jclass, jstring event)
{
    std::string name = plain(env, event);
    astral::luahost::emitEvent(name);
    if (name == "app.foreground") refreshCatalogIfDue(false);
}

ASTRAL_EXPORT JNIEXPORT void JNICALL Java_app_astral_android_Native_recordExitReport(JNIEnv *env, jclass, jstring kind, jlong timestamp,
                                                                                     jint reason, jstring description, jstring trace)
{
    recordProcessExitReport(plain(env, kind), timestamp, reason, fromJava(env, description), fromJava(env, trace));
}

ASTRAL_EXPORT JNIEXPORT jstring JNICALL Java_app_astral_android_Native_exportBackup(JNIEnv *env, jclass)
{
    if (!gSettings || !gThemes) return text(env, "");
    return text(env, astral::android::exportBackup(*gSettings, ASTRAL_VERSION, astral::luahost::plugins(), gThemes->list()));
}

ASTRAL_EXPORT JNIEXPORT jstring JNICALL Java_app_astral_android_Native_applyBackup(JNIEnv *env, jclass, jstring contents)
{
    if (!gSettings || !gThemes || !gMarketplace) return text(env, "astral is not ready");
    auto failure = astral::android::importBackup(plain(env, contents), *gSettings, *gThemes, *gMarketplace, applyTheme,
                                                 [] { astral::navigator::refresh(); });
    if (failure) return text(env, *failure);
    gPacks->invalidate();
    refreshImages();
    applyTheme(gThemes->active());
    callBridge(gRefreshEmoji);
    astral::navigator::refresh();
    return nullptr;
}

ASTRAL_EXPORT JNIEXPORT void JNICALL Java_app_astral_android_Native_setSessionToken(JNIEnv *env, jclass, jstring token)
{
    astral::session::setToken(plain(env, token));
}

ASTRAL_EXPORT JNIEXPORT jstring JNICALL Java_app_astral_android_Native_processGatewayMessage(JNIEnv *env, jclass, jstring message)
{
    auto processed = astral::luahost::processGatewayMessage(fromJava(env, message));
    return processed ? toJava(env, *processed) : nullptr;
}

ASTRAL_EXPORT JNIEXPORT jboolean JNICALL Java_app_astral_android_Native_isTelemetry(JNIEnv *env, jclass, jstring host, jstring path)
{
    return astral::isTelemetryRequest(plain(env, host), plain(env, path));
}

ASTRAL_EXPORT JNIEXPORT jboolean JNICALL Java_app_astral_android_Native_isCurrentUser(JNIEnv *env, jclass, jstring host, jstring path)
{
    return astral::isCurrentUserRequest(plain(env, host), plain(env, path));
}

ASTRAL_EXPORT JNIEXPORT jboolean JNICALL Java_app_astral_android_Native_staffDevToolsEnabled(JNIEnv *, jclass)
{
    return gSettings && gSettings->getBoolean("astral", "staff-devtools", false);
}

ASTRAL_EXPORT JNIEXPORT jstring JNICALL Java_app_astral_android_Native_withStaffUser(JNIEnv *env, jclass, jstring body)
{
    if (!gSettings || !gSettings->getBoolean("astral", "staff-devtools", false)) return nullptr;
    auto transformed = astral::withStaffUser(plain(env, body));
    return transformed ? text(env, *transformed) : nullptr;
}

ASTRAL_EXPORT JNIEXPORT jstring JNICALL Java_app_astral_android_Native_withStaffReady(JNIEnv *env, jclass, jstring message)
{
    if (!gSettings || !gSettings->getBoolean("astral", "staff-devtools", false)) return nullptr;
    auto transformed = astral::withStaffReady(plain(env, message));
    return transformed ? text(env, *transformed) : nullptr;
}

ASTRAL_EXPORT JNIEXPORT void JNICALL Java_app_astral_android_Native_noteBlockedRequest(JNIEnv *, jclass)
{
    gBlockedRequests.fetch_add(1);
}

ASTRAL_EXPORT JNIEXPORT jstring JNICALL Java_app_astral_android_Native_sinkUrl(JNIEnv *env, jclass)
{
    return text(env, astral::sink::url());
}

ASTRAL_EXPORT JNIEXPORT jstring JNICALL Java_app_astral_android_Native_iconPath(JNIEnv *env, jclass, jstring name)
{
    std::string identifier = plain(env, name);
    std::string path = gMarketplace ? gMarketplace->pluginIconPath(identifier) : "";
    if (path.empty() && gPacks) path = gPacks->iconPath(identifier);
    return text(env, path);
}

ASTRAL_EXPORT JNIEXPORT jstring JNICALL Java_app_astral_android_Native_emojiURL(JNIEnv *env, jclass, jstring codepoints)
{
    if (!gPacks || !codepoints) return text(env, "");
    return text(env, gPacks->emojiURL(plain(env, codepoints)));
}

ASTRAL_EXPORT JNIEXPORT jstring JNICALL Java_app_astral_android_Native_composerStyles(JNIEnv *env, jclass, jstring value)
{
    if (!value) return text(env, "[]");
    return text(env, astral::luahost::composerStyles(fromJava(env, value)).dump());
}

ASTRAL_EXPORT JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM *vm, void *)
{
    gVM = vm;
    return JNI_VERSION_1_6;
}
}

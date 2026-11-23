#include <android/log.h>
#include <jni.h>

#include <filesystem>
#include <atomic>
#include <optional>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

#include "AstralDevServer.h"
#include "AstralDiscordCache.h"
#include "AstralLuaHost.h"
#include "AstralNetworkPolicy.h"
#include "AstralPacks.h"
#include "AstralThemes.h"
#include "AstralReactors.h"
#include "AstralRequestSink.h"
#include "AstralSession.h"
#include "AstralSignatures.h"
#include "AstralSettingsNavigator.h"
#include "AstralSettingsPages.h"
#include "AstralSettings.h"
#include "PhoenixProperties.h"
#include "PhoenixWire.h"

#define ASTRAL_EXPORT __attribute__((visibility("default")))

namespace {

constexpr jsize kMaxPropertyNames = 2048;

std::unique_ptr<astral::SettingsStore> gSettings;
std::unique_ptr<astral::Packs> gPacks;
std::unique_ptr<astral::Themes> gThemes;
std::unique_ptr<astral::Signatures> gSignatures;
unsigned gImageGeneration = 0;
std::mutex gWireLock;
JavaVM *gVM = nullptr;
jclass gBridge = nullptr;
jmethodID gDeliver = nullptr;
jmethodID gFetch = nullptr;

std::atomic<bool> gRefreshDecorations{false};
void deliver(const std::vector<uint8_t> &bytes);
void refreshImages();
void applyTheme(const std::string &identifier);
std::string sha256(const std::string &data);

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
    platform.developerLog = developerLog;
    platform.recordFailure = [](const std::string &identifier, const std::string &name, const std::string &error) {
        __android_log_print(ANDROID_LOG_ERROR, "astral", "plugin %s (%s) failed: %s", identifier.c_str(), name.c_str(), error.c_str());
    };
    platform.reactors = astral::reactors::lookup;
    platform.refreshDecorations = [] { gRefreshDecorations = true; };
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

void configurePages(const std::string &discordVersion)
{
    astral::SettingsPagesPlatform pages;
    pages.settings = gSettings.get();
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
    pages.selectEmojiPack = [](const std::string &identifier) { gPacks->selectEmoji(identifier); };
    pages.emojiAnimated = [] { return gPacks->animated(); };
    pages.setEmojiAnimated = [](bool animated) { gPacks->setAnimated(animated); };
    pages.version = ASTRAL_VERSION;
    pages.context = [discordVersion] { return astral::SettingsPageContext{false, discordVersion, "ready"}; };
    pages.softRestart = [] {
        gPacks->invalidate();
        refreshImages();
        applyTheme(gThemes->active());
        for (const auto &plugin : astral::luahost::plugins())
            if (plugin.value("enabled", false)) astral::luahost::reload(plugin.value("id", ""));
    };
    astral::pages::configure(std::move(pages));
    astral::navigator::configure([](std::vector<uint8_t> bytes) { deliver(bytes); },
                                 [](const std::function<void()> &work) {
                                     std::lock_guard<std::mutex> lock(gWireLock);
                                     work();
                                 });
}

void installDecorators()
{
    astral_wire_set_decorator(AstralLuaDecorate);
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
    if (gRefreshDecorations.exchange(false) && astral_wire_refresh_reactions(&buffer)) append(merged, buffer);
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

astral::reactors::FetchResult fetch(const std::string &url, const std::string &token)
{
    astral::reactors::FetchResult result;
    bool attached = false;
    JNIEnv *env = currentEnvironment(attached);
    if (!env || !gBridge || !gFetch) return result;
    jstring jurl = env->NewStringUTF(url.c_str());
    jstring jtoken = env->NewStringUTF(token.c_str());
    auto reply = static_cast<jstring>(env->CallStaticObjectMethod(gBridge, gFetch, jurl, jtoken));
    if (env->ExceptionCheck()) env->ExceptionClear();
    if (reply) {
        const char *utf = env->GetStringUTFChars(reply, nullptr);
        std::string text = utf ? utf : "";
        if (utf) env->ReleaseStringUTFChars(reply, utf);
        size_t newline = text.find('\n');
        result.status = atoi(text.substr(0, newline).c_str());
        if (newline != std::string::npos) result.body = text.substr(newline + 1);
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
                                                                               jobjectArray languages, jstring discordVersion)
{
    size_t resolved = resolveProperties(strings(env, names));
    installCategory();
    std::string data = plain(env, dataDirectory);
    std::string resources = plain(env, resourceDirectory);
    std::string state = data + "/files/astral";
    std::error_code error;
    std::filesystem::create_directories(state + "/plugins", error);
    astral::discord::setDataDirectory(data + "/files");
    gSettings = std::make_unique<astral::SettingsStore>(state + "/settings.json");
    gSignatures = std::make_unique<astral::Signatures>(ASTRAL_SIGNING_KEY, sha256);
    gPacks = std::make_unique<astral::Packs>(*gSettings, state, resources);
    gThemes = std::make_unique<astral::Themes>(*gSettings, state, resources);
    applyTheme(gThemes->active());
    astral_wire_set_emoji_resolver(resolveEmoji);
    bool safeMode = gSettings->getBoolean("astral", "safe-mode", false);
    astral::luahost::start({state + "/plugins", resources + "/plugins"}, gSettings.get(), !safeMode,
                           platform(state, resources, strings(env, languages)));
    installDecorators();
    configurePages(plain(env, discordVersion));
    startDeveloperServer(state);
    __android_log_print(ANDROID_LOG_INFO, "astral", "astral %s resolved %zu Phoenix properties; %zu plugins", ASTRAL_VERSION, resolved,
                        astral::luahost::plugins().size());
    return (jint)resolved;
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

ASTRAL_EXPORT JNIEXPORT jbyteArray JNICALL Java_app_astral_android_Native_pendingUpdates(JNIEnv *env, jclass)
{
    return bytesOf(env, pendingUpdates());
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
    if (gBridge) env->DeleteGlobalRef(gBridge);
    gBridge = static_cast<jclass>(env->NewGlobalRef(bridge));
    gDeliver = env->GetStaticMethodID(gBridge, "deliver", "([B)V");
    gFetch = env->GetStaticMethodID(gBridge, "fetch", "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;");
    astral::reactors::setFetcher(fetch);
}

ASTRAL_EXPORT JNIEXPORT void JNICALL Java_app_astral_android_Native_setSessionToken(JNIEnv *env, jclass, jstring token)
{
    astral::session::setToken(plain(env, token));
}

ASTRAL_EXPORT JNIEXPORT jboolean JNICALL Java_app_astral_android_Native_isTelemetry(JNIEnv *env, jclass, jstring host, jstring path)
{
    return astral::isTelemetryRequest(plain(env, host), plain(env, path));
}

ASTRAL_EXPORT JNIEXPORT jstring JNICALL Java_app_astral_android_Native_sinkUrl(JNIEnv *env, jclass)
{
    return text(env, astral::sink::url());
}

ASTRAL_EXPORT JNIEXPORT jstring JNICALL Java_app_astral_android_Native_iconPath(JNIEnv *env, jclass, jstring name)
{
    return text(env, gPacks ? gPacks->iconPath(plain(env, name)) : "");
}

ASTRAL_EXPORT JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM *vm, void *)
{
    gVM = vm;
    return JNI_VERSION_1_6;
}
}

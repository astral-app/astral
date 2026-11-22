#include <android/log.h>
#include <jni.h>

#include <filesystem>
#include <memory>
#include <string>
#include <vector>

#include "AstralDiscordCache.h"
#include "AstralLuaHost.h"
#include "AstralSettings.h"
#include "PhoenixProperties.h"
#include "PhoenixWire.h"

#define ASTRAL_EXPORT __attribute__((visibility("default")))

namespace {

constexpr jsize kMaxPropertyNames = 2048;

const AstralRowSpec kCategoryRows[] = {
    {"general", "astral", nullptr, nullptr, "res://astral_galaxy", kAstralRowLink, 0, 0, 0},
    {"plugins", "Plugins", nullptr, nullptr, "res://ic_mana_puzzle_piece", kAstralRowLink, 0, 0, 0},
    {"design", "Design", nullptr, nullptr, "res://ic_mana_paintbrush_thick", kAstralRowLink, 0, 0, 0},
    {"marketplace", "Marketplace", nullptr, nullptr, "res://ic_mana_shop", kAstralRowLink, 0, 0, 0},
    {"updates", "Updates", nullptr, nullptr, "res://ic_mana_download", kAstralRowLink, 0, 0, 0},
};

std::unique_ptr<astral::SettingsStore> gSettings;

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
    platform.preferredLanguages = std::move(languages);
    platform.storagePath = state + "/plugin-storage.sqlite";
    platform.bundledDirectory = resources;
    return platform;
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

jstring text(JNIEnv *env, const std::string &value)
{
    return env->NewStringUTF(value.c_str());
}

} // namespace

extern "C" {

ASTRAL_EXPORT JNIEXPORT jint JNICALL Java_app_astral_android_Native_initialize(JNIEnv *env, jclass, jobjectArray names,
                                                                               jstring dataDirectory, jstring resourceDirectory,
                                                                               jobjectArray languages)
{
    size_t resolved = resolveProperties(strings(env, names));
    astral_wire_set_category("astral", kCategoryRows, sizeof(kCategoryRows) / sizeof(kCategoryRows[0]));
    std::string data = plain(env, dataDirectory);
    std::string resources = plain(env, resourceDirectory);
    std::string state = data + "/files/astral";
    std::error_code error;
    std::filesystem::create_directories(state + "/plugins", error);
    astral::discord::setDataDirectory(data + "/files");
    gSettings = std::make_unique<astral::SettingsStore>(state + "/settings.json");
    bool safeMode = gSettings->getBoolean("astral", "safe-mode", false);
    astral::luahost::start({state + "/plugins", resources + "/plugins"}, gSettings.get(), !safeMode,
                           platform(state, resources, strings(env, languages)));
    installDecorators();
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
    if (!astral_settings_rewrite(bytes.data(), bytes.size(), &rewritten)) return nullptr;
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

ASTRAL_EXPORT JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM *, void *)
{
    return JNI_VERSION_1_6;
}
}

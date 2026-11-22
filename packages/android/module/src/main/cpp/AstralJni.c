#include <android/log.h>
#include <jni.h>
#include <stdlib.h>
#include <string.h>

#include "PhoenixProperties.h"
#include "PhoenixWire.h"

#define ASTRAL_EXPORT __attribute__((visibility("default")))

enum { kMaxPropertyNames = 2048 };

static const AstralRowSpec kCategoryRows[] = {
    {"general", "astral", NULL, NULL, "res://astral_galaxy", kAstralRowLink, 0, 0, 0},
    {"plugins", "Plugins", NULL, NULL, "res://ic_mana_puzzle_piece", kAstralRowLink, 0, 0, 0},
    {"design", "Design", NULL, NULL, "res://ic_mana_paintbrush_thick", kAstralRowLink, 0, 0, 0},
    {"marketplace", "Marketplace", NULL, NULL, "res://ic_mana_shop", kAstralRowLink, 0, 0, 0},
    {"updates", "Updates", NULL, NULL, "res://ic_mana_download", kAstralRowLink, 0, 0, 0},
};

static void logInfo(const char *message)
{
    __android_log_write(ANDROID_LOG_INFO, "astral", message);
}

static size_t resolveProperties(JNIEnv *env, jobjectArray names)
{
    if (!names) return 0;
    jsize count = (*env)->GetArrayLength(env, names);
    if (count <= 0 || count > kMaxPropertyNames) return 0;
    const char **utf = calloc((size_t)count, sizeof(char *));
    jstring *strings = calloc((size_t)count, sizeof(jstring));
    if (!utf || !strings) {
        free(utf);
        free(strings);
        return 0;
    }
    for (jsize index = 0; index < count; index++) {
        strings[index] = (jstring)(*env)->GetObjectArrayElement(env, names, index);
        utf[index] = strings[index] ? (*env)->GetStringUTFChars(env, strings[index], NULL) : "";
    }
    const char *missing = NULL;
    size_t resolved = astral_wire_resolve_properties(utf, (size_t)count, &missing);
    for (jsize index = 0; index < count; index++) {
        if (strings[index]) {
            (*env)->ReleaseStringUTFChars(env, strings[index], utf[index]);
            (*env)->DeleteLocalRef(env, strings[index]);
        }
    }
    free(utf);
    free(strings);
    if (missing) __android_log_print(ANDROID_LOG_WARN, "astral", "Phoenix has no property %s; astral keeps its defaults", missing);
    return resolved;
}

ASTRAL_EXPORT JNIEXPORT jint JNICALL Java_app_astral_android_Native_initialize(JNIEnv *env, jclass type, jobjectArray names)
{
    (void)type;
    size_t resolved = resolveProperties(env, names);
    astral_wire_set_category("astral", kCategoryRows, sizeof(kCategoryRows) / sizeof(kCategoryRows[0]));
    __android_log_print(ANDROID_LOG_INFO, "astral", "astral %s resolved %zu Phoenix properties", ASTRAL_VERSION, resolved);
    return (jint)resolved;
}

ASTRAL_EXPORT JNIEXPORT jbyteArray JNICALL Java_app_astral_android_Native_rewrite(JNIEnv *env, jclass type, jbyteArray ops, jint length)
{
    (void)type;
    if (!ops || length <= 0 || length > (*env)->GetArrayLength(env, ops)) return NULL;
    jbyte *bytes = (*env)->GetPrimitiveArrayCritical(env, ops, NULL);
    if (!bytes) return NULL;
    AstralWireBuffer rewritten;
    int changed = astral_settings_rewrite((const uint8_t *)bytes, (size_t)length, &rewritten);
    (*env)->ReleasePrimitiveArrayCritical(env, ops, bytes, JNI_ABORT);
    if (!changed) return NULL;
    jbyteArray result = (*env)->NewByteArray(env, (jsize)rewritten.length);
    if (result) (*env)->SetByteArrayRegion(env, result, 0, (jsize)rewritten.length, (const jbyte *)rewritten.bytes);
    astral_wire_buffer_free(&rewritten);
    return result;
}

ASTRAL_EXPORT JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM *vm, void *reserved)
{
    (void)vm;
    (void)reserved;
    logInfo("astral native library loaded");
    return JNI_VERSION_1_6;
}

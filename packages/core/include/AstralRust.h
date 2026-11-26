#pragma once

#ifdef __cplusplus
#include <cstddef>
#include <cstdint>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

extern "C" {
#else
#include <stddef.h>
#include <stdint.h>
#endif
typedef struct {
    char *bytes;
    size_t length;
} AstralRustString;

typedef struct {
    void (*invoke)(void *context, const char *value, size_t length, int present);
    void (*release)(void *context);
    void *context;
} AstralCallback;

void astral_rust_string_free(AstralRustString string);
AstralRustString astral_rust_string_from(const char *bytes, size_t length);
#ifdef __cplusplus
}

namespace astral::rust {
inline std::string take(AstralRustString string)
{
    std::string text(string.bytes ? string.bytes : "", string.bytes ? string.length : 0);
    astral_rust_string_free(string);
    return text;
}

inline std::vector<std::string> takeList(AstralRustString string)
{
    std::string joined = take(string);
    std::vector<std::string> entries;
    size_t start = 0;
    for (size_t end = joined.find('\0'); end != std::string::npos; end = joined.find('\0', start)) {
        entries.emplace_back(joined, start, end - start);
        start = end + 1;
    }
    return entries;
}

inline std::vector<std::string> takeFrames(AstralRustString string)
{
    std::string joined = take(string);
    std::vector<std::string> entries;
    size_t at = 0;
    while (at + 4 <= joined.size()) {
        uint32_t length = static_cast<uint8_t>(joined[at]) | static_cast<uint8_t>(joined[at + 1]) << 8 |
                          static_cast<uint8_t>(joined[at + 2]) << 16 | static_cast<uint32_t>(static_cast<uint8_t>(joined[at + 3])) << 24;
        at += 4;
        if (length > joined.size() - at) break;
        entries.emplace_back(joined, at, length);
        at += length;
    }
    return entries;
}

inline std::optional<std::string> takeIfPresent(AstralRustString string)
{
    if (!string.bytes) return std::nullopt;
    return take(string);
}

using Reply = std::function<void(std::optional<std::string>)>;

inline AstralRustString give(const std::string &text)
{
    return astral_rust_string_from(text.data(), text.size());
}

inline AstralCallback replyCallback(Reply reply)
{
    if (!reply) return {nullptr, nullptr, nullptr};
    return {[](void *context, const char *value, size_t length, int present) {
                (*static_cast<Reply *>(context))(present ? std::optional<std::string>(std::string(value ? value : "", length))
                                                         : std::nullopt);
            },
            [](void *context) { delete static_cast<Reply *>(context); }, new Reply(std::move(reply))};
}

inline AstralCallback changedCallback(std::function<void()> changed)
{
    if (!changed) return {nullptr, nullptr, nullptr};
    return replyCallback([changed = std::move(changed)](std::optional<std::string>) { changed(); });
}

inline Reply adoptReply(AstralCallback callback)
{
    if (!callback.invoke) {
        if (callback.release) callback.release(callback.context);
        return nullptr;
    }
    auto owned = std::shared_ptr<AstralCallback>(new AstralCallback(callback), [](AstralCallback *held) {
        if (held->release) held->release(held->context);
        delete held;
    });
    return [owned](std::optional<std::string> value) {
        owned->invoke(owned->context, value ? value->data() : nullptr, value ? value->size() : 0, value ? 1 : 0);
    };
}

inline std::function<void()> adoptChanged(AstralCallback callback)
{
    auto reply = adoptReply(callback);
    if (!reply) return [] {};
    return [reply] { reply(std::nullopt); };
}

template <typename Call> std::optional<std::string> takeOptional(Call call)
{
    AstralRustString string{};
    if (!call(&string)) return std::nullopt;
    return take(string);
}
} // namespace astral::rust
#endif

#pragma once

#include <cstddef>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

extern "C" {
typedef struct {
    char *bytes;
    size_t length;
} AstralRustString;

void astral_rust_string_free(AstralRustString string);
AstralRustString astral_rust_string_from(const char *bytes, size_t length);
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

inline std::optional<std::string> takeIfPresent(AstralRustString string)
{
    if (!string.bytes) return std::nullopt;
    return take(string);
}

template <typename Call> std::optional<std::string> takeOptional(Call call)
{
    AstralRustString string{};
    if (!call(&string)) return std::nullopt;
    return take(string);
}
} // namespace astral::rust

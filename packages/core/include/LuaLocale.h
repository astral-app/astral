#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "AstralRust.h"

extern "C" {
AstralRustString astral_lua_best_locale(const char *available, size_t availableLength, const char *preferred, size_t preferredLength);
AstralRustString astral_lua_fill_placeholders(const char *text, size_t textLength, const char *values, size_t valuesLength);
int astral_lua_valid_plugin_identifier(const char *identifier, size_t length);
}

namespace astral::lua {
inline std::string joinedList(const std::vector<std::string> &entries)
{
    std::string joined;
    for (const auto &entry : entries) {
        joined += entry;
        joined.push_back('\0');
    }
    return joined;
}

inline void appendFrame(std::string &frames, const std::string &entry)
{
    uint32_t length = static_cast<uint32_t>(entry.size());
    for (int shift = 0; shift < 32; shift += 8) frames.push_back(static_cast<char>((length >> shift) & 0xFF));
    frames += entry;
}

inline std::optional<std::string> bestLocale(const std::vector<std::string> &available, const std::vector<std::string> &preferred)
{
    std::string availableList = joinedList(available), preferredList = joinedList(preferred);
    return rust::takeIfPresent(
        astral_lua_best_locale(availableList.data(), availableList.size(), preferredList.data(), preferredList.size()));
}

inline std::string fillPlaceholders(const std::string &text, const std::vector<std::pair<std::string, std::string>> &values)
{
    std::string frames;
    for (const auto &[name, value] : values) {
        appendFrame(frames, name);
        appendFrame(frames, value);
    }
    return rust::take(astral_lua_fill_placeholders(text.data(), text.size(), frames.data(), frames.size()));
}

inline bool validPluginIdentifier(const std::string &identifier)
{
    return astral_lua_valid_plugin_identifier(identifier.data(), identifier.size()) != 0;
}
} // namespace astral::lua

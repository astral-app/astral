#pragma once

#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "AstralRust.h"

extern "C" {
void astral_set_strings(const char *json, size_t length);
void astral_load_strings(const char *documents, size_t length);
AstralRustString astral_text(const char *key, size_t keyLength, const char *english, size_t englishLength, const char *values,
                             size_t valuesLength);
}

namespace astral {
using TextValues = std::vector<std::pair<std::string, std::string>>;

constexpr const char *kStringsFile = "i18n/astral.json";

inline void loadStrings(const std::string &documents)
{
    astral_load_strings(documents.data(), documents.size());
}

inline void setStrings(const std::string &json)
{
    astral_set_strings(json.data(), json.size());
}

inline std::string text(std::string_view key, std::string_view english, const TextValues &values = {})
{
    std::string joined;
    for (const auto &[name, value] : values) joined.append(name).append(1, '\0').append(value).append(1, '\0');
    return rust::take(astral_text(key.data(), key.size(), english.data(), english.size(), joined.data(), joined.size()));
}
} // namespace astral

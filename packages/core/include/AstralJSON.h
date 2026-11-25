#pragma once

#include <cstddef>
#include <nlohmann/json.hpp>
#include <string>
#include <utility>
#include <vector>

#include "AstralRust.h"

extern "C" {
AstralRustString astral_json_valid_utf8(const char *text, size_t length);
int astral_json_nests_too_deeply(const char *text, size_t length, size_t maximumDepth);
}

namespace astral {
inline constexpr size_t kMaximumJSONDepth = 128;

inline std::string withValidUTF8(const std::string &text)
{
    return rust::take(astral_json_valid_utf8(text.data(), text.size()));
}

inline bool jsonNestsTooDeeply(const nlohmann::json &value, size_t maximumDepth = kMaximumJSONDepth)
{
    std::vector<std::pair<const nlohmann::json *, size_t>> pending{{&value, 1}};
    while (!pending.empty()) {
        auto [current, depth] = pending.back();
        pending.pop_back();
        if (!current->is_structured()) continue;
        if (depth > maximumDepth) return true;
        for (const nlohmann::json &child : *current) pending.emplace_back(&child, depth + 1);
    }
    return false;
}

inline nlohmann::json parseBoundedJSON(const std::string &text, size_t maximumDepth = kMaximumJSONDepth)
{
    if (astral_json_nests_too_deeply(text.data(), text.size(), maximumDepth)) return nlohmann::json(nlohmann::json::value_t::discarded);
    return nlohmann::json::parse(text, nullptr, false);
}

inline std::string toJSON(const nlohmann::json &value)
{
    return value.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
}
} // namespace astral

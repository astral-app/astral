#pragma once

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "AstralJSON.h"

namespace astral {
struct JSONMember {
    std::string key;
    std::string_view value;
};

std::optional<std::vector<JSONMember>> splitJSONObject(std::string_view text, size_t maximumDepth = kMaximumJSONDepth);
} // namespace astral

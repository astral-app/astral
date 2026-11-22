#pragma once

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "LoaderJSON.h"

namespace loader {
struct JSONMember {
    std::string key;
    std::string_view value;
};

std::optional<std::vector<JSONMember>> splitJSONObject(std::string_view text,
                                                       size_t maximumDepth = kMaximumJSONDepth);
}

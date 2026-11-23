#pragma once

#include <string_view>
#include <nlohmann/json.hpp>

namespace astral {
nlohmann::json parseTOML(std::string_view text);
}

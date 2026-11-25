#pragma once

#include <nlohmann/json.hpp>
#include <string_view>

#include "AstralRust.h"

extern "C" AstralRustString astral_parse_toml(const char *text, size_t length);

namespace astral {
inline nlohmann::json parseTOML(std::string_view text)
{
    std::string json = rust::take(astral_parse_toml(text.data(), text.size()));
    return json.empty() ? nlohmann::json() : nlohmann::json::parse(json, nullptr, false);
}
}

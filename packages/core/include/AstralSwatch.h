#pragma once

#include <cstdint>
#include <optional>
#include <string>

namespace astral::settings {
bool isColorText(const std::string &text);
std::optional<uint32_t> parseColor(const std::string &text);
std::string colorText(uint32_t argb);
std::string swatchURI(uint32_t argb);
} // namespace astral::settings

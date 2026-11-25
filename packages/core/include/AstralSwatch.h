#pragma once

#include <cstdint>
#include <optional>
#include <string>

#include "AstralRust.h"

extern "C" {
int astral_parse_color(const char *text, size_t length, uint32_t *color);
AstralRustString astral_color_text(uint32_t argb);
AstralRustString astral_swatch_uri(uint32_t argb);
}

namespace astral::settings {
inline std::optional<uint32_t> parseColor(const std::string &text)
{
    uint32_t color = 0;
    if (!astral_parse_color(text.data(), text.size(), &color)) return std::nullopt;
    return color;
}

inline bool isColorText(const std::string &text)
{
    return text.empty() || parseColor(text).has_value();
}

inline std::string colorText(uint32_t argb)
{
    return rust::take(astral_color_text(argb));
}

inline std::string swatchURI(uint32_t argb)
{
    return rust::take(astral_swatch_uri(argb));
}
} // namespace astral::settings

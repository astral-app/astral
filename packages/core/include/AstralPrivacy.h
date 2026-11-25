#pragma once

#include <string>
#include <string_view>

#include "AstralRust.h"

extern "C" AstralRustString astral_sanitize_diagnostic_text(const char *text, size_t length);

namespace astral {
inline std::string sanitizeDiagnosticText(std::string_view text)
{
    return rust::take(astral_sanitize_diagnostic_text(text.data(), text.size()));
}
}

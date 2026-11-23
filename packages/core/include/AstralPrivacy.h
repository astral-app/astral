#pragma once

#include <string>
#include <string_view>

namespace astral {

std::string sanitizeDiagnosticText(std::string_view text);

}

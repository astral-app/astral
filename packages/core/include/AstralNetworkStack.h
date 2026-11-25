#pragma once

#include <string>

#include "AstralRust.h"

extern "C" AstralRustString astral_current_network_stack_marker(void);

namespace astral::environment_report {
inline std::string currentNetworkStackMarker()
{
    return rust::take(astral_current_network_stack_marker());
}
}

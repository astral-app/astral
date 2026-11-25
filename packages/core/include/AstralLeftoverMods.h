#pragma once

#include <string>
#include <vector>

#include "AstralRust.h"

extern "C" {
AstralRustString astral_leftover_mods(const char *entries, size_t length);
AstralRustString astral_leftover_mods_marker(const char *directory, size_t length);
}

namespace astral::environment_report {
inline std::vector<std::string> leftoverMods(const std::vector<std::string> &entries)
{
    std::string joined;
    for (const std::string &entry : entries) joined.append(entry).push_back('\0');
    return rust::takeList(astral_leftover_mods(joined.data(), joined.size()));
}

inline std::string leftoverModsMarker(const std::string &directory)
{
    return rust::take(astral_leftover_mods_marker(directory.data(), directory.size()));
}
}

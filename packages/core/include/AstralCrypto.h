#pragma once

#include <string>
#include <string_view>

#include "AstralRust.h"

extern "C" {
AstralRustString astral_sha256(const char *data, size_t length);
AstralRustString astral_hex(const char *data, size_t length);
}

namespace astral::crypto {
inline std::string sha256(std::string_view data)
{
    return rust::take(astral_sha256(data.data(), data.size()));
}

inline std::string hex(std::string_view bytes)
{
    return rust::take(astral_hex(bytes.data(), bytes.size()));
}
} // namespace astral::crypto

#pragma once

#include <string>
#include <string_view>

namespace astral::crypto {

std::string sha256(std::string_view data);
std::string hex(std::string_view bytes);

} // namespace astral::crypto

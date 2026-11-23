#pragma once

#include <string>

namespace astral::session {

bool plausibleToken(const std::string &token);
void setToken(const std::string &token);
std::string token();

} // namespace astral::session

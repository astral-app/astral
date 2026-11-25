#pragma once

#include <optional>
#include <string>
#include <vector>

#include "AstralLuaHost.h"

namespace astral::hostrequests {

std::optional<luahost::HttpRequest> parse(const std::vector<uint8_t> &frame);
std::vector<uint8_t> rebuild(const std::vector<uint8_t> &frame, const luahost::HttpRequest &request);
std::optional<std::vector<uint8_t>> intercept(const std::vector<uint8_t> &frame);
bool wanted(const std::vector<uint8_t> &frame);

} // namespace astral::hostrequests

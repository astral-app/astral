#pragma once

#include <cstdint>
#include <string>
#include <utility>
#include <vector>

namespace astral::sink {

uint16_t start();
std::string url();
std::string respondWith(int status, const std::vector<std::pair<std::string, std::string>> &headers, const std::string &body);

} // namespace astral::sink

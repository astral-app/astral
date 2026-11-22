#pragma once

#include <optional>
#include <string>
#include <utility>
#include <vector>

namespace loader::lua {
std::optional<std::string> bestLocale(const std::vector<std::string> &available, const std::vector<std::string> &preferred);
std::string fillPlaceholders(std::string text, const std::vector<std::pair<std::string, std::string>> &values);
bool validPluginIdentifier(const std::string &identifier);
} // namespace loader::lua
